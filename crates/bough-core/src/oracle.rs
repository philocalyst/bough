//! The dumb oracle: a `Vec` of records in preorder with naive, obviously-correct implementations
//! of every query and of the damage bookkeeping (per-tile rectangle overlap tests).
//!
//! Damage contract (what both implementations mark, tile for tile, 32x32 tiles, clipped to the window):
//! * `touch`: the record's ink;
//! * `set_ink`: nothing if the ink is unchanged; for uniform kinds the tiles where `old` and `new`
//!   cover different pixels (exact symmetric difference); otherwise `old` and `new`;
//! * `insert_run`: the union of each group of 64 input records;
//! * `remove_run`, `move_run`: the union of the ink in each pre-op page chunk of the run
//!   (`page_starts` is an input);
//! * `translate_run`: the union of the whole run's old ink and the union of its new ink (two rects);
//!   `scroll_run`: the same two rects intersected with the scroll container's clip;
//! * `set_clip_rect`: tiles where the old and new clip rects cover different pixels;
//! * `set_clip_run`: each record's old and new visible ink.
//!
//! Visible ink = ink ∩ clips[record.clip] (clip 0 = none): every damage rule above that names a
//! record's ink, and every query (cull, hit), uses the visible ink.

use crate::damage::{MAX_TILES_X, MAX_TILES_Y, RING, TILE};
use crate::nrect::NRect;
use crate::page::{Record, RecordInit};

/// Normal-form half-open rect, i32.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NR {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl NR {
    pub fn empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    pub fn intersect(&self, o: &NR) -> NR {
        NR { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }
    /// Bounding union, ignoring empties.
    pub fn union(&self, o: &NR) -> NR {
        if self.empty() {
            *o
        } else if o.empty() {
            *self
        } else {
            NR { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
        }
    }
    /// Empty rects normalised to NONE (so pixel-set equality is `==`).
    pub fn norm(&self) -> NR {
        if self.empty() { NR::NONE } else { *self }
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.x0 <= x && x < self.x1 && self.y0 <= y && y < self.y1
    }
    pub fn translate(&self, dx: i32, dy: i32) -> NR {
        NR { x0: self.x0 + dx, y0: self.y0 + dy, x1: self.x1 + dx, y1: self.y1 + dy }
    }
    pub fn from_nrect(r: NRect) -> NR {
        let [x0, y0, x1, y1] = r.to_xyxy();
        NR { x0, y0, x1, y1 }
    }
    pub const NONE: NR = NR { x0: 0, y0: 0, x1: 0, y1: 0 };
}

#[derive(Clone)]
pub struct OPlane {
    /// Tile grid covering exactly the window (`tw` x `th`); tiles outside it can never be set.
    pub tw: usize,
    pub th: usize,
    pub tiles: Vec<bool>,
}
impl OPlane {
    pub fn new(tw: usize, th: usize) -> Self {
        OPlane { tw, th, tiles: vec![false; tw * th] }
    }
    pub fn or(&mut self, o: &OPlane) {
        for (a, b) in self.tiles.iter_mut().zip(&o.tiles) {
            *a |= *b;
        }
    }
    pub fn get(&self, tx: usize, ty: usize) -> bool {
        tx < self.tw && ty < self.th && self.tiles[ty * self.tw + tx]
    }
    pub fn any(&self) -> bool {
        self.tiles.iter().any(|&t| t)
    }
    /// Row `ty` as the u128 bit row a `Plane` uses.
    pub fn row(&self, ty: usize) -> u128 {
        let mut r = 0u128;
        if ty < self.th {
            for tx in 0..self.tw {
                if self.tiles[ty * self.tw + tx] {
                    r |= 1u128 << tx;
                }
            }
        }
        r
    }
    fn tile_rect(tx: usize, ty: usize) -> NR {
        NR { x0: tx as i32 * TILE, y0: ty as i32 * TILE, x1: (tx as i32 + 1) * TILE, y1: (ty as i32 + 1) * TILE }
    }
    /// Does a set tile overlap `r` (already clipped to the window)?
    pub fn touches(&self, r: NR) -> bool {
        if r.empty() {
            return false;
        }
        (0..self.th).any(|ty| (0..self.tw).any(|tx| self.get(tx, ty) && !Self::tile_rect(tx, ty).intersect(&r).empty()))
    }
}

pub struct Oracle {
    pub recs: Vec<Record>,
    pub epoch: u32,
    pub window: NR,
    pub planes: Vec<OPlane>, // ring of RING by epoch % RING
    pub uniform: [u64; 4],
    /// Clip rects by id; id 0 = unclipped.
    pub clips: Vec<NR>,
}

impl Oracle {
    pub fn new(w: i32, h: i32) -> Self {
        let (tw, th) = (((w + TILE - 1) / TILE) as usize, ((h + TILE - 1) / TILE) as usize);
        assert!(tw <= MAX_TILES_X && th <= MAX_TILES_Y);
        Oracle { recs: Vec::new(), epoch: 1, window: NR { x0: 0, y0: 0, x1: w, y1: h }, planes: vec![OPlane::new(tw, th); RING], uniform: [0; 4], clips: vec![NR { x0: i32::MIN / 2, y0: i32::MIN / 2, x1: i32::MAX / 2, y1: i32::MAX / 2 }] }
    }
    pub fn set_kind_uniform(&mut self, kind: u8, u: bool) {
        let (w, b) = ((kind >> 6) as usize, kind & 63);
        if u {
            self.uniform[w] |= 1 << b;
        } else {
            self.uniform[w] &= !(1 << b);
        }
    }
    fn is_uniform(&self, kind: u8) -> bool {
        (self.uniform[(kind >> 6) as usize] >> (kind & 63)) & 1 != 0
    }

    // ------------------------------------------------------------ damage
    fn cur(&mut self) -> &mut OPlane {
        let e = self.epoch as usize % RING;
        &mut self.planes[e]
    }
    pub fn add_damage(&mut self, r: NR) {
        let c = r.intersect(&self.window);
        if c.empty() {
            return;
        }
        let pl = self.cur();
        for ty in 0..pl.th {
            for tx in 0..pl.tw {
                if !OPlane::tile_rect(tx, ty).intersect(&c).empty() {
                    pl.tiles[ty * pl.tw + tx] = true;
                }
            }
        }
    }
    /// Tiles where `a` and `b` (clipped to the window) cover different pixels.
    pub fn add_damage_xor(&mut self, a: NR, b: NR) {
        let w = self.window;
        let pl = self.cur();
        for ty in 0..pl.th {
            for tx in 0..pl.tw {
                let t = OPlane::tile_rect(tx, ty).intersect(&w);
                if t.intersect(&a).norm() != t.intersect(&b).norm() {
                    pl.tiles[ty * pl.tw + tx] = true;
                }
            }
        }
    }
    pub fn next_epoch(&mut self) {
        self.epoch += 1;
        let (tw, th) = (self.planes[0].tw, self.planes[0].th);
        let e = self.epoch as usize % RING;
        self.planes[e] = OPlane::new(tw, th);
    }
    pub fn repair(&self, age: u32) -> OPlane {
        let mut out = OPlane::new(self.planes[0].tw, self.planes[0].th);
        for back in 0..age {
            out.or(&self.planes[self.epoch.wrapping_sub(back) as usize % RING]);
        }
        out
    }

    /// Unclipped ink.
    fn raw(&self, i: usize) -> NR {
        NR::from_nrect(self.recs[i].ink)
    }
    /// Visible ink: ink ∩ clip.
    fn ink(&self, i: usize) -> NR {
        self.raw(i).intersect(&self.clips[self.recs[i].clip as usize]).norm()
    }
    fn vis(&self, r: &RecordInit) -> NR {
        NR::from_nrect(r.ink).intersect(&self.clips[r.clip as usize]).norm()
    }
    pub fn new_clip(&mut self, r: NRect) -> u16 {
        self.clips.push(NR::from_nrect(r).norm());
        (self.clips.len() - 1) as u16
    }
    pub fn set_clip_rect(&mut self, id: u16, r: NRect) {
        let (old, new) = (self.clips[id as usize], NR::from_nrect(r).norm());
        self.add_damage_xor(old, new);
        self.clips[id as usize] = new;
    }
    pub fn set_clip_run(&mut self, start: usize, len: usize, id: u16) {
        for i in start..start + len {
            let old = self.ink(i);
            self.recs[i].clip = id;
            let new = self.ink(i);
            self.add_damage(old);
            self.add_damage(new);
        }
    }
    pub fn scroll_run(&mut self, start: usize, len: usize, dx: i32, dy: i32, clip: NRect) {
        let c = NR::from_nrect(clip);
        if len == 0 || (dx == 0 && dy == 0) {
            return;
        }
        let (mut old, mut new) = (NR::NONE, NR::NONE);
        for i in start..start + len {
            let o = self.raw(i);
            if !o.empty() {
                old = old.union(&o);
                let t = o.translate(dx, dy);
                new = new.union(&t);
                self.recs[i].ink = NRect::new(t.x0, t.y0, t.x1, t.y1);
            }
        }
        self.add_damage(old.intersect(&c));
        self.add_damage(new.intersect(&c));
    }

    /// Page-chunk boundaries (start offsets) intersected with `[start, start+len)`.
    fn chunks(page_starts: &[usize], total: usize, start: usize, len: usize) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for (i, &ps) in page_starts.iter().enumerate() {
            let pe = page_starts.get(i + 1).copied().unwrap_or(total);
            let lo = ps.max(start);
            let hi = pe.min(start + len);
            if lo < hi {
                v.push((lo, hi));
            }
        }
        v
    }

    fn to_record(&self, r: &RecordInit) -> Record {
        let ink = NR::from_nrect(r.ink);
        let canon = if ink.empty() { NRect::EMPTY } else { r.ink };
        Record { depth: r.depth, ink: canon, kind: r.kind, payload: r.payload, handle: r.handle, clip: r.clip }
    }

    // ------------------------------------------------------------ mutations
    pub fn insert_run(&mut self, pos: usize, recs: &[RecordInit]) {
        for chunk in recs.chunks(64) {
            let mut u = NR::NONE;
            for r in chunk {
                u = u.union(&self.vis(r));
            }
            self.add_damage(u);
        }
        let new: Vec<Record> = recs.iter().map(|r| self.to_record(r)).collect();
        self.recs.splice(pos..pos, new);
    }

    pub fn remove_run(&mut self, start: usize, len: usize, page_starts: &[usize]) {
        for (lo, hi) in Self::chunks(page_starts, self.recs.len(), start, len) {
            let mut u = NR::NONE;
            for i in lo..hi {
                u = u.union(&self.ink(i));
            }
            self.add_damage(u);
        }
        self.recs.drain(start..start + len);
    }

    pub fn move_run(&mut self, start: usize, len: usize, to: usize, ddepth: i32, page_starts: &[usize]) {
        for (lo, hi) in Self::chunks(page_starts, self.recs.len(), start, len) {
            let mut u = NR::NONE;
            for i in lo..hi {
                u = u.union(&self.ink(i));
            }
            self.add_damage(u);
        }
        let mut run: Vec<Record> = self.recs.drain(start..start + len).collect();
        for r in &mut run {
            r.depth = (r.depth as i32 + ddepth) as u16;
        }
        self.recs.splice(to..to, run);
    }

    pub fn translate_run(&mut self, start: usize, len: usize, dx: i32, dy: i32) {
        if len == 0 || (dx == 0 && dy == 0) {
            return;
        }
        let (mut old, mut new) = (NR::NONE, NR::NONE);
        for i in start..start + len {
            let o = self.raw(i);
            if !o.empty() {
                old = old.union(&o);
                let t = o.translate(dx, dy);
                new = new.union(&t);
                self.recs[i].ink = NRect::new(t.x0, t.y0, t.x1, t.y1);
            }
        }
        self.add_damage(old);
        self.add_damage(new);
    }

    pub fn set_ink(&mut self, pos: usize, ink: NRect) {
        let raw_old = self.raw(pos).norm();
        let n = NR::from_nrect(ink).norm();
        if raw_old == n {
            return;
        }
        let old = self.ink(pos);
        self.recs[pos].ink = if n.empty() { NRect::EMPTY } else { ink };
        let new = self.ink(pos);
        if self.is_uniform(self.recs[pos].kind) {
            self.add_damage_xor(old, new);
        } else {
            self.add_damage(old);
            self.add_damage(new);
        }
    }

    pub fn touch(&mut self, pos: usize) {
        let r = self.ink(pos);
        self.add_damage(r);
    }

    // ------------------------------------------------------------ queries
    pub fn subtree_end(&self, i: usize) -> usize {
        let d = self.recs[i].depth;
        let mut j = i + 1;
        while j < self.recs.len() && self.recs[j].depth > d {
            j += 1;
        }
        j
    }
    pub fn parent(&self, i: usize) -> Option<usize> {
        let d = self.recs[i].depth;
        if d == 0 {
            return None;
        }
        (0..i).rev().find(|&j| self.recs[j].depth == d - 1)
    }
    pub fn children(&self, i: usize) -> Vec<usize> {
        let end = self.subtree_end(i);
        let d = self.recs[i].depth;
        (i + 1..end).filter(|&j| self.recs[j].depth == d + 1).collect()
    }
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        (0..self.recs.len()).rev().find(|&i| self.ink(i).contains(x, y))
    }
    /// Records whose ink meets `bbox` and (if given) touches a set tile; with tiles, everything is
    /// evaluated inside the window (bbox clipped to it).
    pub fn cull(&self, bbox: NRect, tiles: Option<&OPlane>) -> Vec<u32> {
        let mut q = NR::from_nrect(bbox);
        if tiles.is_some() {
            q = q.intersect(&self.window);
        }
        let mut out = Vec::new();
        if q.empty() {
            return out;
        }
        for (i, r) in self.recs.iter().enumerate() {
            let ink = self.ink(i);
            if ink.empty() || ink.intersect(&q).empty() {
                continue;
            }
            if let Some(pl) = tiles
                && !pl.touches(ink.intersect(&self.window))
            {
                continue;
            }
            out.push(r.payload);
        }
        out
    }
    /// The frame cull: records whose ink touches a set tile (inside the window).
    pub fn cull_tiles(&self, pl: &OPlane) -> Vec<u32> {
        let mut out = Vec::new();
        for (i, r) in self.recs.iter().enumerate() {
            if pl.touches(self.ink(i).intersect(&self.window)) {
                out.push(r.payload);
            }
        }
        out
    }
    pub fn changed_since(&self, e: u32) -> Vec<u32> {
        let age = self.epoch.saturating_sub(e);
        if age == 0 {
            return vec![];
        }
        if age as usize > RING {
            return (0..self.recs.len() as u32).collect();
        }
        let pl = self.repair(age);
        (0..self.recs.len()).filter(|&i| pl.touches(self.ink(i).intersect(&self.window))).map(|i| i as u32).collect()
    }
    pub fn pos_of_handle(&self, idx: u32) -> Option<usize> {
        self.recs.iter().position(|r| r.handle == idx)
    }
}
