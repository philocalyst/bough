//! `BlockedPrefix` against the plain prefix sum, and the contract of `invalidate_from(d)`: it may rely
//! on `start[0..=d]` and must recompute everything after (round 6, fix a).
use bough_core::prefix::BlockedPrefix;
use bough_core::rng::Rng;

fn starts(lens: &[u32]) -> Vec<u32> {
    let mut v = vec![0];
    for &l in lens {
        v.push(v.last().unwrap() + l);
    }
    v
}

/// Random page inserts, removals and length changes at random directory indices, each followed by
/// `invalidate_from(first changed index)`; every start and every `locate` is checked. Sizes cross
/// several 32-page block boundaries in both directions.
#[test]
fn invalidate_from_matches_the_plain_prefix_sum() {
    let mut rng = Rng::new(0x51ab);
    for round in 0..200 {
        let mut p = BlockedPrefix::new();
        let mut lens: Vec<u32> = Vec::new();
        for step in 0..300 {
            let n = lens.len();
            let grow = n < 2 || rng.below(100) < if round % 2 == 0 { 60 } else { 45 };
            let d = match rng.below(4) {
                _ if n == 0 => 0,
                0 if grow => {
                    // insert 1..=3 pages after index d (a split), or at 0
                    let d = rng.below(n as u64 + 1) as usize;
                    for _ in 0..1 + rng.below(3) {
                        lens.insert(d, 1 + rng.below(64) as u32);
                        p.grow();
                    }
                    d.saturating_sub(1)
                }
                0 | 1 => {
                    // remove 1..=3 pages at d
                    let d = rng.below(n as u64) as usize;
                    let k = (1 + rng.below(3) as usize).min(n - d);
                    lens.drain(d..d + k);
                    for _ in 0..k {
                        p.shrink();
                    }
                    d.min(lens.len())
                }
                _ => {
                    // lengths of a few pages from d change
                    let d = rng.below(n as u64) as usize;
                    for l in lens[d..].iter_mut().take(1 + rng.below(40) as usize) {
                        *l = 1 + rng.below(64) as u32;
                    }
                    d
                }
            };
            p.invalidate_from(d, &lens);
            let want = starts(&lens);
            assert_eq!(p.snapshot(&lens), want, "round {round} step {step} from {d}");
            if !lens.is_empty() {
                for _ in 0..4 {
                    let pos = rng.below(*want.last().unwrap() as u64) as usize;
                    let (pg, base) = p.locate(pos, &lens);
                    assert!(base <= pos && pos < base + lens[pg] as usize && base == want[pg] as usize);
                }
            }
        }
    }
}

/// The index is honoured: after `invalidate_from(d, lens)`, `start[0..=d]` is what it was (not read
/// from `lens` again) and `start[d+1..]` follows `lens[d..]`. Lengths before `d` are deliberately
/// falsified in the slice passed: an implementation that rebuilds from page 0 (the round-5 one) takes
/// them and fails; one that skips the suffix fails the second half.
#[test]
fn invalidate_from_honours_its_start_index() {
    for n in [1usize, 5, 31, 32, 33, 64, 65, 100, 1000] {
        for d in [0, 1, 30, 31, 32, 33, 63, 64, n / 2, n - 1, n] {
            if d > n {
                continue;
            }
            let lens: Vec<u32> = (0..n).map(|i| 1 + (i as u32 * 7) % 64).collect();
            let mut p = BlockedPrefix::new();
            for _ in 0..n {
                p.grow();
            }
            p.invalidate_from(0, &lens);
            let before = p.snapshot(&lens);
            let mut changed = lens.clone();
            for (i, l) in changed.iter_mut().enumerate() {
                // before d: lies the prefix must not read; from d: the real change
                *l = if i < d { 99 } else { 1 + (i as u32 * 11) % 64 };
            }
            p.invalidate_from(d, &changed);
            let after = p.snapshot(&changed);
            assert_eq!(after[..=d], before[..=d], "n {n} d {d}: the prefix before d was recomputed");
            let mut acc = before[d];
            for i in d..n {
                acc += changed[i];
                assert_eq!(after[i + 1], acc, "n {n} d {d}: start[{}] not recomputed", i + 1);
            }
        }
    }
}
