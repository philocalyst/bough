//! The scene's memory: one reserved range, laid out so that what a turn or a frame crosses first lies together,
//! and addressed by offsets only (threads lane, round 8; `docs/lanes/threads/JOURNAL.md` section 3).
//!
//! ```text
//! base   shared   head, the dirty bitmap, the ranges it names, the motion of columns in motion
//!        W        the working half: what the turn thread reads and writes
//!        F        the published half: what a frame and every other reader reads; written only by `publish`
//! half   plan     one 64-byte entry per column, in paint order
//!        spaces   clip, base and scroll word of every space
//!        live     the live words of every store
//!        columns  pages, packed
//! ```
//!
//! The two halves have the same layout, so an offset means the same thing in both and "which copy" is one base
//! pointer. Nothing in a half is an address: a half is position independent, and the published half is the
//! image a file can hold (bbtree-mmap: the structure's own pages, laid end to end, pointers as offsets).
//!
//! **Who writes what.** The turn thread alone writes W and the shared part. Nobody writes F while a frame reads
//! it: `publish` copies the ranges the turn dirtied from W to F between frames. So a frame is a pure function of
//! (the block it was given, F), the turn never waits for a reader, and no reader holds a count or a lock.
//! Readers that are not the frame (accessibility, the inspector, a worker) bracket their read of F with the
//! epoch, as a sequence lock: [`Ar::read`].
use crate::expr::U;
use crate::store::{Desc, Rect, Space, Tiles};
use core::sync::atomic::{AtomicU32, Ordering, fence};

pub const LINE: usize = 64;
/// Columns a scene may have (the plan is 64 KiB of each half).
pub const MAX_COLS: usize = 1024;
pub const MAX_SPACES: usize = 512;
/// Live words (64 row slots each).
pub const MAX_LIVE: usize = 2048;
/// Ranges the dirty bitmap can name: every column page is one.
pub const MAX_RANGES: usize = 16384;

const HEAD: usize = 0;
const DIRTY: usize = 4 << 10;
const MOTION: usize = 8 << 10;
const RANGES: usize = 32 << 10;
/// Where the working half starts.
const HALVES: usize = 256 << 10;

const PLAN: usize = 0;
const SPACES: usize = MAX_COLS * 64;
const SPACE_STRIDE: usize = 32;
const LIVE: usize = SPACES + MAX_SPACES * SPACE_STRIDE;
/// Where columns start in a half, in lines.
const COLS: u32 = ((LIVE + MAX_LIVE * 8) / LINE) as u32;

// The fixed ranges: the plan in chunks of 64 entries, the spaces in chunks of 128, the live words by line.
const R_PLAN: u32 = 0;
const R_SPACES: u32 = R_PLAN + (MAX_COLS / 64) as u32;
const R_LIVE: u32 = R_SPACES + (MAX_SPACES / 128) as u32;
/// The first range that is a column page.
const R_PAGES: u32 = R_LIVE + (MAX_LIVE / 8) as u32;

pub const MAGIC: u64 = u64::from_le_bytes(*b"boughx01");

/// The delta kernel of a column: the damage of a change of the block, over the column's exposed pages.
pub type DeltaFn = unsafe fn(Half, &Entry, &mut Motion, &U, &U, &mut Tiles);
/// The paint kernel of a column for one backend type.
pub type PaintFn = unsafe fn(Half, &Entry, &U, *mut ());

/// One column, as every pass meets it: one cache line. The page a pass wants is reached from here in one
/// dependent load, and the entries of a scene are contiguous in paint order.
#[repr(C, align(64))]
pub struct Entry {
    /// The block words a record of this column reads.
    pub reads: u64,
    /// The block words that move the column's space: when one of them changed, the space as a whole is
    /// damaged and the column is not differentiated.
    pub skip: u64,
    pub delta: DeltaFn,
    pub paint: PaintFn,
    /// The column's pages: lines from the half's base.
    pub pages: u32,
    /// Records the column has room for.
    pub nrec: u32,
    /// The live words of its store: index into the half's live words.
    pub live: u32,
    /// Row slots of its store.
    pub cap: u32,
    pub space: u16,
    pub flags: u16,
    /// The dirty-range id of page 0.
    pub rid: u32,
    pub desc: &'static Desc,
}
const _: () = assert!(size_of::<Entry>() == 64);

/// A column in motion (its records read `Time`): until when, and the box, in the store's space, the moving
/// records stay inside until then. Not part of a half: only the turn thread reads it.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Motion {
    pub until: u32,
    pub sweep: Rect,
}

#[repr(C)]
pub struct Head {
    pub magic: u64,
    /// Bytes reserved for each half.
    pub half_len: u64,
    pub ncols: u32,
    pub nspaces: u32,
    /// Live words handed out.
    pub nlive: u32,
    /// The next free line of a half.
    pub top: u32,
    pub nranges: u32,
    /// Lines abandoned by columns that outgrew their span and moved.
    pub wasted: u32,
    /// Publishes so far, times two; odd while one is copying.
    pub epoch: AtomicU32,
    /// A column was created since the plan was last put in paint order.
    pub unordered: u32,
    /// The block words that move some space (scroll offsets).
    pub scrolls: u64,
    /// Which words of the dirty bitmap have a bit set.
    pub sum: [u64; MAX_RANGES / 4096],
}

/// The arena: owns the mapping.
pub struct Arena {
    base: *mut u8,
    len: usize,
}
// SAFETY: the mapping is plain memory; what may touch which part of it from which thread is the contract of
// `Ar` (one writer of W and the shared part; any reader of F under the epoch).
unsafe impl Send for Arena {}

#[cfg(unix)]
mod sys {
    use core::ffi::c_void;
    unsafe extern "C" {
        pub fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut c_void;
        pub fn munmap(addr: *mut c_void, len: usize) -> i32;
    }
    pub const PROT_RW: i32 = 1 | 2;
    pub const MAP_PRIVATE: i32 = 2;
    #[cfg(target_os = "macos")]
    pub const MAP_ANON_NORESERVE: i32 = 0x1000 | 0x40;
    #[cfg(not(target_os = "macos"))]
    pub const MAP_ANON_NORESERVE: i32 = 0x20 | 0x4000;
}

impl Arena {
    /// Reserve address space for two halves of `half_len` bytes. Memory is committed by the first touch of
    /// each page and is zero.
    pub fn new(half_len: usize) -> Arena {
        let half_len = half_len.next_multiple_of(64 << 10).max(1 << 20);
        let len = HALVES + 2 * half_len;
        #[cfg(unix)]
        // SAFETY: an anonymous private mapping at an address of the kernel's choosing; the result is checked.
        let base = unsafe {
            let p = sys::mmap(core::ptr::null_mut(), len, sys::PROT_RW, sys::MAP_PRIVATE | sys::MAP_ANON_NORESERVE, -1, 0);
            assert!(p as isize != -1, "could not reserve {len} bytes for the scene");
            p as *mut u8
        };
        #[cfg(not(unix))]
        // SAFETY: the layout is non-zero; the result is checked.
        let base = unsafe {
            let l = std::alloc::Layout::from_size_align(len, 64 << 10).unwrap();
            let p = std::alloc::alloc_zeroed(l);
            if p.is_null() {
                std::alloc::handle_alloc_error(l);
            }
            p
        };
        let a = Arena { base, len };
        let ar = a.ar();
        let h = ar.head();
        h.magic = MAGIC;
        h.half_len = half_len as u64;
        h.top = COLS;
        h.nranges = R_PAGES;
        // the fixed ranges
        for c in 0..(MAX_COLS / 64) as u32 {
            ar.set_range(R_PLAN + c, ((PLAN / LINE) as u32 + c * 64, 64));
        }
        for c in 0..(MAX_SPACES / 128) as u32 {
            ar.set_range(R_SPACES + c, ((SPACES / LINE) as u32 + c * 64, 64));
        }
        for l in 0..(MAX_LIVE / 8) as u32 {
            ar.set_range(R_LIVE + l, ((LIVE / LINE) as u32 + l, 1));
        }
        a
    }
    #[inline(always)]
    pub fn ar(&self) -> Ar {
        Ar { base: self.base }
    }
    pub fn len(&self) -> usize {
        self.len
    }
}
impl Drop for Arena {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: exactly the mapping made in `new`.
        unsafe {
            sys::munmap(self.base as *mut _, self.len);
        }
        #[cfg(not(unix))]
        // SAFETY: allocated in `new` with this layout.
        unsafe {
            std::alloc::dealloc(self.base, std::alloc::Layout::from_size_align_unchecked(self.len, 64 << 10));
        }
    }
}

/// A handle to an arena: its base. `Copy`, one word. Every accessor is an offset from it.
///
/// The contract every `unsafe` block below relies on: the arena outlives its handles (a `World` owns both), and
/// only the turn thread calls anything that writes (everything but [`Ar::front`], [`Ar::epoch`] and
/// [`Ar::read`]).
#[derive(Clone, Copy)]
pub struct Ar {
    base: *mut u8,
}
// SAFETY: see the contract above; readers on other threads use only the reading accessors.
unsafe impl Send for Ar {}
unsafe impl Sync for Ar {}

/// One half: the working one or the published one.
#[derive(Clone, Copy)]
pub struct Half {
    p: *mut u8,
}
// SAFETY: as `Ar`.
unsafe impl Send for Half {}
unsafe impl Sync for Half {}

impl Ar {
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub fn head(&self) -> &mut Head {
        // SAFETY: the head is the first bytes of the mapping; only the turn thread forms this reference
        // (readers use `epoch`).
        unsafe { &mut *(self.base.add(HEAD) as *mut Head) }
    }
    /// The working half.
    #[inline(always)]
    pub fn work(&self) -> Half {
        // SAFETY: inside the mapping.
        Half { p: unsafe { self.base.add(HALVES) } }
    }
    /// The published half.
    #[inline(always)]
    pub fn front(&self) -> Half {
        // SAFETY: inside the mapping; `half_len` is written once, before any handle is shared.
        Half { p: unsafe { self.base.add(HALVES + (*(self.base.add(HEAD) as *const Head)).half_len as usize) } }
    }
    #[inline(always)]
    pub fn epoch(&self) -> u32 {
        // SAFETY: the epoch is an atomic in the head.
        unsafe { (*(self.base.add(HEAD) as *const Head)).epoch.load(Ordering::Acquire) }
    }
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub fn motion(&self, col: usize) -> &mut Motion {
        debug_assert!(col < MAX_COLS);
        // SAFETY: `col < MAX_COLS` entries of 20 bytes fit between MOTION and RANGES; turn thread only.
        unsafe { &mut *(self.base.add(MOTION) as *mut Motion).add(col) }
    }
    #[inline(always)]
    fn set_range(&self, id: u32, r: (u32, u32)) {
        assert!((id as usize) < MAX_RANGES, "the scene has more than {MAX_RANGES} column pages");
        // SAFETY: `id < MAX_RANGES` pairs of words fit between RANGES and HALVES.
        unsafe { *(self.base.add(RANGES) as *mut (u32, u32)).add(id as usize) = r }
    }
    #[inline(always)]
    fn range(&self, id: u32) -> (u32, u32) {
        // SAFETY: as `set_range`; ids come from the dirty bitmap, which only `dirty` sets.
        unsafe { *(self.base.add(RANGES) as *const (u32, u32)).add(id as usize) }
    }
    /// Note that range `id` of W was written: the next publish copies it.
    #[inline(always)]
    pub fn dirty(&self, id: u32) {
        debug_assert!(id < self.head().nranges);
        let w = id as usize >> 6;
        // SAFETY: `id < MAX_RANGES` bits fit between DIRTY and MOTION.
        unsafe { *(self.base.add(DIRTY) as *mut u64).add(w) |= 1 << (id & 63) };
        self.head().sum[w >> 6] |= 1 << (w & 63);
    }
    #[inline(always)]
    pub fn dirty_live(&self, word: u32) {
        self.dirty(R_LIVE + word / 8)
    }
    #[inline(always)]
    pub fn dirty_plan(&self, col: u32) {
        self.dirty(R_PLAN + col / 64)
    }
    #[inline(always)]
    pub fn dirty_space(&self, ix: u16) {
        self.dirty(R_SPACES + ix as u32 / 128)
    }

    /// A new column entry (not yet in paint order).
    pub fn new_col(&self) -> u32 {
        let h = self.head();
        assert!((h.ncols as usize) < MAX_COLS, "the scene has more than {MAX_COLS} columns");
        h.ncols += 1;
        h.unordered = 1;
        h.ncols - 1
    }
    pub fn new_space(&self) -> u16 {
        let h = self.head();
        assert!((h.nspaces as usize) < MAX_SPACES, "the scene has more than {MAX_SPACES} spaces");
        h.nspaces += 1;
        (h.nspaces - 1) as u16
    }
    /// `words` live words, zero.
    pub fn new_live(&self, words: u32) -> u32 {
        let h = self.head();
        assert!((h.nlive + words) as usize <= MAX_LIVE, "the scene has more than {} row slots", MAX_LIVE * 64);
        h.nlive += words;
        h.nlive - words
    }
    /// A span of `npages` pages of `page_lines` lines each: (first line, first range id). Zero memory.
    pub fn new_span(&self, npages: u32, page_lines: u32) -> (u32, u32) {
        let h = self.head();
        let lines = npages * page_lines;
        assert!((h.top as u64 + lines as u64) * LINE as u64 <= h.half_len, "the scene outgrew its reservation of {} bytes per half", h.half_len);
        let (at, rid) = (h.top, h.nranges);
        h.top += lines;
        h.nranges += npages;
        for p in 0..npages {
            self.set_range(rid + p, (at + p * page_lines, page_lines));
        }
        (at, rid)
    }

    /// Publish: copy every range the turn thread dirtied from W to F, and return how many lines that was.
    /// Only between frames: the caller knows no frame is reading F.
    ///
    /// The epoch is odd while the copy runs, so a reader that brackets its read of F with the epoch sees a
    /// whole publish or none of it.
    pub fn publish(&self) -> u32 {
        let h = self.head();
        if h.sum.iter().all(|&s| s == 0) {
            return 0;
        }
        h.epoch.fetch_add(1, Ordering::AcqRel);
        let (w, f) = (self.work().p, self.front().p);
        let mut lines = 0;
        for (si, s) in h.sum.iter_mut().enumerate() {
            let mut sm = core::mem::take(s);
            while sm != 0 {
                let wi = si * 64 + sm.trailing_zeros() as usize;
                sm &= sm - 1;
                // SAFETY: `wi` is a word of the dirty bitmap (the summary only names such words).
                let mut m = unsafe { core::mem::take(&mut *(self.base.add(DIRTY) as *mut u64).add(wi)) };
                while m != 0 {
                    let id = (wi * 64) as u32 + m.trailing_zeros();
                    m &= m - 1;
                    let (at, n) = self.range(id);
                    // SAFETY: a registered range lies inside a half (`new_span` checked it against
                    // `half_len`; the fixed ranges are below COLS); W and F do not overlap.
                    unsafe { core::ptr::copy_nonoverlapping(w.add(at as usize * LINE), f.add(at as usize * LINE), n as usize * LINE) };
                    lines += n;
                }
            }
        }
        h.epoch.fetch_add(1, Ordering::Release);
        lines
    }

    /// Read the published half from any thread, without blocking the turn thread and without writing anything
    /// shared: `f` runs on F and its answer stands only if no publish ran meanwhile; otherwise it runs again.
    /// `f` must tolerate torn bytes (copy plain words out; follow no pointer and index nothing unchecked).
    /// Returns `None` before the first publish.
    #[inline]
    pub fn read<R>(&self, mut f: impl FnMut(Half, u32) -> R) -> Option<(R, u32)> {
        let mut tries = 0;
        loop {
            let e = self.epoch();
            if e == 0 {
                return None;
            }
            if e & 1 == 0 {
                let r = f(self.front(), e);
                fence(Ordering::Acquire);
                #[cfg(not(feature = "mutant_torn"))]
                if self.epoch() != e {
                    tries += 1;
                    continue;
                }
                return Some((r, tries));
            }
            tries += 1;
            core::hint::spin_loop();
        }
    }
}

impl Half {
    #[inline(always)]
    pub fn entry(&self, col: usize) -> *mut Entry {
        debug_assert!(col < MAX_COLS);
        // SAFETY: the plan is the first `MAX_COLS` lines of a half.
        unsafe { (self.p.add(PLAN) as *mut Entry).add(col) }
    }
    /// The first `n` entries. SAFETY: `n <= ncols`, and nobody writes them meanwhile (W: the turn thread
    /// itself; F: a frame, or under the epoch).
    #[inline(always)]
    pub unsafe fn plan<'a>(&self, n: usize) -> &'a [Entry] {
        // SAFETY: the caller's contract; entries below `ncols` were written whole by `Store::seal`.
        unsafe { core::slice::from_raw_parts(self.p.add(PLAN) as *const Entry, n) }
    }
    #[inline(always)]
    pub fn space(&self, ix: u16) -> *mut Space {
        debug_assert!((ix as usize) < MAX_SPACES);
        // SAFETY: `ix < MAX_SPACES` slots of 32 bytes; a `Space` is 28 bytes at alignment 4.
        unsafe { self.p.add(SPACES + ix as usize * SPACE_STRIDE) as *mut Space }
    }
    #[inline(always)]
    pub fn live(&self, word: u32) -> *mut u64 {
        debug_assert!((word as usize) < MAX_LIVE);
        // SAFETY: inside the half's live words.
        unsafe { (self.p.add(LIVE) as *mut u64).add(word as usize) }
    }
    /// The byte at `line` lines from the half's base.
    #[inline(always)]
    pub fn at(&self, line: u32) -> *mut u8 {
        // SAFETY: lines handed out by `new_span` are inside the half.
        unsafe { self.p.add(line as usize * LINE) }
    }
    #[inline(always)]
    pub fn base(&self) -> *mut u8 {
        self.p
    }
}
const _: () = assert!(size_of::<Space>() <= SPACE_STRIDE && size_of::<Motion>() == 20 && size_of::<Head>() <= DIRTY);
const _: () = assert!(MOTION + MAX_COLS * 20 <= RANGES && RANGES + MAX_RANGES * 8 <= HALVES && DIRTY + MAX_RANGES / 8 <= MOTION);
