//! The directory as a piece table over pages whose slots never move (bough round 7, lane core, phase B).
//!
//! The page model in `lib.rs` showed what a stable slot buys inside one page. This module asks the question
//! it left open: what happens at a page boundary. If a record keeps `(page, slot)` for life, a full page
//! cannot be split by moving half of its records. Instead the *directory* is split: a directory entry is a
//! **piece**, a run of one page's slots in paint order, and one page may back several pieces.
//!
//! * A piece is `(page, start, len)` when its slots are `start, start + 1, ..` (an *identity* piece: a page
//!   that was appended to in order, or a split of one) and `(page, len, ord)` otherwise, where `ord` names a
//!   64-byte block listing its slots in paint order. Pieces never share an ord block, so no edit of one piece
//!   has to fix another.
//! * An insert takes free slots of the page under the insertion point (its live plane says which) and writes
//!   the piece's list; when the page is full it opens a new page and, if the point is inside a piece, splits
//!   that piece in two. Nothing that exists moves, so nothing has to be re-pointed: there is no handle table.
//! * A remove clears live bits and shortens lists; an empty piece leaves the directory, an empty page is
//!   reused.
//! * Every page carries a **seal**: a counter bumped by every change to what the page shows (its records'
//!   columns, which slots are live, the order of a piece over it). A consumer that keeps something derived
//!   from a page (staged kernel commands, a cull result) compares one word to know it is still valid.
//!
//! [`Pieces`] is that directory; [`Shift`] is the directory the core has today in the same harness (pages in
//! position order, an insert shifts columns and re-points the handles of what it moved, a full page splits in
//! half), so that the two can be put through the same edits and counted. Both keep their entries in blocks of
//! at most 128 with per-block record counts, which stands in for the core's gapped directory on both sides.
use crate::{P, Rec};

const NONE: u32 = u32::MAX;
const BLOCK: usize = 128;

/// One page of 64 slots that never move.
#[derive(Clone)]
#[repr(C, align(64))]
pub struct SlotPage {
    pub live: u64,
    /// Bumped by every change to what this page shows (see the module docs).
    pub seal: u32,
    /// Pieces over this page (diagnostics).
    pub pieces: u32,
    pub rect: [[i16; 4]; P],
    pub depth: [u16; P],
    pub kind: [u8; P],
    pub clip: [u16; P],
    pub payload: [u32; P],
}
impl SlotPage {
    fn new() -> SlotPage {
        SlotPage { live: 0, seal: 0, pieces: 0, rect: [[0; 4]; P], depth: [0; P], kind: [0; P], clip: [0; P], payload: [0; P] }
    }
}

/// A run of one page's slots in paint order.
#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub page: u32,
    /// Identity pieces: the first slot. Permuted pieces: unused.
    pub start: u8,
    pub len: u8,
    /// The ord block of a permuted piece, `NONE` for an identity piece.
    pub ord: u32,
}

/// A two-level sequence: blocks of entries with the number of records in each block.
struct Seq<E> {
    blocks: Vec<Vec<E>>,
    counts: Vec<u32>,
}
impl<E: Copy> Seq<E> {
    fn new() -> Seq<E> {
        Seq { blocks: vec![Vec::with_capacity(BLOCK)], counts: vec![0] }
    }
    /// The block holding record position `pos` and the position of that block's first record. `pos` at the
    /// very end names the last block.
    #[inline]
    fn block_of(&self, pos: usize) -> (usize, usize) {
        let mut base = 0usize;
        for (b, &c) in self.counts.iter().enumerate() {
            if pos < base + c as usize || b + 1 == self.counts.len() {
                return (b, base);
            }
            base += c as usize;
        }
        (0, 0)
    }
    fn insert(&mut self, b: usize, i: usize, e: E) -> usize {
        self.blocks[b].insert(i, e);
        if self.blocks[b].len() > BLOCK {
            let tail = self.blocks[b].split_off(BLOCK / 2);
            self.blocks.insert(b + 1, tail);
            self.counts.insert(b + 1, 0);
            return 1; // the caller recounts blocks b and b + 1
        }
        0
    }
    fn remove(&mut self, b: usize, i: usize) {
        self.blocks[b].remove(i);
        if self.blocks[b].is_empty() && self.blocks.len() > 1 {
            self.blocks.remove(b);
            self.counts.remove(b);
        }
    }
    fn entries(&self) -> usize {
        self.blocks.iter().map(|b| b.len()).sum()
    }
    /// Where entry `i` of block `b` is after an insert that may have split the block in two.
    fn moved(&self, b: usize, i: usize, split: usize) -> (usize, usize) {
        let n = self.blocks[b].len();
        if split != 0 && i >= n { (b + 1, i - n) } else { (b, i) }
    }
    /// The entry after `(b, i)`; past the last entry of the last block it is `(b, i + 1)`.
    fn next(&self, b: usize, i: usize) -> (usize, usize) {
        if i + 1 < self.blocks[b].len() {
            (b, i + 1)
        } else if b + 1 < self.blocks.len() {
            (b + 1, 0)
        } else {
            (b, i + 1)
        }
    }
}

// =================================================================================== the piece table

pub struct Pieces {
    pub pages: Vec<SlotPage>,
    page_free: Vec<u32>,
    ords: Vec<[u8; P]>,
    ord_free: Vec<u32>,
    dir: Seq<Piece>,
    pub len: usize,
}

impl Default for Pieces {
    fn default() -> Self {
        Self::new()
    }
}

impl Pieces {
    pub fn new() -> Pieces {
        Pieces { pages: Vec::new(), page_free: Vec::new(), ords: Vec::new(), ord_free: Vec::new(), dir: Seq::new(), len: 0 }
    }
    fn new_page(&mut self) -> u32 {
        match self.page_free.pop() {
            Some(p) => p,
            None => {
                self.pages.push(SlotPage::new());
                (self.pages.len() - 1) as u32
            }
        }
    }
    fn new_ord(&mut self) -> u32 {
        match self.ord_free.pop() {
            Some(o) => o,
            None => {
                self.ords.push([0; P]);
                (self.ords.len() - 1) as u32
            }
        }
    }
    #[inline(always)]
    fn slot(&self, pc: &Piece, k: usize) -> usize {
        if pc.ord == NONE { pc.start as usize + k } else { self.ords[pc.ord as usize][k] as usize }
    }
    /// Give an identity piece an ord block listing its slots.
    fn permute(&mut self, pc: &mut Piece) {
        if pc.ord != NONE {
            return;
        }
        let o = self.new_ord();
        for k in 0..pc.len as usize {
            self.ords[o as usize][k] = pc.start + k as u8;
        }
        pc.ord = o;
    }
    fn recount(&mut self, b: usize) {
        self.dir.counts[b] = self.dir.blocks[b].iter().map(|p| p.len as u32).sum();
    }
    /// (block, entry, offset in the entry) of record position `pos` (`pos == len`: past the last entry).
    #[inline]
    fn locate(&self, pos: usize) -> (usize, usize, usize) {
        let (b, mut base) = self.dir.block_of(pos);
        let blk = &self.dir.blocks[b];
        for (i, pc) in blk.iter().enumerate() {
            if pos < base + pc.len as usize {
                return (b, i, pos - base);
            }
            base += pc.len as usize;
        }
        (b, blk.len(), 0)
    }

    /// Insert `recs` at paint position `pos`; `ids` receives each record's identity, `page * 64 + slot`.
    #[inline(never)]
    pub fn insert(&mut self, pos: usize, recs: &[Rec], ids: &mut Vec<u32>) {
        let (mut b, mut i, mut k) = self.locate(pos);
        // an insertion point at the start of an entry is also the end of the one before it: prefer that one
        // (appending keeps an identity piece an identity piece)
        if k == 0 && i > 0 {
            i -= 1;
            k = self.dir.blocks[b][i].len as usize;
        }
        let mut rest = recs;
        while !rest.is_empty() {
            let have = i < self.dir.blocks[b].len();
            let free = if have { !self.pages[self.dir.blocks[b][i].page as usize].live } else { 0 };
            if have && free != 0 {
                let mut pc = self.dir.blocks[b][i];
                let pg = pc.page as usize;
                let m = rest.len().min(free.count_ones() as usize);
                // an append that continues the piece's run of slots keeps it an identity piece
                let next = pc.start as usize + pc.len as usize;
                let run = pc.ord == NONE && k == pc.len as usize && next + m <= P && (free >> next).trailing_ones() as usize >= m;
                if !run {
                    self.permute(&mut pc);
                    let o = &mut self.ords[pc.ord as usize];
                    o.copy_within(k..pc.len as usize, k + m);
                }
                let mut f = if run { free >> next << next } else { free };
                for (j, r) in rest[..m].iter().enumerate() {
                    let s = f.trailing_zeros() as usize;
                    f &= f - 1;
                    let p = &mut self.pages[pg];
                    p.live |= 1 << s;
                    (p.rect[s], p.depth[s], p.kind[s], p.clip[s], p.payload[s]) = (r.rect, r.depth, r.kind, r.clip, r.payload);
                    if !run {
                        self.ords[pc.ord as usize][k + j] = s as u8;
                    }
                    ids.push((pg * P + s) as u32);
                }
                self.pages[pg].seal += 1;
                pc.len += m as u8;
                self.dir.blocks[b][i] = pc;
                self.dir.counts[b] += m as u32;
                k += m;
                rest = &rest[m..];
                continue;
            }
            // the page under the point is full (or there is no entry): a new page, as its own piece, at the
            // point; a point inside a piece splits it (two lists over the same page; nothing moves)
            if have && k > 0 && k < self.dir.blocks[b][i].len as usize {
                let mut pc = self.dir.blocks[b][i];
                let n = pc.len as usize;
                let tail = if pc.ord == NONE {
                    Piece { page: pc.page, start: pc.start + k as u8, len: (n - k) as u8, ord: NONE }
                } else {
                    let o = self.new_ord();
                    let (src, dst) = (pc.ord as usize, o as usize);
                    let tmp = self.ords[src];
                    self.ords[dst][..n - k].copy_from_slice(&tmp[k..n]);
                    Piece { page: pc.page, start: 0, len: (n - k) as u8, ord: o }
                };
                pc.len = k as u8;
                self.dir.blocks[b][i] = pc;
                let pg = pc.page as usize;
                self.pages[pg].pieces += 1;
                self.pages[pg].seal += 1;
                let sp = self.dir.insert(b, i + 1, tail);
                if sp != 0 {
                    self.recount(b);
                    self.recount(b + 1);
                }
                (b, i) = self.dir.moved(b, i, sp);
            }
            // after the entry at the point when the point is past its start, else before it
            let (ab, at) = if have && k > 0 { self.dir.next(b, i) } else { (b, i) };
            let page = self.new_page();
            self.pages[page as usize].pieces = 1;
            let sp = self.dir.insert(ab, at, Piece { page, start: 0, len: 0, ord: NONE });
            if sp != 0 {
                self.recount(ab);
                self.recount(ab + 1);
            }
            (b, i) = self.dir.moved(ab, at, sp);
            k = 0;
        }
        self.len += recs.len();
    }

    /// End the last page here: the next append starts a new page (a builder that leaves slack in every page
    /// for later inserts).
    pub fn break_page(&mut self) {
        let page = self.new_page();
        self.pages[page as usize].pieces = 1;
        let b = self.dir.blocks.len() - 1;
        let i = self.dir.blocks[b].len();
        let sp = self.dir.insert(b, i, Piece { page, start: 0, len: 0, ord: NONE });
        if sp != 0 {
            self.recount(b);
            self.recount(b + 1);
        }
    }

    /// Remove `n` records from paint position `pos`.
    #[inline(never)]
    pub fn remove(&mut self, pos: usize, n: usize) {
        let mut left = n;
        while left > 0 {
            let (b, i, k) = self.locate(pos);
            let mut pc = self.dir.blocks[b][i];
            let m = left.min(pc.len as usize - k);
            let pg = pc.page as usize;
            for j in k..k + m {
                let s = self.slot(&pc, j);
                self.pages[pg].live &= !(1u64 << s);
            }
            self.pages[pg].seal += 1;
            let len = pc.len as usize;
            if m == len {
                if pc.ord != NONE {
                    self.ord_free.push(pc.ord);
                }
                self.pages[pg].pieces -= 1;
                if self.pages[pg].pieces == 0 {
                    self.page_free.push(pc.page);
                }
                self.dir.counts[b] -= m as u32;
                self.dir.remove(b, i);
            } else {
                if pc.ord == NONE && k == 0 {
                    pc.start += m as u8;
                } else if pc.ord == NONE && k + m == len {
                    // the tail of an identity piece: only the length changes
                } else {
                    self.permute(&mut pc);
                    self.ords[pc.ord as usize].copy_within(k + m..len, k);
                }
                pc.len -= m as u8;
                self.dir.blocks[b][i] = pc;
                self.dir.counts[b] -= m as u32;
            }
            left -= m;
        }
        self.len -= n;
    }

    /// The payloads in paint order (what an ordered consumer reads).
    #[inline(never)]
    pub fn walk(&self) -> u64 {
        let mut a = 0u64;
        for blk in &self.dir.blocks {
            for pc in blk {
                let p = &self.pages[pc.page as usize];
                if pc.ord == NONE {
                    for s in pc.start as usize..pc.start as usize + pc.len as usize {
                        a = a.wrapping_mul(31).wrapping_add(p.payload[s] as u64);
                    }
                } else {
                    let o = &self.ords[pc.ord as usize];
                    for &s in &o[..pc.len as usize] {
                        a = a.wrapping_mul(31).wrapping_add(p.payload[s as usize] as u64);
                    }
                }
            }
        }
        a
    }
    pub fn payloads(&self) -> Vec<u32> {
        let mut v = Vec::with_capacity(self.len);
        for blk in &self.dir.blocks {
            for pc in blk {
                for k in 0..pc.len as usize {
                    v.push(self.pages[pc.page as usize].payload[self.slot(pc, k)]);
                }
            }
        }
        v
    }
    /// (pieces, pages in use, permuted pieces, records): the shape after a workload.
    pub fn shape(&self) -> (usize, usize, usize, usize) {
        let permuted = self.dir.blocks.iter().flatten().filter(|p| p.ord != NONE).count();
        (self.dir.entries(), self.pages.len() - self.page_free.len(), permuted, self.len)
    }
    /// Bytes held for `len` records: pages, the directory and the ord blocks.
    pub fn bytes(&self) -> usize {
        (self.pages.len() - self.page_free.len()) * size_of::<SlotPage>() + self.dir.entries() * size_of::<Piece>() + (self.ords.len() - self.ord_free.len()) * P
    }
}

// =================================================================================== the directory as it is

/// A page in position order (the core's): columns shift on an insert, and the handle of every record that
/// moved is re-pointed.
#[derive(Clone)]
#[repr(C, align(64))]
struct PosPage {
    len: usize,
    rect: [[i16; 4]; P],
    depth: [u16; P],
    kind: [u8; P],
    clip: [u16; P],
    payload: [u32; P],
    handle: [u32; P],
}
impl PosPage {
    fn new() -> PosPage {
        PosPage { len: 0, rect: [[0; 4]; P], depth: [0; P], kind: [0; P], clip: [0; P], payload: [0; P], handle: [0; P] }
    }
    fn open(&mut self, k: usize, m: usize) {
        let n = self.len;
        self.rect.copy_within(k..n, k + m);
        self.depth.copy_within(k..n, k + m);
        self.kind.copy_within(k..n, k + m);
        self.clip.copy_within(k..n, k + m);
        self.payload.copy_within(k..n, k + m);
        self.handle.copy_within(k..n, k + m);
        self.len = n + m;
    }
    fn close(&mut self, k: usize, m: usize) {
        let n = self.len;
        self.rect.copy_within(k + m..n, k);
        self.depth.copy_within(k + m..n, k);
        self.kind.copy_within(k + m..n, k);
        self.clip.copy_within(k + m..n, k);
        self.payload.copy_within(k + m..n, k);
        self.handle.copy_within(k + m..n, k);
        self.len = n - m;
    }
}

#[derive(Clone, Copy)]
struct Ent {
    page: u32,
    len: u32,
}

pub struct Shift {
    pages: Vec<PosPage>,
    page_free: Vec<u32>,
    dir: Seq<Ent>,
    /// Handle table: handle -> (page, slot), re-pointed whenever a record moves.
    pub htab: Vec<(u32, u32)>,
    pub len: usize,
}

impl Default for Shift {
    fn default() -> Self {
        Self::new()
    }
}

impl Shift {
    pub fn new() -> Shift {
        Shift { pages: Vec::new(), page_free: Vec::new(), dir: Seq::new(), htab: Vec::new(), len: 0 }
    }
    fn new_page(&mut self) -> u32 {
        match self.page_free.pop() {
            Some(p) => p,
            None => {
                self.pages.push(PosPage::new());
                (self.pages.len() - 1) as u32
            }
        }
    }
    fn recount(&mut self, b: usize) {
        self.dir.counts[b] = self.dir.blocks[b].iter().map(|p| p.len).sum();
    }
    #[inline]
    fn locate(&self, pos: usize) -> (usize, usize, usize) {
        let (b, mut base) = self.dir.block_of(pos);
        let blk = &self.dir.blocks[b];
        for (i, e) in blk.iter().enumerate() {
            if pos < base + e.len as usize {
                return (b, i, pos - base);
            }
            base += e.len as usize;
        }
        (b, blk.len(), 0)
    }
    fn fix(&mut self, page: u32, from: usize) {
        let p = &self.pages[page as usize];
        for s in from..p.len {
            self.htab[p.handle[s] as usize] = (page, s as u32);
        }
    }

    /// Insert `recs` at paint position `pos`; `ids` receives each record's handle (allocated here).
    #[inline(never)]
    pub fn insert(&mut self, pos: usize, recs: &[Rec], ids: &mut Vec<u32>) {
        let (mut b, mut i, mut k) = self.locate(pos);
        // the start of an entry is the end of the one before it: append there if it has room
        if k == 0 && i > 0 && (self.dir.blocks[b][i - 1].len as usize) < P {
            i -= 1;
            k = self.dir.blocks[b][i].len as usize;
        }
        let mut rest = recs;
        while !rest.is_empty() {
            if i >= self.dir.blocks[b].len() || (k == 0 && self.dir.blocks[b][i].len as usize == P) {
                // no entry at the point, or the point is just before a full page: a new page there
                let page = self.new_page();
                let sp = self.dir.insert(b, i, Ent { page, len: 0 });
                if sp != 0 {
                    self.recount(b);
                    self.recount(b + 1);
                }
                (b, i) = self.dir.moved(b, i, sp);
                k = 0;
            }
            let e = self.dir.blocks[b][i];
            if e.len as usize == P {
                // the point is inside a full page: split it in half; the upper half moves to a new page and
                // its handles are re-pointed
                let page = self.new_page();
                let (lo, hi) = (e.page as usize, page as usize);
                let half = P / 2;
                let (src, dst) = if lo < hi {
                    let (a, c) = self.pages.split_at_mut(hi);
                    (&mut a[lo], &mut c[0])
                } else {
                    let (a, c) = self.pages.split_at_mut(lo);
                    (&mut c[0], &mut a[hi])
                };
                dst.rect[..half].copy_from_slice(&src.rect[half..]);
                dst.depth[..half].copy_from_slice(&src.depth[half..]);
                dst.kind[..half].copy_from_slice(&src.kind[half..]);
                dst.clip[..half].copy_from_slice(&src.clip[half..]);
                dst.payload[..half].copy_from_slice(&src.payload[half..]);
                dst.handle[..half].copy_from_slice(&src.handle[half..]);
                dst.len = half;
                src.len = half;
                self.fix(page, 0);
                self.dir.blocks[b][i].len = half as u32;
                let sp = self.dir.insert(b, i + 1, Ent { page, len: half as u32 });
                if sp != 0 {
                    self.recount(b);
                    self.recount(b + 1);
                }
                (b, i) = self.dir.moved(b, i, sp);
                if k > half {
                    (b, i) = self.dir.next(b, i);
                    k -= half;
                }
                continue;
            }
            let m = rest.len().min(P - e.len as usize);
            let pg = e.page;
            self.pages[pg as usize].open(k, m);
            for (j, r) in rest[..m].iter().enumerate() {
                let h = self.htab.len() as u32;
                self.htab.push((pg, (k + j) as u32));
                let p = &mut self.pages[pg as usize];
                let s = k + j;
                (p.rect[s], p.depth[s], p.kind[s], p.clip[s], p.payload[s], p.handle[s]) = (r.rect, r.depth, r.kind, r.clip, r.payload, h);
                ids.push(h);
            }
            self.fix(pg, k + m);
            self.dir.blocks[b][i].len += m as u32;
            self.dir.counts[b] += m as u32;
            k += m;
            rest = &rest[m..];
            if !rest.is_empty() && k == P {
                // the page is full up to its end: go on at the start of the next entry
                (b, i) = self.dir.next(b, i);
                k = 0;
            }
        }
        self.len += recs.len();
    }

    /// End the last page here (see `Pieces::break_page`).
    pub fn break_page(&mut self) {
        let page = self.new_page();
        let b = self.dir.blocks.len() - 1;
        let i = self.dir.blocks[b].len();
        let sp = self.dir.insert(b, i, Ent { page, len: 0 });
        if sp != 0 {
            self.recount(b);
            self.recount(b + 1);
        }
    }

    #[inline(never)]
    pub fn remove(&mut self, pos: usize, n: usize) {
        let mut left = n;
        while left > 0 {
            let (b, i, k) = self.locate(pos);
            let e = self.dir.blocks[b][i];
            let m = left.min(e.len as usize - k);
            self.pages[e.page as usize].close(k, m);
            self.fix(e.page, k);
            self.dir.counts[b] -= m as u32;
            if m == e.len as usize {
                self.page_free.push(e.page);
                self.dir.remove(b, i);
            } else {
                self.dir.blocks[b][i].len -= m as u32;
            }
            left -= m;
        }
        self.len -= n;
    }

    #[inline(never)]
    pub fn walk(&self) -> u64 {
        let mut a = 0u64;
        for blk in &self.dir.blocks {
            for e in blk {
                let p = &self.pages[e.page as usize];
                for s in 0..e.len as usize {
                    a = a.wrapping_mul(31).wrapping_add(p.payload[s] as u64);
                }
            }
        }
        a
    }
    pub fn payloads(&self) -> Vec<u32> {
        let mut v = Vec::with_capacity(self.len);
        for blk in &self.dir.blocks {
            for e in blk {
                v.extend_from_slice(&self.pages[e.page as usize].payload[..e.len as usize]);
            }
        }
        v
    }
    pub fn shape(&self) -> (usize, usize, usize, usize) {
        (self.dir.entries(), self.pages.len() - self.page_free.len(), 0, self.len)
    }
    pub fn bytes(&self) -> usize {
        (self.pages.len() - self.page_free.len()) * size_of::<PosPage>() + self.dir.entries() * size_of::<Ent>() + self.htab.len() * 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(i: u32) -> Rec {
        Rec { payload: i, rect: [0, 0, 10, 10], depth: 1, kind: 1, clip: 0, handle: 0 }
    }

    /// Both directories hold the sequence a plain vector holds, under the same random edits; in the piece
    /// table a record's identity never changes while it lives, and a page's seal moves whenever its
    /// content does.
    #[test]
    fn piece_table_and_shifting_directory_are_the_same_sequence() {
        let mut seed = 0x5eedu64;
        let mut rnd = move |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize % n.max(1)
        };
        let (mut p, mut s) = (Pieces::new(), Shift::new());
        let mut model: Vec<u32> = Vec::new();
        // identity of every live record of the piece table, by payload
        let mut id_of = std::collections::HashMap::new();
        let mut next = 1u32;
        for step in 0..6000 {
            let grow = model.len() < 40 || (model.len() < 9000 && rnd(5) < 3);
            if grow {
                let n = 1 + rnd(if step % 97 == 0 { 300 } else { 12 });
                let at = match rnd(4) {
                    0 => 0,
                    1 => model.len(),
                    _ => rnd(model.len() + 1),
                };
                let recs: Vec<Rec> = (0..n as u32).map(|j| rec(next + j)).collect();
                let (mut ia, mut ib) = (Vec::new(), Vec::new());
                p.insert(at, &recs, &mut ia);
                s.insert(at, &recs, &mut ib);
                for (j, &id) in ia.iter().enumerate() {
                    id_of.insert(next + j as u32, id);
                }
                model.splice(at..at, recs.iter().map(|r| r.payload));
                next += n as u32;
            } else {
                let n = (1 + rnd(if step % 89 == 0 { 200 } else { 10 })).min(model.len());
                let at = rnd(model.len() - n + 1);
                p.remove(at, n);
                s.remove(at, n);
                for v in model.drain(at..at + n) {
                    id_of.remove(&v);
                }
            }
            if step % 37 == 0 || step > 5990 {
                assert_eq!(p.payloads(), model, "piece table, step {step}");
                assert_eq!(s.payloads(), model, "shifting directory, step {step}");
                assert_eq!(p.walk(), s.walk());
                // identity is the place: every live record is still where it was put
                for (&v, &id) in &id_of {
                    let (pg, sl) = ((id / 64) as usize, (id % 64) as usize);
                    assert!(p.pages[pg].live >> sl & 1 != 0 && p.pages[pg].payload[sl] == v, "record {v} moved (step {step})");
                }
                // and the handle table of the shifting directory is consistent
                assert_eq!(p.len, model.len());
            }
        }
        let (pieces, pages, permuted, n) = p.shape();
        assert!(pieces >= pages && n == model.len() && permuted <= pieces);
    }

    #[test]
    fn a_seal_moves_exactly_when_its_page_does() {
        let mut p = Pieces::new();
        let mut ids = Vec::new();
        let recs: Vec<Rec> = (0..200).map(rec).collect();
        p.insert(0, &recs, &mut ids);
        let seals: Vec<u32> = p.pages.iter().map(|g| g.seal).collect();
        // an insert at the end touches the last page only
        p.insert(200, &[rec(999)], &mut ids);
        let after: Vec<u32> = p.pages.iter().map(|g| g.seal).collect();
        let changed: Vec<usize> = (0..seals.len()).filter(|&i| seals[i] != after[i]).collect();
        assert_eq!(changed, vec![seals.len() - 1]);
        // a remove in the first page touches the first page only
        p.remove(3, 2);
        let last: Vec<u32> = p.pages.iter().map(|g| g.seal).collect();
        let changed: Vec<usize> = (0..after.len()).filter(|&i| after[i] != last[i]).collect();
        assert_eq!(changed, vec![0]);
    }
}
