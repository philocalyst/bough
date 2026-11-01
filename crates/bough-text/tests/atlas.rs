use bough_text::atlas::{Atlas, GlyphCache, Instance};
use bough_text::{GlyphRun, Shaper, SimpleScript};

const FONT: &[u8] = zgx_workload::FONT_BYTES;

#[test]
fn shelf_packing_is_disjoint_and_pow2() {
    let mut a = Atlas::new(256, 64);
    let mut rects: Vec<(u32, u32, u32, u32)> = vec![];
    let mut seed = 12345u32;
    for _ in 0..400 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let (w, h) = (3 + (seed >> 8) % 20, 3 + (seed >> 16) % 24);
        let (x, y) = a.alloc(w, h);
        assert!(x + w <= a.width && y + h <= a.height);
        for &(rx, ry, rw, rh) in &rects {
            let disjoint = x + w <= rx || rx + rw <= x || y + h <= ry || ry + rh <= y;
            assert!(disjoint, "({x},{y},{w},{h}) overlaps ({rx},{ry},{rw},{rh})");
        }
        rects.push((x, y, w, h));
    }
    assert!(a.height.is_power_of_two() && a.width.is_power_of_two());
    assert!(a.generation > 0, "must have grown");
}

#[test]
fn instances_for_a_run() {
    let sh = Shaper::new(SimpleScript::build(FONT, 0).unwrap());
    let mut run = GlyphRun::default();
    assert!(sh.shape_into("Hello, world", 16., &mut run));
    let mut cache = GlyphCache::new(FONT);
    let strike = cache.strike(16.);
    let mut out: Vec<Instance> = vec![];
    let n = cache.instances(strike, &run, 10., 30., 0xff_ff_ff_ff, &mut out);
    assert_eq!(n, 11, "one instance per non-space glyph");
    assert_eq!(out.len(), 11);
    let first = out[0];
    // 'H' at 16 px: the left edge is near the origin, the top is above the baseline.
    assert!(first.xy[0] >= 10. && first.xy[0] < 14., "{first:?}");
    assert!(first.xy[1] < 30. && first.xy[1] > 14., "{first:?}");
    assert!(first.wh[0] > 5. && first.wh[1] > 8.);
    // x positions increase and uv rects lie inside the atlas, with ink in them.
    for w in out.windows(2) {
        assert!(w[1].xy[0] > w[0].xy[0]);
    }
    for i in &out {
        let (x, y, w, h) = (i.uv[0] as u32, i.uv[1] as u32, i.uv[2] as u32, i.uv[3] as u32);
        assert!(x + w <= cache.atlas.width && y + h <= cache.atlas.height);
        let mut ink = 0u32;
        for r in 0..h {
            for c in 0..w {
                ink += cache.atlas.pixels[((y + r) * cache.atlas.width + x + c) as usize] as u32;
            }
        }
        assert!(ink > 200, "empty glyph bitmap");
    }
    // Second pass is all hits.
    let r0 = cache.rasterized;
    let mut out2 = vec![];
    cache.instances(strike, &run, 10., 30., 0xff_ff_ff_ff, &mut out2);
    assert_eq!(cache.rasterized, r0);
    assert_eq!(out, out2);
}

#[test]
fn fused_shape_to_instances_equals_two_step() {
    let sh = Shaper::new(SimpleScript::build(FONT, 0).unwrap());
    let mut cache = GlyphCache::new(FONT);
    let strike = cache.strike(14.);
    let mut run = GlyphRun::default();
    let mut st = 0x1234_5678_9abc_def1u64;
    let mut next = || {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        st
    };
    let mut fused_ok = 0;
    for _ in 0..3000 {
        let len = (next() % 60) as usize;
        let s: String = (0..len).map(|_| (0x20 + (next() % 95) as u8) as char).collect();
        let ok = sh.shape_into(&s, 14., &mut run);
        let (mut a, mut b) = (vec![], vec![]);
        let fused = cache.shape_instances_ascii(&sh, strike, &s, 7., 40., 0x11223344, &mut b);
        // The fused prototype writer knows only the dense ASCII table: it refuses the
        // ligature strings that `shape_into` now shapes through the token path.
        let lig = s.contains("ff") || s.contains("fi") || s.contains("fl");
        assert_eq!(fused.is_some(), ok && !lig, "{s:?}");
        if fused.is_some() {
            let n = cache.instances(strike, &run, 7., 40., 0x11223344, &mut a);
            assert_eq!(fused, Some(n), "{s:?}");
            assert_eq!(a, b, "{s:?}");
            fused_ok += 1;
        } else {
            assert!(b.is_empty());
        }
    }
    assert!(fused_ok > 1000);
    // Non-ASCII is declined, not mis-shaped.
    let mut v = vec![];
    assert_eq!(cache.shape_instances_ascii(&sh, strike, "caf\u{e9}", 0., 0., 0, &mut v), None);
}
