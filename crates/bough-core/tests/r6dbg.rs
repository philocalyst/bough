use bough_core::bough::RecordInit;
use bough_core::{Bough, NRect};
#[test]
fn dbg() {
    let n = 1000;
    let rs: Vec<_> = (0..n).map(|i| RecordInit::new(if i == 0 { 0 } else { 1 }, NRect::new(2, 2, 14, 14), 1, i as u32)).collect();
    let mut b = Bough::new(3200, 3200);
    b.insert_run(0, &rs);
    b.check_invariants();
    let pad = [RecordInit::new(1, NRect::EMPTY, 1, 0); 64];
    b.insert_run(n, &pad);
    b.check_invariants();
    b.remove_run(n, 64);
    b.check_invariants();
    b.insert_run(1, &pad[..8]);
    b.check_invariants();
}
