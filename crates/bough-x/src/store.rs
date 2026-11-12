//! Archetype columns in 64-record pages (ROUND8 section 4) and the damage they produce (section 3).
//!
//! A [`Store`] holds the records of one static shape: the fixed part of a view, or the rows of one list. It is
//! not generic. A column is created by the first record described at a position of the shape and is written
//! through a typed pointer; every pass over a column (damage, hit testing, paint) is one function monomorphised
//! for the column's archetype, reached once per column through its [`Desc`].
use crate::arena::{Ar, DeltaFn, Entry, Half, LINE, Motion, PaintFn};
use crate::expr::{StageId, U, U_TIME, Words};
use core::alloc::Layout;
use core::any::TypeId;

/// Device pixels, half open.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}
impl Rect {
    #[inline(always)]
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x0: x, y0: y, x1: x + w, y1: y + h }
    }
    #[inline(always)]
    pub fn shift(self, d: (i32, i32)) -> Rect {
        Rect { x0: self.x0 + d.0, y0: self.y0 + d.1, x1: self.x1 + d.0, y1: self.y1 + d.1 }
    }
    #[inline(always)]
    pub fn and(self, o: Rect) -> Rect {
        Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }
    #[inline(always)]
    pub fn or(self, o: Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    #[inline(always)]
    pub fn empty(self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    #[inline(always)]
    pub fn hits(self, o: Rect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
    #[inline(always)]
    pub fn area(self) -> i64 {
        (self.x1 - self.x0) as i64 * (self.y1 - self.y0) as i64
    }
}

pub const TILE_SHIFT: u32 = 5;
pub const TILE: i32 = 1 << TILE_SHIFT;

/// Damage: one bit per 32-pixel device tile, `stride` words per tile row.
pub struct Tiles {
    pub w: i32,
    pub h: i32,
    pub stride: usize,
    pub rows: usize,
    pub bits: Vec<u64>,
    /// The tile rows that may hold a mark: `lo..hi`. Reading and clearing the damage touch only these.
    pub lo: usize,
    pub hi: usize,
}
impl Tiles {
    pub fn new(w: i32, h: i32) -> Tiles {
        let cols = ((w + TILE - 1) >> TILE_SHIFT) as usize;
        let rows = ((h + TILE - 1) >> TILE_SHIFT) as usize;
        let stride = cols.div_ceil(64).max(1);
        Tiles { w, h, stride, rows, bits: vec![0; stride * rows], lo: usize::MAX, hi: 0 }
    }
    /// Mark the tiles a screen rectangle touches.
    #[inline]
    pub fn mark(&mut self, r: Rect) {
        let (x0, y0, x1, y1) = (r.x0.max(0), r.y0.max(0), r.x1.min(self.w), r.y1.min(self.h));
        if (x0 >= x1) | (y0 >= y1) {
            return;
        }
        let (a, b) = ((x0 >> TILE_SHIFT) as usize, ((x1 - 1) >> TILE_SHIFT) as usize);
        let (c, d) = ((y0 >> TILE_SHIFT) as usize, ((y1 - 1) >> TILE_SHIFT) as usize);
        self.lo = self.lo.min(c);
        self.hi = self.hi.max(d + 1);
        let (wa, wb) = (a >> 6, b >> 6);
        let (ma, mb) = (!0u64 << (a & 63), !0u64 >> (63 - (b & 63)));
        debug_assert!(d < self.rows && wb < self.stride);
        // SAFETY: the rect was cut to the window, so `c..=d` are tile rows and `wa..=wb` words of a row; `bits`
        // holds `rows * stride` words.
        unsafe {
            let mut p = self.bits.as_mut_ptr().add(c * self.stride);
            if wa == wb {
                let m = ma & mb;
                for _ in c..=d {
                    *p.add(wa) |= m;
                    p = p.add(self.stride);
                }
            } else {
                for _ in c..=d {
                    *p.add(wa) |= ma;
                    for w in wa + 1..wb {
                        *p.add(w) = !0;
                    }
                    *p.add(wb) |= mb;
                    p = p.add(self.stride);
                }
            }
        }
    }
    pub fn mark_all(&mut self) {
        self.mark(Rect { x0: 0, y0: 0, x1: self.w, y1: self.h });
    }
    #[inline]
    pub fn clear(&mut self) {
        if self.lo < self.hi {
            self.bits[self.lo * self.stride..self.hi * self.stride].fill(0);
        }
        (self.lo, self.hi) = (usize::MAX, 0);
    }
    #[inline]
    pub fn any(&self) -> bool {
        self.lo < self.hi
    }
    #[inline]
    pub fn count(&self) -> u32 {
        if self.lo < self.hi { self.bits[self.lo * self.stride..self.hi * self.stride].iter().map(|w| w.count_ones()).sum() } else { 0 }
    }
    /// Each marked tile as a screen rectangle.
    pub fn each(&self, mut f: impl FnMut(Rect)) {
        for ty in self.lo.min(self.hi)..self.hi {
            for w in 0..self.stride {
                let mut m = self.bits[ty * self.stride + w];
                while m != 0 {
                    let tx = w * 64 + m.trailing_zeros() as usize;
                    m &= m - 1;
                    f(Rect::new(tx as i32 * TILE, ty as i32 * TILE, TILE, TILE).and(Rect { x0: 0, y0: 0, x1: self.w, y1: self.h }));
                }
            }
        }
    }
}

pub const NO_WORD: u8 = 0xff;

/// A coordinate space: where a store's records are on screen. The origin moves with one word of the block
/// (a scroll offset, an `i32` of device pixels), so scrolling rewrites no record.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Space {
    pub clip: Rect,
    pub base: (i32, i32),
    pub scroll: u8,
}
impl Space {
    pub const fn root(w: i32, h: i32) -> Space {
        Space { clip: Rect { x0: 0, y0: 0, x1: w, y1: h }, base: (0, 0), scroll: NO_WORD }
    }
    #[inline(always)]
    pub fn origin(&self, u: &U) -> (i32, i32) {
        if self.scroll == NO_WORD { self.base } else { (self.base.0, self.base.1 - u.w[self.scroll as usize & 63] as i32) }
    }
    #[inline(always)]
    pub fn reads(&self) -> u64 {
        if self.scroll == NO_WORD { 0 } else { 1 << (self.scroll & 63) }
    }
}

/// One page of a column: four rectangle lanes and the records' constants. Layout results and paint operands are
/// lanes of the same page.
#[repr(C, align(64))]
pub struct Page<A> {
    pub x0: [i32; 64],
    pub y0: [i32; 64],
    pub x1: [i32; 64],
    pub y1: [i32; 64],
    pub rec: [A; 64],
}
impl<A> Page<A> {
    #[inline(always)]
    pub fn rect(&self, i: usize) -> Rect {
        Rect { x0: self.x0[i], y0: self.y0[i], x1: self.x1[i], y1: self.y1[i] }
    }
    #[inline(always)]
    pub fn set_rect(&mut self, i: usize, r: Rect) {
        self.x0[i] = r.x0;
        self.y0[i] = r.y0;
        self.x1[i] = r.x1;
        self.y1[i] = r.y1;
    }
}

/// An archetype: a paint kind applied to expression types. The value is one record's constants.
pub trait Arch: Words {
    /// The words of the block whose change can change what a record of this archetype shows.
    const READS: u64;
    /// The record's place on screen depends on the block (a Frame-stage transform).
    const MOVES: bool = false;
    const NAME: &'static str;
    /// The latest stage among its expressions.
    const STAGE: StageId;
    /// The record's paint reads `Time`: it can change with no turn.
    const TIMED: bool = false;
    /// The time at which this record comes to rest under `u` (0: it is at rest; `u32::MAX`: never). Between now
    /// and then it stays inside the box of where it is now and where it will rest (motion along a segment).
    #[inline(always)]
    fn until(&self, _u: &U) -> u32 {
        0
    }
    /// The derivative: does this record show something else under `new` than under `old`?
    fn changed(&self, old: &U, new: &U) -> bool;
    #[inline(always)]
    fn offset(&self, _u: &U) -> (i32, i32) {
        (0, 0)
    }
    /// Hand the column to the backend method for this paint kind.
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U);
}

/// What a raster implements: one generic method per paint kind, monomorphised per archetype.
pub trait Backend: 'static {
    fn fill<A: crate::kinds::FillArch>(&mut self, col: ColRef<'_, A>, u: &U);
    fn num<A: crate::kinds::NumArch>(&mut self, col: ColRef<'_, A>, u: &U);
    fn label<A: crate::kinds::LabelArch>(&mut self, col: ColRef<'_, A>, u: &U);
    fn outline<A: crate::kinds::OutlineArch>(&mut self, col: ColRef<'_, A>, u: &U);
}

/// A typed view of a column for a pass.
pub struct ColRef<'a, A> {
    pub pages: &'a [Page<A>],
    pub space: &'a Space,
    pub live: Live<'a>,
}
impl<'a, A> ColRef<'a, A> {
    /// Every live record, page by page.
    #[inline(always)]
    pub fn each(&self, mut f: impl FnMut(Rect, &A)) {
        for (pi, pg) in self.pages.iter().enumerate() {
            let mut m = self.live.page(pi);
            while m != 0 {
                let i = m.trailing_zeros() as usize;
                m &= m - 1;
                f(pg.rect(i), &pg.rec[i]);
            }
        }
    }
}

/// Which records of a column are live. Liveness is the store's: a row is live or not, for all of its records.
#[derive(Clone, Copy)]
pub struct Live<'a> {
    pub cap: u32,
    pub rows: &'a [u64],
    pub nrec: u32,
}
impl Live<'_> {
    #[inline(always)]
    pub fn page(&self, pi: usize) -> u64 {
        if self.cap >= 64 {
            self.rows[pi & (self.rows.len() - 1)]
        } else {
            // cap == 1: one row, records packed densely; the row is live as a whole
            let n = self.nrec as usize - pi * 64;
            let m = if n >= 64 { !0 } else { (1u64 << n) - 1 };
            if self.rows[0] & 1 != 0 { m } else { 0 }
        }
    }
}

pub type HoverFn = unsafe fn(*mut (), *const u8, bool);
pub type HitFn = unsafe fn(Half, &Entry, &U, i32, i32) -> u32;
/// A column that hears the pointer: its handler and the hit kernel of its archetype (instantiated only for
/// archetypes that have a handler).
pub type Hov = (HoverFn, HitFn);
#[inline(always)]
pub fn hit_fn<A: Arch>() -> HitFn {
    hit_col::<A>
}

/// The cold face of an archetype: what a store needs to make a column it cannot name the type of.
pub struct Desc {
    pub name: &'static str,
    pub stage: StageId,
    pub rec_size: usize,
    pub rec_off: usize,
    pub page: Layout,
    pub reads: u64,
    pub moves: bool,
    delta: DeltaFn,
    paint: PaintFn,
}

struct Holder<A, B>(core::marker::PhantomData<(A, B)>);
impl<A: Arch, B: Backend> Holder<A, B> {
    const DESC: Desc = Desc {
        name: A::NAME,
        stage: A::STAGE,
        rec_size: size_of::<A>(),
        rec_off: core::mem::offset_of!(Page<A>, rec),
        page: Layout::new::<Page<A>>(),
        reads: A::READS,
        moves: A::MOVES,
        delta: delta_col::<A>,
        paint: paint_col::<A, B>,
    };
}
#[inline(always)]
pub fn desc<A: Arch, B: Backend>() -> &'static Desc {
    &Holder::<A, B>::DESC
}

/// A column as its store knows it: the cold part. What a pass needs is its [`Entry`] in the plan.
pub struct RawCol {
    /// Its entry in the plan.
    pub pass: u32,
    /// Sites (positions of the shape) that share this column.
    pub k: u32,
    /// Records the column has room for: sites x the store's row capacity.
    pub nrec: u32,
    /// Pages its span holds, where the span starts (lines from a half's base) and its first dirty-range id.
    alloc: u32,
    pages: u32,
    rid: u32,
    pub key: TypeId,
    pub desc: &'static Desc,
    /// How many child stores were described before this column's first site: its place in paint order.
    pub seg: u32,
    pub hover: Option<Hov>,
}
impl RawCol {
    /// The constants of record `rec` in the working half, untyped (for a handler that knows the type).
    pub fn rec_ptr(&self, ar: Ar, rec: u32) -> *const u8 {
        assert!(rec < self.alloc * 64);
        // SAFETY: in bounds of the column's span by the assert; `rec_off` and `rec_size` are the archetype's.
        unsafe { ar.work().at(self.pages).add((rec as usize >> 6) * self.desc.page.size() + self.desc.rec_off + (rec as usize & 63) * self.desc.rec_size) }
    }
}

#[derive(Clone, Copy)]
pub struct Site {
    /// The page, in the working half, that holds this site's slot 0 (slot `s` is record `s mod 64` of page
    /// `s / 64` from here).
    pub page: *mut u8,
    pub page_size: u32,
    pub col: u32,
    /// The first record of this site: record = base + row slot.
    pub base: u32,
    /// The dirty-range id of that page.
    pub rid: u32,
}
impl Site {
    /// Store a record's rect. The four rect lanes are the first kilobyte of every page, whatever its archetype.
    ///
    /// SAFETY: `slot` is under the store's capacity and the site's pointers are current (`Store::seal`).
    #[inline(always)]
    pub unsafe fn set_rect(&self, slot: u32, r: Rect) {
        // SAFETY: the page exists (caller); `Page<A>` is `repr(C)` and starts with four `[i32; 64]` lanes.
        unsafe {
            let p = (self.page.add((slot >> 6) as usize * self.page_size as usize) as *mut i32).add((self.base + slot) as usize & 63);
            *p = r.x0;
            *p.add(64) = r.y0;
            *p.add(128) = r.x1;
            *p.add(192) = r.y1;
        }
    }
    /// SAFETY: as `set_rect`.
    #[inline(always)]
    pub unsafe fn rect(&self, slot: u32) -> Rect {
        // SAFETY: as `set_rect`.
        unsafe {
            let p = (self.page.add((slot >> 6) as usize * self.page_size as usize) as *const i32).add((self.base + slot) as usize & 63);
            Rect { x0: *p, y0: *p.add(64), x1: *p.add(128), y1: *p.add(192) }
        }
    }
    /// The record at `slot`.
    ///
    /// SAFETY: as `set_rect`, and `A` is the archetype of the site's column.
    #[inline(always)]
    pub unsafe fn rec<A>(&self, slot: u32) -> *mut A {
        // SAFETY: as `set_rect`; the page is a `Page<A>`.
        unsafe { (&raw mut (*(self.page as *mut Page<A>).add((slot >> 6) as usize)).rec as *mut A).add((self.base + slot) as usize & 63) }
    }
}

/// Which store, from the root: the child index at each level.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Path {
    pub at: [u16; 6],
    pub n: u16,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pass {
    Col(u32),
    Sub(u32),
}

/// What the sink needs to find its sites again: the cold tree. No pass over records reads it; passes read the
/// plan (`arena::Entry`), which `seal` and `reorder` keep in step with it.
pub struct Store {
    pub ar: Ar,
    pub cols: Vec<RawCol>,
    pub sites: Vec<Site>,
    pub subs: Vec<Store>,
    /// Paint order: columns and child stores in the order the view first described them.
    pub order: Vec<Pass>,
    /// Row slots: 1 for a fixed shape, a power of two of at least 64 for a list's window.
    pub cap: u32,
    /// Its live words: index into a half's live words (`cap / 64` of them, at least one).
    pub live: u32,
    space: Space,
    pub space_ix: u16,
    disc: Vec<(u32, Rect)>,
    /// A list's materialised window of rows, `lo..hi`; row `r` is at slot `r mod cap`.
    pub lo: u32,
    pub hi: u32,
    /// The relation version this window was last brought up to.
    pub seen: u32,
    /// The rows of the shape: the type of each row value described here and its first site.
    pub rows: Vec<(TypeId, u32)>,
    /// A list's layout: the rects of one row relative to the row, and where and how tall rows are.
    pub tmpl: Vec<Rect>,
    /// Where the slice `tmpl` was copied from was: an identity check before a content compare.
    pub tmpl_at: usize,
    pub geo: (i32, i32, i32),
    /// Scratch: the rows a list pass visits (row index; top bit set: write without comparing).
    pub plan: Vec<u32>,
}

impl Store {
    pub fn new(ar: Ar, cap: u32, space: Space) -> Store {
        assert!(cap == 1 || (cap >= 64 && cap.is_power_of_two()));
        let live = ar.new_live((cap / 64).max(1));
        let space_ix = ar.new_space();
        let mut st = Store { ar, cols: Vec::new(), sites: Vec::new(), subs: Vec::new(), order: Vec::new(), cap, live, space: Space::root(0, 0), space_ix, disc: Vec::new(), lo: 0, hi: 0, seen: 0, rows: Vec::new(), tmpl: Vec::new(), tmpl_at: 0, geo: (0, 0, 0), plan: Vec::new() };
        st.put_space(space);
        st
    }
    #[inline(always)]
    pub fn space(&self) -> Space {
        self.space
    }
    /// Give the store another space. Nothing happens when it is the one it has.
    #[inline]
    pub fn set_space(&mut self, space: Space) {
        if self.space != space {
            self.put_space(space);
        }
    }
    #[cold]
    #[inline(never)]
    fn put_space(&mut self, space: Space) {
        self.space = space;
        let (ar, w) = (self.ar, self.ar.work());
        // SAFETY: `space_ix` is this store's slot of the spaces table; the turn thread is the only writer.
        unsafe { w.space(self.space_ix).write(space) };
        ar.dirty_space(self.space_ix);
        ar.head().scrolls |= space.reads();
        for c in &self.cols {
            // SAFETY: a sealed column's entry is initialised; an unsealed one (`alloc == 0`) is skipped.
            if c.alloc != 0 {
                unsafe { (*w.entry(c.pass as usize)).skip = space.reads() };
                ar.dirty_plan(c.pass);
            }
        }
    }
    /// The store's live words, in the working half.
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub fn live_mut(&self) -> &mut [u64] {
        // SAFETY: `new_live` handed this store `max(cap / 64, 1)` words from `live`; turn thread only.
        unsafe { core::slice::from_raw_parts_mut(self.ar.work().live(self.live), (self.cap as usize / 64).max(1)) }
    }
    /// Note that the live words were written.
    pub fn live_dirty(&self) {
        let n = (self.cap / 64).max(1);
        let mut w = self.live;
        while w < self.live + n {
            self.ar.dirty_live(w);
            w = (w / 8 + 1) * 8;
        }
    }
    /// A fixed shape is live as a whole.
    #[inline]
    pub fn live_one(&self) {
        let l = self.live_mut();
        if l[0] != 1 {
            l[0] = 1;
            self.live_dirty();
        }
    }
    /// Give the next position of the shape a column. Cold: runs once per position per store, and is not
    /// generic: an archetype is its descriptor here. The column gets its memory in [`Store::seal`].
    ///
    /// A position joins an existing column of its archetype when painting the column as a whole keeps painter's
    /// order: the column is in the same segment (no child store between them) and no record described since, in
    /// a later column, overlaps the new one.
    #[cold]
    #[inline(never)]
    pub fn discover(&mut self, d: &'static Desc, rect: Rect, key: TypeId, hover: Option<Hov>) {
        let seg = self.subs.len() as u32;
        let mut pick = None;
        for (j, c) in self.cols.iter().enumerate() {
            if c.key == key && c.seg == seg && !self.disc.iter().any(|&(cj, r)| cj as usize > j && r.hits(rect)) {
                pick = Some(j);
                break;
            }
        }
        let j = pick.unwrap_or_else(|| {
            self.cols.push(RawCol { pass: self.ar.new_col(), k: 0, nrec: 0, alloc: 0, pages: 0, rid: 0, key, desc: d, seg, hover });
            self.order.push(Pass::Col(self.cols.len() as u32 - 1));
            self.cols.len() - 1
        });
        let c = &mut self.cols[j];
        let base = c.k * self.cap;
        c.k += 1;
        c.nrec = c.k * self.cap;
        self.disc.push((j as u32, rect));
        self.sites.push(Site { page: core::ptr::null_mut(), page_size: 0, col: j as u32, base, rid: 0 });
    }
    /// After the positions of a row were discovered: give every column that grew its pages, write its plan
    /// entry, and point the sites at their pages. Cold.
    ///
    /// A column's span is exactly what its sites need, packed after the last one. A column that had pages and
    /// outgrew them (a fixed shape whose later row joins it) moves to a new span; the old one is abandoned.
    #[cold]
    #[inline(never)]
    pub fn seal(&mut self) {
        let (ar, w) = (self.ar, self.ar.work());
        for c in &mut self.cols {
            let need = c.nrec.div_ceil(64);
            if need <= c.alloc {
                continue;
            }
            let lines = (c.desc.page.size() / LINE) as u32;
            let (at, rid) = ar.new_span(need, lines);
            if c.alloc != 0 {
                // SAFETY: both spans are inside the working half and do not overlap (the new one was just
                // handed out); the old one holds `alloc` pages.
                unsafe { core::ptr::copy_nonoverlapping(w.at(c.pages), w.at(at), c.alloc as usize * c.desc.page.size()) };
                for p in 0..c.alloc {
                    ar.dirty(rid + p);
                }
                ar.head().wasted += c.alloc * lines;
            }
            (c.alloc, c.pages, c.rid) = (need, at, rid);
        }
        for c in &self.cols {
            // SAFETY: `pass` is this column's entry (`new_col`); written whole here before any pass reads it.
            unsafe {
                w.entry(c.pass as usize).write(Entry { reads: c.desc.reads, skip: self.space.reads(), delta: c.desc.delta, paint: c.desc.paint, pages: c.pages, nrec: c.nrec, live: self.live, cap: self.cap, space: self.space_ix, flags: 0, rid: c.rid, desc: c.desc })
            };
            ar.dirty_plan(c.pass);
        }
        for s in &mut self.sites {
            let c = &self.cols[s.col as usize];
            // SAFETY: `base / 64` is a page of the column's span (sized for `nrec` above).
            s.page = unsafe { w.at(c.pages).add((s.base >> 6) as usize * c.desc.page.size()) };
            s.page_size = c.desc.page.size() as u32;
            s.rid = c.rid + (s.base >> 6);
        }
    }

    /// Put the plan in paint order: the order the view first described columns and child stores in. Cold: runs
    /// after a pass that created a column.
    pub fn reorder(&mut self) {
        let ar = self.ar;
        let n = ar.head().ncols as usize;
        let mut perm = Vec::with_capacity(n);
        self.walk(&mut |c| perm.push(c.pass));
        assert!(perm.len() == n, "a column was discovered and not sealed");
        ar.head().unordered = 0;
        if perm.iter().enumerate().all(|(i, &p)| i as u32 == p) {
            return;
        }
        let w = ar.work();
        // SAFETY: entries below `ncols` are initialised (`seal`); they are moved as plain bytes.
        unsafe {
            let old: Vec<[u8; 64]> = (0..n).map(|i| (w.entry(i) as *const [u8; 64]).read()).collect();
            let mo: Vec<Motion> = (0..n).map(|i| *ar.motion(i)).collect();
            for (i, &p) in perm.iter().enumerate() {
                (w.entry(i) as *mut [u8; 64]).write(old[p as usize]);
                *ar.motion(i) = mo[p as usize];
                ar.dirty_plan(i as u32);
            }
        }
        let mut i = 0;
        self.walk(&mut |c| {
            c.pass = i;
            i += 1;
        });
    }
    fn walk(&mut self, f: &mut impl FnMut(&mut RawCol)) {
        for k in 0..self.order.len() {
            match self.order[k] {
                Pass::Col(j) => f(&mut self.cols[j as usize]),
                Pass::Sub(q) => self.subs[q as usize].walk(f),
            }
        }
    }

    /// The topmost record with a hover handler under a point: (path of child indices, column, record).
    pub fn hit(&self, u: &U, x: i32, y: i32, path: &mut Path, best: &mut Option<(Path, u32, u32)>) {
        let w = self.ar.work();
        for p in &self.order {
            match *p {
                Pass::Col(j) => {
                    let c = &self.cols[j as usize];
                    let Some((_, hit)) = c.hover else { continue };
                    // SAFETY: `hit` was instantiated for the archetype `c` was created with; its entry is
                    // sealed (a column with a site is).
                    let r = unsafe { hit(w, &*w.entry(c.pass as usize), u, x, y) };
                    if r != u32::MAX {
                        *best = Some((*path, j, r));
                    }
                }
                Pass::Sub(q) => {
                    assert!((path.n as usize) < path.at.len(), "stores nest at most six deep");
                    path.at[path.n as usize] = q as u16;
                    path.n += 1;
                    self.subs[q as usize].hit(u, x, y, path, best);
                    path.n -= 1;
                }
            }
        }
    }

    pub fn at(&self, path: &Path) -> Option<&Store> {
        let mut s = self;
        for &q in &path.at[..path.n as usize] {
            s = s.subs.get(q as usize)?;
        }
        Some(s)
    }
}

// ---- passes over the plan ----------------------------------------------------------------------------------

/// What a kernel sees of a column: its pages, its space, its live words.
///
/// SAFETY: `A` is the archetype `e` was sealed for, `e` is an entry of `h`, and nobody writes the column
/// meanwhile (W: the caller is the turn thread; F: the caller is a frame).
#[inline(always)]
unsafe fn col_view<'a, A>(h: Half, e: &Entry) -> ColRef<'a, A> {
    // SAFETY: the caller's contract; the span holds `ceil(nrec / 64)` pages (`Store::seal`), the store owns
    // `max(cap / 64, 1)` live words from `live`.
    unsafe {
        ColRef {
            pages: core::slice::from_raw_parts(h.at(e.pages) as *const Page<A>, (e.nrec as usize).div_ceil(64)),
            space: &*h.space(e.space),
            live: Live { cap: e.cap, rows: core::slice::from_raw_parts(h.live(e.live), (e.cap as usize / 64).max(1)), nrec: e.nrec },
        }
    }
}

/// The damage of a change of the block from `old` to `new` (`ch` = the changed words). One pass over the plan:
/// one indirect call per column that reads a changed word. Returns whether a column is now in motion.
///
/// From the block to a page this crosses two dependent lines, whatever the nesting of the view: the plan
/// entry, then the page (its space and live words are reached from the same entry, beside it).
pub fn delta(ar: Ar, old: &U, new: &U, ch: u64, tiles: &mut Tiles) -> bool {
    let (w, hd) = (ar.work(), ar.head());
    if hd.scrolls & ch != 0 {
        // a space moved: everything it shows is somewhere else
        for i in 0..hd.nspaces as u16 {
            // SAFETY: spaces below `nspaces` were written by `Store::new`.
            let s = unsafe { &*w.space(i) };
            if s.reads() & ch != 0 {
                tiles.mark(s.clip);
            }
        }
    }
    let mut moving = false;
    // SAFETY: the turn thread reads its own half; entries below `ncols` are sealed.
    for (i, e) in unsafe { w.plan(hd.ncols as usize) }.iter().enumerate() {
        if (e.reads & ch != 0) & (e.skip & ch == 0) {
            let mo = ar.motion(i);
            // SAFETY: `delta` was instantiated for the archetype the entry was sealed with.
            unsafe { (e.delta)(w, e, mo, old, new, tiles) }
            moving |= mo.until != 0;
        }
    }
    moving
}

/// A frame at time `now` with no turn: damage where records in motion may be. Nothing is evaluated: a column in
/// motion is one compare and one mark. Returns whether anything is still moving.
pub fn tick(ar: Ar, u: &U, now: u32, tiles: &mut Tiles) -> bool {
    let (w, hd) = (ar.work(), ar.head());
    let mut moving = false;
    for i in 0..hd.ncols as usize {
        let mo = ar.motion(i);
        if mo.until != 0 {
            // SAFETY: a sealed entry of the working half; its space is its store's.
            let sp = unsafe { &*w.space((*w.entry(i)).space) };
            #[cfg(not(feature = "mutant_sweep"))]
            tiles.mark(mo.sweep.shift(sp.origin(u)).and(sp.clip));
            let _ = sp;
            if now >= mo.until {
                // this frame shows it at rest; nothing after it
                *mo = Motion::default();
            } else {
                moving = true;
            }
        }
    }
    moving
}

/// Paint the `n` columns of half `h`, in plan order (paint order).
///
/// SAFETY: every entry was sealed for backend type `B`, and nobody writes `h` meanwhile.
pub unsafe fn paint<B: Backend>(h: Half, n: usize, u: &U, b: &mut B) {
    // SAFETY: the caller's contract.
    for e in unsafe { h.plan(n) } {
        unsafe { (e.paint)(h, e, u, b as *mut B as *mut ()) }
    }
}

/// Records, pages and bytes a half holds.
pub fn stats(ar: Ar) -> (usize, usize, usize) {
    let mut t = (0, 0, 0);
    // SAFETY: the turn thread reads its own half.
    for e in unsafe { ar.work().plan(ar.head().ncols as usize) } {
        let np = (e.nrec as usize).div_ceil(64);
        // SAFETY: the store's live words.
        let live = Live { cap: e.cap, rows: unsafe { core::slice::from_raw_parts(ar.work().live(e.live), (e.cap as usize / 64).max(1)) }, nrec: e.nrec };
        t.0 += (0..np).map(|p| live.page(p).count_ones() as usize).sum::<usize>();
        t.1 += np;
        t.2 += np * e.desc.page.size();
    }
    t
}

/// 64 flags (0 or 1) to a mask, bit i = flag i. Eight multiplies; no branch.
#[inline(always)]
pub fn pack(b: &[u8; 64]) -> u64 {
    let mut m = 0u64;
    for k in 0..8 {
        let x = u64::from_le_bytes([b[k * 8], b[k * 8 + 1], b[k * 8 + 2], b[k * 8 + 3], b[k * 8 + 4], b[k * 8 + 5], b[k * 8 + 6], b[k * 8 + 7]]);
        m |= (x.wrapping_mul(0x0102_0408_1020_4080) >> 56) << (k * 8);
    }
    m
}

/// The derivative of one page: which of its records show something else under `new` than under `old`.
#[inline(always)]
pub fn changed_mask<A: Arch>(pg: &Page<A>, old: &U, new: &U) -> u64 {
    let mut b = [0u8; 64];
    for i in 0..64 {
        b[i] = pg.rec[i].changed(old, new) as u8;
    }
    if A::MOVES {
        for i in 0..64 {
            b[i] |= (pg.rec[i].offset(old) != pg.rec[i].offset(new)) as u8;
        }
    }
    pack(&b)
}

/// The same for a page with few live records: only those are evaluated.
#[inline(always)]
pub fn changed_sparse<A: Arch>(pg: &Page<A>, mut live: u64, old: &U, new: &U) -> u64 {
    let mut m = 0u64;
    while live != 0 {
        let i = live.trailing_zeros() as usize;
        live &= live - 1;
        let c = pg.rec[i].changed(old, new) | (A::MOVES && pg.rec[i].offset(old) != pg.rec[i].offset(new));
        m |= (c as u64) << i;
    }
    m
}

unsafe fn delta_col<A: Arch>(h: Half, e: &Entry, mo: &mut Motion, old: &U, new: &U, tiles: &mut Tiles) {
    // SAFETY: the caller instantiated this for the entry's archetype and is the turn thread, on its own half.
    let sv = unsafe { col_view::<A>(h, e) };
    let pages = sv.pages;
    let org = sv.space.origin(new);
    let clip = sv.space.clip;
    let (mut until, mut sweep) = (mo.until, mo.sweep);
    for (pi, pg) in pages.iter().enumerate() {
        let live = sv.live.page(pi);
        if live == 0 {
            continue;
        }
        let m = if live.count_ones() <= 12 { changed_sparse(pg, live, old, new) } else { changed_mask(pg, old, new) & live };
        #[cfg(feature = "mutant_delta")]
        let m = m & 0x5555_5555_5555_5555;
        if m != 0 {
            mark_page(pg, m, org, clip, old, new, tiles);
        }
        if A::TIMED {
            // Records in motion under the new block: until when, and inside which box. Every live record is
            // asked, not only those `m` names: a motion that starts now shows, at this instant, exactly what
            // was shown before it started.
            let mut k = live;
            while k != 0 {
                let i = k.trailing_zeros() as usize;
                k &= k - 1;
                let t = pg.rec[i].until(new);
                if t > new.w[U_TIME] {
                    let mut end = *new;
                    end.w[U_TIME] = t;
                    let r = pg.rect(i);
                    let b = r.shift(pg.rec[i].offset(new)).or(r.shift(pg.rec[i].offset(&end)));
                    #[cfg(feature = "mutant_moves")]
                    let b = r.shift(pg.rec[i].offset(&end));
                    sweep = if until == 0 { b } else { sweep.or(b) };
                    until = until.max(t);
                }
            }
        }
    }
    (mo.until, mo.sweep) = (until, sweep);
}

/// Damage the records of a page named by `m`.
#[inline(always)]
fn mark_page<A: Arch>(pg: &Page<A>, mut m: u64, org: (i32, i32), clip: Rect, old: &U, new: &U, tiles: &mut Tiles) {
    if !A::MOVES {
        // SAFETY: a page starts with its four rect lanes, whatever its archetype.
        return unsafe { mark_rects(&*(pg as *const Page<A> as *const [[i32; 64]; 4]), m, org, clip, tiles) };
    }
    while m != 0 {
        let i = m.trailing_zeros() as usize;
        m &= m - 1;
        let r = pg.rect(i).shift(org);
        #[cfg(not(feature = "mutant_moves"))]
        tiles.mark(r.shift(pg.rec[i].offset(old)).and(clip));
        tiles.mark(r.shift(pg.rec[i].offset(new)).and(clip));
    }
}

/// Damage the rects of a page named by `m`. One body for every archetype.
#[inline(never)]
fn mark_rects(l: &[[i32; 64]; 4], mut m: u64, org: (i32, i32), clip: Rect, tiles: &mut Tiles) {
    let rect = |i: usize| Rect { x0: l[0][i], y0: l[1][i], x1: l[2][i], y1: l[3][i] };
    if m.count_ones() >= 8 {
        // Dense: one mark of the bounding box when the records fill most of it (a column strip, a block of
        // rows), which is what a fan-out looks like on a page.
        let (mut bb, mut sum, mut k) = (Rect { x0: i32::MAX, y0: i32::MAX, x1: i32::MIN, y1: i32::MIN }, 0i64, m);
        while k != 0 {
            let i = k.trailing_zeros() as usize;
            k &= k - 1;
            let r = rect(i);
            bb = bb.or(r);
            sum += r.area();
        }
        if bb.area() <= 2 * sum {
            tiles.mark(bb.shift(org).and(clip));
            return;
        }
    }
    while m != 0 {
        let i = m.trailing_zeros() as usize;
        m &= m - 1;
        tiles.mark(rect(i).shift(org).and(clip));
    }
}

unsafe fn hit_col<A: Arch>(h: Half, e: &Entry, u: &U, x: i32, y: i32) -> u32 {
    // SAFETY: the caller instantiated this for the entry's archetype and is the turn thread, on its own half.
    let sv = unsafe { col_view::<A>(h, e) };
    let pages = sv.pages;
    if x < sv.space.clip.x0 || x >= sv.space.clip.x1 || y < sv.space.clip.y0 || y >= sv.space.clip.y1 {
        return u32::MAX;
    }
    let org = sv.space.origin(u);
    let (x, y) = (x - org.0, y - org.1);
    let mut best = u32::MAX;
    for (pi, pg) in pages.iter().enumerate() {
        let live = sv.live.page(pi);
        if live == 0 {
            continue;
        }
        let m = if live.count_ones() <= 12 {
            let (mut m, mut l) = (0u64, live);
            while l != 0 {
                let i = l.trailing_zeros() as usize;
                l &= l - 1;
                m |= (((x >= pg.x0[i]) & (x < pg.x1[i]) & (y >= pg.y0[i]) & (y < pg.y1[i])) as u64) << i;
            }
            m
        } else {
            let mut b = [0u8; 64];
            for i in 0..64 {
                b[i] = ((x >= pg.x0[i]) & (x < pg.x1[i]) & (y >= pg.y0[i]) & (y < pg.y1[i])) as u8;
            }
            pack(&b) & live
        };
        if m != 0 {
            best = pi as u32 * 64 + 63 - m.leading_zeros();
        }
    }
    best
}

unsafe fn paint_col<A: Arch, B: Backend>(h: Half, e: &Entry, u: &U, b: *mut ()) {
    // SAFETY: the caller instantiated this for the entry's archetype and passes a `*mut B`; nobody writes `h`.
    unsafe { A::paint(&mut *(b as *mut B), col_view::<A>(h, e), u) }
}
