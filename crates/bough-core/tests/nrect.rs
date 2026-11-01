//! NRect algebra: exhaustive over a reduced domain + random over the full range, scalar and SIMD
//! paths against a naive normal-form implementation.

use bough_core::nrect::*;
use bough_core::oracle::NR;
use bough_core::rng::Rng;

fn naive(r: NRect) -> NR {
    NR::from_nrect(r)
}

/// All canonical rects with coordinates in lo..=hi (plus EMPTY).
fn all_rects(lo: i32, hi: i32) -> Vec<NRect> {
    let mut v = vec![NRect::EMPTY];
    for x0 in lo..=hi {
        for x1 in x0 + 1..=hi {
            for y0 in lo..=hi {
                for y1 in y0 + 1..=hi {
                    v.push(NRect::new(x0, y0, x1, y1));
                }
            }
        }
    }
    v
}

fn check_pair(a: NRect, b: NRect) {
    let (na, nb) = (naive(a), naive(b));
    // intersect
    let i = a.intersect(b);
    let ni = na.intersect(&nb);
    assert_eq!(i.is_empty(), ni.empty(), "{a:?} & {b:?}");
    if !ni.empty() {
        assert_eq!(naive(i), ni);
    }
    assert_eq!(a.intersects(b), !ni.empty());
    // union (operands canonical)
    let u = a.union(b);
    let nu = na.union(&nb);
    assert_eq!(u.is_empty(), nu.empty());
    if !nu.empty() {
        assert_eq!(naive(u), nu, "{a:?} | {b:?}");
    } else {
        assert_eq!(u, NRect::EMPTY);
    }
    // threshold test used by cull: rect-vs-nonempty-query
    if !nb.empty() {
        assert_eq!(Thresh::of_rect(b).matches(a), !ni.empty() && !na.empty(), "thresh {a:?} vs {b:?}");
    }
    // SIMD, 2 rects per op, both lanes
    let si = intersect2([a, b], [b, a]);
    assert_eq!(si[0].is_empty(), ni.empty());
    assert_eq!(si[0], i);
    assert_eq!(si[1], i);
    let su = union2([a, b], [b, a]);
    assert_eq!(su[0], u);
    assert_eq!(su[1], u);
    assert_eq!(intersect2_scalar([a, b], [b, a]), si);
    assert_eq!(union2_scalar([a, b], [b, a]), su);
}

#[test]
fn exhaustive_pairs_small_domain() {
    let (lo, hi) = if cfg!(debug_assertions) { (-3, 3) } else { (-5, 5) };
    let rects = all_rects(lo, hi);
    for &a in &rects {
        for &b in &rects {
            check_pair(a, b);
        }
    }
}

#[test]
fn exhaustive_points_small_domain() {
    let rects = all_rects(-4, 4);
    for &r in &rects {
        let n = naive(r);
        let empt = [r, r];
        assert_eq!(is_empty2(empt), [n.empty(); 2]);
        for x in -6..=6 {
            for y in -6..=6 {
                let want = !n.empty() && n.contains(x, y);
                assert_eq!(r.contains_point(x, y), want, "{r:?} ({x},{y})");
                assert_eq!(Thresh::of_point(x, y).unwrap().matches(r), want, "thresh {r:?} ({x},{y})");
            }
        }
    }
}

#[test]
fn exhaustive_raw_lanes_is_empty() {
    // Arbitrary (possibly non-canonical, inverted) lanes: empty test must match the normal form.
    let vals = -8i16..=8;
    for a in vals.clone() {
        for b in vals.clone() {
            for c in vals.clone() {
                for d in vals.clone() {
                    let r = NRect([a, b, c, d]);
                    let want = a as i32 >= -(c as i32) || b as i32 >= -(d as i32);
                    assert_eq!(r.is_empty(), want);
                    assert_eq!(is_empty2([r, NRect::EMPTY]), [want, true]);
                    assert_eq!(is_empty2([NRect::new(0, 0, 1, 1), r]), [false, want]);
                    assert_eq!(r.canon().is_empty(), want);
                }
            }
        }
    }
}

#[test]
fn saturation_extremes() {
    // Lane sums overflow i16 in scalar i16 math; the algebra must not care.
    let big = NRect::new(-32766, -32766, 32766, 32766);
    let tiny = NRect::new(32765, 32765, 32766, 32766);
    let low = NRect::new(-32766, -32766, -32765, -32765);
    assert!(!big.is_empty() && !tiny.is_empty() && !low.is_empty());
    assert!(!big.intersect(tiny).is_empty());
    assert!(big.intersect(tiny) == tiny);
    assert!(tiny.intersect(low).is_empty());
    assert_eq!(big.union(tiny), big);
    assert_eq!(tiny.union(low), NRect::new(-32766, -32766, 32766, 32766));
    assert_eq!(is_empty2([big, tiny]), [false, false]);
    assert_eq!(is_empty2([NRect([i16::MAX; 4]), NRect([i16::MIN; 4])]), [true, false]);
    assert!(big.contains_point(-32766, 32765) && !big.contains_point(32766, 0));
}

fn rand_rect(rng: &mut Rng) -> NRect {
    let ext = match rng.below(4) {
        0 => 16,
        1 => 300,
        2 => 5000,
        _ => 32766,
    };
    let c = |rng: &mut Rng| rng.range(-32766, 32766);
    let x0 = c(rng);
    let y0 = c(rng);
    let w = rng.range(-8, ext);
    let h = rng.range(-8, ext);
    let (x1, y1) = (x0.saturating_add(w).min(40000), y0.saturating_add(h).min(40000));
    // sometimes anchor near zero to get lots of overlaps
    if rng.chance(1, 3) {
        let x0 = rng.range(-50, 50);
        let y0 = rng.range(-50, 50);
        return NRect::new(x0, y0, x0 + rng.range(-3, 80), y0 + rng.range(-3, 80));
    }
    NRect::new(x0, y0, x1, y1)
}

#[test]
fn random_full_range() {
    let mut rng = Rng::new(1);
    let n = if cfg!(debug_assertions) { 200_000 } else { 3_000_000 };
    for _ in 0..n {
        let (a, b) = (rand_rect(&mut rng), rand_rect(&mut rng));
        check_pair(a, b);
        let (x, y) = (rng.range(-32766, 32766), rng.range(-32766, 32766));
        assert_eq!(a.contains_point(x, y), !a.is_empty() && naive(a).contains(x, y));
    }
}

#[test]
fn slice_kernels_vs_naive() {
    let mut rng = Rng::new(2);
    let iters = if cfg!(debug_assertions) { 3_000 } else { 60_000 };
    for _ in 0..iters {
        let n = rng.usize_below(65);
        let rects: Vec<NRect> = (0..n).map(|_| rand_rect(&mut rng)).collect();
        // union_all
        let u = union_all(&rects);
        assert_eq!(u, union_all_scalar(&rects));
        let nu = rects.iter().fold(NR::NONE, |a, &b| a.union(&naive(b)));
        assert_eq!(u.is_empty(), nu.empty());
        if !nu.empty() {
            assert_eq!(naive(u), nu);
        }
        // lt_mask (rect query and point query)
        let q = loop {
            let q = rand_rect(&mut rng);
            if !q.is_empty() {
                break q;
            }
        };
        let t = Thresh::of_rect(q);
        let m = unsafe { lt_mask(rects.as_ptr(), n, t) };
        assert_eq!(m, unsafe { lt_mask_scalar(rects.as_ptr(), n, t) });
        for (i, &r) in rects.iter().enumerate() {
            let want = !r.is_empty() && !naive(r).intersect(&naive(q)).empty();
            assert_eq!((m >> i) & 1 == 1, want, "lt_mask rect {i}");
        }
        if n < 64 {
            assert_eq!(m >> n, 0, "no bits past n");
        }
        let (x, y) = if n > 0 && rng.chance(1, 2) {
            let r = naive(rects[rng.usize_below(n)]);
            (r.x0 + rng.range(-1, 1), r.y0 + rng.range(-1, 1))
        } else {
            (rng.range(-300, 300), rng.range(-300, 300))
        };
        if let Some(t) = Thresh::of_point(x, y) {
            let m = unsafe { lt_mask(rects.as_ptr(), n, t) };
            for (i, &r) in rects.iter().enumerate() {
                let want = !r.is_empty() && naive(r).contains(x, y);
                assert_eq!((m >> i) & 1 == 1, want, "lt_mask point {i}");
            }
        }
        // translate_union
        let (dx, dy) = (rng.range(-40, 40), rng.range(-40, 40));
        // keep inside the domain: only translate rects that stay inside
        let ok = rects.iter().all(|&r| r.is_empty() || {
            let [a, b, c, d] = naive(r).tup();
            [a + dx, c + dx].iter().all(|v| (COORD_MIN..=COORD_MAX).contains(v)) && [b + dy, d + dy].iter().all(|v| (COORD_MIN..=COORD_MAX).contains(v))
        });
        if ok {
            let mut a = rects.clone();
            let mut b = rects.clone();
            let ra = unsafe { translate_union(a.as_mut_ptr(), n, dx, dy) };
            let rb = unsafe { translate_union_scalar(b.as_mut_ptr(), n, dx, dy) };
            assert_eq!(a, b);
            assert_eq!(ra, rb);
            for (i, &r) in rects.iter().enumerate() {
                if r.is_empty() {
                    assert_eq!(a[i], NRect::EMPTY);
                } else {
                    assert_eq!(naive(a[i]), naive(r).translate(dx, dy));
                }
            }
        }
    }
}

trait Tup {
    fn tup(&self) -> [i32; 4];
}
impl Tup for NR {
    fn tup(&self) -> [i32; 4] {
        [self.x0, self.y0, self.x1, self.y1]
    }
}

#[test]
fn group_kernels_match_scalar() {
    use bough_core::nrect::{Thresh, lt8, lt8_scalar, union8, union_all_scalar};
    let mut rng = bough_core::rng::Rng::new(77);
    for _ in 0..scale_n(200_000, 20_000) {
        let mut rs = [NRect::EMPTY; 8];
        for r in rs.iter_mut() {
            if !rng.chance(1, 5) {
                let (x, y) = (rng.range(-300, 300), rng.range(-300, 300));
                *r = NRect::new(x, y, x + rng.range(-5, 200), y + rng.range(-5, 200));
            }
        }
        let (x, y) = (rng.range(-300, 300), rng.range(-300, 300));
        let q = NRect::new(x, y, x + rng.range(1, 200), y + rng.range(1, 200));
        if q.is_empty() {
            continue;
        }
        let t = Thresh::of_rect(q);
        unsafe {
            assert_eq!(lt8(rs.as_ptr(), t), lt8_scalar(rs.as_ptr(), t));
            assert_eq!(union8(rs.as_ptr()), union_all_scalar(&rs));
        }
        if let Some(tp) = Thresh::of_point(x, y) {
            unsafe { assert_eq!(lt8(rs.as_ptr(), tp), lt8_scalar(rs.as_ptr(), tp)) };
        }
    }
}

#[test]
fn compact8_exhaustive() {
    let src: [u32; 8] = core::array::from_fn(|i| 0x1000_0000 + i as u32 * 0x0101_0101);
    for m in 0..256u32 {
        let mut out = [0u32; 16];
        let end = unsafe { bough_core::emit::compact8(src.as_ptr(), m, out.as_mut_ptr()) };
        let n = unsafe { end.offset_from(out.as_ptr()) } as usize;
        let want: Vec<u32> = (0..8).filter(|i| m & (1 << i) != 0).map(|i| src[i]).collect();
        assert_eq!(&out[..n], &want[..], "mask {m:08b}");
    }
}

fn scale_n(release: usize, debug: usize) -> usize {
    if cfg!(debug_assertions) { debug } else { release }
}
