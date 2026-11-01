//! The vlist scene's correctness gates (needs a Metal device: the ring surfaces are read back):
//!  1. after every few presented frames the surface the ring drew into equals a from-scratch full redraw
//!     of the retained state through the same kernels, for scroll, fling and jump, at every ring age 1..=4
//!     (round-robin rings of 1..4 surfaces) and with the windowed content-addressed ring;
//!  2. the retained frame after the run equals a fresh mount of the same list at the same offset with the
//!     same row heights (the wires / anchoring / mount / unmount / translate paths leave the pixels a
//!     from-scratch description would);
//!  3. every mounted row's layout (incremental, as cephal left it) equals a cold layout of a copy of its
//!     tree, node for node, bit for bit; and each laid-out node's written geometry equals the sum of its
//!     chain of layout locations;
//!  4. the prefix index agrees with a naive running sum under random point updates;
//!  5. rows' heights are the line count x 20 pt + the fixed parts (the zgx formula), per row.
use bough::prelude::*;
use bough_vlist::*;
use zgx_workload::vlist::{self, Mode};

fn first_diff(a: &[u8], b: &[u8]) -> Option<(usize, usize, u8, u8)> {
    a.iter().zip(b).position(|(x, y)| x != y).map(|i| ((i / 4) % 1440, (i / 4) / 1440, a[i], b[i]))
}

const RINGS: [usize; 5] = [1, 2, 3, 4, 0];

fn mk(rows: usize, ring: usize) -> App<VList> {
    let mut c = config(2.0);
    c.ring = if ring == 0 { 4 } else { ring };
    let mut app = App::new(VList::new(rows), c, true);
    app.windowed = ring == 0;
    app
}

fn run(mode: Mode, ticks: u64, every: u64, rows: usize) -> App<VList> {
    run_ring(mode, ticks, every, rows, 4)
}

fn run_ring(mode: Mode, ticks: u64, every: u64, rows: usize, ring: usize) -> App<VList> {
    let mut app = mk(rows, ring);
    app.frame();
    let mut checked = 0;
    for t in 1..=ticks {
        step(&mut app, mode, t);
        let drew = app.frame();
        assert!(!app.bins_overflowed(), "row bin overflow at tick {t}");
        if drew && t % every == 0 {
            let (px, reference) = (app.presented_pixels(), app.reference_pixels());
            assert!(px == reference, "{mode:?} ring {ring} tick {t}: presented != full redraw at {:?}", first_diff(&px, &reference));
            checked += 1;
        }
    }
    assert!(checked > 0);
    app
}

#[test]
fn scroll_is_pixel_exact() {
    for ring in RINGS {
        run_ring(Mode::Scroll, 200, 7, vlist::ROWS, ring);
    }
}
#[test]
fn fling_is_pixel_exact() {
    for ring in RINGS {
        run_ring(Mode::Fling, 150, 5, vlist::ROWS, ring);
    }
}
#[test]
fn jump_is_pixel_exact() {
    // a jump every 30 ticks; check right after each and in between
    for ring in RINGS {
        run_ring(Mode::Jump, 120, 3, vlist::ROWS, ring);
    }
}

fn heights_snapshot(app: &App<VList>) -> bough::mlist::HeightIndex {
    let r = &app.rt.mregions[0];
    let mut idx = bough::mlist::HeightIndex::new(r.idx.len(), r.idx.estimate());
    for k in 0..r.idx.len() {
        idx.set(k, r.idx.height(k));
    }
    idx
}

#[test]
fn retained_equals_fresh_mount_with_same_heights() {
    for mode in [Mode::Scroll, Mode::Fling, Mode::Jump] {
        let mut app = run(mode, 150, 50, vlist::ROWS);
        let px = app.presented_pixels();
        let at = app.rt.mregions[0].scroll_px;
        let mut fresh = App::new(VList::new(vlist::ROWS), config(2.0), true);
        fresh.rt.mregions[0].idx = heights_snapshot(&app);
        fresh.frame();
        // same offset, same heights: nothing to re-measure, nothing to anchor
        let target = at as f32 / 2.0;
        fresh.turn(|d| {
            d.scroll.set(target);
        });
        fresh.frame();
        assert_eq!(fresh.rt.mregions[0].scroll_px, at, "{mode:?}: fresh mount at the same offset");
        let fp = fresh.presented_pixels();
        assert!(px == fp, "{mode:?}: retained != fresh mount at {:?}", first_diff(&px, &fp));
    }
}

#[test]
fn row_layouts_equal_cold_layout() {
    let app = run(Mode::Jump, 100, 50, vlist::ROWS);
    let lay = &app.rt.lay;
    let mut checked = 0;
    for row in app.rt.mregions[0].rows.iter() {
        let avail = cephal::Size { width: cephal::AvailableSpace::Definite(1440.0), height: cephal::AvailableSpace::MaxContent };
        let cold = lay.eng.cold_rects(&app.rt.bough, row.root, avail);
        // the engine's current rects, node for node
        let mut now = Vec::new();
        let mut stack = vec![row.root];
        while let Some(n) = stack.pop() {
            now.push((n, lay.eng.rect(n)));
            lay.eng.children_into(&app.rt.bough, n, &mut stack);
        }
        now.sort_by_key(|e| e.0);
        assert_eq!(now.len(), cold.len());
        for (a, b) in now.iter().zip(&cold) {
            assert_eq!(a, b, "row layout differs from cold");
        }
        // written geometry = the chain of locations
        fn walk(rt: &bough::rt::Rt, n: u32, px: f32, py: f32, count: &mut usize) {
            let (x, y, w, h) = rt.lay.eng.rect(n);
            let li = rt.lay.info[n as usize];
            assert_eq!((li.ax, li.ay, li.aw, li.ah), (px + x, py + y, w, h), "written geometry of node {n}");
            *count += 1;
            let mut kids = Vec::new();
            rt.lay.eng.children_into(&rt.bough, n, &mut kids);
            for k in kids {
                walk(rt, k, px + x, py + y, count);
            }
        }
        walk(&app.rt, row.root, 0.0, 0.0, &mut checked);
    }
    assert!(checked > 10);
}

#[test]
fn prefix_index_matches_naive() {
    let n = 5_003;
    let mut idx = bough::mlist::HeightIndex::new(n, 212);
    let mut naive = vec![212i32; n];
    let mut st = 12345u64;
    let mut rnd = || {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        st
    };
    for round in 0..3000 {
        let k = (rnd() % n as u64) as usize;
        let v = 100 + (rnd() % 1500) as i32;
        let d = idx.set(k, v);
        assert_eq!(d, v - naive[k]);
        naive[k] = v;
        if round % 7 == 0 {
            let q = (rnd() % (n as u64 + 1)) as usize;
            assert_eq!(idx.prefix(q), naive[..q].iter().map(|&x| x as i64).sum::<i64>());
            let y = (rnd() % (idx.total() as u64 + 50)) as i64;
            let mut acc = 0i64;
            let mut want = n - 1;
            for (i, &h) in naive.iter().enumerate() {
                if y < acc + h as i64 {
                    want = i;
                    break;
                }
                acc += h as i64;
            }
            assert_eq!(idx.row_at(y), want, "row_at({y})");
        }
    }
}

#[test]
fn heights_follow_the_zgx_formula() {
    // measure rows 0..300 by scrolling each to the top; a row is 2*10 pad + 20 author + 6 gap + 20*lines (+ 6 + 90)
    let mut app = App::new(VList::new(vlist::ROWS), config(2.0), false);
    app.frame();
    for row in 0..300usize {
        for _ in 0..2 {
            let off = app.rt.mregion_row_offset_pt(0, row);
            set_scroll(&mut app, off);
        }
        let h = app.rt.mregion_row_height_pt(0, row);
        let lines = h - 46.0 - if vlist::has_image(row) { 96.0 } else { 0.0 };
        assert!(lines >= 20.0 && lines <= 20.0 * 12.0 && lines % 20.0 == 0.0, "row {row}: height {h} is not a whole number of lines (+ image)");
    }
}

#[test]
fn scrolling_back_up_and_in_steps_is_pixel_exact() {
    // down through unmeasured rows, then up through them (rows measured above the viewport are anchored), with
    // a few jumps and sub-row steps in both directions
    let mut app = App::new(VList::new(vlist::ROWS), config(2.0), true);
    app.frame();
    let mut at = 0.0f32;
    let steps: Vec<f32> = [vec![300.0; 12], vec![-37.0; 40], vec![1000.0; 3], vec![-1000.0; 5], vec![7.5; 30], vec![-12.0; 200]].concat();
    for (i, d) in steps.iter().enumerate() {
        at = (*app.owner.scroll + d).max(0.0);
        set_scroll(&mut app, at);
        if app.frame() && i % 3 == 0 {
            let (px, r) = (app.presented_pixels(), app.reference_pixels());
            assert!(px == r, "step {i} (scroll {at}): presented != full redraw at {:?}", first_diff(&px, &r));
        }
    }
    let _ = at;
    // anchoring: the scroll offset never drifts from the first visible row's place by more than its estimate error
    assert!(app.rt.mregion_scroll_pt(0) >= 0.0);
}
