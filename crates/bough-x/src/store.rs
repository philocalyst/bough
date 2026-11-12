//! Archetype columns in 64-record pages (ROUND8 section 4) and the damage they produce (section 3).
//!
//! A [`Store`] holds the records of one static shape: the fixed part of a view, or the rows of one list. It is
//! not generic. A column is created by the first record described at a position of the shape and is written
//! through a typed pointer; every pass over a column (damage, hit testing, paint) is one function monomorphised
//! for the column's archetype, reached once per column through its [`Desc`].
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

pub type DeltaFn = unsafe fn(&RawCol, &StoreView, &U, &U, &mut Tiles);
pub type ActiveFn = unsafe fn(&RawCol, &StoreView, &U) -> (u32, Rect);
pub type HoverFn = unsafe fn(*mut (), *const u8, bool);
pub type HitFn = unsafe fn(&RawCol, &StoreView, &U, i32, i32) -> u32;
/// A column that hears the pointer: its handler and the hit kernel of its archetype (instantiated only for
/// archetypes that have a handler).
pub type Hov = (HoverFn, HitFn);
#[inline(always)]
pub fn hit_fn<A: Arch>() -> HitFn {
    hit_col::<A>
}

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
    };
}
#[inline(always)]
pub fn desc<A: Arch, B: Backend>() -> &'static Desc {
    &Holder::<A, B>::DESC
}

/// A column: pages of one archetype, type-erased.
pub struct RawCol {
    pages: *mut u8,
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
    pub hover: Option<Hov>,
}
impl RawCol {
    fn grow(&mut self, npages: u32) {
        if npages <= self.npages {
            return;
        }
        let one = self.desc.page;
        let new = Layout::from_size_align(one.size() * npages as usize, one.align()).unwrap();
        // SAFETY: `new` has non-zero size (a page is at least 1 KiB).
        let p = unsafe { alloc_zeroed(new) };
        if p.is_null() {
            handle_alloc_error(new);
        }
        if self.npages != 0 {
            // SAFETY: the old allocation holds `npages` pages of this layout; the new one is larger and distinct.
            unsafe {
                core::ptr::copy_nonoverlapping(self.pages, p, one.size() * self.npages as usize);
                dealloc(self.pages, Layout::from_size_align_unchecked(one.size() * self.npages as usize, one.align()));
            }
        }
        self.pages = p;
        self.npages = npages;
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
    /// The constants of record `rec`, untyped (for a handler that knows the type).
    pub fn rec_ptr(&self, rec: u32) -> *const u8 {
        assert!(rec < self.npages * 64);
        // SAFETY: in bounds of the page array by the assert; `rec_off` and `rec_size` are the archetype's.
        unsafe { self.pages.add((rec as usize >> 6) * self.desc.page.size() + self.desc.rec_off + (rec as usize & 63) * self.desc.rec_size) }
    }
}
impl Drop for RawCol {
    fn drop(&mut self) {
        if self.npages != 0 {
            let one = self.desc.page;
            // SAFETY: allocated in `grow` with exactly this layout.
            unsafe { dealloc(self.pages, Layout::from_size_align_unchecked(one.size() * self.npages as usize, one.align())) }
        }
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

pub struct Store {
    pub cols: Vec<RawCol>,
    pub sites: Vec<Site>,
    pub subs: Vec<Store>,
    /// Paint order: columns and child stores in the order the view first described them.
    pub order: Vec<Pass>,
    /// Row slots: 1 for a fixed shape, a power of two of at least 64 for a list's window.
    pub cap: u32,
    pub row_live: Vec<u64>,
    pub space: Space,
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
    /// The block words some column here reads, and whether some column reads `Time`.
    pub reads: u64,
    pub timed: bool,
}

static COL_ID: AtomicU32 = AtomicU32::new(1);

impl Store {
    pub fn new(cap: u32, space: Space) -> Store {
        assert!(cap == 1 || (cap >= 64 && cap.is_power_of_two()));
        Store { cols: Vec::new(), sites: Vec::new(), subs: Vec::new(), order: Vec::new(), cap, row_live: vec![0; (cap as usize / 64).max(1)], space, disc: Vec::new(), lo: 0, hi: 0, seen: 0, rows: Vec::new(), tmpl: Vec::new(), tmpl_at: 0, geo: (0, 0, 0), plan: Vec::new(), reads: 0, timed: false }
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
            self.cols.push(RawCol { pages: core::ptr::null_mut(), npages: 0, nrec: 0, k: 0, reads: d.reads, delta: d.delta, id: COL_ID.fetch_add(1, Ordering::Relaxed), key, arch, desc: d, seg, hover });
            self.order.push(Pass::Col(self.cols.len() as u32 - 1));
            self.cols.len() - 1
        });
        self.reads |= d.reads;
        self.timed |= d.timed;
        let c = &mut self.cols[j];
        let base = c.k * self.cap;
        c.k += 1;
        c.nrec = c.k * self.cap;
        c.grow(c.nrec.div_ceil(64));
        self.disc.push((j as u32, rect));
        self.sites.push(Site { page: core::ptr::null_mut(), page_size: 0, col: j as u32, base });
        self.fix_sites();
    }
    /// Point every site at its column's pages. Run whenever a column's pages may have moved.
    fn fix_sites(&mut self) {
        for s in &mut self.sites {
            let c = &self.cols[s.col as usize];
            // SAFETY: `base / 64` is a page of the column (`discover` grew it to hold the site).
            s.page = unsafe { c.pages.add((s.base >> 6) as usize * c.desc.page.size()) };
            s.page_size = c.desc.page.size() as u32;
        }
    }

    /// The damage of a change of the block from `old` to `new` (`ch` = the changed words), for this store and
    /// the stores under it. One indirect call per column that reads a changed word. A column whose records
    /// read `Time` also says, once, until when and where they will be in motion.
    pub fn delta(&self, old: &U, new: &U, ch: u64, tiles: &mut Tiles) {
        if self.space.reads() & ch != 0 {
            // the space moved: everything it shows is somewhere else
            tiles.mark(self.space.clip);
            if self.timed & (self.reads & ch != 0) {
                self.resweep(new, tiles);
            }
        } else if self.reads & ch != 0 {
            let live = Live { cap: self.cap, rows: &self.row_live, nrec: 0 };
            for c in &self.cols {
                if c.reads & ch != 0 {
                    let sv = StoreView { space: &self.space, live: Live { nrec: c.nrec, ..live } };
                    // SAFETY: `delta` and `active` were instantiated for the archetype `c` was created with.
                    unsafe {
                        (c.delta)(c, &sv, old, new, tiles);
                        if c.desc.timed {
                            tiles.set_sweep(c.id, self.space, (c.desc.active)(c, &sv, new));
                        }
                    }
                }
            }
        }
        for s in &self.subs {
            s.delta(old, new, ch, tiles);
        }
    }

    /// Records of this store were written: say again which of its timed columns are in motion. Cold unless
    /// the store has such a column.
    #[inline(never)]
    pub fn resweep(&self, u: &U, tiles: &mut Tiles) {
        for c in &self.cols {
            if c.desc.timed {
                let sv = StoreView { space: &self.space, live: Live { cap: self.cap, rows: &self.row_live, nrec: c.nrec } };
                // SAFETY: `active` was instantiated for the archetype `c` was created with.
                tiles.set_sweep(c.id, self.space, unsafe { (c.desc.active)(c, &sv, u) });
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
    pub fn hit(&self, u: &U, x: i32, y: i32, path: &mut Path, best: &mut Option<(Path, u32, u32)>) {
        for p in &self.order {
            match *p {
                Pass::Col(j) => {
                    let c = &self.cols[j as usize];
                    let Some((_, hit)) = c.hover else { continue };
                    let sv = StoreView { space: &self.space, live: Live { cap: self.cap, rows: &self.row_live, nrec: c.nrec } };
                    // SAFETY: `hit` was instantiated for the archetype `c` was created with.
                    let r = unsafe { hit(c, &sv, u, x, y) };
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

unsafe fn delta_col<A: Arch>(col: &RawCol, sv: &StoreView, old: &U, new: &U, tiles: &mut Tiles) {
    // SAFETY: the caller instantiated this for the column's archetype.
    let pages = unsafe { col.all::<A>() };
    let org = sv.space.origin(new);
    let clip = sv.space.clip;
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
    }
}

/// The column's bound in `Time` under `u`: until when some live record is in motion, and the box they stay
/// inside until then. Every live record is asked, not the ones a difference names: a motion that starts now
/// shows, at this instant, exactly what was shown before it started. Runs in a turn, never in a frame.
unsafe fn active_col<A: Arch>(col: &RawCol, sv: &StoreView, u: &U) -> (u32, Rect) {
    // SAFETY: the caller instantiated this for the column's archetype.
    let pages = unsafe { col.all::<A>() };
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

unsafe fn hit_col<A: Arch>(col: &RawCol, sv: &StoreView, u: &U, x: i32, y: i32) -> u32 {
    // SAFETY: the caller instantiated this for the column's archetype.
    let pages = unsafe { col.all::<A>() };
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

unsafe fn paint_col<A: Arch, B: Backend>(col: &RawCol, sv: &StoreView, u: &U, b: *mut ()) {
    // SAFETY: the caller instantiated this for the column's archetype and passes a `*mut B`.
    unsafe { A::paint(&mut *(b as *mut B), ColRef { pages: col.all::<A>(), space: sv.space, live: sv.live }, u) }
}
