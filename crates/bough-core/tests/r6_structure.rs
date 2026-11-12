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

/// Brute-force topmost record at a pixel.
fn hit_ref(b: &Bough, x: i32, y: i32) -> Option<usize> {
    let q = NRect::new(x, y, x + 1, y + 1);
    b.records().iter().rposition(|r| r.ink.intersects(q))
}

/// Chunks are runs of directory entries that split and merge (fix b). Trees of 70..400 pages (so
/// several chunks) take random page-level edits: runs of 1..300 records inserted and removed anywhere,
/// which split pages inside a chunk, overflow chunks, empty whole chunks and leave small ones to merge.
/// `check_invariants` holds every chunk to `1..=CHUNK` entries, the cover of the directory, and its
/// summary to the exact union; hits through the chunk level are compared with a scan of every record.
#[test]
fn chunks_follow_page_splits_and_removals() {
    let mut rng = bough_core::rng::Rng::new(0xC4);
    for round in 0..12 {
        let n = 4500 + round * 2000;
        let mut b = Bough::new(3200, 3200);
        let rec = |rng: &mut bough_core::rng::Rng, i: u32| {
            let (x, y) = (rng.below(3100) as i32, rng.below(3100) as i32);
            RecordInit::new(1, NRect::new(x, y, x + 1 + rng.below(60) as i32, y + 1 + rng.below(60) as i32), 1, i)
        };
        let mut rs = vec![RecordInit::new(0, NRect::EMPTY, 1, 0)];
        for i in 1..n {
            rs.push(rec(&mut rng, i as u32));
        }
        b.insert_run(0, &rs);
        b.check_invariants();
        for step in 0..400 {
            let len = b.len();
            let k = match rng.below(4) {
                0 => 1 + rng.below(8) as usize,
                1 => 1 + rng.below(70) as usize,
                2 => 64 * (1 + rng.below(4) as usize),
                _ => 1 + rng.below(300) as usize,
            };
            // removals cluster at a few places so chunks drain; a round in three shrinks overall
            let remove = len > 2000 && rng.below(100) < if round % 3 == 0 { 62 } else { 48 };
            if remove {
                let at = 1 + (rng.below(8) as usize * (len / 8) + rng.below(200) as usize).min(len - 1 - k.min(len - 1));
                b.remove_run(at.min(len - k.min(len - 1)), k.min(len - 1));
            } else {
                let at = 1 + rng.below(len as u64) as usize;
                let new: Vec<_> = (0..k).map(|i| rec(&mut rng, (step * 1000 + i) as u32)).collect();
                b.insert_run(at.min(len), &new);
            }
            b.check_invariants();
            for _ in 0..3 {
                let (x, y) = (rng.below(3200) as i32, rng.below(3200) as i32);
                assert_eq!(b.hit(x, y), hit_ref(&b, x, y), "round {round} step {step} hit ({x}, {y})");
            }
        }
        // drain to nothing through the middle, then refill: chunks empty and merge all the way down
        while b.len() > 1 {
            let len = b.len();
            let k = (1 + rng.below(500) as usize).min(len - 1);
            b.remove_run(1 + rng.below((len - k) as u64) as usize, k);
            b.check_invariants();
        }
        b.insert_run(1, &rs[1..]);
        b.check_invariants();
    }
}

/// A full chunk next to a one-page chunk holds 65 pages together: they must stay two chunks (a merge
/// test that is off by one makes a 65-entry chunk, which the 64-lane summary scan cannot hold).
#[test]
fn a_full_chunk_and_a_single_page_do_not_merge() {
    let rs = recs(128 * 64);
    let mut b = Bough::new(3200, 3200);
    b.insert_run(0, &rs);
    assert_eq!(b.num_pages(), 128);
    b.remove_run(65 * 64, 63 * 64); // pages 65..128 go whole: chunk 1 keeps page 64 alone
    assert_eq!(b.num_pages(), 65);
    b.check_invariants();
    b.remove_run(64 * 64, 64); // and then nothing: one full chunk
    b.check_invariants();
}

/// `Page::dir` is a hint (fix c): handles on every page must keep resolving to their record's position
/// through any number of page inserts and removals before them, in both directions, including hints
/// stale by hundreds of entries, and batched handle writes must settle the right pages.
#[test]
fn handles_resolve_through_stale_directory_hints() {
    let mut rng = bough_core::rng::Rng::new(0xD1);
    let n = 20_000;
    let mut b = Bough::new(3200, 3200);
    b.insert_run(0, &recs(n));
    // model: the payload at each position; handles are on records chosen by payload
    let mut model: Vec<u32> = (0..n as u32).collect();
    let mut hs: Vec<(bough_core::Handle, u32)> = Vec::new();
    for pos in (1..n).step_by(37) {
        hs.push((b.attach_handle(pos), pos as u32));
    }
    let mut next = n as u32;
    for step in 0..300 {
        let len = model.len();
        // most edits near the front, so every later page's hint goes stale by the page count changed
        let at = 1 + if rng.below(3) > 0 { rng.below(200) as usize } else { rng.below(len as u64 - 1) as usize };
        let k = 1 + rng.below(if step % 7 == 0 { 3000 } else { 200 }) as usize;
        if rng.below(2) == 0 && len > 6000 {
            let (at, k) = (at.min(len - 1), k.min(len - 1 - at.min(len - 2)).max(1));
            let gone: Vec<u32> = model.drain(at..at + k).collect();
            b.remove_run(at, k);
            hs.retain(|(h, pay)| {
                let dead = gone.contains(pay);
                assert_eq!(b.handle_live(*h), !dead);
                !dead
            });
        } else {
            let new: Vec<_> = (0..k).map(|i| RecordInit::new(1, NRect::new(2, 2, 14, 14), 1, next + i as u32)).collect();
            b.insert_run(at, &new);
            model.splice(at..at, (0..k as u32).map(|i| next + i));
            next += k as u32;
        }
        // a third of the handles each step: the others' pages keep older hints
        for (i, (h, pay)) in hs.iter().enumerate() {
            if (i + step) % 3 == 0 {
                let pos = b.pos_of(*h).expect("live handle");
                assert_eq!(model[pos], *pay, "step {step}: handle of payload {pay} resolved to {pos}");
                assert_eq!(b.payload_at(pos), *pay);
            }
        }
        if step % 10 == 0 {
            let writes: Vec<_> = hs.iter().step_by(5).map(|(h, _)| (*h, NRect::new(step as i32, step as i32, step as i32 + 9, step as i32 + 9))).collect();
            b.set_inks_handles(&writes, true);
            for (h, _) in hs.iter().step_by(5) {
                assert_eq!(b.ink_at(b.pos_of(*h).unwrap()), NRect::new(step as i32, step as i32, step as i32 + 9, step as i32 + 9));
            }
            b.check_invariants();
        }
    }
    b.check_invariants();
}
