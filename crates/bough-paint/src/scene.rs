//! The scene: columns of records (one per archetype), the paint order, the window lane, the tile bins, and
//! the CPU frame. The columns live in one page-aligned arena whose words are the GPU's buffer (`metal.rs`
//! wraps it without a copy), in the seam's page layout: 64 records per page, rect lanes then records.
//!
//! A frame is: compare the block with the one last drawn (a mask); for the columns whose archetype reads a
//! changed word, one pass over the column (`window`: the window lane, the damage); rebuild the bins if a
//! window moved; shade the damaged tiles. A column's kernels are reached through its `&'static Desc`:
//! once per column for the window pass, once per run of a tile's entries for shading. Nothing is decoded,
//! restaged or looked up per record.
use crate::expr::{Cx, U};
use crate::paint::{Cost, EMPTY, NO_CLIP, NO_HIT, PCode, Paint, SpecSink, StageSink, WinSink};
use crate::tile::{Flat, Shade, Stats, Tile};
use bough_kernel::cpu::Pool;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use wgsl_rs::std::{Vec4, Vec4i, Vec4u};

/// Lanes before the records in a page: x0, y0, x1, y1 (Q8) and the paint-order key.
pub const LANES: usize = 5;
/// `order` word: column in the high 12 bits, record index in the low 20.
pub const IDX_BITS: u32 = 20;
pub const IDX_MASK: u32 = (1 << IDX_BITS) - 1;
/// The arena's alignment: a VM page on Apple silicon, so Metal can wrap it without a copy.
pub const VM_PAGE: usize = 16384;

/// A column's records as raw words (what a `Desc` function receives).
#[derive(Clone, Copy)]
pub struct ColView {
    base: *const u32,
    pub len: u32,
    /// Words per record.
    pub w: u32,
}
impl ColView {
    #[inline(always)]
    fn page(&self, i: u32) -> *const u32 {
        // SAFETY (callers): i < len <= capacity, so the page is inside the column's allocation.
        unsafe { self.base.add((i as usize >> 6) * 64 * (LANES + self.w as usize)) }
    }
    #[inline(always)]
    pub fn rect(&self, i: u32) -> [i32; 4] {
        let (p, r) = (self.page(i), i as usize & 63);
        // SAFETY: the four rect lanes are the page's first 256 words.
        unsafe { [*p.add(r) as i32, *p.add(64 + r) as i32, *p.add(128 + r) as i32, *p.add(192 + r) as i32] }
    }
    #[inline(always)]
    pub fn z(&self, i: u32) -> u32 {
        // SAFETY: the key lane is words 256..320 of the page.
        unsafe { *self.page(i).add(256 + (i as usize & 63)) }
    }
    /// Record `i` in place.
    ///
    /// # Safety
    /// `P` is the column's archetype (`size_of::<P>() == 4 * self.w`) and `i < self.len`.
    #[inline(always)]
    pub unsafe fn rec<P: Paint>(&self, i: u32) -> &P {
        // SAFETY: `P: Words` (whole words, alignment <= 4, any bits valid); the record's words follow the lanes.
        unsafe { &*(self.page(i).add(64 * LANES + (i as usize & 63) * self.w as usize) as *const P) }
    }
}

/// Damaged tiles of a frame, one bit each, row-major.
pub struct Damage {
    pub bits: Vec<u64>,
    pub tx: usize,
    pub ty: usize,
}
impl Damage {
    pub fn new(tx: usize, ty: usize) -> Damage {
        Damage { bits: vec![0; (tx * ty).div_ceil(64)], tx, ty }
    }
    pub fn clear(&mut self) {
        self.bits.fill(0);
    }
    pub fn all(&mut self) {
        for t in 0..self.tx * self.ty {
            self.bits[t / 64] |= 1 << (t % 64);
        }
    }
    /// The tiles a pixel window touches.
    #[inline]
    pub fn add(&mut self, w: [i16; 4]) {
        let x0 = (w[0].max(0) as usize) >> 5;
        let y0 = (w[1].max(0) as usize) >> 5;
        let x1 = ((w[2].max(0) as usize + 31) >> 5).min(self.tx);
        let y1 = ((w[3].max(0) as usize + 31) >> 5).min(self.ty);
        for y in y0..y1 {
            for x in x0..x1 {
                let t = y * self.tx + x;
                self.bits[t / 64] |= 1 << (t % 64);
            }
        }
    }
    pub fn tiles(&self, out: &mut Vec<u32>) {
        out.clear();
        for (i, &w) in self.bits.iter().enumerate() {
            let mut m = w;
            while m != 0 {
                out.push((i * 64) as u32 + m.trailing_zeros());
                m &= m - 1;
            }
        }
    }
    pub fn count(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }
}

/// What the scene knows about an archetype: its constants (from the paint type) and its kernels (the paint
/// type's code, monomorphised). One per paint type, in rodata.
pub struct Desc {
    pub name: &'static str,
    /// Words per record.
    pub words: usize,
    pub reads: u64,
    pub geom_reads: u64,
    pub opaque_core: bool,
    pub hit: bool,
    pub cost: Cost,
    pub layers: usize,
    pub code: &'static PCode,
    /// The window pass: for every record, its window and cover under `new` into the lanes (by key), and
    /// damage for a record whose window moved or whose operands' values changed. Returns whether a window or
    /// cover moved.
    pub window: fn(ColView, old: &U, new: &U, mask: u64, first: bool, win: &mut [[i16; 4]], cover: &mut [[i16; 4]], dmg: &mut Damage) -> bool,
    pub flat: fn(ColView, zs: &[u32], order: &[u32], u: &U, t: &mut Tile),
    /// `first`: the index of `zs[0]` in the tile's list (its occlusion mask).
    pub shade: fn(ColView, zs: &[u32], first: usize, order: &[u32], u: &U, t: &mut Tile),
    /// The specification at one pixel: (blended accumulator, some layer inked the pixel).
    pub spec: fn(ColView, i: u32, u: &U, x: i32, y: i32, acc: Vec4u, atlas: &[u8], aw: u32, tabs: &[u32]) -> (Vec4u, bool),
    /// The record as the generic kernel's commands.
    pub stage: fn(ColView, i: u32, u: &U, out: &mut bough_kernel::stage::Scene),
    pub pick: fn(ColView, i: u32, u: &U) -> u32,
    /// One rewritten record: damage where it was and where it is now, its lanes brought up to date. Returns
    /// whether its window or cover moved.
    pub rewin: fn(ColView, i: u32, u: &U, win: &mut [[i16; 4]], cover: &mut [[i16; 4]], dmg: &mut Damage) -> bool,
}

fn i16s(v: Vec4i) -> [i16; 4] {
    [v.x as i16, v.y as i16, v.z as i16, v.w as i16]
}

fn window_pass<P: Paint>(c: ColView, old: &U, new: &U, mask: u64, first: bool, win: &mut [[i16; 4]], cover: &mut [[i16; 4]], dmg: &mut Damage) -> bool {
    let geom = first || P::GEOM_READS & mask != 0;
    let mut moved = false;
    for i in 0..c.len {
        // SAFETY: this function is only stored in `Desc::of::<P>()`, which a column of `P` holds.
        let (rect, z, p) = (c.rect(i), c.z(i) as usize, unsafe { c.rec::<P>(i) });
        let cn = Cx { u: new, rect };
        if geom {
            let mut ws = WinSink(EMPTY);
            p.paint(&cn, NO_CLIP, &mut ws);
            let w = i16s(ws.0);
            let cv = if !P::OPAQUE_CORE {
                [0; 4]
            } else if cfg!(feature = "mutant_cut") {
                w
            } else {
                i16s(p.cover(&cn, NO_CLIP))
            };
            if first || w != win[z] || cv != cover[z] {
                if !first {
                    dmg.add(win[z]);
                }
                dmg.add(w);
                (win[z], cover[z]) = (w, cv);
                moved = true;
                continue;
            }
        }
        if !first && p.changed(&Cx { u: old, rect }, &cn, mask) {
            dmg.add(win[z]);
        }
    }
    moved
}
fn rewin_one<P: Paint>(c: ColView, i: u32, u: &U, win: &mut [[i16; 4]], cover: &mut [[i16; 4]], dmg: &mut Damage) -> bool {
    // SAFETY: as `window_pass`.
    let (rect, z, p) = (c.rect(i), c.z(i) as usize, unsafe { c.rec::<P>(i) });
    let cx = Cx { u, rect };
    let mut ws = WinSink(EMPTY);
    p.paint(&cx, NO_CLIP, &mut ws);
    let w = i16s(ws.0);
    let cv = if P::OPAQUE_CORE { i16s(p.cover(&cx, NO_CLIP)) } else { [0; 4] };
    dmg.add(win[z]);
    dmg.add(w);
    let moved = w != win[z] || cv != cover[z];
    (win[z], cover[z]) = (w, cv);
    moved
}
fn flat_run<P: Paint>(c: ColView, zs: &[u32], order: &[u32], u: &U, t: &mut Tile) {
    for &z in zs {
        let i = order[z as usize] & IDX_MASK;
        // SAFETY: as `window_pass`; `order` names records of this column for these keys.
        unsafe { c.rec::<P>(i) }.paint(&Cx { u, rect: c.rect(i) }, NO_CLIP, &mut Flat(t));
    }
}
fn shade_run<P: Paint>(c: ColView, zs: &[u32], first: usize, order: &[u32], u: &U, t: &mut Tile) {
    for (j, &z) in zs.iter().enumerate() {
        t.cur = first + j;
        let i = order[z as usize] & IDX_MASK;
        // SAFETY: as `flat_run`.
        unsafe { c.rec::<P>(i) }.paint(&Cx { u, rect: c.rect(i) }, NO_CLIP, &mut Shade(t));
    }
}
fn spec_one<P: Paint>(c: ColView, i: u32, u: &U, x: i32, y: i32, acc: Vec4u, atlas: &[u8], aw: u32, tabs: &[u32]) -> (Vec4u, bool) {
    let mut s = SpecSink { x, y, acc, atlas, aw, tabs, ink: false };
    // SAFETY: as `window_pass`.
    unsafe { c.rec::<P>(i) }.paint(&Cx { u, rect: c.rect(i) }, NO_CLIP, &mut s);
    (s.acc, s.ink)
}
fn stage_one<P: Paint>(c: ColView, i: u32, u: &U, out: &mut bough_kernel::stage::Scene) {
    // SAFETY: as `window_pass`.
    unsafe { c.rec::<P>(i) }.paint(&Cx { u, rect: c.rect(i) }, NO_CLIP, &mut StageSink(out));
}
fn pick_one<P: Paint>(c: ColView, i: u32, u: &U) -> u32 {
    // SAFETY: as `window_pass`.
    unsafe { c.rec::<P>(i) }.pick_id(&Cx { u, rect: c.rect(i) })
}

struct D<P>(PhantomData<P>);
impl<P: Paint> D<P> {
    const DESC: Desc = Desc {
        name: std::any::type_name::<P>(),
        words: {
            assert!(size_of::<P>() % 4 == 0 && align_of::<P>() <= 4);
            size_of::<P>() / 4
        },
        reads: P::READS,
        geom_reads: P::GEOM_READS,
        opaque_core: P::OPAQUE_CORE,
        hit: P::HIT,
        cost: P::COST,
        layers: P::LAYERS,
        code: P::CODE,
        window: window_pass::<P>,
        flat: flat_run::<P>,
        shade: shade_run::<P>,
        spec: spec_one::<P>,
        stage: stage_one::<P>,
        pick: pick_one::<P>,
        rewin: rewin_one::<P>,
    };
}
impl Desc {
    pub const fn of<P: Paint>() -> &'static Desc {
        &D::<P>::DESC
    }
}

pub struct Col {
    pub desc: &'static Desc,
    /// First word in the arena.
    pub base: usize,
    /// Capacity in records (a multiple of 64).
    pub cap: usize,
    pub len: usize,
}
pub struct ColId<P>(pub usize, PhantomData<P>);
impl<P> Clone for ColId<P> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<P> Copy for ColId<P> {}

/// Words in VM-page-aligned memory that never moves.
pub struct Arena {
    ptr: *mut u32,
    words: usize,
}
impl Arena {
    pub fn new(words: usize) -> Arena {
        let bytes = (words * 4).div_ceil(VM_PAGE).max(1) * VM_PAGE;
        let layout = std::alloc::Layout::from_size_align(bytes, VM_PAGE).unwrap();
        // SAFETY: non-zero size; a null return is checked.
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) } as *mut u32;
        assert!(!ptr.is_null());
        Arena { ptr, words: bytes / 4 }
    }
    pub fn ptr(&self) -> *mut u32 {
        self.ptr
    }
    pub fn bytes(&self) -> usize {
        self.words * 4
    }
    pub fn words(&self) -> &[u32] {
        // SAFETY: `ptr` is `words` initialised (zeroed) words owned by this arena.
        unsafe { std::slice::from_raw_parts(self.ptr, self.words) }
    }
    fn words_mut(&mut self) -> &mut [u32] {
        // SAFETY: as `words`, uniquely borrowed.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.words) }
    }
}
impl Drop for Arena {
    fn drop(&mut self) {
        // SAFETY: allocated in `new` with this layout.
        unsafe { std::alloc::dealloc(self.ptr as *mut u8, std::alloc::Layout::from_size_align(self.words * 4, VM_PAGE).unwrap()) }
    }
}
// SAFETY: the arena owns its memory; shared access is read-only (`words`), mutation needs `&mut`.
unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

/// Per-tile entry lists (paint-order keys, back to front, cut at the last opaque cover).
#[derive(Default)]
pub struct Bins {
    pub off: Vec<u32>,
    pub list: Vec<u32>,
    count: Vec<u32>,
    floor: Vec<u32>,
}
impl Bins {
    #[inline]
    pub fn tile(&self, t: usize) -> &[u32] {
        &self.list[self.off[t] as usize..self.off[t + 1] as usize]
    }
    /// Walk the window lane in paint order (plain data: no archetype is consulted).
    pub fn build(&mut self, win: &[[i16; 4]], cover: &[[i16; 4]], tx: usize, ty: usize, cut: bool) {
        let nt = tx * ty;
        self.count.clear();
        self.count.resize(nt, 0);
        self.floor.clear();
        self.floor.resize(nt, 0);
        let span = |w: &[i16; 4]| ((w[0].max(0) as usize) >> 5, (w[1].max(0) as usize) >> 5, ((w[2].max(0) as usize + 31) >> 5).min(tx), ((w[3].max(0) as usize + 31) >> 5).min(ty));
        for (z, w) in win.iter().enumerate() {
            let (x0, y0, x1, y1) = span(w);
            let c = cover[z];
            let covers = cut && c[0] < c[2];
            for y in y0..y1 {
                for x in x0..x1 {
                    let t = y * tx + x;
                    let (ox, oy) = (x as i32 * 32, y as i32 * 32);
                    if covers && c[0] as i32 <= ox && c[1] as i32 <= oy && c[2] as i32 >= ox + 32 && c[3] as i32 >= oy + 32 {
                        self.floor[t] = z as u32;
                        self.count[t] = 1;
                    } else {
                        self.count[t] += 1;
                    }
                }
            }
        }
        self.off.clear();
        self.off.reserve(nt + 1);
        let mut at = 0u32;
        for t in 0..nt {
            self.off.push(at);
            at += self.count[t];
            self.count[t] = 0;
        }
        self.off.push(at);
        self.list.clear();
        self.list.resize(at as usize, 0);
        for (z, w) in win.iter().enumerate() {
            let (x0, y0, x1, y1) = span(w);
            for y in y0..y1 {
                for x in x0..x1 {
                    let t = y * tx + x;
                    if (z as u32) < self.floor[t] {
                        continue;
                    }
                    self.list[(self.off[t] + self.count[t]) as usize] = z as u32;
                    self.count[t] += 1;
                }
            }
        }
    }
}

/// What a frame did.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    /// Block words that changed.
    pub mask: u64,
    /// Columns whose window pass ran, and the records they hold.
    pub cols_visited: u32,
    pub recs_visited: u32,
    pub rebinned: bool,
    pub tiles: u32,
    pub raster: Stats,
}

pub struct Scene {
    pub fw: u32,
    pub fh: u32,
    pub tx: usize,
    pub ty: usize,
    pub arena: Arena,
    used: usize,
    pub cols: Vec<Col>,
    /// Paint-order key -> (column << 20 | index).
    pub order: Vec<u32>,
    /// By key: the record's pixel window and its opaque cover under the block last seen.
    pub win: Vec<[i16; 4]>,
    pub cover: Vec<[i16; 4]>,
    pub bins: Bins,
    pub damage: Damage,
    /// The block the surface shows.
    pub prev: U,
    pub first: bool,
    pub atlas: Vec<u8>,
    pub aw: u32,
    pub cut: bool,
    pub flat: bool,
    tiles: Vec<u32>,
    /// Keys of records rewritten since the last `update` (`set`).
    touched: Vec<u32>,
    /// The corner tables: a region of the arena after the columns (so the GPU reads them from the buffer it
    /// already has), handles by key, and the tables allocated but not yet filled.
    tab_base: usize,
    tab_used: usize,
    tab_map: std::collections::HashMap<(i32, i32), u32>,
    tab_jobs: Vec<TabJob>,
}

/// Words reserved for corner tables (4 MiB of address space; pages are touched only when a table is filled).
pub const TAB_WORDS: usize = 1 << 24;
/// A table to fill: its handle (word offset in the table region), key, and entries per side.
#[derive(Clone, Copy, Debug)]
pub struct TabJob {
    pub base: u32,
    pub r: i32,
    pub w: i32,
    pub n: u32,
}

/// `BOUGH_PAINT_PROF=1`: nanoseconds and tile entries per column, accumulated by `shade` (a measurement
/// build's switch: it times every run, so the totals are for proportions, never for a result).
pub static PROF: std::sync::Mutex<[(u64, u64); 32]> = std::sync::Mutex::new([(0, 0); 32]);
static PROF_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

struct OutPtr(*mut u32);
// SAFETY: workers write disjoint tiles through it (`Scene::shade`).
unsafe impl Sync for OutPtr {}

impl Scene {
    pub fn new(fw: u32, fh: u32, arena_words: usize) -> Scene {
        let (tx, ty) = (fw.div_ceil(32) as usize, fh.div_ceil(32) as usize);
        Scene {
            fw,
            fh,
            tx,
            ty,
            arena: Arena::new(arena_words + TAB_WORDS),
            tab_base: arena_words,
            tab_used: 0,
            tab_map: Default::default(),
            tab_jobs: Vec::new(),
            used: 0,
            cols: Vec::new(),
            order: Vec::new(),
            win: Vec::new(),
            cover: Vec::new(),
            bins: Bins::default(),
            damage: Damage::new(tx, ty),
            prev: U::default(),
            first: true,
            atlas: Vec::new(),
            aw: 1,
            cut: true,
            flat: true,
            tiles: Vec::new(),
            touched: Vec::new(),
        }
    }
    /// A column for archetype `P` holding up to `cap` records.
    pub fn column<P: Paint>(&mut self, cap: usize) -> ColId<P> {
        let desc = Desc::of::<P>();
        let cap = cap.div_ceil(64).max(1) * 64;
        let words = cap * (LANES + desc.words);
        assert!(self.used + words <= self.tab_base, "arena too small");
        assert!(self.cols.len() < 1 << (32 - IDX_BITS) && cap <= 1 << IDX_BITS);
        self.cols.push(Col { desc, base: self.used, cap, len: 0 });
        self.used += words;
        ColId(self.cols.len() - 1, PhantomData)
    }
    /// Append a record: its rect lanes (Q8) and its value. Paint order is mount order. Returns its key.
    pub fn push<P: Paint>(&mut self, col: ColId<P>, rect: [i32; 4], p: P) -> u32 {
        let p = self.memo(rect, p);
        let z = self.order.len() as u32;
        let c = &mut self.cols[col.0];
        assert!(std::ptr::eq(c.desc, Desc::of::<P>()) || c.desc.name == Desc::of::<P>().name);
        assert!(c.len < c.cap, "column {} full", c.desc.name);
        let (i, w, base) = (c.len, c.desc.words, c.base);
        c.len += 1;
        let page = base + (i >> 6) * 64 * (LANES + w);
        let r = i & 63;
        let words = self.arena.words_mut();
        for l in 0..4 {
            words[page + l * 64 + r] = rect[l] as u32;
        }
        words[page + 256 + r] = z;
        let at = page + 64 * LANES + r * w;
        // SAFETY: `P: Words`: `w` whole words with no padding; the destination is `w` words inside the arena.
        unsafe { std::ptr::copy_nonoverlapping(&p as *const P as *const u32, words[at..at + w].as_mut_ptr(), w) };
        self.order.push((col.0 as u32) << IDX_BITS | i as u32);
        self.win.push([0; 4]);
        self.cover.push([0; 4]);
        self.first = true;
        z
    }
    /// Rewrite record `i` of a column in place (a turn's write: a rect the layout moved, an operand a CPU
    /// stage expression re-evaluated, a transition retargeted). Returns its key. The next `update` damages
    /// where it was and where it is; nothing else about the frame changes.
    pub fn set<P: Paint>(&mut self, col: ColId<P>, i: usize, rect: [i32; 4], p: P) -> u32 {
        let p = self.memo(rect, p);
        let c = &self.cols[col.0];
        assert!(std::ptr::eq(c.desc, Desc::of::<P>()) || c.desc.name == Desc::of::<P>().name);
        assert!(i < c.len);
        let (w, base) = (c.desc.words, c.base);
        let page = base + (i >> 6) * 64 * (LANES + w);
        let r = i & 63;
        let words = self.arena.words_mut();
        for l in 0..4 {
            words[page + l * 64 + r] = rect[l] as u32;
        }
        let z = words[page + 256 + r];
        let at = page + 64 * LANES + r * w;
        // SAFETY: as `push`.
        unsafe { std::ptr::copy_nonoverlapping(&p as *const P as *const u32, words[at..at + w].as_mut_ptr(), w) };
        self.touched.push(z);
        z
    }
    /// Give the record's memoisable layers their table handles (`Paint::memo`), allocating a table for a
    /// key not seen before. Cold: once per record mount or rewrite.
    fn memo<P: Paint>(&mut self, rect: [i32; 4], mut p: P) -> P {
        let zero = U::default();
        let (map, jobs, used) = (&mut self.tab_map, &mut self.tab_jobs, &mut self.tab_used);
        p.memo(&Cx { u: &zero, rect }, &mut |r, w| {
            *map.entry((r, w)).or_insert_with(|| {
                let n = crate::ops::ops::t_n(r, w);
                let words = 2 + (n * n) as usize;
                if *used + words > TAB_WORDS {
                    return crate::ops::ops::T_MISS;
                }
                let base = *used as u32;
                *used += words;
                jobs.push(TabJob { base, r, w, n });
                base
            })
        });
        // a fresh table's header names no key until it is filled
        let tb = self.tab_base;
        let words = self.arena.words_mut();
        for j in &self.tab_jobs {
            if words[tb + j.base as usize] == 0 && words[tb + j.base as usize + 1] == 0 {
                (words[tb + j.base as usize], words[tb + j.base as usize + 1]) = (u32::MAX, u32::MAX);
            }
        }
        p
    }
    /// The corner tables' words (handles index it).
    pub fn tabs(&self) -> &[u32] {
        &self.arena.words()[self.tab_base..self.tab_base + TAB_WORDS]
    }
    /// The table region's first word in the arena, and its words in use.
    pub fn tab_region(&self) -> (usize, usize) {
        (self.tab_base, self.tab_used)
    }
    /// Tables allocated and not yet filled.
    pub fn tab_jobs(&self) -> &[TabJob] {
        &self.tab_jobs
    }
    /// The pending tables' entries have been written by someone else (the GPU, into this arena): name
    /// their keys in the headers, which is what makes a shader use them.
    pub fn tabs_filled(&mut self) {
        let tb = self.tab_base;
        let jobs = std::mem::take(&mut self.tab_jobs);
        let words = self.arena.words_mut();
        for j in &jobs {
            (words[tb + j.base as usize], words[tb + j.base as usize + 1]) = (j.r as u32, j.w as u32);
        }
    }
    /// Fill the pending tables on this thread (the CPU-only path; the same leaf the GPU kernel runs).
    pub fn fill_tabs(&mut self) {
        let tb = self.tab_base;
        let jobs = self.tab_jobs.clone();
        let words = self.arena.words_mut();
        for j in &jobs {
            for y in 0..j.n {
                for x in 0..j.n {
                    let v = crate::ops::ops::x_cfill(j.r, j.w, x, y);
                    words[tb + j.base as usize + 2 + (y * j.n + x) as usize] = if cfg!(feature = "mutant_tab") { v + 1 } else { v };
                }
            }
        }
        self.tabs_filled();
    }
    pub fn view(&self, col: usize) -> ColView {
        let c = &self.cols[col];
        // SAFETY: `base` is inside the arena (checked at `column`).
        ColView { base: unsafe { self.arena.ptr().add(c.base) }, len: c.len as u32, w: c.desc.words as u32 }
    }
    pub fn records(&self) -> usize {
        self.order.len()
    }
    /// Words the columns occupy (lanes and records, by capacity).
    pub fn used_words(&self) -> usize {
        self.used
    }

    /// Steps 1 to 3 of a frame: the mask, the window passes, the bins. Leaves the damage in `self.damage`.
    pub fn update(&mut self, u: &U) -> FrameStats {
        let mut fs = FrameStats::default();
        if !self.tab_jobs.is_empty() {
            self.fill_tabs();
        }
        let first = self.first;
        let mask = if first { u64::MAX } else { self.prev.diff(u, 64) };
        fs.mask = mask;
        if first {
            self.damage.all();
        }
        let mut moved = first;
        let mut touched = std::mem::take(&mut self.touched);
        if !first {
            for &z in &touched {
                let o = self.order[z as usize];
                let ci = (o >> IDX_BITS) as usize;
                let v = self.view(ci);
                moved |= (self.cols[ci].desc.rewin)(v, o & IDX_MASK, u, &mut self.win, &mut self.cover, &mut self.damage);
            }
        }
        touched.clear();
        self.touched = touched;
        for ci in 0..self.cols.len() {
            let d = self.cols[ci].desc;
            if !first && d.reads & mask == 0 {
                continue;
            }
            fs.cols_visited += 1;
            fs.recs_visited += self.cols[ci].len as u32;
            let v = self.view(ci);
            moved |= (d.window)(v, &self.prev, u, mask, first, &mut self.win, &mut self.cover, &mut self.damage);
        }
        if moved {
            self.bins.build(&self.win, &self.cover, self.tx, self.ty, self.cut);
            fs.rebinned = true;
        }
        self.prev = *u;
        self.first = false;
        fs
    }

    /// Shade `tiles` (indices, any order, no duplicates) into `out` on the caller plus `extra` pool workers.
    pub fn shade(&self, u: &U, tiles: &[u32], out: &mut [u32], stride: usize, pool: &Pool, extra: usize) -> Stats {
        assert!(out.len() >= (self.fh as usize - 1) * stride + self.fw as usize);
        let optr = OutPtr(out.as_mut_ptr());
        let prof = *PROF_ON.get_or_init(|| std::env::var("BOUGH_PAINT_PROF").is_ok());
        let next = AtomicUsize::new(0);
        const CHUNK: usize = 8;
        let total = std::sync::Mutex::new(Stats::default());
        let work = |_id: usize| {
            let optr = &optr;
            let mut t = Tile::new(&self.atlas, self.aw, self.tabs());
            loop {
                let a = next.fetch_add(CHUNK, Relaxed);
                if a >= tiles.len() {
                    break;
                }
                for &ti in &tiles[a..(a + CHUNK).min(tiles.len())] {
                    let (ox, oy) = ((ti as usize % self.tx) as i32 * 32, (ti as usize / self.tx) as i32 * 32);
                    let (cols, nrows) = ((self.fw as usize - ox as usize).min(32), (self.fh as usize - oy as usize).min(32));
                    let list = self.bins.tile(ti as usize);
                    t.begin(ox, oy, cols, nrows, self.flat);
                    if self.flat {
                        self.runs(list, |c, _, zs| (self.cols[c].desc.flat)(self.view(c), zs, &self.order, u, &mut t));
                    }
                    t.cover_masks(list, &self.cover, self.cut && list.len() > 1);
                    t.prepare();
                    if prof {
                        self.runs(list, |c, first, zs| {
                            let t0 = std::time::Instant::now();
                            (self.cols[c].desc.shade)(self.view(c), zs, first, &self.order, u, &mut t);
                            let mut p = PROF.lock().unwrap();
                            p[c].0 += t0.elapsed().as_nanos() as u64;
                            p[c].1 += zs.len() as u64;
                        });
                    } else {
                        self.runs(list, |c, first, zs| (self.cols[c].desc.shade)(self.view(c), zs, first, &self.order, u, &mut t));
                    }
                    t.st.entries += list.len() as u64;
                    let at = oy as usize * stride + ox as usize;
                    // SAFETY: the tile's rows [oy, oy + nrows) x columns [ox, ox + cols) lie inside `out`
                    // (asserted above), `out` is exclusively borrowed for this call, and tiles are disjoint
                    // and each is claimed by one worker (the atomic counter), so nothing else touches them.
                    unsafe { t.store(optr.0.add(at), stride) };
                }
            }
            total.lock().unwrap().add(&t.st);
        };
        let extra = extra.min(tiles.len() / (2 * CHUNK));
        pool.run(extra, &work);
        total.into_inner().unwrap()
    }
    /// Maximal runs of one column in a tile's list.
    #[inline(always)]
    fn runs(&self, list: &[u32], mut f: impl FnMut(usize, usize, &[u32])) {
        let mut a = 0;
        while a < list.len() {
            let c = self.order[list[a] as usize] >> IDX_BITS;
            let mut b = a + 1;
            while b < list.len() && self.order[list[b] as usize] >> IDX_BITS == c {
                b += 1;
            }
            f(c as usize, a, &list[a..b]);
            a = b;
        }
    }

    /// A whole CPU frame: update, then shade what the update damaged. `out` holds the previous frame.
    pub fn frame(&mut self, u: &U, out: &mut [u32], stride: usize, pool: &Pool, extra: usize) -> FrameStats {
        let mut fs = self.update(u);
        let mut tiles = std::mem::take(&mut self.tiles);
        self.damage.tiles(&mut tiles);
        self.damage.clear();
        fs.tiles = tiles.len() as u32;
        fs.raster = self.shade(u, &tiles, out, stride, pool, extra);
        self.tiles = tiles;
        fs
    }
    /// The tiles the last `frame` shaded.
    pub fn last_tiles(&self) -> &[u32] {
        &self.tiles
    }
    /// Forget what the surface shows: the next frame redraws everything.
    pub fn invalidate(&mut self) {
        self.first = true;
    }

    /// The scene under `u` as the generic kernel's commands, in paint order: the oracle's input, and the
    /// baseline's (what round 7 restaged on the CPU whenever state changed).
    pub fn stage(&self, u: &U) -> bough_kernel::stage::Scene {
        let mut s = bough_kernel::stage::Scene::default();
        for &o in &self.order {
            let c = (o >> IDX_BITS) as usize;
            (self.cols[c].desc.stage)(self.view(c), o & IDX_MASK, u, &mut s);
        }
        s
    }
    /// The archetypes' specification, pixel by pixel over every record (small frames only).
    pub fn spec(&self, u: &U, out: &mut [u32], stride: usize) {
        for y in 0..self.fh as i32 {
            for x in 0..self.fw as i32 {
                let mut acc = Vec4 { x: 0, y: 0, z: 0, w: 0 };
                for &o in &self.order {
                    let c = (o >> IDX_BITS) as usize;
                    acc = (self.cols[c].desc.spec)(self.view(c), o & IDX_MASK, u, x, y, acc, &self.atlas, self.aw, self.tabs()).0;
                }
                out[y as usize * stride + x as usize] = crate::ops::ops::x_store(acc);
            }
        }
    }
    /// The pick: the id of the topmost record that inks pixel (x, y), from the tile's bin (front to back).
    /// `NO_HIT` when nothing inks it or the topmost record carries no id.
    pub fn pick(&self, u: &U, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.fw as i32 || y >= self.fh as i32 {
            return NO_HIT;
        }
        let t = (y as usize >> 5) * self.tx + (x as usize >> 5);
        for &z in self.bins.tile(t).iter().rev() {
            let o = self.order[z as usize];
            let c = (o >> IDX_BITS) as usize;
            let d = self.cols[c].desc;
            let (_, ink) = (d.spec)(self.view(c), o & IDX_MASK, u, x, y, Vec4 { x: 0, y: 0, z: 0, w: 0 }, &self.atlas, self.aw, self.tabs());
            if ink {
                return if d.hit { (d.pick)(self.view(c), o & IDX_MASK, u) } else { NO_HIT };
            }
        }
        NO_HIT
    }
}
