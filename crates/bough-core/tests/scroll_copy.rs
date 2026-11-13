//! Scroll by copy (bough-metal RESULTS §15): differential tests of the core half against simple oracles.
//!  1. `Plane::shift_within` == a per-tile oracle: tile t is set iff it was set, or t lies in the clip's tile span
//!     and the pixel rect of some set tile of that span, moved by d, overlaps t's pixel rect.
//!  2. `Bough::scroll_run_copy`'s damage covers every pixel a copy of the previous frame gets wrong: a pixel
//!     model of the records (each record paints its ink; static records are tested by their band rule) shows
//!     that every pixel of the clip whose value differs between "the old frame moved by d" and "the new frame"
//!     lies in a marked tile.
use bough_core::damage::{Plane, TILE};
use bough_core::rng::Rng;
use bough_core::{Bough, NRect, RecordInit};

fn oracle_shift(p: &Plane, c: NRect, dx: i32, dy: i32) -> Plane {
    let mut out = p.clone();
    let [cx0, cy0, cx1, cy1] = c.to_xyxy();
    let (tx0, ty0) = (cx0.div_euclid(TILE), cy0.div_euclid(TILE));
    let (tx1, ty1) = ((cx1 - 1).div_euclid(TILE) + 1, (cy1 - 1).div_euclid(TILE) + 1);
    for y in ty0..ty1 {
        for x in tx0..tx1 {
            if !p.tile(x as usize, y as usize) {
                continue;
            }
            // moved pixel rect [32x + dx, 32x + 32 + dx) x ...
            let (px0, py0) = (x * TILE + dx, y * TILE + dy);
            for ty in ty0..ty1 {
                for tx in tx0..tx1 {
                    let (qx0, qy0) = (tx * TILE, ty * TILE);
                    if px0 < qx0 + TILE && qx0 < px0 + TILE && py0 < qy0 + TILE && qy0 < py0 + TILE {
                        out.rows[ty as usize] |= 1u128 << tx;
                    }
                }
            }
        }
    }
    out
}

#[test]
fn shift_within_equals_the_tile_oracle() {
    let mut rng = Rng(0x5c_c0_11);
    for case in 0..3000 {
        let rows = 50u32;
        let mut p = Plane::new(rows);
        for _ in 0..rng.below(12) {
            let x0 = rng.range(0, 1400);
            let y0 = rng.range(0, 1580);
            let r = NRect::new(x0, y0, x0 + rng.range(1, 200), y0 + rng.range(1, 120));
            p.add(r, NRect::new(0, 0, 1440, 1600));
        }
        let cx0 = rng.range(0, 1200);
        let cy0 = rng.range(0, 1400);
        let c = NRect::new(cx0, cy0, (cx0 + rng.range(1, 900)).min(1440), (cy0 + rng.range(1, 900)).min(1600));
        let (dx, dy) = if rng.below(3) == 0 { (rng.range(-90, 90), 0) } else { (0, rng.range(-200, 200)) };
        let want = oracle_shift(&p, c, dx, dy);
        let mut got = p.clone();
        got.shift_within(c, dx, dy);
        for y in 0..rows as usize {
            assert_eq!(got.rows[y], want.rows[y], "case {case}: row {y}, c {c:?}, d ({dx}, {dy})");
        }
        // the row band stays exact enough for repair (every set row inside it)
        let (lo, hi) = got.occ_range();
        assert!((0..rows as usize).all(|y| got.rows[y] == 0 || (lo <= y && y < hi)), "case {case}: row band");
    }
}

/// One rect record per entry; `band < 0` = an arbitrary bitmap (text), else a solid box.
fn scene(rng: &mut Rng, n: usize) -> Vec<RecordInit> {
    (0..n)
        .map(|i| {
            let x0 = rng.range(0, 1300);
            let y0 = rng.range(-200, 1500);
            let r = NRect::new(x0, y0, x0 + rng.range(4, 300), y0 + rng.range(4, 300));
            RecordInit::new(1, r, (i % 2) as u8, i as u32)
        })
        .collect()
}

#[test]
fn scroll_run_copy_damage_covers_every_wrong_pixel() {
    // pixel model, rows of the clip only: a record of kind 1 paints a value that depends on the pixel's offset
    // inside it (text-like), kind 0 a constant (a solid box: its pixels depend on its edges within band 2 here
    // only through coverage, which the model makes exact: inside = colour)
    let (w, h) = (1440, 1600);
    let mut rng = Rng(0xc0b7);
    for case in 0..200 {
        let mut b = Bough::new(w, h);
        let n = 40 + rng.below(80) as usize;
        let recs = scene(&mut rng, n);
        b.insert_run(0, &recs);
        b.next_epoch();
        let start = rng.below(n as u64 / 2) as usize;
        let len = 1 + rng.below((n - start) as u64 - 1) as usize;
        let cx0 = rng.range(0, 700);
        let cy0 = rng.range(0, 800);
        let c = NRect::new(cx0, cy0, cx0 + rng.range(32, 700), cy0 + rng.range(32, 800));
        let dy = rng.range(-150, 150);
        if dy == 0 {
            continue;
        }
        let paint = |recs: &[(NRect, u8, u32)], x: i32, y: i32| -> u64 {
            let mut v = 0u64;
            for &(r, kind, id) in recs {
                let [x0, y0, x1, y1] = r.to_xyxy();
                if x >= x0 && x < x1 && y >= y0 && y < y1 {
                    v = if kind == 0 { id as u64 + 1 } else { ((id as u64 + 1) << 32) ^ ((x - x0) as u64 * 7919 + (y - y0) as u64) };
                }
            }
            v
        };
        let before: Vec<(NRect, u8, u32)> = (0..n).map(|i| (b.ink_at(i), b.kind_at(i), b.payload_at(i))).collect();
        b.scroll_run_copy(start, len, 0, dy, c, |kind, _| if kind == 0 { 2 } else { -1 });
        let after: Vec<(NRect, u8, u32)> = (0..n).map(|i| (b.ink_at(i), b.kind_at(i), b.payload_at(i))).collect();
        assert_eq!(b.scroll_rec(0).map(|r| (r.dx, r.dy)), Some((0, dy)), "case {case}");
        let marks = b.repair(1);
        let [x0, y0, x1, y1] = c.intersect(NRect::new(0, 0, w, h)).to_xyxy();
        let mut wrong = 0;
        for y in (y0..y1).step_by(1) {
            for x in (x0..x1).step_by(3) {
                // what the copy leaves: the old frame at p - d if that is inside the clip, else nothing (exposed)
                let sy = y - dy;
                if sy < y0 || sy >= y1 {
                    assert!(marks.tile((x / TILE) as usize, (y / TILE) as usize), "case {case}: exposed pixel ({x}, {y}) unmarked");
                    continue;
                }
                let copied = paint(&before, x, sy);
                let truth = paint(&after, x, y);
                if copied != truth {
                    wrong += 1;
                    assert!(marks.tile((x / TILE) as usize, (y / TILE) as usize), "case {case}: pixel ({x}, {y}) wrong after the copy and unmarked (c {c:?}, dy {dy}, run {start}+{len})");
                }
            }
        }
        let _ = wrong;
    }
}

/// The same oracle with transformed records (`set_run_xform`, a turn): the kernels map a pixel of a transformed
/// record through the affine in absolute coordinates, so its pixels are not a function of the offset from the
/// record (the model paints a value of the absolute pixel); a copy is wrong wherever one lands or was, whether it
/// moves with the run or stays (a static turned box, whose band rule would only see its cover's edges). The
/// band closure keeps answering 2 for kind 0: the core must not ask it about a transformed page.
#[test]
fn scroll_run_copy_damage_covers_transformed_records() {
    let (w, h) = (1440, 1600);
    let mut rng = Rng(0x7e5f);
    let (c30, s30) = (0.866_025_4f32, 0.5f32);
    let mut checked = 0;
    for case in 0..200 {
        let mut b = Bough::new(w, h);
        let n = 40 + rng.below(80) as usize;
        let recs = scene(&mut rng, n);
        b.insert_run(0, &recs);
        // two transformed runs, one likely inside the scrolled run and one likely outside it
        let mut xf = vec![false; n];
        for _ in 0..2 {
            let s = rng.below(n as u64 - 4) as usize;
            let l = 1 + rng.below(4) as usize;
            let k = [rng.range(100, 1300) as f32, rng.range(100, 1500) as f32];
            b.set_run_xform(s, l, [c30, -s30, s30, c30], k, [0.0, 0.0]);
            xf[s..s + l].iter_mut().for_each(|v| *v = true);
        }
        b.next_epoch();
        let start = rng.below(n as u64 / 2) as usize;
        let len = 1 + rng.below((n - start) as u64 - 1) as usize;
        let cx0 = rng.range(0, 700);
        let cy0 = rng.range(0, 800);
        let c = NRect::new(cx0, cy0, cx0 + rng.range(32, 700), cy0 + rng.range(32, 800));
        let dy = rng.range(-150, 150);
        if dy == 0 {
            continue;
        }
        let paint = |recs: &[(NRect, u8, u32, bool)], x: i32, y: i32| -> u64 {
            let mut v = 0u64;
            for &(r, kind, id, t) in recs {
                let [x0, y0, x1, y1] = r.to_xyxy();
                if x >= x0 && x < x1 && y >= y0 && y < y1 {
                    v = if t {
                        ((id as u64 + 1) << 40) ^ (x as u64 * 104_729 + y as u64 * 7919)
                    } else if kind == 0 {
                        id as u64 + 1
                    } else {
                        ((id as u64 + 1) << 32) ^ ((x - x0) as u64 * 7919 + (y - y0) as u64)
                    };
                }
            }
            v
        };
        let before: Vec<(NRect, u8, u32, bool)> = (0..n).map(|i| (b.ink_at(i), b.kind_at(i), b.payload_at(i), xf[i])).collect();
        b.scroll_run_copy(start, len, 0, dy, c, |kind, _| if kind == 0 { 2 } else { -1 });
        let after: Vec<(NRect, u8, u32, bool)> = (0..n).map(|i| (b.ink_at(i), b.kind_at(i), b.payload_at(i), xf[i])).collect();
        let marks = b.repair(1);
        let [x0, y0, x1, y1] = c.intersect(NRect::new(0, 0, w, h)).to_xyxy();
        for y in y0..y1 {
            for x in (x0..x1).step_by(3) {
                let sy = y - dy;
                if sy < y0 || sy >= y1 {
                    continue;
                }
                if paint(&before, x, sy) != paint(&after, x, y) {
                    checked += 1;
                    assert!(marks.tile((x / TILE) as usize, (y / TILE) as usize), "case {case}: pixel ({x}, {y}) wrong after the copy and unmarked (c {c:?}, dy {dy}, run {start}+{len})");
                }
            }
        }
    }
    assert!(checked > 10_000, "the transformed records were exercised ({checked} wrong pixels checked)");
}
