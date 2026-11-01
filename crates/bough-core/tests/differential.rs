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
    ww: i32,
    wh: i32,
    /// Largest translation step (page offsets grow with it).
    tmax: i32,
    /// Replica kept by `freeze_into` (epoch snapshots).
    snap: Bough,
}

fn rand_ink(rng: &mut Rng) -> NRect {
    rand_ink_in(rng, 256, 192)
}

/// Mostly small rects scattered over (and a little past) a `ww` x `wh` window, some large ones.
fn rand_ink_in(rng: &mut Rng, ww: i32, wh: i32) -> NRect {
    let x0 = rng.range(-40, ww + 40);
    let y0 = rng.range(-40, wh + 40);
    let (w, h) = if rng.chance(1, 5) { (rng.range(-4, ww / 2), rng.range(-4, wh / 2)) } else { (rng.range(-4, 100), rng.range(-4, 100)) };
    NRect::new(x0, y0, x0 + w, y0 + h)
}

impl H {
    fn new(seed: u64, w: i32, h: i32) -> H {
        let mut b = Bough::new(w, h);
        let mut o = Oracle::new(w, h);
        // kinds 0 and 2 are uniform content (xor damage), 1, 3, 4 are not
        for k in [0u8, 2] {
            b.set_kind_uniform(k, true);
            o.set_kind_uniform(k, true);
        }
        // three clips (ids 1..=3) that records may reference
        let mut crng = Rng::new(seed ^ 0xC11F);
        for _ in 0..3 {
            let r = rand_ink_in(&mut crng, w, h);
            assert_eq!(b.new_clip(r), o.new_clip(r));
        }
        H {
            b,
            o,
            rng: Rng::new(seed),
            payload: 1,
            latest: HashMap::new(),
            old: Vec::new(),
            profile: seed % 4,
            max_n: 700,
            ww: w,
            wh: h,
            tmax: 12,
            snap: Bough::new(w, h),
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
            let ink = if self.rng.chance(1, 6) { NRect::EMPTY } else { rand_ink_in(&mut self.rng, self.ww, self.wh) };
            let mut r = RecordInit::new(depth, ink, self.rng.below(5) as u8, self.payload);
            if self.rng.chance(1, 4) {
                r = r.with_clip(self.rng.range(1, 3) as u16);
            }
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
        let (dx, dy) = (self.rng.range(-self.tmax, self.tmax), self.rng.range(-self.tmax, self.tmax));
        let ok = self.o.recs[start..start + len].iter().all(|r| {
            r.ink.is_empty() || {
                let q = NR::from_nrect(r.ink);
                q.x0 + dx > -20000 && q.x1 + dx < 20000 && q.y0 + dy > -20000 && q.y1 + dy < 20000
            }
        });
        if !ok {
            return;
        }
        self.o.translate_run(start, len, dx, dy);
        self.b.translate_run(start, len, dx, dy);
    }

    fn do_scroll(&mut self) {
        let n = self.o.recs.len();
        if n == 0 {
            return;
        }
        let i = self.rng.usize_below(n);
        let len = self.o.subtree_end(i) - i;
        let dy = self.rng.range(-self.tmax, self.tmax);
        let ok = self.o.recs[i..i + len].iter().all(|r| {
            r.ink.is_empty() || {
                let q = NR::from_nrect(r.ink);
                q.y0 + dy > -20000 && q.y1 + dy < 20000
            }
        });
        if !ok {
            return;
        }
        let clip = rand_ink_in(&mut self.rng, self.ww, self.wh);
        self.o.scroll_run(i, len, 0, dy, clip);
        self.b.scroll_run(i, len, 0, dy, clip);
    }

    fn do_set_ink(&mut self) {
        let n = self.o.recs.len();
        if n == 0 {
            return;
        }
        let p = self.rng.usize_below(n);
        let ink = if self.rng.chance(1, 6) { NRect::EMPTY } else { rand_ink_in(&mut self.rng, self.ww, self.wh) };
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

    /// Batched ink writes (sorted positions, unsorted positions, or handles) vs sequential oracle writes.
    fn do_set_inks(&mut self) {
        let n = self.o.recs.len();
        if n == 0 {
            return;
        }
        let k = self.rng.usize_below(24) + 1;
        let mut ws: Vec<(usize, NRect)> = (0..k)
            .map(|_| {
                let p = self.rng.usize_below(n);
                let ink = if self.rng.chance(1, 6) { NRect::EMPTY } else { rand_ink_in(&mut self.rng, self.ww, self.wh) };
                (p, ink)
            })
            .collect();
        let damage = !self.rng.chance(1, 4);
        match self.rng.below(3) {
            0 => ws.sort_by_key(|w| w.0),
            1 => {}
            _ => {
                let hs: Vec<(Handle, NRect)> = ws.iter().filter_map(|&(p, ink)| self.handle_at(p).map(|h| (h, ink))).collect();
                for &(h, ink) in &hs {
                    let p = self.b.pos_of(h).unwrap();
                    self.o_set_ink(p, ink, damage);
                }
                self.b.set_inks_handles(&hs, damage);
                return;
            }
        }
        for &(p, ink) in &ws {
            self.o_set_ink(p, ink, damage);
        }
        self.b.set_inks(&ws, damage);
    }

    fn o_set_ink(&mut self, p: usize, ink: NRect, damage: bool) {
        if damage {
            self.o.set_ink(p, ink);
        } else {
            let n = NR::from_nrect(ink);
            self.o.recs[p].ink = if n.empty() { NRect::EMPTY } else { ink };
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
        if std::env::var("DIFF_TRACE").is_ok() {
            eprintln!("op {r} n={}", self.o.recs.len());
        }
        match r {
            0..=22 if !big => self.do_insert(),
            0..=22 => self.do_remove(),
            23..=36 => self.do_remove(),
            37..=47 => self.do_move(),
            48..=56 => self.do_translate(),
            57 => {
                let id = self.rng.range(1, 3) as u16;
                let r = rand_ink_in(&mut self.rng, self.ww, self.wh);
                self.o.set_clip_rect(id, r);
                self.b.set_clip_rect(id, r);
            }
            58 => {
                if let Some((i, len)) = self.pick_subtree() {
                    let id = self.rng.range(0, 3) as u16;
                    self.o.set_clip_run(i, len, id);
                    self.b.set_clip_run(i, len, id);
                }
            }
            59..=60 => self.do_scroll(),
            61..=66 => self.do_set_ink(),
            67..=70 => self.do_set_inks(),
            71..=84 => self.do_touch(),
            85..=94 => {
                self.o.next_epoch();
                self.b.next_epoch();
            }
            _ => {
                // burst: more marks in one epoch than the pending buffer holds (forces mid-epoch flushes)
                for _ in 0..300 {
                    self.do_touch();
                }
            }
        }
    }

    fn check_plane(&self, p: &Plane, o: &OPlane, what: &str) {
        for ty in 0..128 {
            assert_eq!(p.rows[ty], o.row(ty), "{what}: row {ty}");
        }
    }

    /// Pixel bbox of the oracle plane's set tiles, clipped to the window.
    fn obbox(&self, o: &OPlane) -> NRect {
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for ty in 0..o.th {
            for tx in 0..o.tw {
                if o.get(tx, ty) {
                    x0 = x0.min(tx as i32 * 32);
                    y0 = y0.min(ty as i32 * 32);
                    x1 = x1.max(tx as i32 * 32 + 32);
                    y1 = y1.max(ty as i32 * 32 + 32);
                }
            }
        }
        if x0 == i32::MAX {
            return NRect::EMPTY;
        }
        let w = self.o.window;
        NRect::new(x0, y0, x1.min(w.x1), y1.min(w.y1))
    }

    fn verify(&mut self) {
        self.b.check_invariants();
        assert_eq!(self.b.records(), self.o.recs, "records diverged");
        assert_eq!(self.b.len(), self.o.recs.len());
        assert_eq!(self.b.epoch(), self.o.epoch);
        for age in 1..=RING as u32 {
            let mut p = Plane::EMPTY;
            let bb = self.b.repair_into(age, &mut p);
            let o = self.o.repair(age);
            self.check_plane(&p, &o, &format!("repair({age})"));
            assert_eq!(bb, self.obbox(&o), "repair({age}) bbox");
            // the frame cull: every record touching a damaged tile, paint order
            let mut out = Vec::new();
            self.b.cull_tiles(&p, bb, &mut out);
            assert_eq!(out, self.o.cull_tiles(&o), "cull_tiles age {age}");
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
            let mut v = Vec::new();
            self.b.children_into(p, &mut v);
            assert_eq!(v, self.o.children(p), "children_into({p})");
            self.b.ancestors(p, &mut v);
            let mut want = Vec::new();
            let mut q = p;
            while let Some(a) = self.o.parent(q) {
                want.push(a);
                q = a;
            }
            assert_eq!(v, want, "ancestors({p})");
        }
        for _ in 0..8 {
            let (x, y) = if self.rng.chance(1, 2) {
                let q = NR::from_nrect(self.o.recs[self.rng.usize_below(n)].ink);
                if q.empty() { (0, 0) } else { (q.x0 + self.rng.range(-1, 1), q.y0 + self.rng.range(-1, 1)) }
            } else {
                (self.rng.range(-50, self.ww + 50), self.rng.range(-50, self.wh + 50))
            };
            assert_eq!(self.b.hit(x, y), self.o.hit(x, y), "hit({x},{y})");
        }
        let mut out = Vec::new();
        for _ in 0..3 {
            let bbox = rand_ink_in(&mut self.rng, self.ww, self.wh);
            self.b.cull(bbox, None, &mut out);
            assert_eq!(out, self.o.cull(bbox, None), "cull {bbox:?}");
            let age = self.rng.range(1, RING as i32) as u32;
            let (bp, op) = (self.b.repair(age), self.o.repair(age));
            self.b.cull(bbox, Some(&bp), &mut out);
            assert_eq!(out, self.o.cull(bbox, Some(&op)), "cull tiles {bbox:?} age {age}");
        }
        // epoch snapshot: the replica answers like the live structure
        if self.rng.chance(1, 3) {
            self.b.freeze_into(&mut self.snap);
            assert_eq!(self.snap.records(), self.b.records(), "snapshot records");
            assert_eq!(self.snap.len(), self.b.len());
            for age in 1..=RING as u32 {
                let (mut p1, mut p2) = (Plane::EMPTY, Plane::EMPTY);
                let (b1, b2) = (self.b.repair_into(age, &mut p1), self.snap.repair_into(age, &mut p2));
                assert_eq!(b1, b2);
                let (mut o1, mut o2) = (Vec::new(), Vec::new());
                self.b.cull_tiles(&p1, b1, &mut o1);
                self.snap.cull_tiles(&p2, b2, &mut o2);
                assert_eq!(o1, o2, "snapshot cull age {age}");
            }
            for _ in 0..4 {
                let (x, y) = (self.rng.range(-50, self.ww + 50), self.rng.range(-50, self.wh + 50));
                assert_eq!(self.snap.hit(x, y), self.b.hit(x, y));
            }
        }
        for _ in 0..2 {
            let e = self.rng.range(self.o.epoch as i32 - 6, self.o.epoch as i32).max(0) as u32;
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
    // Window whose tile row needs the high half of the u128 (tx >= 64): 2600 x 800, spans straddling word 64.
    for seed in 0..scale(150, 20) as u64 {
        run_seq(50_000 + seed, 60, 2600, 800);
    }
}

#[test]
fn differential_full_window() {
    // The maximum 4096 x 4096 window (128 x 128 tiles): phantom-free row words.
    for seed in 0..scale(12, 2) as u64 {
        run_seq(90_000 + seed, 60, 4096, 4096);
    }
}

#[test]
fn differential_far_translates() {
    // Big translation steps: page offsets reach thousands of px, so relative storage and the
    // checked conversions (normalise on overflow) are exercised.
    for seed in 0..scale(400, 40) as u64 {
        let mut t = H::new(70_000 + seed, 256, 192);
        t.tmax = 9000;
        for _ in 0..60 {
            t.step();
            t.verify();
        }
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
