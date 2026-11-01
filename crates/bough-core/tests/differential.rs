//! Randomised differential tests: Bough vs the dumb Vec oracle. After every op, the full record
//! sequence, every damage plane in the ring, handles, and sampled queries must agree.

use bough_core::damage::RING;
use bough_core::oracle::{NR, OPlane, Oracle};
use bough_core::page::RecordInit;
use bough_core::rng::Rng;
use bough_core::{Bough, Handle, NRect, Plane};
use std::collections::HashMap;

struct H {
    b: Bough,
    o: Oracle,
    rng: Rng,
    payload: u32,
    latest: HashMap<u32, Handle>,
    old: Vec<Handle>,
    profile: u64,
    max_n: usize,
}

fn rand_ink(rng: &mut Rng) -> NRect {
    let x0 = rng.range(-40, 300);
    let y0 = rng.range(-40, 240);
    let (w, h) = (rng.range(-4, 100), rng.range(-4, 100));
    NRect::new(x0, y0, x0 + w, y0 + h)
}

impl H {
    fn new(seed: u64, w: i32, h: i32) -> H {
        H {
            b: Bough::new(w, h),
            o: Oracle::new(w, h),
            rng: Rng::new(seed),
            payload: 1,
            latest: HashMap::new(),
            old: Vec::new(),
            profile: seed % 4,
            max_n: 700,
        }
    }

    fn gen_subtree(&mut self, root_depth: u16, size: usize) -> Vec<RecordInit> {
        let mut v: Vec<RecordInit> = Vec::with_capacity(size);
        for i in 0..size {
            let depth = if i == 0 {
                root_depth
            } else {
                let prev = v[i - 1].depth;
                if self.rng.chance(1, 2) { prev + 1 } else { self.rng.range(root_depth as i32 + 1, prev as i32 + 1) as u16 }
            };
            let ink = if self.rng.chance(1, 6) { NRect::EMPTY } else { rand_ink(&mut self.rng) };
            let mut r = RecordInit::new(depth, ink, self.rng.below(5) as u8, self.payload);
            self.payload += 1;
            if self.rng.chance(1, 7) {
                let h = self.b.new_handle();
                self.latest.insert(h.idx, h);
                r.handle = h.idx;
            }
            v.push(r);
        }
        v
    }

    /// (v, k) -> (insert position, depth)
    fn pick_insert_point(&mut self) -> (usize, u16) {
        let n = self.o.recs.len();
        if n == 0 {
            return (0, 0);
        }
        let vsel: Option<usize> = match self.profile {
            1 | 2 => Some(0), // append / scroll: always under the root
            _ => {
                if self.rng.chance(1, 8) { None } else { Some(self.rng.usize_below(n)) }
            }
        };
        let kids = match vsel {
            Some(v) => self.o.children(v),
            None => (0..n).filter(|&j| self.o.recs[j].depth == 0).collect(),
        };
        let c = kids.len();
        let k = match self.profile {
            1 => c,
            2 => 0,
            _ => self.rng.usize_below(c + 1),
        };
        let p = if k < c {
            kids[k]
        } else {
            match vsel {
                Some(v) => self.o.subtree_end(v),
                None => n,
            }
        };
        (p, vsel.map(|v| self.o.recs[v].depth + 1).unwrap_or(0))
    }

    fn do_insert(&mut self) {
        let size = match self.rng.below(10) {
            0 => self.rng.usize_below(260) + 1,
            1..=3 => self.rng.usize_below(24) + 1,
            _ => self.rng.usize_below(8) + 1,
        };
        let (p, d) = self.pick_insert_point();
        let recs = self.gen_subtree(d, size);
        self.o.insert_run(p, &recs);
        self.b.insert_run(p, &recs);
    }

    fn pick_subtree(&mut self) -> Option<(usize, usize)> {
        let n = self.o.recs.len();
        if n == 0 {
            return None;
        }
        let i = match self.profile {
            2 => {
                // last child of root (scroll step removes at the bottom)
                if n == 1 { 0 } else { *self.o.children(0).last().unwrap_or(&0) }
            }
            _ => self.rng.usize_below(n),
        };
        let e = self.o.subtree_end(i);
        Some((i, e - i))
    }

    fn do_remove(&mut self) {
        let Some((i, len)) = self.pick_subtree() else { return };
        if i == 0 && self.o.recs.len() > 1 && self.profile != 0 && self.rng.chance(9, 10) {
            return; // keep the root in the structured profiles
        }
        let ps = self.b.page_starts();
        self.o.remove_run(i, len, &ps);
        self.b.remove_run(i, len);
    }

    fn do_move(&mut self) {
        let n = self.o.recs.len();
        if n < 2 {
            return;
        }
        let i = self.rng.usize_below(n);
        let e = self.o.subtree_end(i);
        let len = e - i;
        if len == n {
            return;
        }
        // destination parent outside the subtree (or top-level)
        let vsel = loop {
            if self.rng.chance(1, 10) {
                break None;
            }
            let v = self.rng.usize_below(n);
            if v < i || v >= e {
                break Some(v);
            }
        };
        let kids: Vec<usize> = match vsel {
            Some(v) => self.o.children(v),
            None => (0..n).filter(|&j| self.o.recs[j].depth == 0).collect(),
        };
        let k = self.rng.usize_below(kids.len() + 1);
        let p = if k < kids.len() { kids[k] } else { vsel.map(|v| self.o.subtree_end(v)).unwrap_or(n) };
        // p is in pre-removal coordinates and never strictly inside (i, e)
        let to = if p >= e { p - len } else { p };
        let nd = vsel.map(|v| self.o.recs[v].depth + 1).unwrap_or(0);
        // a top-level destination placed before the very first record would make a non-root first
        // record only if the moved root isn't depth 0: nd = 0 then, fine.
        let dd = nd as i32 - self.o.recs[i].depth as i32;
        let ps = self.b.page_starts();
        self.o.move_run(i, len, to, dd, &ps);
        if self.rng.chance(1, 2) && dd == 0 {
            self.b.move_run(i, len, to);
        } else {
            self.b.reparent_run(i, len, to, nd);
        }
    }

    fn do_translate(&mut self) {
        let n = self.o.recs.len();
        if n == 0 {
            return;
        }
        let (start, len) = if self.rng.chance(1, 2) {
            let i = self.rng.usize_below(n);
            (i, self.o.subtree_end(i) - i)
        } else {
            let i = self.rng.usize_below(n);
            (i, self.rng.usize_below(n - i + 1))
        };
        let (dx, dy) = (self.rng.range(-12, 12), self.rng.range(-12, 12));
        let ok = self.o.recs[start..start + len].iter().all(|r| {
            r.ink.is_empty() || {
                let q = NR::from_nrect(r.ink);
                q.x0 + dx > -20000 && q.x1 + dx < 20000 && q.y0 + dy > -20000 && q.y1 + dy < 20000
            }
        });
        if !ok {
            return;
        }
        let ps = self.b.page_starts();
        self.o.translate_run(start, len, dx, dy, &ps);
        self.b.translate_run(start, len, dx, dy);
    }

    fn do_set_ink(&mut self) {
        let n = self.o.recs.len();
        if n == 0 {
            return;
        }
        let p = self.rng.usize_below(n);
        let ink = if self.rng.chance(1, 6) { NRect::EMPTY } else { rand_ink(&mut self.rng) };
        if self.rng.chance(1, 4) {
            self.o.set_ink(p, ink);
            self.b.set_ink(p, ink);
        } else if let Some(h) = self.handle_at(p) {
            self.o.set_ink(p, ink);
            assert!(self.b.set_ink_handle(h, ink));
        } else {
            self.o.set_ink(p, ink);
            self.b.set_ink(p, ink);
        }
    }

    fn handle_at(&self, p: usize) -> Option<Handle> {
        let idx = self.o.recs[p].handle;
        if idx == 0 { None } else { self.latest.get(&idx).copied() }
    }

    fn do_touch(&mut self) {
        let n = self.o.recs.len();
        if n == 0 {
            return;
        }
        let p = self.rng.usize_below(n);
        self.o.touch(p);
        match self.handle_at(p) {
            Some(h) if self.rng.chance(1, 2) => assert!(self.b.touch_handle(h)),
            _ => self.b.touch(p),
        }
    }

    fn step(&mut self) {
        let big = self.o.recs.len() > self.max_n;
        let r = self.rng.below(100);
        match r {
            0..=22 if !big => self.do_insert(),
            0..=22 => self.do_remove(),
            23..=36 => self.do_remove(),
            37..=47 => self.do_move(),
            48..=60 => self.do_translate(),
            61..=70 => self.do_set_ink(),
            71..=84 => self.do_touch(),
            85..=94 => {
                self.o.next_epoch();
                self.b.next_epoch();
            }
            _ => {
                // stale-handle probe: release a handle through remove of its record happens above
            }
        }
    }

    fn check_plane(&self, p: &Plane, o: &OPlane, what: &str) {
        let ob = o.bbox;
        if ob.empty() {
            assert!(p.bbox.is_empty(), "{what}: bbox should be empty, got {:?}", p.bbox);
        } else {
            assert_eq!(NR::from_nrect(p.bbox), ob, "{what}: bbox");
        }
        for ty in 0..96 {
            assert_eq!(p.rows[ty], o.row(ty), "{what}: row {ty}");
        }
    }

    fn verify(&mut self) {
        self.b.check_invariants();
        assert_eq!(self.b.records(), self.o.recs, "records diverged");
        assert_eq!(self.b.len(), self.o.recs.len());
        assert_eq!(self.b.epoch(), self.o.epoch);
        for age in 1..=RING as u32 {
            let p = self.b.repair(age);
            let o = self.o.repair(age);
            self.check_plane(&p, &o, &format!("repair({age})"));
        }
        // handles
        for (&idx, &h) in &self.latest {
            assert_eq!(self.b.pos_of(h), self.o.pos_of_handle(idx), "handle {h:?}");
        }
        for h in &self.old {
            assert_eq!(self.b.pos_of(*h), None, "stale handle {h:?} must not resolve");
        }
        // retire handles whose records are gone so idx reuse is exercised
        let gone: Vec<u32> = self.latest.keys().copied().filter(|&i| self.o.pos_of_handle(i).is_none()).collect();
        for i in gone {
            let h = self.latest.remove(&i).unwrap();
            assert!(!self.b.handle_live(h));
            self.old.push(h);
        }
        if self.old.len() > 64 {
            self.old.drain(..32);
        }
        // queries
        let n = self.o.recs.len();
        if n == 0 {
            return;
        }
        for _ in 0..6 {
            let p = self.rng.usize_below(n);
            assert_eq!(self.b.subtree_end(p), self.o.subtree_end(p), "subtree_end({p})");
            assert_eq!(self.b.parent(p), self.o.parent(p), "parent({p})");
            assert_eq!(self.b.children(p).collect::<Vec<_>>(), self.o.children(p), "children({p})");
        }
        for _ in 0..8 {
            let (x, y) = if self.rng.chance(1, 2) {
                let q = NR::from_nrect(self.o.recs[self.rng.usize_below(n)].ink);
                if q.empty() { (0, 0) } else { (q.x0 + self.rng.range(-1, 1), q.y0 + self.rng.range(-1, 1)) }
            } else {
                (self.rng.range(-50, 320), self.rng.range(-50, 260))
            };
            assert_eq!(self.b.hit(x, y), self.o.hit(x, y), "hit({x},{y})");
        }
        let mut out = Vec::new();
        for _ in 0..3 {
            let bbox = rand_ink(&mut self.rng);
            self.b.cull(bbox, None, &mut out);
            assert_eq!(out, self.o.cull(bbox, None), "cull {bbox:?}");
            let age = self.rng.range(1, 8) as u32;
            let (bp, op) = (self.b.repair(age), self.o.repair(age));
            self.b.cull(bbox, Some(&bp), &mut out);
            assert_eq!(out, self.o.cull(bbox, Some(&op)), "cull tiles {bbox:?} age {age}");
            // cull against the repair region's own bbox
            if !bp.bbox.is_empty() {
                self.b.cull(bp.bbox, Some(&bp), &mut out);
                assert_eq!(out, self.o.cull(bp.bbox, Some(&op)));
            }
        }
        for _ in 0..2 {
            let e = self.rng.range(0, self.o.epoch as i32) as u32;
            self.b.changed_since(e, &mut out);
            assert_eq!(out, self.o.changed_since(e), "changed_since({e})");
        }
    }
}

fn run_seq(seed: u64, steps: usize, w: i32, h: i32) {
    let mut t = H::new(seed, w, h);
    for step in 0..steps {
        t.step();
        t.verify();
        let _ = step;
    }
}

fn scale(release: usize, debug: usize) -> usize {
    if cfg!(debug_assertions) { debug } else { release }
}

#[test]
fn differential_small_window() {
    for seed in 0..scale(3000, 400) as u64 {
        run_seq(seed + 1, 40, 256, 192);
    }
}

#[test]
fn differential_long_sequences() {
    for seed in 0..scale(60, 8) as u64 {
        run_seq(10_000 + seed, 600, 256, 192);
    }
}

#[test]
fn differential_wide_window() {
    // Window whose tile row needs the high half of the u128 (tx >= 64): 1400 x 800.
    for seed in 0..scale(150, 20) as u64 {
        run_seq(50_000 + seed, 60, 1400, 800);
    }
}

#[test]
fn differential_full_window() {
    // The maximum 2048 x 1536 window (128 x 96 tiles): phantom-free row words.
    for seed in 0..scale(12, 2) as u64 {
        run_seq(90_000 + seed, 60, 2048, 1536);
    }
}

#[test]
fn page_pool_is_type_stable() {
    let mut t = H::new(7, 256, 192);
    t.profile = 3;
    let mut seen: HashMap<u32, u32> = HashMap::new();
    for _ in 0..800 {
        t.step();
        for (id, ver) in t.b.page_versions() {
            let e = seen.entry(id).or_insert(ver);
            assert!(ver >= *e, "page version went backwards");
            *e = ver;
        }
        assert!(t.b.pool().total_pages() <= t.b.num_pages() + t.b.pool().free_pages());
    }
    t.verify();
}

#[test]
fn big_bulk_insert_and_remove() {
    // 100k-record mount, then peel it off in odd-sized runs; every step valid.
    let mut b = Bough::new(1024, 768);
    let mut rng = Rng::new(5);
    let n = 20_000;
    let recs: Vec<RecordInit> = (0..n)
        .map(|i| RecordInit::new(if i == 0 { 0 } else { 1 }, rand_ink(&mut rng), 0, i as u32))
        .collect();
    b.insert_run(0, &recs);
    b.check_invariants();
    assert_eq!(b.len(), n);
    assert_eq!(b.subtree_end(0), n);
    assert_eq!(b.parent(n - 1), Some(0));
    let mut left = n;
    let mut payloads: Vec<u32> = (0..n as u32).collect();
    while left > 1 {
        let len = (rng.usize_below(1500) + 1).min(left - 1);
        let start = 1 + rng.usize_below(left - len);
        b.remove_run(start, len);
        payloads.drain(start..start + len);
        left -= len;
        b.check_invariants();
        // position addressing across many directory pages (multi-block prefix)
        for _ in 0..50 {
            let p = rng.usize_below(left);
            assert_eq!(b.payload_at(p), payloads[p], "payload_at({p})");
            assert_eq!(b.subtree_end(p), if p == 0 { left } else { p + 1 });
            assert_eq!(b.parent(p), if p == 0 { None } else { Some(0) });
        }
    }
    assert_eq!(b.len(), 1);
}
