//! Round 6: structural edits of the directory at sizes that cross its internal boundaries (prefix
//! blocks of 32 pages, chunk summaries), checked by `check_invariants` after every step.
use bough_core::bough::RecordInit;
use bough_core::{Bough, NRect};

fn recs(n: usize) -> Vec<RecordInit> {
    (0..n).map(|i| RecordInit::new(if i == 0 { 0 } else { 1 }, NRect::new(2, 2, 14, 14), 1, i as u32)).collect()
}

/// A split at the tail, its inverse, then a split at the front: the cost harness's warm-up, which caught
/// a prefix rebuilt from `d + 1` instead of `d` that the tests of the time let through on a stale build.
#[test]
fn tail_split_and_inverse_then_front_split() {
    for n in [1usize, 63, 64, 65, 1000, 2047, 2048, 2049, 5000] {
        let rs = recs(n);
        let mut b = Bough::new(3200, 3200);
        b.insert_run(0, &rs);
        b.check_invariants();
        let starts = b.page_starts();
        let pad = [RecordInit::new(1, NRect::EMPTY, 1, 0); 64];
        b.insert_run(n, &pad);
        b.check_invariants();
        b.remove_run(n, 64);
        b.check_invariants();
        assert_eq!(b.page_starts(), starts, "n {n}");
        b.insert_run(1.min(n), &pad[..8]);
        b.check_invariants();
        assert_eq!(b.len(), n + 8);
        for pos in [0, n / 2, n + 7] {
            assert_eq!(b.record(pos).payload, if pos >= 1.min(n) && pos < 1.min(n) + 8 { 0 } else if pos == 0 { 0 } else { (pos - 8) as u32 }, "n {n} pos {pos}");
        }
    }
}
