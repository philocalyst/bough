//! Round 6 witnesses (found by the `core` lane's probe, `docs/lanes/core/witness-r6.rs`): three places where
//! two core answers about the same scene disagreed. Each test fails on the round's base commit.
use bough_core::{Bough, NRect, RecordInit};

/// A scroll by copy moves a record without marking the tiles the copy supplies. A hit cell saved before
/// the scroll must not survive it: the record under the pointer changed.
#[test]
fn hit_cell_is_void_after_a_scroll_by_copy_through_it() {
    let mut b = Bough::new(320, 320);
    b.insert_run(0, &[RecordInit::new(0, NRect::EMPTY, 0, 0), RecordInit::new(1, NRect::new(100, 100, 120, 120), 1, 1)]);
    b.next_epoch();
    let e = b.epoch();
    assert_eq!(b.hit(110, 110), Some(1));
    let cell = b.hit_cell(110, 110, b.hit_top(110, 110));
    assert!(b.cell_valid(cell, e));
    b.scroll_run_copy(1, 1, 0, 64, NRect::new(0, 0, 320, 320), |_, _| -1);
    assert_eq!(b.hit(110, 110), None, "the record moved away");
    assert!(!b.cell_valid(cell, e), "the cell certified a hit that no longer holds");
    // a scroll elsewhere leaves a cell outside its clip valid
    let mut c = Bough::new(320, 320);
    c.insert_run(0, &[RecordInit::new(0, NRect::EMPTY, 0, 0), RecordInit::new(1, NRect::new(10, 10, 20, 20), 1, 1), RecordInit::new(1, NRect::new(200, 200, 220, 220), 1, 2)]);
    c.next_epoch();
    let e = c.epoch();
    let cell = c.hit_cell(15, 15, c.hit_top(15, 15));
    c.scroll_run_copy(2, 1, 0, 32, NRect::new(160, 160, 320, 320), |_, _| -1);
    assert!(c.cell_valid(cell, e), "a scroll in another clip must not void the cell");
}

/// `hit` and `cull` must agree on where a transformed record is.
#[test]
fn cull_finds_a_transformed_record_where_hit_finds_it() {
    let mut c = Bough::new(320, 320);
    c.insert_run(0, &[RecordInit::new(0, NRect::new(10, 10, 20, 20), 1, 42)]);
    c.set_run_xform(0, 1, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0], [100.0, 0.0]);
    assert_eq!(c.hit(115, 15), Some(0));
    let mut out = Vec::new();
    c.cull(NRect::new(110, 10, 120, 20), None, &mut out);
    assert_eq!(out, vec![42], "cull at the transformed place");
    c.cull(NRect::new(10, 10, 20, 20), None, &mut out);
    assert_eq!(out, Vec::<u32>::new(), "nothing is left at the untransformed place");
    // through the tile plane too: the transform's own damage covers the new place
    let mut scratch = bough_core::damage::Plane::default();
    c.cull_repair(1, &mut scratch, &mut out);
    assert_eq!(out, vec![42]);
}

/// A page returned to the pool and reused by an untransformed run carries no transform.
#[test]
fn a_reused_page_does_not_inherit_a_transform() {
    let mut c = Bough::new(320, 320);
    c.insert_run(0, &[RecordInit::new(0, NRect::new(10, 10, 20, 20), 1, 42)]);
    c.set_run_xform(0, 1, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0], [100.0, 0.0]);
    c.remove_run(0, 1);
    c.insert_run(0, &[RecordInit::new(0, NRect::new(10, 10, 20, 20), 1, 99)]);
    assert_eq!(c.xform_at(0), None);
    assert_eq!(c.ink_at(0), NRect::new(10, 10, 20, 20));
}
