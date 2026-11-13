//! Archetype columns in 64-record pages (ROUND8 section 4) and the damage they produce (section 3).
//!
//! A [`Store`] holds the records of one static shape: the fixed part of a view, or the rows of one list. It is
//! not generic. A column is created by the first record described at a position of the shape and is written
//! through a typed pointer; every pass over a column (damage, hit testing, paint) is one function monomorphised
//! for the column's archetype, reached once per column through its [`Desc`].
use crate::arena::{Ar, Entry, Half, LINE, PaintFn};
use crate::expr::{StageId, U, U_TIME, Words};
use core::sync::atomic::{AtomicU32, Ordering};
use core::any::TypeId;
use std::alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error};

pub use crate::expr::Rect;

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
    /// Damage that is still to come: the boxes records in motion stay inside, and until when. A frame marks
    /// them; nothing is evaluated. At most one entry per column that has a record in motion.
    pub sweeps: Vec<Sweep>,
}

/// Records of one column are in motion until `until` (a value of `Time`; `u32::MAX`: endlessly) and stay inside
/// `rect` (in the column's space) until then.
#[derive(Clone, Copy, Debug)]
pub struct Sweep {
    pub col: u32,
    pub until: u32,
    pub rect: Rect,
    pub space: Space,
}
impl Tiles {
    /// Replace column `col`'s entry: `until == 0` means none of its records is in motion.
    #[inline]
    pub fn set_sweep(&mut self, col: u32, space: Space, (until, rect): (u32, Rect)) {
        match self.sweeps.iter().position(|s| s.col == col) {
            Some(i) if until == 0 => {
                self.sweeps.swap_remove(i);
            }
            Some(i) => self.sweeps[i] = Sweep { col, until, rect, space },
            None if until != 0 => self.sweeps.push(Sweep { col, until, rect, space }),
            None => {}
        }
    }
    /// A frame at time `now` under block `u`: damage where records in motion may be. The frame in which a
    /// motion comes to rest is its last.
    #[inline(never)]
    pub fn tick(&mut self, now: u32, u: &U) {
        let mut i = 0;
        while i < self.sweeps.len() {
            let s = self.sweeps[i];
            #[cfg(not(feature = "mutant_sweep"))]
            self.mark(s.rect.shift(s.space.origin(u)).and(s.space.clip));
            if now >= s.until {
                self.sweeps.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }
    pub fn new(w: i32, h: i32) -> Tiles {
        let cols = ((w + TILE - 1) >> TILE_SHIFT) as usize;
        let rows = ((h + TILE - 1) >> TILE_SHIFT) as usize;
        let stride = cols.div_ceil(64).max(1);
        Tiles { w, h, stride, rows, bits: vec![0; stride * rows], lo: usize::MAX, hi: 0, sweeps: Vec::new() }
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

/// One operand of an archetype, as data: its name, its expression's code, its stage, the block words it reads,
/// and where its words are in the record. Read only by the tools that look at records from outside the kernels
/// (inspector, accessibility, tests); a constant nobody names is not emitted.
#[derive(Debug)]
pub struct Op {
    pub name: &'static str,
    pub code: &'static crate::expr::Code,
    /// The operand's expression type, written out by the compiler (`Map<fill, (Un<Theme>, Rec<u32>)>`, with
    /// paths): the names of the lenses and functions it was built from.
    pub ty: fn() -> &'static str,
    pub stage: StageId,
    pub reads: u64,
    /// First record word of the operand, and how many.
    pub word: u16,
    pub words: u16,
}

/// An archetype: a paint kind applied to expression types. The value is one record's constants.
pub trait Arch: Words {
    /// The operands, in record order (empty for an archetype that does not describe itself).
    const OPS: &'static [Op] = &[];
    /// The words of the block whose change can change what a record of this archetype shows.
    const READS: u64;
    /// The record's place on screen depends on the block (a Frame-stage transform).
    const MOVES: bool = false;
    const NAME: &'static str;
    /// The latest stage among its expressions.
    const STAGE: StageId;
    /// The record's paint reads `Time`: it can change with no turn.
    const TIMED: bool = false;
    /// No record of this archetype ever rests (it reads `Time` through something that gives no bound).
    const ENDLESS: bool = false;
    /// The value of `Time` from which this record shows the same thing whatever the time, under the other
    /// words of `u` (0: it is at rest now; `u32::MAX`: never).
    #[inline(always)]
    fn until(&self, _u: &U) -> u32 {
        0
    }
    /// The box (in the store's space) the record's ink stays inside from now until it rests, given its rect.
    #[inline(always)]
    fn sweep(&self, r: Rect, _u: &U) -> Rect {
        r
    }
    /// The derivative: does this record show something else under `new` than under `old`?
    fn changed(&self, old: &U, new: &U) -> bool;
    #[inline(always)]
    fn offset(&self, _u: &U) -> (i32, i32) {
        (0, 0)
    }
    /// Hand the column to the backend method for this paint kind.
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U);
    /// The value of operand `op` (an index into [`Arch::OPS`]) under block `u`. Read by reflection only, so it
    /// is instantiated only in a program that reflects.
    #[inline]
    fn val(&self, _op: usize, _u: &U) -> crate::reflect::Val {
        crate::reflect::Val::None
    }
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

pub struct StoreView<'a> {
    pub space: &'a Space,
    pub live: Live<'a>,
}

pub type DeltaFn = unsafe fn(&ColEnt, &U, &U, &mut Tiles);
pub type ActiveFn = unsafe fn(&RawCol, &StoreView, &U) -> (u32, Rect);
/// The hit kernel of a column: the topmost live record under a point, from the column's table entry alone.
pub type HitFn = unsafe fn(&ColEnt, &U, i32, i32) -> u32;
/// A column whose records have handlers: their handler face (kinds, hit kernel, trampoline, captures), constant
/// per record-and-handler type (`event.rs`). The hit kernel is instantiated only for archetypes with a handler.
pub type Hov = &'static crate::event::Ons;
#[inline(always)]
pub const fn hit_fn<A: Arch>() -> HitFn {
    hit_col::<A>
}

/// A column as the path from a block write to its pages sees it: one line, in a table of all the columns of a
/// world in paint order. Everything a delta kernel needs is here or one load from here (the space), so the
/// store tree is not on that path. The pointers are into the store tree and hold until its shape next changes
/// ([`SHAPE`]): the world rebuilds the table then.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct ColEnt {
    pub reads: u64,
    /// The words that move the column's space: if one changed, the space is damaged as a whole instead.
    pub skip: u64,
    pub delta: DeltaFn,
    pub pages: *const u8,
    pub live: *const u64,
    pub space: *const Space,
    pub npages: u32,
    pub nrec: u32,
    pub cap: u32,
    pub timed: u32,
}
impl ColEnt {
    /// SAFETY: the entry is current (built since the store tree last changed shape).
    #[inline(always)]
    unsafe fn live(&self) -> Live<'_> {
        // SAFETY: `live` is the store's `row_live`, which holds `max(cap / 64, 1)` words.
        Live { cap: self.cap, rows: unsafe { core::slice::from_raw_parts(self.live, (self.cap as usize / 64).max(1)) }, nrec: self.nrec }
    }
}

/// Bumped whenever a store gains a column or a child: what the world's column table is checked against.
pub static SHAPE: AtomicU32 = AtomicU32::new(0);

/// The cold face of an archetype: what a store needs to run passes over a column it cannot name the type of.
pub struct Desc {
    pub name: &'static str,
    pub stage: StageId,
    pub rec_size: usize,
    pub rec_off: usize,
    pub page: Layout,
    pub reads: u64,
    pub moves: bool,
    /// Its records read `Time`.
    pub timed: bool,
    delta: DeltaFn,
    active: ActiveFn,
    paint: unsafe fn(&RawCol, &StoreView, &U, *mut ()),
    /// The same paint over a half and its plan entry: what a frame calls (`arena::Entry`).
    paint_at: PaintFn,
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
        timed: A::TIMED,
        delta: delta_col::<A>,
        active: if A::TIMED { active_col::<A> } else { active_none },
        paint: paint_col::<A, B>,
        paint_at: paint_at_col::<A, B>,
    };
}
#[inline(always)]
pub fn desc<A: Arch, B: Backend>() -> &'static Desc {
    &Holder::<A, B>::DESC
}

/// A column: pages of one archetype, type-erased. The pages are a span of the scene's working half.
pub struct RawCol {
    pages: *mut u8,
    /// Where the span starts (lines from a half's base), its first dirty-range id, and the column's id in the
    /// column bitmap.
    pub line: u32,
    pub rid: u32,
    pub cid: u32,
    pub npages: u32,
    /// Records the column has room for: sites x the store's row capacity.
    pub nrec: u32,
    /// Sites (positions of the shape) that share this column.
    pub k: u32,
    pub reads: u64,
    /// The archetype's delta kernel, beside `reads` and the pages it runs over: a block write reaches a page
    /// through this header alone (block, header, page).
    delta: DeltaFn,
    /// The column's name in the list of sweeps (unique in the process).
    pub id: u32,
    pub key: TypeId,
    /// The archetype (the record type, without its handler): what a generated kernel or GPU function is keyed
    /// on. Columns are per store and site; archetypes are per program.
    pub arch: TypeId,
    pub desc: &'static Desc,
    /// How many child stores were described before this column's first site: its place in paint order.
    pub seg: u32,
    /// The records' handlers, if they have any.
    pub hover: Option<Hov>,
    /// The handlers' captures, one per record, when they capture anything (a CPU lane: the GPU never reads it).
    caps: *mut u8,
}
impl RawCol {
    fn grow(&mut self, npages: u32, ar: Ar, cap: u32) {
        if npages <= self.npages {
            return;
        }
        if let Some(o) = self.hover
            && o.caps != 0
        {
            let (old, new) = (o.caps as usize * self.npages as usize * 64, o.caps as usize * npages as usize * 64);
            let lay = Layout::from_size_align(new, o.caps_align as usize).unwrap();
            // SAFETY: `new` is non-zero (`caps != 0`, `npages > 0`).
            let p = unsafe { alloc_zeroed(lay) };
            if p.is_null() {
                handle_alloc_error(lay);
            }
            if old != 0 {
                // SAFETY: the old lane holds `old` bytes of this alignment; the new one is larger and distinct.
                unsafe {
                    core::ptr::copy_nonoverlapping(self.caps, p, old);
                    dealloc(self.caps, Layout::from_size_align_unchecked(old, o.caps_align as usize));
                }
            }
            self.caps = p;
        }
        // The pages: a span of the working half, exactly what the sites need. A column that had pages and
        // outgrew them (a fixed shape whose later row joins it, during the store's first description) moves;
        // the old span is abandoned. Nothing has published or read the old place: it is the first pass.
        let lines = (self.desc.page.size() / LINE) as u32;
        let (at, rid) = ar.new_span(npages, lines);
        let p = ar.work().at(at);
        if self.npages != 0 && ar.head().adopt == 0 {
            // SAFETY: both spans are inside the working half and distinct; the old one holds `npages` pages.
            unsafe { core::ptr::copy_nonoverlapping(self.pages, p, self.desc.page.size() * self.npages as usize) };
            (0..self.npages).for_each(|k| ar.dirty(rid + k));
            ar.head().wasted += self.npages * lines;
        }
        (self.pages, self.line, self.rid, self.npages) = (p, at, rid, npages);
        ar.set_cid(self.cid, rid, self.k, cap / 64);
    }
    /// SAFETY: `A` is the archetype this column was created for and `pg < npages`.
    #[inline(always)]
    pub unsafe fn page<A>(&self, pg: u32) -> *mut Page<A> {
        unsafe { (self.pages as *mut Page<A>).add(pg as usize) }
    }
    /// SAFETY: `A` is the archetype this column was created for.
    #[inline(always)]
    unsafe fn all<A>(&self) -> &[Page<A>] {
        unsafe { core::slice::from_raw_parts(self.pages as *const Page<A>, self.npages as usize) }
    }
    /// Page `pg` as bytes: four rect lanes of 64 `i32` each, then the records (`desc.rec_off`, `desc.rec_size`).
    #[inline]
    pub fn raw_page(&self, pg: u32) -> *const u8 {
        assert!(pg < self.npages);
        // SAFETY: in bounds of the page array by the assert.
        unsafe { self.pages.add(pg as usize * self.desc.page.size()) }
    }
    /// The captures of record `rec`'s handlers (a dangling, aligned pointer when they capture nothing).
    pub fn caps_ptr(&self, rec: u32) -> *const u8 {
        match self.hover {
            Some(o) if o.caps != 0 => {
                assert!(rec < self.npages * 64);
                // SAFETY: the lane holds `npages * 64` captures of `o.caps` bytes.
                unsafe { self.caps.add(rec as usize * o.caps as usize) }
            }
            Some(o) => o.caps_align as usize as *const u8,
            None => core::ptr::dangling(),
        }
    }
    /// The constants of record `rec`, untyped (for a handler that knows the type).
    pub fn rec_ptr(&self, rec: u32) -> *const u8 {
        assert!(rec < self.npages * 64);
        // SAFETY: in bounds of the page array by the assert; `rec_off` and `rec_size` are the archetype's.
        unsafe { self.pages.add((rec as usize >> 6) * self.desc.page.size() + self.desc.rec_off + (rec as usize & 63) * self.desc.rec_size) }
    }
}
impl Drop for RawCol {
    fn drop(&mut self) {
        // the pages are the arena's; only the captures are this column's own allocation
        if self.npages != 0
            && let Some(o) = self.hover
            && o.caps != 0
        {
            // SAFETY: allocated in `grow` with this size and alignment.
            unsafe { dealloc(self.caps, Layout::from_size_align_unchecked(o.caps as usize * self.npages as usize * 64, o.caps_align as usize)) }
        }
    }
}

/// A store's live words: `max(cap / 64, 1)` words of the working half. Reads and writes like a slice; a writer
/// notes the write (`Store::live_dirty`).
pub struct LiveWords {
    p: *mut u64,
    n: usize,
}
impl core::ops::Deref for LiveWords {
    type Target = [u64];
    #[inline(always)]
    fn deref(&self) -> &[u64] {
        // SAFETY: `Store::new` was handed these `n` words by the arena; the turn thread alone writes them.
        unsafe { core::slice::from_raw_parts(self.p, self.n) }
    }
}
impl core::ops::DerefMut for LiveWords {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [u64] {
        // SAFETY: as `deref`.
        unsafe { core::slice::from_raw_parts_mut(self.p, self.n) }
    }
}

#[derive(Clone, Copy)]
pub struct Site {
    /// The page that holds this site's slot 0 (slot `s` is record `s mod 64` of page `s / 64` from here).
    pub page: *mut u8,
    pub page_size: u32,
    pub col: u32,
    /// The first record of this site: record = base + row slot.
    pub base: u32,
    /// The captures of this site's slot 0, when its handlers capture anything (`event.rs`).
    pub cap: *mut u8,
    /// The dirty-range id of that page: a writer of slot `s` notes `rid + s / 64`.
    pub rid: u32,
}
impl Site {
    /// Store a record's rect. The four rect lanes are the first kilobyte of every page, whatever its archetype.
    ///
    /// SAFETY: `slot` is under the store's capacity and the site's pointers are current (`Store::fix_sites`).
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

/// Where a list's rows are described and their shape: (call site, leaves, bytes).
pub type RowSrc = (&'static core::panic::Location<'static>, u32, u32);

pub struct Store {
    /// The scene's memory: this store's pages, live words and space slot are in its working half.
    pub ar: Ar,
    /// Its slot in the half's spaces and its first live word there; a list's id in the list bitmap.
    pub space_ix: u16,
    pub live_at: u32,
    pub lid: u32,
    pub cols: Vec<RawCol>,
    pub sites: Vec<Site>,
    pub subs: Vec<Store>,
    /// Paint order: columns and child stores in the order the view first described them.
    pub order: Vec<Pass>,
    /// Row slots: 1 for a fixed shape, a power of two of at least 64 for a list's window.
    pub cap: u32,
    pub row_live: LiveWords,
    pub space: Space,
    disc: Vec<(u32, Rect)>,
    /// A list's materialised window of rows, `lo..hi`; row `r` is at slot `r mod cap`.
    pub lo: u32,
    pub hi: u32,
    /// The relation version this window was last brought up to.
    pub seen: u32,
    /// The rows of the shape: the type of each row value described here and its first site.
    pub rows: Vec<(TypeId, u32)>,
    /// A list's rows: where they are described, how many records a row has, a row's size (`sink::list_sync`).
    pub src: Option<RowSrc>,
    /// A list's layout: the rects of one row relative to the row, and where and how tall rows are.
    pub tmpl: Vec<Rect>,
    /// Where the slice `tmpl` was copied from was: an identity check before a content compare.
    pub tmpl_at: usize,
    pub geo: (i32, i32, i32),
    /// Scratch: the rows a list pass visits (row index; top bit set: write without comparing).
    pub plan: Vec<u32>,
    /// The block words some column here reads, and whether some column reads `Time`.
    pub reads: u64,
    pub timed: bool,
}

static COL_ID: AtomicU32 = AtomicU32::new(1);

impl Store {
    pub fn new(ar: Ar, cap: u32, space: Space) -> Store {
        Store::with_space(ar, cap, space, None)
    }
    /// A list's store: its rows are in the space of the store that holds the list, so it has that store's slot
    /// of the half's spaces and never writes one.
    pub fn new_list(parent: &Store, cap: u32) -> Store {
        Store::with_space(parent.ar, cap, parent.space, Some(parent.space_ix))
    }
    fn with_space(ar: Ar, cap: u32, space: Space, shared: Option<u16>) -> Store {
        assert!(cap == 1 || (cap >= 64 && cap.is_power_of_two()));
        let n = (cap as usize / 64).max(1);
        let live_at = ar.new_live(n as u32);
        let space_ix = shared.unwrap_or_else(|| ar.new_space());
        let row_live = LiveWords { p: ar.work().live(live_at), n };
        let lid = if cap > 1 { ar.new_list() } else { u32::MAX };
        let mut st = Store { ar, space_ix, live_at, lid, cols: Vec::new(), sites: Vec::new(), subs: Vec::new(), order: Vec::new(), cap, row_live, space, disc: Vec::new(), lo: 0, hi: 0, seen: 0, rows: Vec::new(), src: None, tmpl: Vec::new(), tmpl_at: 0, geo: (0, 0, 0), plan: Vec::new(), reads: 0, timed: false };
        if shared.is_some() {
            // the slot is the parent's to write
        } else if ar.head().adopt != 0 {
            // Found in a mapped image: it has the space the image had. What the view says now may differ, and
            // then the sink sees a change like any other.
            // SAFETY: a space index answered by the replay is below `MAX_SPACES`; the slot is plain words.
            st.space = unsafe { ar.work().space(space_ix).read() };
        } else {
            st.sync_space();
        }
        st
    }
    /// A store that owns nothing: what a world mapped from an image has until the view is described.
    pub fn placeholder(ar: Ar) -> Store {
        Store { ar, space_ix: 0, live_at: 0, lid: u32::MAX, cols: Vec::new(), sites: Vec::new(), subs: Vec::new(), order: Vec::new(), cap: 1, row_live: LiveWords { p: ar.work().live(0), n: 0 }, space: Space::root(0, 0), disc: Vec::new(), lo: 0, hi: 0, seen: 0, rows: Vec::new(), src: None, tmpl: Vec::new(), tmpl_at: 0, geo: (0, 0, 0), plan: Vec::new(), reads: 0, timed: false }
    }
    /// `space` was assigned: put it where a frame reads it.
    #[cold]
    #[inline(never)]
    pub fn sync_space(&self) {
        // SAFETY: `space_ix` is this store's slot of the half's spaces; the turn thread is the only writer.
        unsafe { self.ar.work().space(self.space_ix).write(self.space) };
        self.ar.dirty_space(self.space_ix);
    }
    /// The live words were written.
    #[inline]
    pub fn live_dirty(&self) {
        let (mut w, end) = (self.live_at, self.live_at + self.row_live.n as u32);
        while w < end {
            self.ar.dirty_live(w);
            w = (w / 8 + 1) * 8;
        }
    }
    /// A fixed shape is live as a whole.
    #[inline]
    pub fn live_one(&mut self) {
        if self.row_live[0] != 1 {
            self.row_live[0] = 1;
            self.live_dirty();
        }
    }
    /// This list wrote rows whole in the slot pages `mask` names, or moved its window (`mask` may be 0: the
    /// live words alone). One note.
    #[inline(always)]
    pub fn rows_dirty(&self, mask: u64) {
        self.ar.dirty_list(self.lid, mask);
    }
    /// What the list bitmap knows this list by: its first column id, its columns, its live words.
    fn list_desc(&self) -> [u32; 4] {
        [self.cols.first().map_or(0, |c| c.cid), self.cols.len() as u32, self.live_at, self.row_live.n as u32]
    }
    #[inline(always)]
    pub fn view(&self) -> StoreView<'_> {
        StoreView { space: &self.space, live: Live { cap: self.cap, rows: &self.row_live, nrec: 0 } }
    }
    /// Give the next position of the shape a column. Cold: runs once per position per store, and is not
    /// generic: an archetype is its descriptor here.
    ///
    /// A position joins an existing column of its archetype when painting the column as a whole keeps painter's
    /// order: the column is in the same segment (no child store between them) and no record described since, in
    /// a later column, overlaps the new one.
    #[cold]
    #[inline(never)]
    pub fn discover(&mut self, d: &'static Desc, rect: Rect, key: TypeId, arch: TypeId, hover: Option<Hov>) {
        let seg = self.subs.len() as u32;
        let mut pick = None;
        for (j, c) in self.cols.iter().enumerate() {
            if c.key == key && c.seg == seg && !self.disc.iter().any(|&(cj, r)| cj as usize > j && r.hits(rect)) {
                pick = Some(j);
                break;
            }
        }
        let j = pick.unwrap_or_else(|| {
            self.cols.push(RawCol { pages: core::ptr::null_mut(), line: 0, rid: 0, cid: self.ar.new_cid(), npages: 0, nrec: 0, k: 0, reads: d.reads, delta: d.delta, id: COL_ID.fetch_add(1, Ordering::Relaxed), key, arch, desc: d, seg, hover, caps: core::ptr::null_mut() });
            self.order.push(Pass::Col(self.cols.len() as u32 - 1));
            self.cols.len() - 1
        });
        self.reads |= d.reads;
        self.timed |= d.timed;
        SHAPE.fetch_add(1, Ordering::Relaxed);
        let c = &mut self.cols[j];
        let base = c.k * self.cap;
        c.k += 1;
        c.nrec = c.k * self.cap;
        self.disc.push((j as u32, rect));
        self.sites.push(Site { page: core::ptr::null_mut(), page_size: 0, col: j as u32, base, cap: core::ptr::null_mut(), rid: 0 });
    }
    /// After the positions of a row were discovered: give every column that grew its pages and point the sites
    /// at them. Cold. A column's span is sized once per row described, not once per position.
    #[cold]
    #[inline(never)]
    pub fn seal(&mut self) {
        let (ar, cap) = (self.ar, self.cap);
        for c in &mut self.cols {
            c.grow(c.nrec.div_ceil(64), ar, cap);
        }
        if cap > 1 {
            // a list's columns are discovered together, so their ids are consecutive: one note names them all
            assert!(self.cols.iter().enumerate().all(|(i, c)| c.cid == self.cols[0].cid + i as u32));
            ar.set_list(self.lid, self.list_desc());
        }
        self.fix_sites();
    }
    /// After adopting an image: whether the columns of this store and of the stores under it are described in
    /// the image's column bitmap as they are here.
    pub fn cids_same(&self) -> bool {
        self.cols.iter().all(|c| self.ar.cid_is(c.cid, c.rid, c.k, self.cap / 64))
            && (self.cap == 1 || self.cols.is_empty() || self.ar.list_is(self.lid, self.list_desc()))
            && self.subs.iter().all(|s| s.cids_same())
    }
    /// Point every site at its column's pages. Run whenever a column's pages may have moved.
    fn fix_sites(&mut self) {
        for s in &mut self.sites {
            let c = &self.cols[s.col as usize];
            // SAFETY: `base / 64` is a page of the column (`discover` grew it to hold the site).
            s.page = unsafe { c.pages.add((s.base >> 6) as usize * c.desc.page.size()) };
            s.page_size = c.desc.page.size() as u32;
            s.rid = c.rid + (s.base >> 6);
            if let Some(o) = c.hover
                && o.caps != 0
            {
                // SAFETY: the lane holds a capture for every record of the column, `base` among them.
                s.cap = unsafe { c.caps.add(s.base as usize * o.caps as usize) };
            }
        }
    }

    /// Column `c` as a table entry.
    pub fn ent(&self, c: &RawCol) -> ColEnt {
        ColEnt { reads: c.reads, skip: self.space.reads(), delta: c.delta, pages: c.pages, live: self.row_live.as_ptr(), space: &self.space, npages: c.npages, nrec: c.nrec, cap: self.cap, timed: c.desc.timed as u32 }
    }
    /// Column `c` as a plan entry: its offset form, for a frame, a reader or an image.
    pub fn entry(&self, c: &RawCol) -> Entry {
        Entry::new(c.desc, c.desc.paint_at, self.space.reads(), c.line, c.nrec, self.live_at, self.cap, self.space_ix)
    }
    /// Append this store's columns and those of the stores under it, in paint order; `cold` gets, per entry,
    /// the store and column it came from (for the rare things the entry does not carry).
    pub fn table(&self, out: &mut Vec<ColEnt>, cold: &mut Vec<(*const Store, u32)>) {
        for p in &self.order {
            match *p {
                Pass::Col(j) => {
                    out.push(self.ent(&self.cols[j as usize]));
                    cold.push((self, j));
                }
                Pass::Sub(q) => self.subs[q as usize].table(out, cold),
            }
        }
    }
    /// The stores, this one and those under it, whose space moves with a block word.
    pub fn moving_spaces(&self, out: &mut Vec<(u64, *const Store)>) {
        if self.space.reads() != 0 {
            out.push((self.space.reads(), self));
        }
        for s in &self.subs {
            s.moving_spaces(out);
        }
    }

    /// The same damage as the world's table walk, by walking the tree: the reference the table is held to
    /// (`tests/table.rs`), and what a caller without a world uses.
    pub fn delta(&self, old: &U, new: &U, ch: u64, tiles: &mut Tiles) {
        if self.space.reads() & ch != 0 {
            // the space moved: everything it shows is somewhere else
            tiles.mark(self.space.clip);
            if self.timed & (self.reads & ch != 0) {
                self.resweep(new, tiles);
            }
        } else if self.reads & ch != 0 {
            for c in &self.cols {
                if c.reads & ch != 0 {
                    // SAFETY: `delta` was instantiated for the archetype `c` was created with; the entry is
                    // built here, from the store as it is.
                    unsafe { (c.delta)(&self.ent(c), old, new, tiles) };
                    if c.desc.timed {
                        self.resweep_col(c, new, tiles);
                    }
                }
            }
        }
        for s in &self.subs {
            s.delta(old, new, ch, tiles);
        }
    }
    /// Say again whether column `c` is in motion, until when and where.
    #[inline(never)]
    pub fn resweep_col(&self, c: &RawCol, u: &U, tiles: &mut Tiles) {
        let sv = StoreView { space: &self.space, live: Live { cap: self.cap, rows: &self.row_live, nrec: c.nrec } };
        // SAFETY: `active` was instantiated for the archetype `c` was created with.
        tiles.set_sweep(c.id, self.space, unsafe { (c.desc.active)(c, &sv, u) });
    }

    /// Records of this store were written: say again which of its timed columns are in motion. Cold unless
    /// the store has such a column.
    #[inline(never)]
    pub fn resweep(&self, u: &U, tiles: &mut Tiles) {
        for c in &self.cols {
            if c.desc.timed {
                self.resweep_col(c, u, tiles);
            }
        }
    }

    /// The block words read by some column of this store or of the stores under it.
    pub fn reads_all(&self) -> u64 {
        self.subs.iter().fold(self.reads, |m, s| m | s.reads_all())
    }

    /// Paint everything, in the order the view described it.
    pub fn paint<B: Backend>(&self, u: &U, b: &mut B) {
        for p in &self.order {
            match *p {
                Pass::Col(j) => {
                    let c = &self.cols[j as usize];
                    let sv = StoreView { space: &self.space, live: Live { cap: self.cap, rows: &self.row_live, nrec: c.nrec } };
                    // SAFETY: `paint` was instantiated for the archetype of `c` and for `B`, the backend type every
                    // column of this world was described with (`World<B>` is the only caller).
                    unsafe { (c.desc.paint)(c, &sv, u, b as *mut B as *mut ()) }
                }
                Pass::Sub(q) => self.subs[q as usize].paint(u, b),
            }
        }
    }

    /// The topmost record with a hover handler under a point: (path of child indices, column, record).
    /// The records under (x, y) whose handlers hear one of `kinds`, bottom first: per column, the topmost of
    /// its records there, columns in paint order. (Two records of one column that overlap each other: only the
    /// upper is found.) The last entry is what the raster's pick names.
    pub fn chain(&self, u: &U, x: i32, y: i32, kinds: u32, path: &mut Path, out: &mut Vec<(Path, u32, u32)>) {
        for p in &self.order {
            match *p {
                Pass::Col(j) => {
                    let c = &self.cols[j as usize];
                    let Some(o) = c.hover else { continue };
                    if o.kinds & kinds == 0 {
                        continue;
                    }
                    // SAFETY: `hit` was instantiated for the archetype `c` was created with; the entry is built
                    // here, from the store as it is.
                    let r = unsafe { (o.hit)(&self.ent(c), u, x, y) };
                    if r != u32::MAX {
                        out.push((*path, j, r));
                    }
                }
                Pass::Sub(q) => {
                    assert!((path.n as usize) < path.at.len(), "stores nest at most six deep");
                    path.at[path.n as usize] = q as u16;
                    path.n += 1;
                    self.subs[q as usize].chain(u, x, y, kinds, path, out);
                    path.n -= 1;
                }
            }
        }
    }

    /// The topmost record with a pointer handler under a point: (path of child indices, column, record).
    pub fn hit(&self, u: &U, x: i32, y: i32, path: &mut Path, best: &mut Option<(Path, u32, u32)>) {
        let mut v = Vec::new();
        self.chain(u, x, y, crate::event::kind::POINTER, path, &mut v);
        if let Some(&b) = v.last() {
            *best = Some(b);
        }
    }

    pub fn at(&self, path: &Path) -> Option<&Store> {
        let mut s = self;
        for &q in &path.at[..path.n as usize] {
            s = s.subs.get(q as usize)?;
        }
        Some(s)
    }

    /// Records, pages and bytes held, with the stores under this one.
    pub fn stats(&self) -> (usize, usize, usize) {
        let mut t = (0, 0, 0);
        for c in &self.cols {
            let live = Live { cap: self.cap, rows: &self.row_live, nrec: c.nrec };
            t.0 += (0..c.npages as usize).map(|p| live.page(p).count_ones() as usize).sum::<usize>();
            t.1 += c.npages as usize;
            t.2 += c.npages as usize * c.desc.page.size();
        }
        for s in &self.subs {
            let u = s.stats();
            t = (t.0 + u.0, t.1 + u.1, t.2 + u.2);
        }
        t
    }
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

unsafe fn delta_col<A: Arch>(e: &ColEnt, old: &U, new: &U, tiles: &mut Tiles) {
    // SAFETY: the caller instantiated this for the column's archetype and the entry is current: `pages` holds
    // `npages` pages of `A`, `space` is the column's store's.
    let (pages, space, lv) = unsafe { (core::slice::from_raw_parts(e.pages as *const Page<A>, e.npages as usize), &*e.space, e.live()) };
    let org = space.origin(new);
    let clip = space.clip;
    for (pi, pg) in pages.iter().enumerate() {
        let live = lv.page(pi);
        if live == 0 {
            continue;
        }
        let m = if live.count_ones() <= 12 { changed_sparse(pg, live, old, new) } else { changed_mask(pg, old, new) & live };
        #[cfg(feature = "mutant_delta")]
        let m = m & 0x5555_5555_5555_5555;
        if m != 0 {
            mark_page(pg, m, org, clip, old, new, tiles);
        }
    }
}

/// The column's bound in `Time` under `u`: until when some live record is in motion, and the box they stay
/// inside until then. Every live record is asked, not the ones a difference names: a motion that starts now
/// shows, at this instant, exactly what was shown before it started. Runs in a turn, never in a frame.
unsafe fn active_col<A: Arch>(col: &RawCol, sv: &StoreView, u: &U) -> (u32, Rect) {
    // SAFETY: the caller instantiated this for the column's archetype.
    let pages = unsafe { col.all::<A>() };
    if A::ENDLESS {
        // every record is in motion for ever: the column is, wherever its space shows it, if it has a record
        let any = (0..pages.len()).any(|pi| sv.live.page(pi) != 0);
        return if any { (u32::MAX, Rect::EVERYWHERE) } else { (0, Rect::default()) };
    }
    let now = u.w[U_TIME];
    let (mut until, mut sweep) = (0u32, Rect { x0: i32::MAX, y0: i32::MAX, x1: i32::MIN, y1: i32::MIN });
    for (pi, pg) in pages.iter().enumerate() {
        let mut k = sv.live.page(pi);
        while k != 0 {
            let i = k.trailing_zeros() as usize;
            k &= k - 1;
            let t = pg.rec[i].until(u);
            if t > now {
                sweep = sweep.or(pg.rec[i].sweep(pg.rect(i), u));
                until = until.max(t);
            }
        }
    }
    (until, sweep)
}
unsafe fn active_none(_: &RawCol, _: &StoreView, _: &U) -> (u32, Rect) {
    (0, Rect::default())
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

unsafe fn hit_col<A: Arch>(e: &ColEnt, u: &U, x: i32, y: i32) -> u32 {
    // SAFETY: the caller instantiated this for the column's archetype and the entry is current: `pages` holds
    // `npages` pages of `A`, `space` is the column's store's.
    let (pages, space, lv) = unsafe { (core::slice::from_raw_parts(e.pages as *const Page<A>, e.npages as usize), &*e.space, e.live()) };
    if x < space.clip.x0 || x >= space.clip.x1 || y < space.clip.y0 || y >= space.clip.y1 {
        return u32::MAX;
    }
    let org = space.origin(u);
    let (x, y) = (x - org.0, y - org.1);
    let mut best = u32::MAX;
    for (pi, pg) in pages.iter().enumerate() {
        let live = lv.page(pi);
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

unsafe fn paint_col<A: Arch, B: Backend>(col: &RawCol, sv: &StoreView, u: &U, b: *mut ()) {
    // SAFETY: the caller instantiated this for the column's archetype and passes a `*mut B`.
    unsafe { A::paint(&mut *(b as *mut B), ColRef { pages: col.all::<A>(), space: sv.space, live: sv.live }, u) }
}

unsafe fn paint_at_col<A: Arch, B: Backend>(h: Half, e: &Entry, u: &U, b: *mut ()) {
    // SAFETY: the caller instantiated this for the entry's archetype, passes a `*mut B`, and nobody writes `h`
    // meanwhile; the entry's span holds `ceil(nrec / 64)` pages of `A`, its store owns `max(cap / 64, 1)` live
    // words from `live`, and `space` is its store's slot.
    unsafe {
        let col = ColRef {
            pages: core::slice::from_raw_parts(h.at(e.pages) as *const Page<A>, (e.nrec as usize).div_ceil(64)),
            space: &*h.space(e.space),
            live: Live { cap: e.cap, rows: core::slice::from_raw_parts(h.live(e.live), (e.cap as usize / 64).max(1)), nrec: e.nrec },
        };
        A::paint(&mut *(b as *mut B), col, u)
    }
}

/// Paint the `n` columns of half `h` in plan order (paint order): what a frame does.
///
/// SAFETY: every entry was written for backend type `B`, and nobody writes `h` meanwhile.
pub unsafe fn paint_half<B: Backend>(h: Half, n: usize, u: &U, b: &mut B) {
    // SAFETY: the caller's contract.
    unsafe {
        for e in h.plan(n) {
            (e.paint())(h, e, u, b as *mut B as *mut ())
        }
    }
}
