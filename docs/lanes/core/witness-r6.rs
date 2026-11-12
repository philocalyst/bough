use bough_core::{Bough, NRect, RecordInit};
fn main() {
    let mut b = Bough::new(320, 320);
    b.insert_run(
        0,
        &[
            RecordInit::new(0, NRect::EMPTY, 0, 0),
            RecordInit::new(1, NRect::new(100, 100, 120, 120), 1, 1),
        ],
    );
    b.next_epoch();
    let e = b.epoch();
    let before = b.hit(110, 110);
    let cell = b.hit_cell(110, 110, b.hit_top(110, 110));
    b.scroll_run_copy(1, 1, 0, 64, NRect::new(0, 0, 320, 320), |_, _| -1);
    println!(
        "copy: before={before:?} after={:?} cell={cell:?} valid={}",
        b.hit(110, 110),
        b.cell_valid(cell, e)
    );
    let mut c = Bough::new(320, 320);
    c.insert_run(0, &[RecordInit::new(0, NRect::new(10, 10, 20, 20), 1, 42)]);
    c.set_run_xform(0, 1, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0], [100.0, 0.0]);
    let mut out = Vec::new();
    c.cull(NRect::new(110, 10, 120, 20), None, &mut out);
    println!(
        "affine: ink={:?} hit={:?} cull={out:?}",
        c.ink_at(0),
        c.hit(115, 15)
    );
    c.remove_run(0, 1);
    c.insert_run(0, &[RecordInit::new(0, NRect::new(10, 10, 20, 20), 1, 99)]);
    println!("reused: ink={:?} xform={:?}", c.ink_at(0), c.xform_at(0));
}
