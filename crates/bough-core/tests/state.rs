//! Record state planes against a scalar model: a `Vec` of records, each with its sixteen bits and the
//! sixteen its consumer last saw. Bits must follow their records through every page operation (shift,
//! split, overflow, trim, merge, free, move), range writes must be the same as per-record writes,
//! summaries and scans must agree with a walk, and `flips` must name exactly the records whose bit differs
//! from the seen one.
use bough_core::bough::RecordInit;
use bough_core::rng::Rng;
use bough_core::state::STATE_PLANES;
use bough_core::{Bough, NRect};

#[derive(Clone, Copy, PartialEq, Debug)]
struct M {
    payload: u32,
    bits: u16,
    seen: u16,
}

struct T {
    b: Bough,
    m: Vec<M>,
    rng: Rng,
    next: u32,
    hs: Vec<(bough_core::Handle, u32)>,
}

impl T {
    fn new(seed: u64) -> T {
        let mut t = T { b: Bough::new(512, 512), m: Vec::new(), rng: Rng::new(seed), next: 1, hs: Vec::new() };
        t.b.insert_run(0, &[RecordInit::new(0, NRect::EMPTY, 1, 0)]);
        t.m.push(M { payload: 0, bits: 0, seen: 0 });
        t
    }
    fn below(&mut self, n: usize) -> usize {
        self.rng.below(n as u64) as usize
    }
    fn run(&mut self) -> (usize, usize) {
        let n = self.m.len();
        let start = self.below(n);
        let len = match self.below(5) {
            0 => 1,
            1 => 1 + self.below(9),
            2 => 64 * (1 + self.below(3)),
            _ => 1 + self.below(n - start),
        };
        (start, len.min(n - start))
    }
    fn step(&mut self) {
        let n = self.m.len();
        let f = self.below(STATE_PLANES) as u8;
        match self.below(100) {
            0..=19 if n < 3000 => {
                let pos = 1 + self.below(n);
                let k = [1, 3, 8, 30, 64, 65, 200][self.below(7)];
                let recs: Vec<_> = (0..k).map(|i| RecordInit::new(1, NRect::new(1, 1, 9, 9), 1, self.next + i as u32)).collect();
                self.b.insert_run(pos, &recs);
                self.m.splice(pos..pos, (0..k as u32).map(|i| M { payload: self.next + i, bits: 0, seen: 0 }));
                self.next += k as u32;
            }
            0..=29 if n > 1 => {
                let (start, len) = self.run();
                let start = start.max(1);
                let len = len.min(n - start);
                if len > 0 {
                    self.b.remove_run(start, len);
                    let gone: Vec<u32> = self.m.drain(start..start + len).map(|m| m.payload).collect();
                    self.hs.retain(|h| !gone.contains(&h.1));
                }
            }
            30..=39 if n > 2 => {
                let (start, len) = self.run();
                let start = start.max(1);
                let len = len.min(n - start);
                if len > 0 {
                    let to = 1 + self.below(n - len);
                    self.b.move_run(start, len, to.min(n - len));
                    let run: Vec<M> = self.m.drain(start..start + len).collect();
                    let to = to.min(n - len);
                    self.m.splice(to..to, run);
                }
            }
            40..=64 => {
                let (start, len) = self.run();
                let on = self.below(3) != 0;
                self.b.set_state_run(start, len, f, on);
                for m in &mut self.m[start..start + len] {
                    m.bits = if on { m.bits | 1 << f } else { m.bits & !(1 << f) };
                }
            }
            65..=74 => {
                let pos = self.below(n);
                let on = self.below(2) == 0;
                self.b.set_state(pos, f, on);
                let m = &mut self.m[pos];
                m.bits = if on { m.bits | 1 << f } else { m.bits & !(1 << f) };
            }
            75..=79 if n > 1 => {
                // a handle on a record, then a write through it
                let pos = 1 + self.below(n - 1);
                if self.b.handle_at(pos).is_none() {
                    let h = self.b.attach_handle(pos);
                    self.hs.push((h, self.m[pos].payload));
                }
                let i = self.below(self.hs.len().max(1));
                if let Some(&(h, pay)) = self.hs.get(i) {
                    let on = self.below(2) == 0;
                    assert!(self.b.set_state_handle(h, f, on));
                    let m = self.m.iter_mut().find(|m| m.payload == pay).unwrap();
                    m.bits = if on { m.bits | 1 << f } else { m.bits & !(1 << f) };
                    assert_eq!(self.b.state_handle(h, f), Some(on));
                }
            }
            80..=91 => {
                // the consumer of plane f: read the flips, then acknowledge them
                let mut got = Vec::new();
                self.b.flips(f, |pos, now| got.push((pos, now)));
                got.sort();
                let want: Vec<(usize, bool)> = self.m.iter().enumerate().filter(|(_, m)| (m.bits ^ m.seen) >> f & 1 != 0).map(|(i, m)| (i, m.bits >> f & 1 != 0)).collect();
                assert_eq!(got, want, "flips of plane {f}");
                self.b.commit_state(f);
                for m in &mut self.m {
                    m.seen = (m.seen & !(1 << f)) | (m.bits & 1 << f);
                }
                let mut none = 0;
                self.b.flips(f, |_, _| none += 1);
                assert_eq!(none, 0, "flips after commit");
            }
            92..=95 if n > 1 => {
                // cut pages at both ends of a run (what a transform or far content does): the cut keeps
                // each record's state on its side
                let (start, len) = self.run();
                if len > 0 {
                    self.b.set_run_xform(start, len, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0], [0.0, 0.0]);
                }
            }
            _ => {}
        }
    }
    fn verify(&mut self, what: &str) {
        self.b.check_invariants();
        let n = self.m.len();
        assert_eq!(self.b.len(), n);
        let recs = self.b.records();
        for (pos, m) in self.m.iter().enumerate() {
            assert_eq!(recs[pos].payload, m.payload, "{what}");
        }
        // every bit of a sample of records, and of every record on a few planes
        for _ in 0..40 {
            let (pos, f) = (self.below(n), self.below(STATE_PLANES) as u8);
            assert_eq!(self.b.state(pos, f), self.m[pos].bits >> f & 1 != 0, "{what}: state({pos}, {f})");
        }
        for f in [0u8, 3, 15, self.below(STATE_PLANES) as u8] {
            let all: Vec<bool> = (0..n).map(|pos| self.b.state(pos, f)).collect();
            let want: Vec<bool> = self.m.iter().map(|m| m.bits >> f & 1 != 0).collect();
            assert_eq!(all, want, "{what}: plane {f}");
            for _ in 0..6 {
                let (start, len) = self.run();
                let set: Vec<usize> = (start..start + len).filter(|&i| want[i]).collect();
                assert_eq!(self.b.any_state(start, len, f), !set.is_empty(), "{what}: any_state({start}, {len}, {f})");
                assert_eq!(self.b.count_state(start, len, f), set.len(), "{what}: count_state");
                assert_eq!(self.b.next_state(start, f), (start..n).find(|&i| want[i]), "{what}: next_state({start}, {f})");
            }
        }
        // flips never lie between consumer turns either (read-only)
        let f = self.below(STATE_PLANES) as u8;
        let mut got = Vec::new();
        self.b.flips(f, |pos, now| got.push((pos, now)));
        got.sort();
        let want: Vec<(usize, bool)> = self.m.iter().enumerate().filter(|(_, m)| (m.bits ^ m.seen) >> f & 1 != 0).map(|(i, m)| (i, m.bits >> f & 1 != 0)).collect();
        assert_eq!(got, want, "{what}: flips of plane {f}");
    }
}

#[test]
fn state_planes_equal_the_scalar_model() {
    let seeds = if cfg!(debug_assertions) { 4 } else { 40 };
    for seed in 0..seeds {
        let mut t = T::new(0x57A7E + seed);
        for step in 0..500 {
            t.step();
            t.verify(&format!("seed {seed} step {step}"));
        }
    }
}

/// A range write costs one word per page: the 4,000 records of a subtree are selected and cleared, and
/// the consumer sees each record exactly once per change.
#[test]
fn a_range_is_a_mask_and_a_flip_is_seen_once() {
    let mut b = Bough::new(512, 512);
    let recs: Vec<_> = (0..5000).map(|i| RecordInit::new(if i == 0 { 0 } else { 1 }, NRect::new(1, 1, 9, 9), 1, i)).collect();
    b.insert_run(0, &recs);
    assert!(!b.any_state(0, 5000, 7));
    b.set_state_run(500, 4000, 7, true);
    assert_eq!(b.count_state(0, 5000, 7), 4000);
    assert!(b.any_state(4499, 1, 7) && !b.any_state(4500, 500, 7) && !b.any_state(0, 500, 7));
    assert_eq!(b.next_state(0, 7), Some(500));
    let mut seen = vec![0u8; 5000];
    b.flips(7, |pos, now| {
        assert!(now);
        seen[pos] += 1;
    });
    assert!(seen.iter().enumerate().all(|(i, &c)| c == ((500..4500).contains(&i)) as u8));
    b.commit_state(7);
    b.set_state_run(600, 10, 7, false);
    let mut off = Vec::new();
    b.flips(7, |pos, now| off.push((pos, now)));
    off.sort();
    assert_eq!(off, (600..610).map(|p| (p, false)).collect::<Vec<_>>());
    // setting a bit back to what the consumer saw is not a flip
    b.set_state_run(600, 10, 7, true);
    let mut none = 0;
    b.flips(7, |_, _| none += 1);
    assert_eq!(none, 0);
    b.check_invariants();
}
