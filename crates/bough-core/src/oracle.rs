//! The dumb oracle: a `Vec` of records in preorder with naive, obviously-correct implementations
//! of every query and of the damage bookkeeping (per-tile rectangle overlap tests).

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
    pub bbox: NR, // NONE (empty) when clean
}
impl OPlane {
    pub fn new(tw: usize, th: usize) -> Self {
        OPlane { tw, th, tiles: vec![false; tw * th], bbox: NR::NONE }
    }
    pub fn or(&mut self, o: &OPlane) {
        for (a, b) in self.tiles.iter_mut().zip(&o.tiles) {
            *a |= *b;
        }
        self.bbox = self.bbox.union(&o.bbox);
    }
    pub fn get(&self, tx: usize, ty: usize) -> bool {
        tx < self.tw && ty < self.th && self.tiles[ty * self.tw + tx]
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
}

pub struct Oracle {
    pub recs: Vec<Record>,
    pub epoch: u32,
    pub window: NR,
    pub planes: Vec<OPlane>, // ring of 8 by epoch % 8
}

impl Oracle {
    pub fn new(w: i32, h: i32) -> Self {
        let (tw, th) = (((w + TILE - 1) / TILE) as usize, ((h + TILE - 1) / TILE) as usize);
        assert!(tw <= MAX_TILES_X && th <= MAX_TILES_Y);
        Oracle { recs: Vec::new(), epoch: 1, window: NR { x0: 0, y0: 0, x1: w, y1: h }, planes: vec![OPlane::new(tw, th); RING] }
    }

    // ------------------------------------------------------------ damage
    pub fn add_damage(&mut self, r: NR) {
        let c = r.intersect(&self.window);
        if c.empty() {
            return;
        }
        let pl = &mut self.planes[self.epoch as usize % RING];
        pl.bbox = pl.bbox.union(&c);
        for ty in 0..pl.th {
            for tx in 0..pl.tw {
                let t = NR { x0: tx as i32 * TILE, y0: ty as i32 * TILE, x1: (tx as i32 + 1) * TILE, y1: (ty as i32 + 1) * TILE };
                if !t.intersect(&c).empty() {
                    pl.tiles[ty * pl.tw + tx] = true;
                }
            }
        }
    }
    pub fn next_epoch(&mut self) {
        self.epoch += 1;
        let (tw, th) = (self.planes[0].tw, self.planes[0].th);
        self.planes[self.epoch as usize % RING] = OPlane::new(tw, th);
    }
    pub fn repair(&self, age: u32) -> OPlane {
        let mut out = OPlane::new(self.planes[0].tw, self.planes[0].th);
        for back in 0..age {
            out.or(&self.planes[self.epoch.wrapping_sub(back) as usize % RING]);
        }
        out
    }

    fn ink(&self, i: usize) -> NR {
        NR::from_nrect(self.recs[i].ink)
    }
    fn non_empty(r: NR) -> NR {
        if r.empty() { NR::NONE } else { r }
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
        Record { depth: r.depth, ink: canon, stamp: self.epoch, kind: r.kind, payload: r.payload, handle: r.handle }
    }

    // ------------------------------------------------------------ mutations
    pub fn insert_run(&mut self, pos: usize, recs: &[RecordInit]) {
        for chunk in recs.chunks(64) {
            let mut u = NR::NONE;
            for r in chunk {
                u = u.union(&NR::from_nrect(r.ink));
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
            r.stamp = self.epoch;
        }
        self.recs.splice(to..to, run);
    }

    pub fn translate_run(&mut self, start: usize, len: usize, dx: i32, dy: i32, page_starts: &[usize]) {
        if len == 0 || (dx == 0 && dy == 0) {
            return;
        }
        for (lo, hi) in Self::chunks(page_starts, self.recs.len(), start, len) {
            let (mut old, mut new) = (NR::NONE, NR::NONE);
            for i in lo..hi {
                let o = self.ink(i);
                old = old.union(&o);
                if !o.empty() {
                    new = new.union(&o.translate(dx, dy));
                    let t = o.translate(dx, dy);
                    self.recs[i].ink = NRect::new(t.x0, t.y0, t.x1, t.y1);
                }
                self.recs[i].stamp = self.epoch;
            }
            self.add_damage(old);
            self.add_damage(new);
        }
    }

    pub fn set_ink(&mut self, pos: usize, ink: NRect) {
        let old = self.ink(pos);
        self.add_damage(Self::non_empty(old));
        let n = NR::from_nrect(ink);
        self.recs[pos].ink = if n.empty() { NRect::EMPTY } else { ink };
        self.recs[pos].stamp = self.epoch;
        self.add_damage(Self::non_empty(n));
    }

    pub fn touch(&mut self, pos: usize) {
        self.recs[pos].stamp = self.epoch;
        let r = self.ink(pos);
        self.add_damage(Self::non_empty(r));
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
    pub fn cull(&self, bbox: NRect, tiles: Option<&OPlane>) -> Vec<u32> {
        let q = NR::from_nrect(bbox);
        let mut out = Vec::new();
        if q.empty() {
            return out;
        }
        for (i, r) in self.recs.iter().enumerate() {
            let ink = self.ink(i);
            if ink.empty() || ink.intersect(&q).empty() {
                continue;
            }
            if let Some(pl) = tiles {
                let c = ink.intersect(&self.window);
                let mut hit = false;
                for ty in 0..pl.th {
                    for tx in 0..pl.tw {
                        let t = NR { x0: tx as i32 * TILE, y0: ty as i32 * TILE, x1: (tx as i32 + 1) * TILE, y1: (ty as i32 + 1) * TILE };
                        if !c.empty() && !t.intersect(&c).empty() && pl.get(tx, ty) {
                            hit = true;
                        }
                    }
                }
                if !hit {
                    continue;
                }
            }
            out.push(r.payload);
        }
        out
    }
    pub fn changed_since(&self, e: u32) -> Vec<u32> {
        (0..self.recs.len()).filter(|&i| self.recs[i].stamp > e).map(|i| i as u32).collect()
    }
    pub fn pos_of_handle(&self, idx: u32) -> Option<usize> {
        self.recs.iter().position(|r| r.handle == idx)
    }
}
