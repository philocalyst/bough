//! The scene's memory: one reserved range holding a working half and a published half of one layout, addressed
//! by offsets only (threads lane, round 8; `docs/r8/ARENA.md`).
//!
//! ```text
//! base   shared   head, the dirty bitmaps, the ranges and columns they name
//!        W        the working half: what the turn thread reads and writes
//!        F        the published half: what a frame and every other reader reads; written only by `publish`
//! half   plan     one 64-byte entry per column, in paint order (the offset form of `store::ColEnt`)
//!        spaces   clip, base and scroll word of every space
//!        live     the live words of every store
//!        columns  pages, packed
//! ```
//!
//! The store tree and the world's column table (`store::ColEnt`) point into W. The two halves have the same
//! layout, so the same thing in F is the same pointer plus the distance between the halves, and nothing *in* a
//! half is an address: a half is position independent, and the published half is the image a file can hold
//! (bbtree-mmap: the structure's own pages, laid end to end, pointers as offsets).
//!
//! **Who writes what.** The turn thread alone writes W and the shared part. Nobody writes F while a frame reads
//! it: `publish` copies what the turn dirtied from W to F between frames. So a frame is a pure function of
//! (the block it was given, F), the turn never waits for a reader, and no reader holds a count or a lock.
//! Readers that are not the frame (accessibility, the inspector, a worker) bracket their read of F with the
//! epoch, as a sequence lock: [`Ar::read`].
use crate::expr::U;
use crate::store::{Desc, Space};
use core::cell::Cell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};
use std::sync::Arc;

pub const LINE: usize = 64;
/// Columns a scene may have (the plan is 64 KiB of each half).
pub const MAX_COLS: usize = 1024;
pub const MAX_SPACES: usize = 512;
/// Live words (64 row slots each).
pub const MAX_LIVE: usize = 2048;
/// Ranges the page bitmap can name: every column page is one.
pub const MAX_RANGES: usize = 16384;
/// Columns the column bitmap can name (a column that moved to a larger span keeps its id), and lists (one
/// note covers every column of a list).
pub const MAX_CID: usize = 2048;
pub const MAX_LISTS: usize = 1024;

const HEAD: usize = 0;
const DIRTY: usize = 4 << 10;
const CMASK: usize = 8 << 10;
const RANGES: usize = 32 << 10;
const CDESC: usize = 160 << 10;
const LDESC: usize = 192 << 10;
/// Where the working half starts.
const HALVES: usize = 256 << 10;
/// An image's half is a whole number of these, so it maps at any VM page size up to 64 KiB.
const IMAGE_ALIGN: usize = 64 << 10;

const PLAN: usize = 0;
const SPACES: usize = MAX_COLS * 64;
const SPACE_STRIDE: usize = 32;
const LIVE: usize = SPACES + MAX_SPACES * SPACE_STRIDE;
/// Where columns start in a half, in lines.
const COLS: u32 = ((LIVE + MAX_LIVE * 8) / LINE) as u32;

// The fixed ranges, computed, not stored: the plan in chunks of 64 entries, the spaces in chunks of 128, the
// live words by line.
const R_PLAN: u32 = 0;
const R_SPACES: u32 = R_PLAN + (MAX_COLS / 64) as u32;
const R_LIVE: u32 = R_SPACES + (MAX_SPACES / 128) as u32;
/// The first range that is a column page.
const R_PAGES: u32 = R_LIVE + (MAX_LIVE / 8) as u32;

pub const MAGIC: u64 = u64::from_le_bytes(*b"boughx02");
/// The layout constants, folded: an image written with other constants is refused.
pub const LAYOUT: u64 = (MAX_COLS as u64) << 48 ^ (MAX_SPACES as u64) << 36 ^ (MAX_LIVE as u64) << 24 ^ (MAX_RANGES as u64) << 4 ^ (HALVES as u64) << 8 ^ size_of::<Head>() as u64 ^ (size_of::<Space>() as u64) << 60 ^ (MAX_CID as u64) << 12 ^ (MAX_LISTS as u64) << 20;

/// The paint kernel of a column for one backend type, over a half.
pub(crate) type PaintFn = unsafe fn(Half, &Entry, &U, *mut ());

/// What code and constants in the binary are named relative to, so that a half holds no address: a kernel is
/// `anchor + offset`. The offsets are the same in every run of one binary, wherever it is loaded.
static ANCHOR: u8 = 0;
#[inline(always)]
fn anchor() -> isize {
    &raw const ANCHOR as isize
}
/// An address in this binary as an offset from the anchor. Text and constants lie within 2 GiB of each other.
#[inline]
pub fn rel(addr: usize) -> i32 {
    i32::try_from(addr as isize - anchor()).expect("the binary's text and constants span more than 2 GiB")
}

/// One column as a frame, a reader or an image meets it: the offset form of the world's `store::ColEnt`, one
/// cache line, in paint order. Written when the store tree changes shape (cold), never by a turn.
///
/// Nothing in it is an address: pages are lines from the half's base, the paint kernel and the descriptor are
/// offsets from the binary's anchor. So a half can be written to a file and mapped by another run of the same
/// binary.
#[repr(C, align(64))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The block words a record of this column reads.
    pub reads: u64,
    /// The block words that move the column's space.
    pub skip: u64,
    paint: i32,
    desc: i32,
    /// The column's pages: lines from the half's base.
    pub pages: u32,
    /// Records the column has room for.
    pub nrec: u32,
    /// The live words of its store: index into the half's live words.
    pub live: u32,
    /// Row slots of its store.
    pub cap: u32,
    pub space: u16,
    /// [`TIMED`]: its records read `Time`.
    pub flags: u16,
    /// The shape of a page, for a reader that walks records without the archetype's code: lines per page,
    /// where the record lane starts in a page, bytes per record.
    pub page_lines: u16,
    pub rec_off: u16,
    pub rec_size: u16,
    _spare: [u16; 7],
}
const _: () = assert!(size_of::<Entry>() == 64);
impl Entry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(desc: &'static Desc, paint: PaintFn, skip: u64, pages: u32, nrec: u32, live: u32, cap: u32, space: u16) -> Entry {
        Entry { reads: desc.reads, skip, paint: rel(paint as usize), desc: rel(desc as *const Desc as usize), pages, nrec, live, cap, space, flags: if desc.timed { TIMED } else { 0 }, page_lines: (desc.page.size() / LINE) as u16, rec_off: desc.rec_off as u16, rec_size: desc.rec_size.min(u16::MAX as usize) as u16, _spare: [0; 7] }
    }
    /// SAFETY: the entry was written by this binary (`Entry::new`), or is an image's entry that `World::adopt`
    /// found equal to one this binary wrote (or whose origin the caller of `World::paint_image` vouches for).
    #[inline(always)]
    pub unsafe fn paint(&self) -> PaintFn {
        // SAFETY: `anchor + paint` is the address `Entry::new` was given.
        unsafe { core::mem::transmute::<isize, PaintFn>(anchor() + self.paint as isize) }
    }
    /// Whether the entry names no descriptor (never written).
    #[inline(always)]
    pub fn unsealed(&self) -> bool {
        self.desc == 0 && self.paint == 0
    }
}

pub const TIMED: u16 = 1;

/// The first bytes of the mapping. Only the turn thread forms a reference to it, and only a shared one: every
/// field it changes is a `Cell`, so two handles to one arena cannot alias a `&mut`. Other threads reach the two
/// atomics (`epoch`, `front_ncols`) through raw pointers and nothing else.
#[repr(C)]
pub(crate) struct Head {
    pub magic: Cell<u64>,
    /// What must be equal for an image to be this binary's: the layout constants of this module.
    pub layout: Cell<u64>,
    /// The binary that wrote it (size and modification time of the executable): a first filter, not a proof.
    pub exe: Cell<[u64; 2]>,
    /// The backend type the plan's paint kernels were instantiated for, hashed.
    pub schema: Cell<u64>,
    /// A checksum of the structural part: the head, the plan, the spaces, the live words, the ranges, the
    /// column and list descriptions. It catches corruption; it does not make the plan's code offsets trusted
    /// (`World::adopt` does, by comparing them with this binary's own).
    pub check: Cell<u64>,
    /// The surface the scene was laid out for.
    pub size: Cell<[i32; 2]>,
    /// The block the published half was shaded with.
    pub shown: Cell<[u32; 64]>,
    /// What follows the half in the file, as (byte offset in the file, bytes): 0 the first frame's pixels,
    /// 1 glyph columns, 2 layout shapes, 3 spare.
    pub sections: Cell<[[u64; 2]; 4]>,
    /// Replaying the allocations of a mapped image (`World::adopt`): requests are answered from these cursors
    /// and checked against what the image holds. `bad`: the view asked for something else than the image has.
    pub adopt: Cell<u32>,
    pub bad: Cell<u32>,
    pub cur: Cell<[u32; 6]>,
    /// Bytes reserved for each half. Written once, before the arena is shared.
    pub half_len: Cell<u64>,
    /// Entries of the plan in the working half.
    pub ncols: Cell<u32>,
    pub nspaces: Cell<u32>,
    /// Live words handed out.
    pub nlive: Cell<u32>,
    /// The next free line of a half.
    pub top: Cell<u32>,
    pub nranges: Cell<u32>,
    /// Column ids handed out.
    pub ncid: Cell<u32>,
    /// List ids handed out.
    pub nlist: Cell<u32>,
    /// Lines abandoned by columns that outgrew their span and moved.
    pub wasted: Cell<u32>,
    /// Publishes so far, times two; odd while one is copying.
    pub epoch: AtomicU32,
    /// Entries of the plan in the published half, as of the last publish.
    pub front_ncols: AtomicU32,
    /// Which words of the page bitmap have a bit set, and which lists of the list bitmap have a mask.
    pub sum: [Cell<u64>; MAX_RANGES / 4096],
    pub csum: [Cell<u64>; MAX_LISTS / 64],
}

/// The mapping. It lives as long as the arena or any reader of it does, so no handle can outlive it.
pub(crate) struct Mapping {
    base: *mut u8,
    len: usize,
}
// SAFETY: plain memory. Who may touch which part from which thread is in the types: the turn thread's `Ar`
// (crate-private, reached only through `&mut World` or `&World`), and `Reader`, which performs atomic loads of
// the published half and of two atomics in the head and nothing else.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

/// The arena: the turn thread's side of the mapping.
pub(crate) struct Arena {
    map: Arc<Mapping>,
}

#[cfg(unix)]
mod sys {
    use core::ffi::c_void;
    unsafe extern "C" {
        pub fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut c_void;
        pub fn munmap(addr: *mut c_void, len: usize) -> i32;
    }
    pub const PROT_RW: i32 = 1 | 2;
    pub const MAP_PRIVATE: i32 = 2;
    pub const MAP_FIXED: i32 = 0x10;
    #[cfg(target_os = "macos")]
    pub const MAP_ANON_NORESERVE: i32 = 0x1000 | 0x40;
    #[cfg(not(target_os = "macos"))]
    pub const MAP_ANON_NORESERVE: i32 = 0x20 | 0x4000;
}

impl Arena {
    /// Reserve address space for two halves of `half_len` bytes. Memory is committed by the first touch of
    /// each page and is zero.
    pub(crate) fn new(half_len: usize) -> Arena {
        let a = Arena::reserve(half_len);
        let ar = a.ar();
        let h = ar.head();
        h.magic.set(MAGIC);
        h.layout.set(LAYOUT);
        h.half_len.set(a.half_len() as u64);
        h.top.set(COLS);
        h.nranges.set(R_PAGES);
        a
    }
    fn reserve(half_len: usize) -> Arena {
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
        Arena { map: Arc::new(Mapping { base, len }) }
    }
    /// The turn thread's handle. Crate-private: nothing outside the crate can hold one, and inside it a handle
    /// lives in a `World` or a `Store` of that world, beside the arena that keeps the mapping alive.
    #[inline(always)]
    pub(crate) fn ar(&self) -> Ar {
        Ar { base: self.map.base }
    }
    /// A reader of the published half for any thread: it keeps the mapping alive itself.
    pub(crate) fn reader(&self) -> Reader {
        Reader { map: self.map.clone() }
    }
}
struct HeadCounts {
    ncols: u32,
    nspaces: u32,
    nlive: u32,
    top: u32,
    nranges: u32,
    ncid: u32,
    nlist: u32,
}

/// Why an image was not taken.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    Io,
    /// Not an image, or one with other layout constants.
    Layout,
    /// Written by another binary, or for another backend type: its kernels are not ours.
    Binary,
    /// A count or an offset points outside the image.
    Bounds,
    /// The structural part is not what was written.
    Checksum,
}

/// Size and modification time of the running executable: what an image's kernels offsets are valid for.
fn exe_id() -> [u64; 2] {
    let Ok(m) = std::env::current_exe().and_then(std::fs::metadata) else { return [0, 0] };
    let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
    [m.len(), t]
}

impl Arena {
    /// Bytes of a half that are in use, rounded to the file's alignment.
    fn used(&self) -> usize {
        (self.ar().head().top.get() as usize * LINE).next_multiple_of(IMAGE_ALIGN)
    }
    /// Write the scene as an image: the shared part, then the working half up to its last used line. The
    /// caller has published (nothing is dirty), so the published half is the same bytes.
    ///
    /// The file is the memory, byte for byte: mapping it needs no fix-up, so every page of it stays shared
    /// with the file until someone writes it.
    pub(crate) fn save(&self, path: &std::path::Path, shown: &U, size: [i32; 2], schema: u64) -> std::io::Result<usize> {
        use std::io::Write;
        let (ar, used) = (self.ar(), self.used());
        let h = ar.head();
        assert!(!ar.pending(), "publish before saving: the image is the published scene");
        h.shown.set(shown.w);
        h.size.set(size);
        h.exe.set(exe_id());
        h.schema.set(schema);
        h.check.set(ar.checksum());
        let tmp = path.with_extension("tmp");
        let mut f = std::fs::File::create(&tmp)?;
        // SAFETY: the shared part and the used prefix of W are inside the mapping and initialised (zero or
        // written); the turn thread is the caller.
        f.write_all(unsafe { core::slice::from_raw_parts(self.map.base, HALVES + used) })?;
        f.sync_data().ok();
        drop(f);
        std::fs::rename(&tmp, path)?;
        Ok(HALVES + used)
    }

    /// Map an image as a scene: the shared part and the working half are the file, copy on write; the
    /// published half is the same file pages again. Nothing is read but the head and the structural part
    /// (to check them), and nothing is written.
    ///
    /// Everything a pass follows without checking is covered by the checksum, and each count and offset is
    /// bounded here, so an image that is accepted cannot lead a pass out of the mapping. Record words are not
    /// checked: a kernel indexes nothing with them.
    #[cfg(unix)]
    pub(crate) fn open(path: &std::path::Path, half_len: usize, schema: u64) -> Result<Arena, Refused> {
        use std::os::fd::AsRawFd;
        let f = std::fs::File::open(path).map_err(|_| Refused::Io)?;
        let flen = f.metadata().map_err(|_| Refused::Io)?.len() as usize;
        if flen < HALVES + COLS as usize * LINE || (flen - HALVES) % IMAGE_ALIGN != 0 {
            return Err(Refused::Layout);
        }
        let a = Arena::reserve(half_len);
        if flen - HALVES > a.half_len() {
            return Err(Refused::Bounds);
        }
        // SAFETY: both targets are inside our own reservation; MAP_FIXED replaces those pages with private
        // (copy-on-write) views of the file. The results are checked.
        unsafe {
            let p = sys::mmap(a.map.base as *mut _, flen, sys::PROT_RW, sys::MAP_PRIVATE | sys::MAP_FIXED, f.as_raw_fd(), 0);
            let q = sys::mmap(a.map.base.add(HALVES + a.half_len()) as *mut _, flen - HALVES, sys::PROT_RW, sys::MAP_PRIVATE | sys::MAP_FIXED, f.as_raw_fd(), HALVES as i64);
            if p as isize == -1 || q as isize == -1 {
                return Err(Refused::Io);
            }
        }
        let ar = a.ar();
        // the head is mapped; it is plain words, and nothing in it is believed before it is checked
        let h = ar.head();
        if h.magic.get() != MAGIC || h.layout.get() != LAYOUT {
            return Err(Refused::Layout);
        }
        if h.half_len.get() as usize != a.half_len() {
            // the reservation is a property of the process, not of the image
            return Err(Refused::Layout);
        }
        if h.exe.get() != exe_id() || h.exe.get() == [0, 0] || h.schema.get() != schema {
            return Err(Refused::Binary);
        }
        let top = (flen - HALVES) / LINE;
        let ok = (h.ncols.get() as usize) <= MAX_COLS
            && (h.nspaces.get() as usize) <= MAX_SPACES
            && (h.nlive.get() as usize) <= MAX_LIVE
            && (h.nranges.get() as usize) <= MAX_RANGES
            && (h.ncid.get() as usize) <= MAX_CID
            && (h.nlist.get() as usize) <= MAX_LISTS
            && h.nranges.get() >= R_PAGES
            && (COLS as usize..=top).contains(&(h.top.get() as usize))
            && !ar.pending()
            && h.adopt.get() == 0;
        if !ok {
            return Err(Refused::Bounds);
        }
        if ar.checksum() != h.check.get() {
            return Err(Refused::Checksum);
        }
        let h = HeadCounts { ncols: h.ncols.get(), nspaces: h.nspaces.get(), nlive: h.nlive.get(), top: h.top.get(), nranges: h.nranges.get(), ncid: h.ncid.get(), nlist: h.nlist.get() };
        // every entry: its pages, live words and space inside what the image holds
        let w = ar.work();
        for i in 0..h.ncols as usize {
            // SAFETY: `i < ncols <= MAX_COLS`; the plan is mapped.
            let e = unsafe { &*w.entry(i) };
            let np = (e.nrec as usize).div_ceil(64);
            let ok = !e.unsealed()
                && (e.cap == 1 || (e.cap >= 64 && e.cap.is_power_of_two()))
                && e.page_lines != 0
                && e.rec_off as usize + 64 * e.rec_size as usize <= e.page_lines as usize * LINE
                && e.pages >= COLS
                && e.pages as usize + np * e.page_lines as usize <= h.top as usize
                && e.live as usize + (e.cap as usize / 64).max(1) <= h.nlive as usize
                && (e.space as u32) < h.nspaces;
            if !ok {
                return Err(Refused::Bounds);
            }
        }
        for id in R_PAGES..h.nranges {
            let (at, n) = ar.range(id);
            if at < COLS || at as usize + n as usize > h.top as usize {
                return Err(Refused::Bounds);
            }
        }
        for c in 0..h.ncid {
            let (rid, k, stride) = ar.cdesc(c);
            if rid < R_PAGES || rid as u64 + k as u64 * stride as u64 > h.nranges as u64 {
                return Err(Refused::Bounds);
            }
        }
        for l in 0..h.nlist {
            let [cid0, ncols, live, nlive] = ar.ldesc(l);
            if cid0 as u64 + ncols as u64 > h.ncid as u64 || live as u64 + nlive as u64 > h.nlive as u64 {
                return Err(Refused::Bounds);
            }
        }
        Ok(a)
    }
    pub(crate) fn half_len(&self) -> usize {
        (self.map.len - HALVES) / 2
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: exactly the mapping made in `reserve`; the last holder (arena or reader) is going.
        unsafe {
            sys::munmap(self.base as *mut _, self.len);
        }
        #[cfg(not(unix))]
        // SAFETY: allocated in `reserve` with this layout.
        unsafe {
            std::alloc::dealloc(self.base, std::alloc::Layout::from_size_align_unchecked(self.len, 64 << 10));
        }
    }
}

/// The turn thread's handle to an arena: its base. `Copy`, one word; every accessor is an offset from it.
///
/// Crate-private. The invariants the `unsafe` blocks below rely on, kept by the crate and not expressible by
/// its users: a handle is stored only in the `World` that owns the arena or in a `Store` of that world (so the
/// mapping outlives it); it is used only through `&World` / `&mut World` (so by one thread at a time; it is
/// neither `Send` nor `Sync`); and the published half is written only by `publish`, which takes `&mut World`.
#[derive(Clone, Copy)]
pub(crate) struct Ar {
    base: *mut u8,
}

/// One half: the working one or the published one. Crate-private, as `Ar`.
#[derive(Clone, Copy)]
pub(crate) struct Half {
    p: *mut u8,
}

impl Ar {
    #[inline(always)]
    pub fn head(&self) -> &Head {
        // SAFETY: the head is the first bytes of the mapping, which outlives the handle; a shared reference,
        // formed by the turn thread only (readers touch its two atomics through raw pointers).
        unsafe { &*(self.base.add(HEAD) as *const Head) }
    }
    /// Whether something was written that the next publish will copy.
    #[inline]
    pub fn pending(&self) -> bool {
        let h = self.head();
        h.sum.iter().any(|s| s.get() != 0) || h.csum.iter().any(|s| s.get() != 0)
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
        Half { p: unsafe { self.base.add(HALVES + self.head().half_len.get() as usize) } }
    }
    #[inline(always)]
    fn set_range(&self, id: u32, r: (u32, u32)) {
        assert!((id as usize) < MAX_RANGES, "the scene has more than {MAX_RANGES} column pages");
        // SAFETY: `id < MAX_RANGES` pairs of words fit between RANGES and CDESC.
        unsafe { *(self.base.add(RANGES) as *mut (u32, u32)).add(id as usize) = r }
    }
    /// Range `id` as (first line, lines) of a half. The fixed ranges are arithmetic; a column page is looked up.
    #[inline(always)]
    fn range(&self, id: u32) -> (u32, u32) {
        if id >= R_PAGES {
            // SAFETY: as `set_range`; the index is masked into the table.
            unsafe { *(self.base.add(RANGES) as *const (u32, u32)).add(id as usize & (MAX_RANGES - 1)) }
        } else if id >= R_LIVE {
            ((LIVE / LINE) as u32 + (id - R_LIVE), 1)
        } else if id >= R_SPACES {
            ((SPACES / LINE) as u32 + (id - R_SPACES) * 64, 64)
        } else {
            ((PLAN / LINE) as u32 + id * 64, 64)
        }
    }
    /// Column `cid` of the column bitmap: (first range id, sites, pages per site).
    #[inline(always)]
    fn cdesc(&self, cid: u32) -> (u32, u32, u32) {
        // SAFETY: `MAX_CID` descriptions of 16 bytes fit between CDESC and HALVES; the index is masked.
        let d = unsafe { (self.base.add(CDESC) as *const [u32; 4]).add(cid as usize & (MAX_CID - 1)).read() };
        (d[0], d[1], d[2])
    }
    /// Note that range `id` of W was written: the next publish copies it.
    #[inline(always)]
    pub fn dirty(&self, id: u32) {
        debug_assert!(id < self.head().nranges.get());
        // the index is masked into the bitmap: an id out of range notes some other range, never memory
        let w = (id as usize & (MAX_RANGES - 1)) >> 6;
        // SAFETY: `w < MAX_RANGES / 64` words fit between DIRTY and CMASK.
        unsafe { *(self.base.add(DIRTY) as *mut u64).add(w) |= 1 << (id & 63) };
        let s = &self.head().sum[w >> 6];
        s.set(s.get() | 1 << (w & 63));
    }
    /// Note that list `lid` wrote rows whole in the slot pages `mask` names (bit `p`: row slots `64p..64p+64`),
    /// or moved its window: the page there of every site of every one of its columns, and its live words. One
    /// note per list per pass, whatever the number of rows, sites and columns; `publish` works out the pages.
    #[inline(always)]
    pub fn dirty_list(&self, lid: u32, mask: u64) {
        debug_assert!(lid < self.head().nlist.get());
        let lid = lid as usize & (MAX_LISTS - 1);
        // SAFETY: `lid < MAX_LISTS` masks fit between CMASK and RANGES.
        unsafe { *(self.base.add(CMASK) as *mut u64).add(lid) |= mask };
        let s = &self.head().csum[lid >> 6];
        s.set(s.get() | 1 << (lid & 63));
    }
    /// List `lid`: (its first column id, its columns, its first live word, its live words).
    #[inline(always)]
    fn ldesc(&self, lid: u32) -> [u32; 4] {
        // SAFETY: `MAX_LISTS` descriptions of 16 bytes fit between LDESC and HALVES; the index is masked.
        unsafe { (self.base.add(LDESC) as *const [u32; 4]).add(lid as usize & (MAX_LISTS - 1)).read() }
    }
    /// An id for a list in the list bitmap.
    pub fn new_list(&self) -> u32 {
        let h = self.head();
        if h.adopt.get() != 0 {
            return self.replay(5, 1, h.nlist.get());
        }
        assert!((h.nlist.get() as usize) < MAX_LISTS, "the scene has more than {MAX_LISTS} lists");
        h.nlist.set(h.nlist.get() + 1);
        h.nlist.get() - 1
    }
    /// Describe list `lid`. While adopting an image nothing is written (`list_is` checks afterwards).
    pub fn set_list(&self, lid: u32, d: [u32; 4]) {
        if self.head().adopt.get() != 0 {
            return;
        }
        // SAFETY: `lid < nlist <= MAX_LISTS`; see `ldesc`.
        unsafe { (self.base.add(LDESC) as *mut [u32; 4]).add(lid as usize & (MAX_LISTS - 1)).write(d) }
    }
    pub fn list_is(&self, lid: u32, d: [u32; 4]) -> bool {
        self.ldesc(lid) == d
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

    pub fn new_space(&self) -> u16 {
        let h = self.head();
        if h.adopt.get() != 0 {
            return self.replay(0, 1, h.nspaces.get()) as u16;
        }
        assert!((h.nspaces.get() as usize) < MAX_SPACES, "the scene has more than {MAX_SPACES} spaces");
        h.nspaces.set(h.nspaces.get() + 1);
        (h.nspaces.get() - 1) as u16
    }
    /// `words` live words, zero.
    pub fn new_live(&self, words: u32) -> u32 {
        let h = self.head();
        if h.adopt.get() != 0 {
            return self.replay(1, words, h.nlive.get());
        }
        assert!((h.nlive.get() + words) as usize <= MAX_LIVE, "the scene has more than {} row slots", MAX_LIVE * 64);
        h.nlive.set(h.nlive.get() + words);
        h.nlive.get() - words
    }
    /// A span of `npages` pages of `page_lines` lines each: (first line, first range id). Zero memory.
    pub fn new_span(&self, npages: u32, page_lines: u32) -> (u32, u32) {
        let h = self.head();
        let lines = npages * page_lines;
        if h.adopt.get() != 0 {
            let (at, rid) = (COLS + self.replay(2, lines, h.top.get() - COLS), R_PAGES + self.replay(3, npages, h.nranges.get() - R_PAGES));
            // the image must hold exactly this span: every page a range of this size, in place
            if h.bad.get() == 0 && (0..npages).any(|p| self.range(rid + p) != (at + p * page_lines, page_lines)) {
                h.bad.set(1);
            }
            return (at, rid);
        }
        assert!((h.top.get() as u64 + lines as u64) * LINE as u64 <= h.half_len.get(), "the scene outgrew its reservation of {} bytes per half", h.half_len.get());
        let (at, rid) = (h.top.get(), h.nranges.get());
        h.top.set(h.top.get() + lines);
        h.nranges.set(h.nranges.get() + npages);
        for p in 0..npages {
            self.set_range(rid + p, (at + p * page_lines, page_lines));
        }
        (at, rid)
    }
    /// An id for a column in the column bitmap.
    pub fn new_cid(&self) -> u32 {
        let h = self.head();
        if h.adopt.get() != 0 {
            return self.replay(4, 1, h.ncid.get());
        }
        assert!((h.ncid.get() as usize) < MAX_CID, "the scene has made more than {MAX_CID} columns");
        h.ncid.set(h.ncid.get() + 1);
        h.ncid.get() - 1
    }
    /// Describe column `cid` for the column bitmap: its first range id, its sites, and the pages per site
    /// (`cap / 64`; 0 for a fixed shape, which notes pages one by one). While adopting an image nothing is
    /// written: the world checks the descriptions when the pass is over (`cid_is`).
    pub fn set_cid(&self, cid: u32, rid: u32, k: u32, stride: u32) {
        if self.head().adopt.get() != 0 {
            return;
        }
        // SAFETY: `cid < ncid <= MAX_CID`; see `cdesc`.
        unsafe { (self.base.add(CDESC) as *mut [u32; 4]).add(cid as usize & (MAX_CID - 1)).write([rid, k, stride, 0]) }
    }
    pub fn cid_is(&self, cid: u32, rid: u32, k: u32, stride: u32) -> bool {
        self.cdesc(cid) == (rid, k, stride)
    }
    /// Answer an allocation from cursor `k` while adopting an image: the same requests in the same order get
    /// the same answers as when the image was made. A request the image has no room for marks the adoption
    /// bad and is answered with 0, which is inside the arena, so whatever follows stays in bounds.
    #[cold]
    fn replay(&self, k: usize, n: u32, have: u32) -> u32 {
        let h = self.head();
        let mut cur = h.cur.get();
        if cur[k] + n > have {
            h.bad.set(1);
            return 0;
        }
        cur[k] += n;
        h.cur.set(cur);
        cur[k] - n
    }
    /// Begin replaying allocations against the image this arena was opened from.
    pub fn adopt_begin(&self) {
        let h = self.head();
        h.adopt.set(1);
        h.bad.set(0);
        h.cur.set([0; 6]);
    }
    /// End it: whether the view asked for exactly what the image holds.
    pub fn adopt_end(&self) -> bool {
        let h = self.head();
        let whole = h.cur.get() == [h.nspaces.get(), h.nlive.get(), h.top.get() - COLS, h.nranges.get() - R_PAGES, h.ncid.get(), h.nlist.get()];
        h.adopt.set(0);
        h.bad.get() == 0 && whole
    }

    /// The checksum of the structural part, with the head as it is.
    pub fn checksum(&self) -> u64 {
        let h = self.head();
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let mut eat = |p: *const u8, bytes: usize| {
            // SAFETY: the callers below pass ranges inside the shared part or a half's fixed regions.
            for k in 0..bytes / 8 {
                x = (x ^ unsafe { (p as *const u64).add(k).read_unaligned() }).wrapping_mul(0x0000_0100_0000_01B3).rotate_left(23);
            }
        };
        // the head without what changes while it is in use
        let c = [h.magic.get() ^ h.wasted.get() as u64, h.layout.get(), h.exe.get()[0], h.exe.get()[1], h.schema.get(), h.half_len.get(), h.ncols.get() as u64, h.nspaces.get() as u64, h.nlive.get() as u64, h.top.get() as u64, h.nranges.get() as u64, h.ncid.get() as u64, h.nlist.get() as u64, h.size.get()[0] as u64, h.size.get()[1] as u64];
        eat(c.as_ptr() as *const u8, size_of_val(&c));
        let (shown, sections) = (h.shown.get(), h.sections.get());
        eat(shown.as_ptr() as *const u8, 256);
        eat(sections.as_ptr() as *const u8, 64);
        let w = self.work();
        eat(w.entry(0) as *const u8, (h.ncols.get() as usize).min(MAX_COLS) * 64);
        eat(w.space(0) as *const u8, (h.nspaces.get() as usize).min(MAX_SPACES) * SPACE_STRIDE);
        eat(w.live(0) as *const u8, (h.nlive.get() as usize).min(MAX_LIVE) * 8);
        // SAFETY: the three tables are in the shared part.
        eat(unsafe { self.base.add(RANGES) }, (h.nranges.get() as usize).min(MAX_RANGES) * 8);
        eat(unsafe { self.base.add(CDESC) }, (h.ncid.get() as usize).min(MAX_CID) * 16);
        eat(unsafe { self.base.add(LDESC) }, (h.nlist.get() as usize).min(MAX_LISTS) * 16);
        x
    }

    /// Publish: copy everything the turn thread dirtied from W to F, and return how many lines that was.
    /// Only between frames: the caller holds `&mut World`, so no frame of this world is reading F.
    ///
    /// Readers on other threads (`Reader`) may be loading F meanwhile. So that this is not a data race, every
    /// word of F is stored atomically here and loaded atomically there; the epoch, odd while the copy runs,
    /// lets a reader throw away what it loaded across a publish.
    pub fn publish(&self) -> u32 {
        let h = self.head();
        if !self.pending() {
            return 0;
        }
        h.epoch.fetch_add(1, Ordering::AcqRel);
        let (w, f) = (self.work().p, self.front().p);
        let mut lines = 0;
        let mut copy = |id: u32| {
            let (at, n) = self.range(id);
            // the mutant publishes the plan and forgets every other column page
            if cfg!(not(feature = "mutant_publish")) || id < R_PAGES || id % 2 == 0 {
                // SAFETY: a range lies inside a half (`new_span` checked it against `half_len`; the fixed
                // ranges are below COLS; an image's are bounded at `open`); W and F do not overlap; lines are
                // 64-byte aligned, so every word is an aligned `u64`. W is read plainly (the turn thread's own);
                // F is written through atomics.
                unsafe {
                    let (src, dst) = (w.add(at as usize * LINE) as *const u64, f.add(at as usize * LINE) as *mut u64);
                    for k in 0..n as usize * (LINE / 8) {
                        AtomicU64::from_ptr(dst.add(k)).store(src.add(k).read(), Ordering::Relaxed);
                    }
                }
            }
            lines += n;
        };
        for (si, s) in h.sum.iter().enumerate() {
            let mut sm = s.replace(0);
            while sm != 0 {
                let wi = si * 64 + sm.trailing_zeros() as usize;
                sm &= sm - 1;
                // SAFETY: `wi` is a word of the page bitmap (the summary only names such words).
                let mut m = unsafe { core::mem::take(&mut *(self.base.add(DIRTY) as *mut u64).add(wi)) };
                while m != 0 {
                    copy((wi * 64) as u32 + m.trailing_zeros());
                    m &= m - 1;
                }
            }
        }
        for (si, s) in h.csum.iter().enumerate() {
            let mut sm = s.replace(0);
            while sm != 0 {
                let lid = (si * 64) as u32 + sm.trailing_zeros();
                sm &= sm - 1;
                // SAFETY: `lid < MAX_LISTS`: a word of the list bitmap.
                let m = unsafe { core::mem::take(&mut *(self.base.add(CMASK) as *mut u64).add(lid as usize)) };
                let [cid0, ncols, live, nlive] = self.ldesc(lid);
                let mut w = live;
                while w < live + nlive {
                    copy(R_LIVE + w / 8);
                    w = (w / 8 + 1) * 8;
                }
                for cid in cid0..cid0 + ncols {
                    let (rid, k, stride) = self.cdesc(cid);
                    for site in 0..k {
                        if stride > 64 {
                            // more slot pages than the mask names: all of them
                            (0..stride).for_each(|p| copy(rid + site * stride + p));
                        } else {
                            let mut b = m;
                            while b != 0 {
                                copy(rid + site * stride + b.trailing_zeros());
                                b &= b - 1;
                            }
                        }
                    }
                }
            }
        }
        h.front_ncols.store(h.ncols.get(), Ordering::Relaxed);
        h.epoch.fetch_add(1, Ordering::Release);
        lines
    }
}

/// A reader of the published half, for any thread: accessibility, an inspector, a worker. It keeps the mapping
/// alive itself, never blocks the turn thread, and writes nothing shared.
///
/// Every load is an atomic load of a word that `publish` stores atomically, so reading while a publish runs is
/// not a data race; what was loaded across a publish is thrown away by the epoch check and the closure runs
/// again. The closure is handed a [`Snapshot`] it cannot keep, and may run more than once: it must only compute
/// an answer from what it loads.
pub struct Reader {
    map: Arc<Mapping>,
}

/// The published half during one attempt of [`Reader::read`]. Every accessor bounds what it is asked for and
/// answers `None` (or zero) outside the half: a snapshot taken across a publish holds arbitrary words.
pub struct Snapshot<'a> {
    f: *const u8,
    half: usize,
    ncols: u32,
    epoch: u32,
    _m: PhantomData<&'a Mapping>,
}

/// One column of a snapshot's plan, as plain numbers.
#[derive(Clone, Copy, Debug)]
pub struct ColumnView {
    /// The block words its records read.
    pub reads: u64,
    /// Row slots of its store (1: a fixed shape) and records it has room for.
    pub cap: u32,
    pub nrec: u32,
    /// Bytes per record.
    pub rec_size: u32,
    pages: u32,
    live: u32,
    page_bytes: u32,
    rec_off: u32,
}

impl Reader {
    fn head(&self) -> *const Head {
        self.map.base as *const Head
    }
    /// Run `f` on the published half; its answer stands only if no publish ran meanwhile, otherwise `f` runs
    /// again. Returns the answer and how many attempts were thrown away, or `None` before the first publish.
    pub fn read<R>(&self, mut f: impl for<'a> FnMut(&Snapshot<'a>) -> R) -> Option<(R, u32)> {
        // SAFETY: the mapping is alive (`map`); `epoch`, `front_ncols` are atomics; `half_len` was written once
        // before the arena could be shared (a `Reader` is made from a finished `World`). No reference to the
        // head is formed.
        let (epoch, ncols, half) = unsafe { (&(*self.head()).epoch, &(*self.head()).front_ncols, (&raw const (*self.head()).half_len).cast::<u64>().read() as usize) };
        let mut tries = 0;
        loop {
            let e = epoch.load(Ordering::Acquire);
            if e == 0 {
                return None;
            }
            if e & 1 == 0 {
                // SAFETY: the published half starts `HALVES + half` bytes into the mapping.
                let snap = Snapshot { f: unsafe { self.map.base.add(HALVES + half) }, half, ncols: ncols.load(Ordering::Relaxed).min(MAX_COLS as u32), epoch: e, _m: PhantomData };
                let r = f(&snap);
                fence(Ordering::Acquire);
                if cfg!(feature = "mutant_torn") || epoch.load(Ordering::Acquire) == e {
                    return Some((r, tries));
                }
            }
            tries += 1;
            core::hint::spin_loop();
        }
    }
}
// SAFETY: a `Reader` performs atomic loads only (see `Snapshot::word`), on a mapping it keeps alive.
unsafe impl Send for Reader {}
unsafe impl Sync for Reader {}

impl Snapshot<'_> {
    /// The 64-bit word at byte `off` of the half, or `None` outside it.
    #[inline(always)]
    fn word(&self, off: usize) -> Option<u64> {
        if off & 7 != 0 || off.checked_add(8)? > self.half {
            return None;
        }
        // SAFETY: in bounds of the published half and aligned; the only writer of these bytes (`publish`)
        // stores them through the same atomic type.
        Some(unsafe { AtomicU64::from_ptr(self.f.add(off) as *mut u64) }.load(Ordering::Relaxed))
    }
    pub fn epoch(&self) -> u32 {
        self.epoch
    }
    /// Columns of the published plan, in paint order.
    pub fn columns(&self) -> u32 {
        self.ncols
    }
    /// Column `i`, if its numbers describe pages inside the half.
    pub fn column(&self, i: u32) -> Option<ColumnView> {
        if i >= self.ncols {
            return None;
        }
        let at = PLAN + i as usize * 64;
        let (w0, w3, w4, w5, w6) = (self.word(at)?, self.word(at + 24)?, self.word(at + 32)?, self.word(at + 40)?, self.word(at + 48)?);
        // the fields of `Entry`, by their offsets (`repr(C)`; asserted below)
        let c = ColumnView { reads: w0, pages: w3 as u32, nrec: (w3 >> 32) as u32, live: w4 as u32, cap: (w4 >> 32) as u32, page_bytes: ((w5 >> 32) as u16) as u32 * LINE as u32, rec_off: (w5 >> 48) as u32, rec_size: w6 as u16 as u32 };
        let np = (c.nrec as usize).div_ceil(64);
        let ok = (c.cap == 1 || (c.cap >= 64 && c.cap.is_power_of_two()))
            && c.page_bytes != 0
            && c.rec_off as usize + 64 * c.rec_size as usize <= c.page_bytes as usize
            && (c.pages as usize * LINE).checked_add(np.checked_mul(c.page_bytes as usize)?)? <= self.half
            && c.live as usize + (c.cap as usize / 64).max(1) <= MAX_LIVE;
        ok.then_some(c)
    }
    /// Whether record `rec` of column `c` is live.
    pub fn live(&self, c: &ColumnView, rec: u32) -> bool {
        if rec >= c.nrec {
            return false;
        }
        if c.cap == 1 {
            return self.word(LIVE + c.live as usize * 8).is_some_and(|w| w & 1 != 0);
        }
        let slot = rec % c.cap;
        self.word(LIVE + (c.live as usize + slot as usize / 64) * 8).is_some_and(|w| w >> (slot & 63) & 1 != 0)
    }
    /// Word `k` (32 bits) of record `rec` of column `c`, or `None` outside the record.
    pub fn record_word(&self, c: &ColumnView, rec: u32, k: u32) -> Option<u32> {
        if rec >= c.nrec || (k + 1) * 4 > c.rec_size {
            return None;
        }
        let off = c.pages as usize * LINE + (rec as usize >> 6) * c.page_bytes as usize + c.rec_off as usize + (rec as usize & 63) * c.rec_size as usize + k as usize * 4;
        let w = self.word(off & !7)?;
        Some((w >> ((off & 7) * 8)) as u32)
    }
    /// A record's rect (x0, y0, x1, y1) in its store's space.
    pub fn rect(&self, c: &ColumnView, rec: u32) -> Option<[i32; 4]> {
        if rec >= c.nrec {
            return None;
        }
        let page = c.pages as usize * LINE + (rec as usize >> 6) * c.page_bytes as usize;
        let mut r = [0i32; 4];
        for (lane, v) in r.iter_mut().enumerate() {
            let off = page + lane * 256 + (rec as usize & 63) * 4;
            *v = (self.word(off & !7)? >> ((off & 7) * 8)) as i32;
        }
        Some(r)
    }
}
const _: () = {
    assert!(core::mem::offset_of!(Entry, pages) == 24 && core::mem::offset_of!(Entry, nrec) == 28 && core::mem::offset_of!(Entry, live) == 32 && core::mem::offset_of!(Entry, cap) == 36);
    assert!(core::mem::offset_of!(Entry, page_lines) == 44 && core::mem::offset_of!(Entry, rec_off) == 46 && core::mem::offset_of!(Entry, rec_size) == 48);
};

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
        // SAFETY: the caller's contract; entries below `ncols` were written whole by the world's `retable`.
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
const _: () = assert!(size_of::<Space>() <= SPACE_STRIDE && size_of::<Head>() <= DIRTY);
const _: () = assert!(LDESC + MAX_LISTS * 16 <= HALVES && CDESC + MAX_CID * 16 <= LDESC && CMASK + MAX_LISTS * 8 <= RANGES && RANGES + MAX_RANGES * 8 <= CDESC && DIRTY + MAX_RANGES / 8 <= CMASK && CMASK + MAX_CID * 8 <= RANGES);
