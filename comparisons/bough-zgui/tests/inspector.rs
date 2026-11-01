//! The inspector app on the laid-out gallery (round 4): picking by pointer is the target's hit testing; the
//! overlay shows the picked ink, its layout box and the tiles the target's last frame repaired; the
//! inspector's own frames are exact (presented == full redraw).
use bough::prelude::*;
use bough_zgui::gallery::{Doc, GalleryL};
use bough_zgui::inspector::{self, Inspector, MINI};
use bough_zgui::*;

fn px(p: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
    let i = (y * w + x) * 4;
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

#[test]
fn the_inspector_picks_overlays_and_follows_damage() {
    // one surface: every frame repairs only its damage (age 1), so the overlay shows the hover's tiles alone
    let mut tc = config(WIDTH, HEIGHT);
    tc.ring = 1;
    let mut target = App::new(GalleryL::fresh(), tc, true);
    target.frame();
    let mut insp = App::new(Inspector::new(WIDTH, HEIGHT), config(1000.0, 600.0), true);
    insp.frame();
    // pick the "Button" heading (zgui's box at 41,205): the target's own hit test answers
    inspector::pick(&mut insp, &target, 60.0, 210.0);
    insp.frame();
    let p = insp.owner.picked.expect("a record under the pointer");
    let r = target.record_view(p);
    assert_eq!(r.kind_name(), "text", "{r:?}");
    assert!(insp.owner.info.contains("text"), "{}", *insp.owner.info);
    assert!(insp.owner.tree.iter().any(|l| l.contains("> text")), "the tree marks the pick: {:?}", *insp.owner.tree);
    let (pp, rr) = (insp.presented_pixels(), insp.reference_pixels());
    assert!(pp == rr, "inspector frame exact");
    // the overlay: the picked ink's centre is tinted red over the minimap
    let [x0, y0, x1, y1] = r.ink.to_xyxy();
    let (cx, cy) = ((10.0 + (x0 + x1) as f32 * 0.5 * MINI) as usize, (10.0 + (y0 + y1) as f32 * 0.5 * MINI) as usize);
    let c = px(&pp, 1000, cx, cy);
    assert!(c[2] > c[1] + 40 && c[2] > c[0] + 20, "ink overlay (red over the blue layout box) at ({cx},{cy}): {c:?}");
    // the target repaints a hovered swatch: the inspector's overlay marks that tile
    target.pointer_move(41.0, 41.0);
    target.frame();
    inspector::sync(&mut insp, &target);
    insp.frame();
    let d = insp.owner.damage.clone();
    assert!(d.iter().any(|&r| r != 0), "the target's last frame repaired tiles");
    let (pp, rr) = (insp.presented_pixels(), insp.reference_pixels());
    assert!(pp == rr, "inspector frame exact after the damage overlay changed");
    let (tx, ty) = (41 / 32, 41 / 32);
    let c = px(&pp, 1000, (10.0 + (tx as f32 + 0.5) * 32.0 * MINI) as usize, (10.0 + (ty as f32 + 0.5) * 32.0 * MINI) as usize);
    assert!(c[2] > 120 && c[1] > 60, "the repaired tile is marked orange: {c:?}");
    assert_eq!(d.iter().map(|r| r.count_ones()).sum::<u32>() < 12, true, "only the hovered swatch's tiles: {d:?}");
    let far = px(&pp, 1000, (10.0 + 45.5 * 32.0 * MINI) as usize, (10.0 + 28.5 * 32.0 * MINI) as usize);
    assert!(far[2] < 100, "a tile the frame did not repair is not marked: {far:?}");
}
