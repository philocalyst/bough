//! Damage planes (16x16 device-pixel tile bitmaps), a history ring for buffer-age repair,
//! and tile-plane -> rect-list extraction.

use crate::instance::*;

pub const TILE: i32 = 16;
pub const TX: usize = (W / TILE) as usize; // 120
pub const TY: usize = (H / TILE) as usize; // 90
pub const WORDS_PER_ROW: usize = 2; // 128 bits, tiles 120..127 are phantom
const _: () = assert!(TX <= WORDS_PER_ROW * 64);

#[derive(Clone)]
pub struct Plane {
    pub rows: [[u64; WORDS_PER_ROW]; TY],
    pub any: bool,
    pub full: bool,
    /// inclusive row range that may hold set bits (rows outside are stale/undefined)
    pub r0: u8,
    pub r1: u8,
}

impl Plane {
    pub const fn new() -> Self {
        Plane { rows: [[0; WORDS_PER_ROW]; TY], any: false, full: false, r0: 255, r1: 0 }
    }
    #[inline]
    pub fn clear(&mut self) {
        if self.any {
            for y in self.r0 as usize..=self.r1 as usize {
                self.rows[y] = [0; WORDS_PER_ROW];
            }
            self.any = false;
            self.full = false;
            self.r0 = 255;
            self.r1 = 0;
        }
    }
    /// Copy only the live row range.
    #[inline]
    pub fn copy_from(&mut self, o: &Plane) {
        self.any = o.any;
        self.full = o.full;
        self.r0 = o.r0;
        self.r1 = o.r1;
        if o.any {
            self.rows[o.r0 as usize..=o.r1 as usize].copy_from_slice(&o.rows[o.r0 as usize..=o.r1 as usize]);
        }
    }
    #[inline]
    pub fn add_rect(&mut self, r: &IRect) {
        let r = r.intersect(&IRect::FULL);
        if r.is_empty() {
            return;
        }
        self.any = true;
        let tx0 = (r.x0 / TILE) as u32;
        let tx1 = ((r.x1 - 1) / TILE) as u32; // inclusive
        let ty0 = (r.y0 / TILE) as usize;
        let ty1 = ((r.y1 - 1) / TILE) as usize;
        // row mask over 128 tiles as two words
        let mut m = [0u64; 2];
        for w in 0..2u32 {
            let lo = w * 64;
            let a = tx0.max(lo);
            let b = tx1.min(lo + 63);
            if a <= b {
                let n = b - a + 1;
                let bits = if n == 64 { !0u64 } else { ((1u64 << n) - 1) << (a - lo) };
                m[w as usize] = bits;
            }
        }
        self.r0 = self.r0.min(ty0 as u8);
        self.r1 = self.r1.max(ty1 as u8);
        for y in ty0..=ty1 {
            self.rows[y][0] |= m[0];
            self.rows[y][1] |= m[1];
        }
    }
    pub fn add_full(&mut self) {
        self.add_rect(&IRect::FULL);
        self.full = true;
    }
    #[inline]
    pub fn or_with(&mut self, o: &Plane) {
        if !o.any {
            return;
        }
        // rows outside [r0,r1] of `self` are stale only if self was never `any`; clear them lazily
        if !self.any {
            self.r0 = o.r0;
            self.r1 = o.r1;
            self.any = true;
            self.full = o.full;
            self.rows[o.r0 as usize..=o.r1 as usize].copy_from_slice(&o.rows[o.r0 as usize..=o.r1 as usize]);
            return;
        }
        let (n0, n1) = (self.r0.min(o.r0), self.r1.max(o.r1));
        // widen: rows newly inside self's range must start from zero
        for y in n0..self.r0.min(n1 + 1) {
            self.rows[y as usize] = [0; WORDS_PER_ROW];
        }
        for y in (self.r1 + 1).max(n0)..=n1 {
            self.rows[y as usize] = [0; WORDS_PER_ROW];
        }
        self.r0 = n0;
        self.r1 = n1;
        self.full |= o.full;
        for y in o.r0 as usize..=o.r1 as usize {
            self.rows[y][0] |= o.rows[y][0];
            self.rows[y][1] |= o.rows[y][1];
        }
    }
}

pub const HIST: usize = 16;

/// Ring of the last HIST epochs' damage planes. Epoch e lives in slot e % HIST. Epoch 0 is "never drawn".
pub struct History {
    pub planes: Vec<Plane>,
    pub epoch: u64,
}

impl History {
    pub fn new() -> Self {
        History { planes: vec![Plane::new(); HIST], epoch: 0 }
    }
    /// Commit `acc` as the next epoch's damage and clear it.
    pub fn push(&mut self, acc: &mut Plane) -> u64 {
        self.epoch += 1;
        self.planes[(self.epoch % HIST as u64) as usize].copy_from(acc);
        acc.clear();
        self.epoch
    }
    /// Union of damage for epochs (since, epoch]. `None` = too old / never drawn -> full repaint.
    pub fn repair(&self, since: Option<u64>, out: &mut Plane) -> bool {
        out.clear();
        let Some(since) = since else { return false };
        if self.epoch - since >= HIST as u64 {
            return false;
        }
        for e in since + 1..=self.epoch {
            out.or_with(&self.planes[(e % HIST as u64) as usize]);
        }
        true
    }
}

/// A repair region extracted from a plane.
#[derive(Default)]
pub struct Region {
    pub full: bool,
    pub bbox: IRect,
    pub rects: Vec<IRect>,
}

impl Region {
    pub fn full() -> Self {
        Region { full: true, bbox: IRect::FULL, rects: vec![IRect::FULL] }
    }
    pub fn set_full(&mut self) {
        self.full = true;
        self.bbox = IRect::FULL;
        self.rects.clear();
        self.rects.push(IRect::FULL);
    }
    /// Extract tile-aligned rects (horizontal runs merged vertically) from a plane. No allocation.
    pub fn from_plane(&mut self, p: &Plane) {
        const CAP: usize = 64;
        self.full = false;
        self.rects.clear();
        let mut bb = IRect::EMPTY;
        let mut overflow = false;
        // (tile x0, tile x1, start row)
        let mut prev = [(0u32, 0u32, 0i32); CAP];
        let mut np = 0usize;
        let mut cur = [(0u32, 0u32, 0i32); CAP];
        if !p.any {
            self.bbox = IRect::EMPTY;
            return;
        }
        for ty in p.r0 as usize..=p.r1 as usize + 1 {
            let mut nc = 0usize;
            if ty <= p.r1 as usize {
                for w in 0..WORDS_PER_ROW {
                    let mut bits = p.rows[ty][w];
                    while bits != 0 {
                        let s = bits.trailing_zeros();
                        let run = (bits >> s).trailing_ones();
                        let (a, b) = (w as u32 * 64 + s, w as u32 * 64 + s + run);
                        bits &= if s + run >= 64 { 0 } else { !(((1u64 << run) - 1) << s) };
                        if nc > 0 && cur[nc - 1].1 == a {
                            cur[nc - 1].1 = b; // run crosses the word boundary
                        } else if nc < CAP {
                            cur[nc] = (a, b, ty as i32);
                            nc += 1;
                        } else {
                            overflow = true;
                        }
                    }
                }
            }
            // runs identical to an open run continue it; open runs not continued are emitted
            let mut matched = [false; CAP];
            for c in cur.iter_mut().take(nc) {
                for pi in 0..np {
                    if !matched[pi] && prev[pi].0 == c.0 && prev[pi].1 == c.1 {
                        matched[pi] = true;
                        c.2 = prev[pi].2;
                        break;
                    }
                }
            }
            for pi in 0..np {
                if !matched[pi] {
                    let pr = prev[pi];
                    let r = IRect::new(pr.0 as i32 * TILE, pr.2 * TILE, pr.1 as i32 * TILE, ty as i32 * TILE);
                    self.rects.push(r);
                    bb = bb.union(&r);
                }
            }
            prev = cur;
            np = nc;
        }
        self.bbox = bb.intersect(&IRect::FULL);
        if overflow || self.rects.len() > MAX_RECTS {
            if overflow {
                bb = IRect::EMPTY;
                for ty in p.r0 as usize..=p.r1 as usize {
                    for w in 0..WORDS_PER_ROW {
                        let b = p.rows[ty][w];
                        if b != 0 {
                            let x0 = (w as u32 * 64 + b.trailing_zeros()) as i32 * TILE;
                            let x1 = (w as u32 * 64 + 64 - b.leading_zeros()) as i32 * TILE;
                            bb = bb.union(&IRect::new(x0, ty as i32 * TILE, x1, ty as i32 * TILE + TILE));
                        }
                    }
                }
                self.bbox = bb.intersect(&IRect::FULL);
            }
            self.rects.clear();
            self.rects.push(self.bbox);
        }
    }
}
