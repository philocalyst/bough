//! The page pipeline as a model (bough round 7, lane core).
//!
//! This crate is a measuring instrument, not the runtime: the smallest real form of four reformulations, on
//! real page-shaped data, with nothing else in the way. What it answers is in `docs/lanes/core/JOURNAL.md`.
//!
//! * [`Sheet`]: records in pages of 64 with structure-of-arrays lanes (rects as four `i16` lanes, an effect
//!   radius lane) and one `u64` bit-plane per page per fact (live, visible, dirty, animated). Damage is
//!   record-space first: a write sets a bit. The frame is two stages in the manner of simdjson: stage 1
//!   classifies whole pages into work words with no data-dependent branch, stage 2 visits only set bits and
//!   projects them onto a tile bitmap of any window size ([`Tiles`]). Every pass is a map over pages whose
//!   result merges by OR, so the one-core loop and the many-core loop are the same function over a page range.
//! * [`ShiftPage`] against [`SelPage`]: the page as the core has it today (an insert moves every column and
//!   re-points the handles of the records it moved) against a page whose slots never move and whose paint
//!   order is a 64-byte selection vector (the vectorised query engines' trick: reorder the selection, not the
//!   data).
//! * [`Dir`]: a directory whose entries instance shared template page images (image, offset, first id), so
//!   that mounting a run of identical-shape rows writes directory entries and operand lanes and nothing else.
//! * operand lanes as the seal: a re-description of a run compares the operands it computes with the operand
//!   lane, a page at a time.
//!
//! The code is portable Rust over fixed 64-element arrays, written so that the compiler's vectoriser sees
//! whole lanes (checked in the disassembly on aarch64 and x86-64); there is no per-architecture intrinsic.
#![forbid(unsafe_code)]

/// Records per page.
pub const P: usize = 64;
pub const TILE_SHIFT: u32 = 5;

/// The four rect lanes of one page, normal form `[x0, x1) x [y0, y1)`, page-relative.
#[derive(Clone)]
#[repr(C, align(64))]
pub struct Lanes {
    pub x0: [i16; P],
    pub y0: [i16; P],
    pub x1: [i16; P],
    pub y1: [i16; P],
}
impl Lanes {
    pub const EMPTY: Lanes = Lanes { x0: [0; P], y0: [0; P], x1: [0; P], y1: [0; P] };
}

/// A tile bitmap for a window of any size: `stride` words per tile row.
#[derive(Clone)]
pub struct Tiles {
    pub tw: u32,
    pub th: u32,
    pub stride: usize,
    pub rows: Vec<u64>,
}
impl Tiles {
    pub fn new(w_px: i32, h_px: i32) -> Tiles {
        let (tw, th) = (((w_px + 31) >> TILE_SHIFT) as u32, ((h_px + 31) >> TILE_SHIFT) as u32);
        let stride = (tw as usize).div_ceil(64);
        Tiles { tw, th, stride, rows: vec![0; stride * th as usize] }
    }
    pub fn clear(&mut self) {
        self.rows.fill(0);
    }
    /// Set tiles `[tx0, tx1) x [ty0, ty1)` (non-empty, inside the window).
    #[inline(always)]
    pub fn mark(&mut self, tx0: u32, ty0: u32, tx1: u32, ty1: u32) {
        let (w0, w1) = ((tx0 >> 6) as usize, ((tx1 - 1) >> 6) as usize);
        let stride = self.stride;
        // one range check for the whole span; the row loops below index nothing
        let band = &mut self.rows[ty0 as usize * stride..ty1 as usize * stride];
        if w0 == w1 {
            let m = (u64::MAX >> (64 - (tx1 - tx0))) << (tx0 & 63);
            for w in band.iter_mut().skip(w0).step_by(stride) {
                *w |= m;
            }
        } else {
            let first = u64::MAX << (tx0 & 63);
            let last = u64::MAX >> (63 - ((tx1 - 1) & 63));
            for r in band.chunks_exact_mut(stride) {
                r[w0] |= first;
                for w in &mut r[w0 + 1..w1] {
                    *w = u64::MAX;
                }
                r[w1] |= last;
            }
        }
    }
    /// `self |= o` (the merge of two workers' results).
    pub fn or_assign(&mut self, o: &Tiles) {
        for (a, b) in self.rows.iter_mut().zip(&o.rows) {
            *a |= *b;
        }
    }
    pub fn count(&self) -> u32 {
        self.rows.iter().map(|r| r.count_ones()).sum()
    }
}

/// Tile spans of a page's 64 records, as lanes.
pub struct Spans {
    pub tx0: [u16; P],
    pub ty0: [u16; P],
    pub tx1: [u16; P],
    pub ty1: [u16; P],
}
impl Spans {
    pub const ZERO: Spans = Spans { tx0: [0; P], ty0: [0; P], tx1: [0; P], ty1: [0; P] };
}

pub struct Sheet {
    pub lanes: Vec<Lanes>,
    /// Effect radius per record, px (a blur or a shadow damages more than its rect): data in a lane, so the
    /// projection has no branch for it.
    pub grow: Vec<[u8; P]>,
    /// Page frame: absolute = lane + off (a scrolled or stamped page changes only this).
    pub off: Vec<[i16; 2]>,
    pub live: Vec<u64>,
    pub vis: Vec<u64>,
    pub dirty: Vec<u64>,
    pub anim: Vec<u64>,
    /// One bit per page: its dirty word is non-zero.
    pub sum: Vec<u64>,
    pub w: i16,
    pub h: i16,
}

impl Sheet {
    /// `n` records laid out as rows of `per_row` records, `row_h` px tall and `row_w` px wide, the records of
    /// a row side by side (a table of cells, or a list of rows when `per_row` is 1).
    pub fn grid(n: usize, per_row: usize, row_w: i32, row_h: i32, w: i16, h: i16) -> Sheet {
        let np = n.div_ceil(P);
        let mut s = Sheet {
            lanes: vec![Lanes::EMPTY; np],
            grow: vec![[0; P]; np],
            off: vec![[0; 2]; np],
            live: vec![0; np],
            vis: vec![0; np],
            dirty: vec![0; np],
            anim: vec![0; np],
            sum: vec![0; np.div_ceil(64)],
            w,
            h,
        };
        let cw = row_w / per_row as i32;
        for i in 0..n {
            let (p, k) = (i / P, i % P);
            let (r, c) = ((i / per_row) as i32, (i % per_row) as i32);
            // the page frame is its first record's row: lanes stay small however long the list is
            if k == 0 {
                s.off[p] = [0, (r * row_h).min(i16::MAX as i32) as i16];
            }
            let oy = s.off[p][1] as i32;
            let y = (r * row_h).min(i16::MAX as i32 - row_h);
            let l = &mut s.lanes[p];
            l.x0[k] = (c * cw) as i16;
            l.x1[k] = ((c + 1) * cw) as i16;
            l.y0[k] = (y - oy) as i16;
            l.y1[k] = (y + row_h - oy) as i16;
            s.live[p] |= 1 << k;
            if y < h as i32 {
                s.vis[p] |= 1 << k;
            }
        }
        s
    }
    pub fn pages(&self) -> usize {
        self.lanes.len()
    }

    /// The write: one bit (and, the first time a page is touched, one bit of the summary).
    #[inline(always)]
    pub fn mark(&mut self, i: usize) {
        let p = i >> 6;
        let old = self.dirty[p];
        self.dirty[p] = old | 1u64 << (i & 63);
        if old == 0 {
            self.sum[p >> 6] |= 1u64 << (p & 63);
        }
    }

    // ------------------------------------------------------------------ stage 1
    /// Stage 1 by scan: `work = dirty | (animated & visible)` for every page of `pages`, 64 pages at a time,
    /// no data-dependent branch (a byte per page, packed to one word per 64 pages); then the set bits of
    /// those words are the pages with work. Costs the same whether nothing or everything is dirty.
    #[inline(never)]
    pub fn classify_scan(&self, pages: std::ops::Range<usize>, out: &mut Vec<u32>) {
        let (d, a, v) = (&self.dirty[pages.clone()], &self.anim[pages.clone()], &self.vis[pages.clone()]);
        let mut base = pages.start;
        let mut emit = |m: u64, base: usize| {
            let mut m = m;
            while m != 0 {
                out.push((base + m.trailing_zeros() as usize) as u32);
                m &= m - 1;
            }
        };
        let (dc, ac, vc) = (d.chunks_exact(P), a.chunks_exact(P), v.chunks_exact(P));
        let (dr, ar, vr) = (dc.remainder(), ac.remainder(), vc.remainder());
        for ((d, a), v) in dc.zip(ac).zip(vc) {
            let mut nz = [0u8; P];
            for i in 0..P {
                nz[i] = ((d[i] | (a[i] & v[i])) != 0) as u8;
            }
            emit(pack(&nz), base);
            base += P;
        }
        let mut m = 0u64;
        for i in 0..dr.len() {
            m |= (((dr[i] | (ar[i] & vr[i])) != 0) as u64) << i;
        }
        emit(m, base);
    }
    /// Stage 1 by summary: only the pages whose summary bit is set are looked at (dirty only: an animated
    /// record sets its bit when it starts).
    #[inline(never)]
    pub fn classify_sum(&self, out: &mut Vec<u32>) {
        for (si, &s) in self.sum.iter().enumerate() {
            let mut s = s;
            while s != 0 {
                out.push((si * 64 + s.trailing_zeros() as usize) as u32);
                s &= s - 1;
            }
        }
    }
    pub fn clear_dirty(&mut self, work: &[u32]) {
        for &p in work {
            self.dirty[p as usize] = 0;
        }
        self.sum.fill(0);
    }

    // ------------------------------------------------------------------ stage 2
    /// The tile spans of every record of page `p` and the mask of records whose clipped, grown rect is not
    /// empty: whole lanes, no branch.
    #[inline(always)]
    pub fn spans(&self, p: usize, out: &mut Spans) -> u64 {
        let (l, g, [ox, oy]) = (&self.lanes[p], &self.grow[p], self.off[p]);
        let (w, h) = (self.w, self.h);
        let mut ok = [0u8; P];
        for i in 0..P {
            let gr = g[i] as i16;
            let x0 = l.x0[i].saturating_add(ox).saturating_sub(gr).max(0).min(w);
            let y0 = l.y0[i].saturating_add(oy).saturating_sub(gr).max(0).min(h);
            let x1 = l.x1[i].saturating_add(ox).saturating_add(gr).max(0).min(w);
            let y1 = l.y1[i].saturating_add(oy).saturating_add(gr).max(0).min(h);
            out.tx0[i] = (x0 >> TILE_SHIFT) as u16;
            out.ty0[i] = (y0 >> TILE_SHIFT) as u16;
            out.tx1[i] = ((x1 + 31) >> TILE_SHIFT) as u16;
            out.ty1[i] = ((y1 + 31) >> TILE_SHIFT) as u16;
            ok[i] = ((x0 < x1) & (y0 < y1)) as u8;
        }
        pack(&ok)
    }
    /// One record's tile span, scalar (the sparse path: a page with a few bits set).
    #[inline(always)]
    pub fn span1(&self, p: usize, i: usize) -> Option<(u32, u32, u32, u32)> {
        let (l, [ox, oy]) = (&self.lanes[p], self.off[p]);
        let gr = self.grow[p][i] as i32;
        let (w, h) = (self.w as i32, self.h as i32);
        let x0 = (l.x0[i] as i32 + ox as i32 - gr).max(0).min(w);
        let y0 = (l.y0[i] as i32 + oy as i32 - gr).max(0).min(h);
        let x1 = (l.x1[i] as i32 + ox as i32 + gr).max(0).min(w);
        let y1 = (l.y1[i] as i32 + oy as i32 + gr).max(0).min(h);
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        Some(((x0 >> TILE_SHIFT) as u32, (y0 >> TILE_SHIFT) as u32, ((x1 + 31) >> TILE_SHIFT) as u32, ((y1 + 31) >> TILE_SHIFT) as u32))
    }

    /// Stage 2: project the work of pages `work` onto `tiles`. `dense_at`: pages with at least that many set
    /// bits compute their spans as lanes; sparser ones take the scalar path per bit (65: always scalar).
    #[inline(never)]
    pub fn project(&self, work: &[u32], tiles: &mut Tiles, dense_at: u32) {
        for &p in work {
            let p = p as usize;
            let m = (self.dirty[p] | self.anim[p]) & self.vis[p];
            if m.count_ones() >= dense_at {
                // the spans are written whole before any is read: no need to clear them
                let mut sp = Spans::ZERO;
                let mut m = m & self.spans(p, &mut sp);
                while m != 0 {
                    let i = m.trailing_zeros() as usize;
                    m &= m - 1;
                    tiles.mark(sp.tx0[i] as u32, sp.ty0[i] as u32, sp.tx1[i] as u32, sp.ty1[i] as u32);
                }
            } else {
                let mut m = m;
                while m != 0 {
                    let i = m.trailing_zeros() as usize;
                    m &= m - 1;
                    if let Some((a, b, c, d)) = self.span1(p, i) {
                        tiles.mark(a, b, c, d);
                    }
                }
            }
        }
    }

    /// Stage 2 on `threads` cores: the same function over sub-ranges of the work list, one tile bitmap per
    /// worker, merged by OR. Threads are scoped (spawned per call): the measured time includes their start.
    pub fn project_par(&self, work: &[u32], tiles: &mut Tiles, dense_at: u32, threads: usize) {
        if threads <= 1 || work.len() < threads {
            return self.project(work, tiles, dense_at);
        }
        let chunk = work.len().div_ceil(threads);
        let (tw, th, stride) = (tiles.tw, tiles.th, tiles.stride);
        let parts: Vec<Tiles> = std::thread::scope(|s| {
            let hs: Vec<_> = work
                .chunks(chunk)
                .map(|c| {
                    s.spawn(move || {
                        let mut t = Tiles { tw, th, stride, rows: vec![0; stride * th as usize] };
                        self.project(c, &mut t, dense_at);
                        t
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for t in &parts {
            tiles.or_assign(t);
        }
    }

    /// Scroll: every page of `pages` moves by `dy` (one frame word per page; no lane is touched).
    #[inline(never)]
    pub fn translate(&mut self, pages: std::ops::Range<usize>, dy: i16) {
        for o in &mut self.off[pages] {
            o[1] = o[1].saturating_add(dy);
        }
    }
    /// Re-derive the visible plane of `pages` after a move: whole lanes, no branch.
    #[inline(never)]
    pub fn cull(&mut self, pages: std::ops::Range<usize>) {
        let mut sp = Spans::ZERO;
        for p in pages {
            self.vis[p] = self.live[p] & self.spans(p, &mut sp);
        }
    }
}

/// 64 bytes of 0/1 to a 64-bit mask (eight multiplies; the vectoriser's movemask on either architecture is
/// not reliably found, this is).
#[inline(always)]
pub fn pack(b: &[u8; P]) -> u64 {
    let mut m = 0u64;
    for (k, c) in b.chunks_exact(8).enumerate() {
        let v = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
        m |= (v.wrapping_mul(0x0102_0408_1020_4080) >> 56) << (8 * k);
    }
    m
}

// =================================================================================== pages under edits

/// One record's non-rect columns (what the core's page stores beside the ink).
#[derive(Clone, Copy, Default)]
pub struct Rec {
    pub rect: [i16; 4],
    pub depth: u16,
    pub kind: u8,
    pub clip: u16,
    pub payload: u32,
    pub handle: u32,
}

/// The page as the core has it today: slot = paint position. An insert shifts every column and re-points the
/// handle of every record it moved (`htab[handle] = slot`).
#[repr(C, align(64))]
pub struct ShiftPage {
    pub len: usize,
    pub rect: [[i16; 4]; P],
    pub depth: [u16; P],
    pub kind: [u8; P],
    pub clip: [u16; P],
    pub payload: [u32; P],
    pub handle: [u32; P],
}
impl ShiftPage {
    pub fn new() -> Box<ShiftPage> {
        Box::new(ShiftPage { len: 0, rect: [[0; 4]; P], depth: [0; P], kind: [0; P], clip: [0; P], payload: [0; P], handle: [0; P] })
    }
    /// Insert at paint position `k` (`len < 64`).
    #[inline(never)]
    pub fn insert(&mut self, k: usize, r: Rec, htab: &mut [u32]) {
        let n = self.len;
        self.rect.copy_within(k..n, k + 1);
        self.depth.copy_within(k..n, k + 1);
        self.kind.copy_within(k..n, k + 1);
        self.clip.copy_within(k..n, k + 1);
        self.payload.copy_within(k..n, k + 1);
        self.handle.copy_within(k..n, k + 1);
        (self.rect[k], self.depth[k], self.kind[k], self.clip[k], self.payload[k], self.handle[k]) = (r.rect, r.depth, r.kind, r.clip, r.payload, r.handle);
        self.len = n + 1;
        for s in k..=n {
            htab[self.handle[s] as usize] = s as u32;
        }
    }
    #[inline(never)]
    pub fn remove(&mut self, k: usize, htab: &mut [u32]) {
        let n = self.len;
        self.rect.copy_within(k + 1..n, k);
        self.depth.copy_within(k + 1..n, k);
        self.kind.copy_within(k + 1..n, k);
        self.clip.copy_within(k + 1..n, k);
        self.payload.copy_within(k + 1..n, k);
        self.handle.copy_within(k + 1..n, k);
        self.len = n - 1;
        for s in k..n - 1 {
            htab[self.handle[s] as usize] = s as u32;
        }
    }
    /// Sum of payloads in paint order (what an ordered consumer does: the bin pass, a hit test).
    #[inline(never)]
    pub fn walk(&self) -> u64 {
        let mut a = 0u64;
        for s in 0..self.len {
            a = a.wrapping_mul(31).wrapping_add(self.payload[s] as u64);
        }
        a
    }
}

/// A page whose slots never move: a record keeps `(page, slot)` for life, so that pair is its identity (no
/// handle table, nothing to re-point), a dirty bit set through it never has to be carried, and every
/// per-record column outside the page can be addressed by it. Paint order is a selection vector: `ord[k]` is
/// the slot of the record at paint position `k`. An insert writes one free slot and moves at most 63 bytes.
#[repr(C, align(64))]
pub struct SelPage {
    pub len: usize,
    pub live: u64,
    /// `ord[k] == k` for every `k < len`: the page was only ever appended to (an ordered walk needs no
    /// indirection).
    pub identity: bool,
    pub ord: [u8; P],
    pub rect: [[i16; 4]; P],
    pub depth: [u16; P],
    pub kind: [u8; P],
    pub clip: [u16; P],
    pub payload: [u32; P],
}
impl SelPage {
    pub fn new() -> Box<SelPage> {
        Box::new(SelPage { len: 0, live: 0, identity: true, ord: [0; P], rect: [[0; 4]; P], depth: [0; P], kind: [0; P], clip: [0; P], payload: [0; P] })
    }
    /// Insert at paint position `k` (`len < 64`); returns the slot (the record's identity within the page).
    #[inline(never)]
    pub fn insert(&mut self, k: usize, r: Rec) -> usize {
        let n = self.len;
        let s = (!self.live).trailing_zeros() as usize;
        self.live |= 1 << s;
        (self.rect[s], self.depth[s], self.kind[s], self.clip[s], self.payload[s]) = (r.rect, r.depth, r.kind, r.clip, r.payload);
        self.ord.copy_within(k..n, k + 1);
        self.ord[k] = s as u8;
        self.identity &= s == k;
        self.len = n + 1;
        s
    }
    #[inline(never)]
    pub fn remove(&mut self, k: usize) {
        let n = self.len;
        let s = self.ord[k] as usize;
        self.live &= !(1 << s);
        self.ord.copy_within(k + 1..n, k);
        self.identity &= k == n - 1;
        self.len = n - 1;
    }
    #[inline(never)]
    pub fn walk(&self) -> u64 {
        let mut a = 0u64;
        if self.identity {
            for s in 0..self.len {
                a = a.wrapping_mul(31).wrapping_add(self.payload[s] as u64);
            }
        } else {
            for k in 0..self.len {
                a = a.wrapping_mul(31).wrapping_add(self.payload[self.ord[k] as usize] as u64);
            }
        }
        a
    }
}

// =================================================================================== template instancing

/// A directory that instances page images: entry `d` shows image `img[d]` moved by `off[d]`, and its records'
/// ids are `id0[d] + slot`. A run of identical-shape rows is a few shared images and one entry per page.
#[derive(Default)]
pub struct Dir {
    pub images: Vec<Lanes>,
    pub img: Vec<u32>,
    pub off: Vec<[i32; 2]>,
    pub id0: Vec<u32>,
}

/// The page images of a template of `k` records per row, rows `row_h` apart, pre-tiled to a whole number of
/// pages: `lcm(k, 64)` records, so that every repetition of the block is the same bytes at a larger offset.
/// Returns (images, rows per block).
pub fn tile_template(rects: &[[i16; 4]], row_h: i32) -> (Vec<Lanes>, usize) {
    let k = rects.len();
    let block = lcm(k, P);
    let mut images = vec![Lanes::EMPTY; block / P];
    for i in 0..block {
        let (p, s, row, j) = (i / P, i % P, (i / k) as i32, i % k);
        let [x0, y0, x1, y1] = rects[j];
        // relative to the page's first row, so every page image is small numbers
        let first_row = ((p * P) / k) as i32;
        let dy = ((row - first_row) * row_h) as i16;
        let l = &mut images[p];
        (l.x0[s], l.y0[s], l.x1[s], l.y1[s]) = (x0, y0 + dy, x1, y1 + dy);
    }
    (images, block / k)
}
fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}
pub fn lcm(a: usize, b: usize) -> usize {
    a / gcd(a, b) * b
}

impl Dir {
    /// Mount `rows` rows of a `k`-record template whose tiled images are `images[first..first + n]`: entries
    /// only. Nothing per record is written.
    #[inline(never)]
    pub fn stamp_shared(&mut self, first: u32, n_images: usize, k: usize, rows: usize, row_h: i32, id_base: u32) {
        let pages = (rows * k).div_ceil(P);
        self.img.reserve(pages);
        self.off.reserve(pages);
        self.id0.reserve(pages);
        for p in 0..pages {
            let first_row = ((p * P) / k) as i32;
            self.img.push(first + (p % n_images) as u32);
            self.off.push([0, first_row * row_h]);
            self.id0.push(id_base + (p * P) as u32);
        }
    }
    /// The same mount with private pages: each page is a copy of its image (what a store that cannot share
    /// pays), plus the entry.
    #[inline(never)]
    pub fn stamp_copy(&mut self, first: u32, n_images: usize, k: usize, rows: usize, row_h: i32, id_base: u32) {
        let pages = (rows * k).div_ceil(P);
        self.images.reserve(pages);
        for p in 0..pages {
            let first_row = ((p * P) / k) as i32;
            let img = self.images[first as usize + p % n_images].clone();
            self.img.push(self.images.len() as u32);
            self.images.push(img);
            self.off.push([0, first_row * row_h]);
            self.id0.push(id_base + (p * P) as u32);
        }
    }
}

/// The mount as a per-record store does it: every record written field by field into the page columns, a
/// handle entry per record (the shape of `RecordInit` -> `insert_run`, without its directory upkeep).
#[inline(never)]
pub fn mount_per_record(pages: &mut Vec<Box<ShiftPage>>, htab: &mut Vec<u32>, rects: &[[i16; 4]], rows: usize, row_h: i32) {
    for r in 0..rows {
        for (j, &[x0, y0, x1, y1]) in rects.iter().enumerate() {
            if pages.last().is_none_or(|p| p.len == P) {
                pages.push(ShiftPage::new());
            }
            let pg = pages.last_mut().unwrap();
            let s = pg.len;
            let dy = (r as i32 * row_h).min(30000) as i16;
            pg.rect[s] = [x0, y0 + dy, x1, y1 + dy];
            pg.depth[s] = (j != 0) as u16 + 1;
            pg.kind[s] = 1;
            pg.clip[s] = 0;
            pg.payload[s] = htab.len() as u32;
            pg.handle[s] = htab.len() as u32;
            pg.len = s + 1;
            htab.push(s as u32);
        }
    }
}

// =================================================================================== operand lanes as the seal

/// Re-describe a run whose description is (template, operands): compute each row's operand with `f` and
/// compare with the operand lane, a page at a time; returns the number of rows that differ and writes them.
/// `f` is the user's row function reduced to what varies (here: one colour word).
#[inline(always)]
pub fn redescribe_ops(lane: &mut [[u32; P]], rows: usize, mut f: impl FnMut(usize) -> u32) -> usize {
    let mut changed = 0;
    for (p, page) in lane.iter_mut().enumerate().take(rows.div_ceil(P)) {
        let n = (rows - p * P).min(P);
        let mut new = [0u32; P];
        for i in 0..n {
            new[i] = f(p * P + i);
        }
        let mut ne = [0u8; P];
        for i in 0..P {
            ne[i] = (new[i] != page[i]) as u8;
        }
        let m = pack(&ne) & if n == P { u64::MAX } else { (1u64 << n) - 1 };
        if m != 0 {
            changed += m.count_ones() as usize;
            *page = new;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The projection of any dirty set equals marking each dirty record alone, on every path (lanes, scalar,
    /// workers), for windows wider than 64 tiles too.
    #[test]
    fn projection_paths_agree() {
        for (w, h) in [(1280i16, 800i16), (5120, 2880)] {
            let mut s = Sheet::grid(5000, 5, w as i32, 24, w, h);
            // a blur on some records: damage grows by its radius
            for p in 0..s.pages() {
                s.grow[p][7] = 12;
            }
            let mut seed = 12345u64;
            let mut want = Tiles::new(w as i32, h as i32);
            for _ in 0..700 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let i = (seed >> 33) as usize % 5000;
                s.mark(i);
                if let Some((a, b, c, d)) = s.span1(i / P, i % P) {
                    want.mark(a, b, c, d);
                }
            }
            let (mut w1, mut w2) = (Vec::new(), Vec::new());
            s.classify_scan(0..s.pages(), &mut w1);
            s.classify_sum(&mut w2);
            assert_eq!(w1, w2);
            for dense_at in [0, 8, 65] {
                let mut t = Tiles::new(w as i32, h as i32);
                s.project(&w1, &mut t, dense_at);
                assert_eq!(t.rows, want.rows, "dense_at {dense_at}");
                let mut t = Tiles::new(w as i32, h as i32);
                s.project_par(&w1, &mut t, dense_at, 4);
                assert_eq!(t.rows, want.rows, "workers, dense_at {dense_at}");
            }
            assert!(want.count() > 0);
        }
    }

    /// Both pages hold the same sequence under the same random edits.
    #[test]
    fn selection_page_is_the_shift_page() {
        let (mut a, mut b) = (ShiftPage::new(), SelPage::new());
        let mut htab = vec![0u32; 4096];
        let mut seed = 99u64;
        let mut next = 1u32;
        for step in 0..20000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let r = (seed >> 33) as usize;
            if a.len < P && (a.len == 0 || r % 3 != 0) {
                let k = r % (a.len + 1);
                let rec = Rec { payload: next, handle: next % 4096, ..Rec::default() };
                next += 1;
                a.insert(k, rec, &mut htab);
                b.insert(k, rec);
            } else {
                let k = r % a.len;
                a.remove(k, &mut htab);
                b.remove(k);
            }
            assert_eq!(a.len, b.len);
            assert_eq!(a.walk(), b.walk(), "step {step}");
            assert_eq!(b.live.count_ones() as usize, b.len);
        }
    }

    /// A stamped run shows the rects a per-record mount writes.
    #[test]
    fn stamped_run_is_the_mounted_run() {
        let rects = [[0i16, 0, 960, 32], [8, 8, 72, 24], [80, 8, 300, 24]];
        let rows = 500;
        let (images, _) = tile_template(&rects, 32);
        let n = images.len();
        let mut d = Dir { images, ..Dir::default() };
        d.stamp_shared(0, n, 3, rows, 32, 0);
        let mut c = Dir { images: d.images.clone(), ..Dir::default() };
        c.stamp_copy(0, n, 3, rows, 32, 0);
        for i in 0..rows * 3 {
            let (p, s, row, j) = (i / P, i % P, (i / 3) as i32, i % 3);
            for dir in [&d, &c] {
                let l = &dir.images[dir.img[p] as usize];
                let (ox, oy) = (dir.off[p][0], dir.off[p][1]);
                let got = [l.x0[s] as i32 + ox, l.y0[s] as i32 + oy, l.x1[s] as i32 + ox, l.y1[s] as i32 + oy];
                let want = [rects[j][0] as i32, rects[j][1] as i32 + row * 32, rects[j][2] as i32, rects[j][3] as i32 + row * 32];
                assert_eq!(got, want, "record {i}");
                assert_eq!(dir.id0[p] + s as u32, i as u32);
            }
        }
    }

    #[test]
    fn operand_lane_finds_the_changed_rows() {
        let rows = 1000usize;
        let mut lane = vec![[0u32; P]; rows.div_ceil(P)];
        let tint = |i: usize, sel: usize| if i == sel { 0x2f6bff } else if i % 2 == 0 { 0xf5f5f5 } else { 0xe6e6e6 };
        assert_eq!(redescribe_ops(&mut lane, rows, |i| tint(i, usize::MAX)), rows);
        assert_eq!(redescribe_ops(&mut lane, rows, |i| tint(i, usize::MAX)), 0);
        assert_eq!(redescribe_ops(&mut lane, rows, |i| tint(i, 17)), 1);
        assert_eq!(redescribe_ops(&mut lane, rows, |i| tint(i, 999)), 2);
    }
}
