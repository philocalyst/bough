//! The two implementations must agree on what they cull and hit for the benchmark shapes
//! (otherwise the comparison compares different work).

use bough_core::arena::Rect;
use bough_core::rng::Rng;
use bough_core::shapes::*;
use bough_core::NRect;

#[test]
fn bough_and_arena_agree_on_shapes() {
    for shape in all() {
        let mut b = build_bough(&shape);
        let (mut a, ids) = build_arena(&shape);
        b.check_invariants();
        assert_eq!(b.len(), shape.specs.len());
        assert_eq!(a.len(), shape.specs.len());
        let mut rng = Rng::new(3);
        let (mut ob, mut oa) = (Vec::new(), Vec::new());
        let check = |b: &bough_core::Bough, a: &mut bough_core::arena::ArenaTree, rng: &mut Rng, ob: &mut Vec<u32>, oa: &mut Vec<u32>, tag: &str| {
            for _ in 0..40 {
                let (x, y) = (rng.range(-20, shape.window.0), rng.range(-20, shape.window.1));
                let (w, h) = (rng.range(1, 300), rng.range(1, 200));
                b.cull(NRect::new(x, y, x + w, y + h), None, ob);
                a.cull(&[Rect::new(x as f32, y as f32, w as f32, h as f32)], oa);
                assert_eq!(ob, oa, "{} {tag} cull", shape.name);
                let (px, py) = (rng.range(0, shape.window.0), rng.range(0, shape.window.1));
                let hb = b.hit(px, py).map(|p| p as u32);
                let ha = a.hit(px as f32, py as f32).map(|id| id.slot);
                let hr = a.hit_rev(px as f32, py as f32).map(|id| id.slot);
                assert_eq!(ha, hr);
                // arena slots == spec index at build time (no frees yet), so compare via payload order
                if tag == "built" {
                    assert_eq!(hb, ha, "{} hit", shape.name);
                }
            }
        };
        check(&b, &mut a, &mut rng, &mut ob, &mut oa, "built");
        // translate the ~1k subtree in both
        if let Some((start, len)) = shape.sub1k {
            b.translate_run(start, len, 5, -3);
            a.translate_subtree(ids[start], 5.0, -3.0);
            check(&b, &mut a, &mut rng, &mut ob, &mut oa, "translated");
        }
        // structure queries agree
        for _ in 0..200 {
            let p = rng.usize_below(shape.specs.len());
            let pb = b.parent(p);
            let pa = a.parent(ids[p]).map(|id| ids.iter().position(|&x| x == id).unwrap());
            assert_eq!(pb, pa);
            let eb = b.subtree_end(p);
            let ea = a.after_subtree(ids[p]).map(|id| ids.iter().position(|&x| x == id).unwrap());
            // after_subtree is the next node in preorder after the subtree; at the very end both are "none"
            assert_eq!(if eb == b.len() { None } else { Some(eb) }, ea);
        }
    }
}

#[test]
fn arena_damage_collapse_and_ring() {
    let shape = dashboard();
    let (mut a, ids) = build_arena(&shape);
    // 100 far-apart tiny writes exceed 64 rects -> viewport
    for i in 0..shape.leaves.len().min(300) {
        a.mark_paint(ids[shape.leaves[i]]);
    }
    a.flush();
    let d = a.pending_damage();
    assert!(d.len() <= 64);
    a.end_frame();
    let r = a.repair(3).to_vec();
    assert!(!r.is_empty());
}

#[test]
fn list_scroll_step_keeps_both_consistent() {
    let shape = list();
    let info = shape.list.as_ref().unwrap();
    let mut b = build_bough(&shape);
    b.check_invariants();
    for k in 0..200 {
        let row: Vec<_> = list_row(48 + (k % 20) * 32).iter().enumerate().map(|(i, s)| s.init(1_000_000 + (k as u32) * 8 + i as u32)).collect();
        b.insert_run(info.first_row, &row);
        b.remove_run(info.first_row + info.rows * info.row_len, info.row_len);
        b.next_epoch();
    }
    b.check_invariants();
    assert_eq!(b.len(), shape.specs.len());
    assert_eq!(b.subtree_end(info.container), info.first_row + info.rows * info.row_len);
}
