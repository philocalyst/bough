//! Attempts 1 and 2 of `RESULTS.md`: the GPU-visible glyph pool *is* the shaper's output, and a
//! retained label's new value is applied as a patch, with damage emitted as GPU tile marks.
//!
//! # Representation
//!
//! Structure of arrays, one *slot* per glyph, in blocks of `16 << k` slots (power-of-two size
//! classes, free lists; a run grows in place up to its class, phantom slots draw nothing):
//!
//! | column | type | per | meaning |
//! |---|---|---|---|
//! | `x` | `i32` | slot | pen x in font units, relative to the run origin |
//! | `ent` | `u16` | slot | entry id = the *token* (ASCII byte, Latin-1 codepoint, ligature); 0 draws nothing |
//! | `runs` | [`RunHeader`], 32 B | run | origin, scale, colour, space, strike, window `(base, len, x0)` |
//!
//! The vertex shader computes, for slot `k` of run `r` (`e = tok_entries[r.strike][ent[k]]`):
//! `px = floor(fma(float(x[k] - r.x0), r.scale, r.origin.x + 0.5)) + e.geo.x`, `py = r.origin.y + e.geo.y`
//! (plus the space's translation). That is bit for bit the 48-byte reference instance of
//! `GlyphCache::instances`; [`GlyphPool::expand`] is the same formula on the CPU and is the
//! oracle. 6 bytes per glyph instead of 48, and no glyph-id lookup at all: for ASCII the entry
//! id *is* the byte, so a label's `ent` column is its string, widened.
//!
//! # Delta (Attempt 1)
//!
//! A new value of a retained label is diffed against the old bytes (one NEON compare for
//! labels up to 16 bytes). If every changed byte stays in its *position-equivalence class*
//! (`Shaper::pos_class`; the ten digits are one class in DejaVu Sans), no pen position can
//! change: the update stores the 16 widened bytes as the `ent` lanes (one store pair, changed
//! or not) and nothing else. Otherwise the steps are recomputed from the first changed byte
//! only (`x` of the slot before it is kept), and for equal-length edits only up to the last
//! changed byte when the suffix's pen comes out unchanged.
//!
//! # Damage: tile marks
//!
//! Output is at the GPU's unit of cost (`crates/bough-damage-lab`): bits OR-ed straight into
//! the frame's damage plane, one `u128` per 32x32 tile row (bit `t` = device pixels
//! `32t..32t+32`).
//! * class-preserving patch: the tiles of each changed glyph's old and new box (exact at tile
//!   granularity; O(changed glyphs));
//! * window / whole-run rewrite: O(1), no glyph comparison: the tiles of
//!   `[min(old, new pen at the first change) + left, max(old, new end pen) + over)` over the
//!   line's rows, from the strike's conservative [`LineBounds`].

use crate::atlas::{GlyphCache, Instance, LineBounds, StrikeId, TokStrike, KIND_GLYPH};
use bough_mem::Col;
use crate::shape::{tokenize, Shaper};

/// `(x0, y0, -x1, -y1)` in pixels; union is lane-wise `min`.
pub type NRect = [i32; 4];
/// The union identity (and "no damage").
pub const EMPTY: NRect = [1 << 29; 4];

#[inline(always)]
pub fn union(a: NRect, b: NRect) -> NRect {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2]), a[3].min(b[3])]
}

/// Empty iff `x0 >= x1` or `y0 >= y1`.
#[inline]
pub fn is_empty(r: &NRect) -> bool {
    r[0] + r[2] >= 0 || r[1] + r[3] >= 0
}

/// Tiles covering `[x0, x1)`, clamped to the 128-column plane.
#[inline(always)]
pub fn col_mask(x0: i32, x1: i32) -> u128 {
    let t0 = (x0 >> 5).max(0);
    let t1 = ((x1 - 1) >> 5).min(127);
    if x1 <= x0 || t1 < t0 {
        return 0;
    }
    // Bits t0..=t1; `2 << 127` wraps to 0 and the subtraction still yields the right mask.
    (2u128 << t1).wrapping_sub(1u128 << t0)
}

/// Where tile marks go: one `u128` per 32 px tile row (bit `t` = device pixels `32t..32t+32`).
/// `[u128]` is the plain plane; bough-core's `Plane` (through the façade) also keeps its row band.
pub trait Marks {
    /// Tile rows in the plane.
    fn rows(&self) -> i32;
    /// `plane[r] |= m` for `r` in `r0..=r1` (`r0 >= 0`; `r1` clamped to the plane).
    fn or_rows(&mut self, r0: i32, r1: i32, m: u128);
}

impl Marks for [u128] {
    #[inline(always)]
    fn rows(&self) -> i32 {
        self.len() as i32
    }
    /// Straight-line for the usual one or two rows (a text line under 32 px spans at most two).
    #[inline(always)]
    fn or_rows(&mut self, r0: i32, r1: i32, m: u128) {
        let r1 = r1.min(self.len() as i32 - 1);
        if r1 < r0 || r0 < 0 {
            return;
        }
        // SAFETY: 0 <= r0 <= r1 < self.len().
        unsafe {
            let p = self.as_mut_ptr();
            *p.add(r0 as usize) |= m;
            if r1 > r0 {
                *p.add(r0 as usize + 1) |= m;
                if r1 > r0 + 1 {
                    more_rows(p, r0 + 2, r1, m);
                }
            }
        }
    }
}

impl Marks for Vec<u128> {
    #[inline(always)]
    fn rows(&self) -> i32 {
        self.as_slice().rows()
    }
    #[inline(always)]
    fn or_rows(&mut self, r0: i32, r1: i32, m: u128) {
        self.as_mut_slice().or_rows(r0, r1, m)
    }
}
impl<const N: usize> Marks for [u128; N] {
    #[inline(always)]
    fn rows(&self) -> i32 {
        N as i32
    }
    #[inline(always)]
    fn or_rows(&mut self, r0: i32, r1: i32, m: u128) {
        self.as_mut_slice().or_rows(r0, r1, m)
    }
}

/// OR the tiles of the half-open pixel box `[x0, x1) x [y0, y1)` into a damage plane.
/// Straight-line for the usual one or two rows.
#[inline(always)]
pub fn mark<P: Marks + ?Sized>(plane: &mut P, x0: i32, y0: i32, x1: i32, y1: i32) {
    let r0 = (y0 >> 5).max(0);
    let r1 = ((y1 - 1) >> 5).min(plane.rows() - 1);
    let m = col_mask(x0, x1);
    if m == 0 || y1 <= y0 {
        return;
    }
    plane.or_rows_px(r0, r1, m, [x0, y0, x1, y1]);
}

/// `plane[r] |= m` for `r` in `r0..=r1` (clamped to the plane; `r0 >= 0`).
#[inline(always)]
pub fn or_rows<P: Marks + ?Sized>(plane: &mut P, r0: i32, r1: i32, m: u128) {
    plane.or_rows(r0, r1, m);
}

#[cold]
#[inline(never)]
unsafe fn more_rows(p: *mut u128, r0: i32, r1: i32, m: u128) {
    for r in r0..=r1 {
        // SAFETY: the caller clamped r1 to the plane.
        unsafe { *p.add(r as usize) |= m };
    }
}

#[inline(always)]
pub fn mark_rect<P: Marks + ?Sized>(plane: &mut P, r: &NRect) {
    mark(plane, r[0], r[1], -r[2], -r[3]);
}

/// GPU-visible per-run header, 32 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunHeader {
    /// Pen origin (baseline) in pixels relative to the space. `origin[1]` must be integral.
    pub origin: [f32; 2],
    /// Pixels per font unit (`size / upem`).
    pub scale: f32,
    pub color: u32,
    pub space: u16,
    pub strike: u16,
    /// First slot and slot count of the run: a *window* onto shaped slots.
    pub base: u32,
    pub len: u32,
    /// Pen subtracted from every slot's `x` (exact integer subtraction, before the fma): a
    /// window that starts mid-text (a wrapped line, a stream line) is drawn from its own
    /// origin without rewriting any slot.
    pub x0: i32,
}
const _: () = assert!(core::mem::size_of::<RunHeader>() == 32);

const IDENTITY: u32 = 1;

/// A retained label: CPU-side state, owned by the caller (keep them contiguous). The first
/// 64-byte line holds everything the equal-length patch reads; the run header in the pool
/// stays the GPU's copy.
#[repr(C, align(64))]
pub struct Label {
    /// First 16 bytes of the text, zero padded (the whole text for labels up to 16 bytes).
    head: [u8; 16],
    base: u32,
    nbytes: u32,
    /// Advance width in font units.
    width: i32,
    /// `IDENTITY`: slot `i` is byte `i` (ASCII through the dense table). Otherwise the slots
    /// are the token path's glyphs (ligatures, Latin-1).
    flags: u32,
    scale: f32,
    /// Origin x + 0.5 (space-relative), for `pen_px`.
    oxh: f32,
    /// Origin y (space-relative, integral).
    oy: i32,
    space: u32,
    pub run: u32,
    strike: u32,
    /// Slot capacity of the label's block.
    cap: u32,
    pub bounds: LineBounds,
    /// Snapped pen x (pixels, space-relative) of the first 16 slots: the digit patch's damage
    /// is `[min pen + cell.left, max pen + cell.right)` over the changed lanes. Maintained by
    /// every path that moves pens (only those).
    pen: [i16; 16],
    /// Union of the ten digit boxes, `[left, top, right, bottom]` relative to pen/baseline.
    dcell: [i32; 4],
    /// Derived from `pen`, `dcell` and the space translation `tspace`: the tile columns
    /// `[t0[k], t1[k]]` a digit at slot `k` can touch (`0xFF, 0` = off the plane) and the tile
    /// rows `trow..=trow_end` of the digit cell. Recomputed when pens move or the space moved.
    t0: [u8; 16],
    t1: [u8; 16],
    trow: i32,
    trow_end: i32,
    tspace: [i32; 2],
    /// The full text (`cap + 16` bytes, zero padded) for labels longer than 16 bytes and the
    /// cold paths.
    text: Box<[u8]>,
}

impl Label {
    pub fn text(&self) -> &[u8] {
        &self.text[..self.nbytes as usize]
    }
    pub fn is_identity(&self) -> bool {
        self.flags & IDENTITY != 0
    }
    pub fn width(&self) -> i32 {
        self.width
    }
}

/// The GPU-visible columns of a [`GlyphPool`] (carved from the shared region by the façade, so the
/// GPU reads the slots, run headers and space translations where the shaper writes them).
pub struct PoolColumns {
    pub x: Col<i32>,
    pub ent: Col<u16>,
    pub runs: Col<RunHeader>,
    pub spaces: Col<[i32; 2]>,
}

#[derive(Default)]
pub struct GlyphPool {
    pub x: Col<i32>,
    pub ent: Col<u16>,
    pub runs: Col<RunHeader>,
    /// Integral device-pixel translation of each space (the GPU adds it to runs and instances).
    pub spaces: Col<[i32; 2]>,
    /// Run headers released by `free_label` / `free_run`, reused before the column grows.
    free_runs: Vec<u32>,
    /// Released contiguous run-header ranges by size class (`1 << k` headers): a multi-line text's
    /// lines are consecutive headers (one record draws the range).
    free_ranges: [Vec<u32>; 12],
    /// How many updates took each path: equal-length class patch, window, window stopped at
    /// an unchanged suffix, full replacement.
    pub stats: [u64; 4],
    free: [Vec<u32>; 24],
    // Scratch for the cold paths (full replacement, token path).
    sx: Vec<i32>,
    se: Vec<u16>,
    sc: Vec<u32>,
}

/// The snapped pixel pen of font-unit pen `x` (the shader's `floor(fma(float(x), scale, ox + 0.5))`).
#[inline(always)]
pub fn pen_px(x: i32, scale: f32, oxh: f32) -> i32 {
    (x as f32).mul_add(scale, oxh).floor() as i32
}

#[inline(always)]
fn gbox(ibox: &[i32; 4], px: i32, oy: i32) -> NRect {
    [ibox[0] + px, ibox[1] + oy, ibox[2] - px, ibox[3] - oy]
}

fn class_of(n: usize) -> usize {
    (n.max(16).next_power_of_two().trailing_zeros() - 4) as usize
}

enum Outcome {
    Done,
    /// A token of the new text is not rasterized in the strike yet (cold path).
    Miss,
    /// Codepoint outside the covered set: the caller must use the full shaper.
    Refused,
}

/// 16 bytes at `p`, lanes `>= n` zeroed, `n <= 16`. Reads 16 bytes when that cannot cross a
/// page (the common case: one load), else the bytes one by one.
///
/// # Safety
/// `p` must be valid for `n` bytes of reads.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn load16(p: *const u8, n: usize) -> core::arch::aarch64::uint8x16_t {
    use core::arch::aarch64::*;
    const IOTA: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    unsafe {
        let keep = vcltq_u8(vld1q_u8(IOTA.as_ptr()), vdupq_n_u8(n as u8));
        if (p as usize & 4095) <= 4096 - 16 {
            // The 16 bytes lie in one page that holds at least one valid byte, so the load
            // cannot fault; the bytes past `n` are masked off. Done in asm because reading
            // past the end of an allocation is not expressible in Rust.
            let v: uint8x16_t;
            core::arch::asm!("ldr {v:q}, [{p}]", p = in(reg) p, v = lateout(vreg) v, options(pure, readonly, nostack, preserves_flags));
            vandq_u8(v, keep)
        } else {
            let mut buf = [0u8; 16];
            core::ptr::copy_nonoverlapping(p, buf.as_mut_ptr(), n);
            vld1q_u8(buf.as_ptr())
        }
    }
}

impl GlyphPool {
    /// The pool's columns and free lists as memory-ledger lines.
    pub fn ledger(&self, l: &mut bough_mem::Ledger) {
        use bough_mem::Scales::Content;
        const G: &str = "text";
        l.col(G, &self.x, Content);
        l.col(G, &self.ent, Content);
        l.col(G, &self.runs, Content);
        l.col(G, &self.spaces, Content); // addressable whole (the runtime sets every space at start)
        let fr: usize = self.free_runs.capacity() + self.free_ranges.iter().map(|v| v.capacity()).sum::<usize>() + self.free.iter().map(|v| v.capacity()).sum::<usize>();
        l.many(G, "glyph pool free lists", Content, fr * 4, fr * 4);
        let sc = self.sx.capacity() * 4 + self.se.capacity() * 2 + self.sc.capacity() * 4;
        l.many(G, "glyph pool scratch", Content, sc, sc);
    }
    pub fn new() -> Self {
        let mut p = GlyphPool::default();
        p.spaces.push([0, 0]);
        p
    }

    /// A pool on carved (GPU-visible) columns.
    pub fn with_columns(c: PoolColumns) -> Self {
        let mut p = GlyphPool { x: c.x, ent: c.ent, runs: c.runs, spaces: c.spaces, ..Self::default() };
        p.spaces.clear();
        p.spaces.push([0, 0]);
        p
    }

    /// A run header (reusing a released one).
    pub(crate) fn new_run(&mut self, h: RunHeader) -> u32 {
        match self.free_runs.pop() {
            Some(r) => {
                self.runs[r as usize] = h;
                r
            }
            None => {
                self.runs.push(h);
                (self.runs.len() - 1) as u32
            }
        }
    }

    /// `n` consecutive run headers (rounded up to a power of two), all empty (`len` 0). Returns the first.
    pub fn alloc_run_range(&mut self, n: usize) -> (u32, u32) {
        let cap = n.max(1).next_power_of_two();
        let k = cap.trailing_zeros() as usize;
        let first = match self.free_ranges[k].pop() {
            Some(f) => f,
            None => {
                let f = self.runs.len() as u32;
                self.runs.resize(f as usize + cap, RunHeader::default());
                f
            }
        };
        for r in &mut self.runs[first as usize..first as usize + cap] {
            r.len = 0;
        }
        (first, cap as u32)
    }

    /// Release a range from [`alloc_run_range`](Self::alloc_run_range).
    pub fn free_run_range(&mut self, first: u32, cap: u32) {
        for r in &mut self.runs[first as usize..(first + cap) as usize] {
            r.len = 0;
        }
        self.free_ranges[cap.trailing_zeros() as usize].push(first);
    }

    /// Release a run header (its id may be returned by the next allocation). Draws nothing.
    pub fn free_run(&mut self, run: u32) {
        self.runs[run as usize].len = 0;
        self.free_runs.push(run);
    }

    pub fn set_space(&mut self, space: u16, t: [i32; 2]) {
        if self.spaces.len() <= space as usize {
            self.spaces.resize(space as usize + 1, [0, 0]);
        }
        self.spaces[space as usize] = t;
    }

    /// A block of `cap` slots (rounded up to a power of two >= 16). Slots past a run's `len`
    /// are never drawn, so blocks are neither cleared on allocation nor on release.
    pub(crate) fn alloc_block(&mut self, cap: usize, _run: u32) -> u32 {
        let c = class_of(cap);
        match self.free[c].pop() {
            Some(b) => b,
            None => {
                let b = self.x.len() as u32;
                let size = 16usize << c;
                // SAFETY: zero is a valid i32 / u16.
                unsafe {
                    self.x.grow_zeroed(self.x.len() + size);
                    self.ent.grow_zeroed(self.ent.len() + size);
                }
                b
            }
        }
    }

    pub(crate) fn free_block(&mut self, base: u32, cap: u32) {
        self.free[class_of(cap as usize)].push(base);
    }

    /// A new, empty label. `origin` is the pen origin in pixels relative to `space`
    /// (`origin[1]` integral); `font_size` in pixels. The strike is made ASCII-complete here
    /// (every printable ASCII glyph rasterized once), so no ASCII path checks readiness.
    #[allow(clippy::too_many_arguments)]
    pub fn alloc_label(
        &mut self,
        sh: &Shaper,
        strike: StrikeId,
        font_size: f32,
        origin: [f32; 2],
        color: u32,
        space: u16,
        cache: &mut GlyphCache,
    ) -> Label {
        let tt = sh.script.tokens.as_ref().expect("token model");
        let bounds = cache.line_bounds(strike, tt, sh.script.upem);
        assert!(bounds.bottom - bounds.top <= 3 * 32 + 1, "line taller than 4 tile rows");
        assert_eq!(origin[1], origin[1].floor(), "baseline must be integral");
        let dcell = cache.label_strike(strike, &tt.gid);
        if self.spaces.len() <= space as usize {
            self.spaces.resize(space as usize + 1, [0, 0]);
        }
        let base = self.alloc_block(16, u32::MAX);
        let scale = font_size / sh.script.upem as f32;
        let run = self.new_run(RunHeader { origin, scale, color, space, strike: strike as u16, base, len: 0, x0: 0 });
        Label {
            head: [0; 16],
            base,
            nbytes: 0,
            width: 0,
            flags: IDENTITY,
            scale,
            oxh: origin[0] + 0.5,
            oy: origin[1] as i32,
            space: space as u32,
            run,
            strike: strike as u32,
            cap: 16,
            bounds,
            pen: [0; 16],
            dcell,
            t0: [0xFF; 16],
            t1: [0; 16],
            trow: 0,
            trow_end: -1,
            tspace: [i32::MIN, i32::MIN],
            text: vec![0u8; 32].into_boxed_slice(),
        }
    }

    /// Move a label's pen origin to `origin` (pixels relative to its space; `origin[1]` integral): the run
    /// header, the label's snapped pens (they are `pen_px` of the slots at the origin) and its cached tile
    /// spans (stale: recomputed by the next digit patch). No slot is rewritten and nothing is marked: the
    /// caller damages the line's old and new extents (it holds both inks). What a layout write-back does
    /// to a one-line text whose box moved.
    pub fn move_label(&mut self, lb: &mut Label, origin: [f32; 2]) {
        debug_assert_eq!(origin[1], origin[1].floor(), "baseline must be integral");
        let h = &mut self.runs[lb.run as usize];
        h.origin = origin;
        let (base, len) = (h.base as usize, h.len as usize);
        lb.oxh = origin[0] + 0.5;
        lb.oy = origin[1] as i32;
        // pens are maintained only while the text fits 16 slots (`window`, `replace`)
        if len <= 16 {
            for k in 0..len {
                lb.pen[k] = pen_px(self.x[base + k], lb.scale, lb.oxh) as i16;
            }
        }
        lb.tspace = [i32::MIN, i32::MIN];
    }

    /// The label's width on one line with trailing spaces excluded (font units): the pen at its first
    /// trailing space, or its end pen (as `Para::line_width`; a label whose slots are not its bytes, a
    /// ligature or Latin-1 text, answers its whole width).
    pub fn label_line_width(&self, lb: &Label) -> i32 {
        let t = lb.text();
        let mut e = t.len();
        while e > 0 && t[e - 1] == b' ' {
            e -= 1;
        }
        if e == t.len() || !lb.is_identity() {
            lb.width
        } else if e == 0 {
            0
        } else {
            self.x[self.runs[lb.run as usize].base as usize + e]
        }
    }

    /// Release a label's block, marking the whole line (O(1)).
    pub fn free_label<P: Marks + ?Sized>(&mut self, lb: Label, plane: &mut P) {
        let h = self.runs[lb.run as usize];
        if h.len > 0 {
            self.mark_line(&lb, 0, lb.width, plane);
        }
        self.free_block(h.base, lb.cap);
        self.free_run(lb.run);
    }

    /// Set a retained label's text: writes only what changed into the pool and ORs the damage
    /// into `plane` (one `u128` per 32 px tile row). `false` if the text needs the full shaper
    /// (a codepoint outside the covered set; the label is unchanged). Rasterizes glyphs the
    /// strike has not seen (cold path).
    #[inline]
    pub fn set_text<P: Marks + ?Sized>(&mut self, sh: &Shaper, cache: &mut GlyphCache, lb: &mut Label, new: &str, plane: &mut P) -> bool {
        self.set_text_d::<true, P>(sh, cache, lb, new, plane)
    }

    /// `set_text` with the damage output switchable at compile time (`DAMAGE = false` writes
    /// no marks), to measure the instance rewrite and the damage separately.
    #[inline(always)]
    pub fn set_text_d<const DAMAGE: bool, P: Marks + ?Sized>(
        &mut self,
        sh: &Shaper,
        cache: &mut GlyphCache,
        lb: &mut Label,
        new: &str,
        plane: &mut P,
    ) -> bool {
        let s = new.as_bytes();
        let n = s.len();
        #[cfg(target_arch = "aarch64")]
        if lb.flags == IDENTITY && n == lb.nbytes as usize && n.wrapping_sub(1) < 16 {
            // SAFETY: n <= 16 bytes of `s`; the strike id was validated at allocation.
            if unsafe { self.patch16::<DAMAGE, P>(sh, cache.tok_unchecked(lb.strike as StrikeId), lb, s, plane) } {
                return true;
            }
        }
        self.slow::<DAMAGE, P>(sh, cache, lb, new, plane)
    }

    /// Equal-length update of an identity label of at most 16 bytes. `false` = not applicable
    /// (non-ASCII, or a changed byte left its position class); nothing was written then.
    #[cfg(target_arch = "aarch64")]
    #[inline(always)]
    unsafe fn patch16<const D: bool, P: Marks + ?Sized>(&mut self, sh: &Shaper, ts: &TokStrike, lb: &mut Label, s: &[u8], plane: &mut P) -> bool {
        use core::arch::aarch64::*;
        unsafe {
            let n = s.len();
            let vn = load16(s.as_ptr(), n);
            let vo = vld1q_u8(lb.head.as_ptr());
            let eq = vceqq_u8(vn, vo);
            // 4 bits per lane (the shrn trick): bit 4i+3 set iff lane i differs.
            let m64 = vget_lane_u64::<0>(vreinterpret_u64_u8(vshrn_n_u16::<4>(vreinterpretq_u16_u8(eq))));
            let m = !m64 & 0x8888_8888_8888_8888;
            if m == 0 {
                return true;
            }
            // Position-class test, vector form for the common case: every lane unchanged or
            // digit -> digit (the digits are one class). Exact per-lane fallback otherwise.
            let d0 = vdupq_n_u8(b'0');
            let ten = vdupq_n_u8(10);
            let digits = vandq_u8(vcltq_u8(vsubq_u8(vn, d0), ten), vcltq_u8(vsubq_u8(vo, d0), ten));
            let all_digits = vminvq_u8(vorrq_u8(eq, digits)) == 0xFF;
            if !all_digits {
                if vmaxvq_u8(vn) >= 0x80 {
                    return false;
                }
                let cls = sh.pos_class.as_ptr();
                let mut k = m;
                while k != 0 {
                    let i = (k.trailing_zeros() >> 2) as usize;
                    k &= k - 1;
                    if *cls.add(lb.head[i] as usize) != *cls.add(s[i] as usize) {
                        return false;
                    }
                }
            }
            if D {
                let sp = *self.spaces.get_unchecked(lb.space as usize);
                if all_digits {
                    // Digits do not move and share one cell box, so the tiles each slot can
                    // touch are cached per label: the damage is the span from the smallest
                    // changed t0 to the largest changed t1, on the cell's rows.
                    if sp != lb.tspace {
                        retile(lb, sp);
                    }
                    let ne = vmvnq_u8(eq);
                    let t0 = vminvq_u8(vorrq_u8(vandq_u8(ne, vld1q_u8(lb.t0.as_ptr())), eq)) as u32;
                    let t1 = vmaxvq_u8(vandq_u8(ne, vld1q_u8(lb.t1.as_ptr()))) as u32;
                    if t1 >= t0 {
                        let mm = (2u128 << t1).wrapping_sub(1u128 << t0);
                        or_rows(plane, lb.trow, lb.trow_end, mm);
                    }
                } else {
                    // Exact per glyph (another class, e.g. '(' -> '['): rare.
                    let oy = lb.oy + sp[1];
                    let mut k = m;
                    while k != 0 {
                        let i = (k.trailing_zeros() >> 2) as usize;
                        k &= k - 1;
                        let bo = ts.ibox.get_unchecked(*lb.head.get_unchecked(i) as usize);
                        let bn = ts.ibox.get_unchecked(*s.get_unchecked(i) as usize);
                        let px = *lb.pen.get_unchecked(i) as i32 + sp[0];
                        mark_rect(plane, &union(gbox(bo, px, oy), gbox(bn, px, oy)));
                    }
                }
            }
            // The instance rewrite: all 16 entry lanes (unchanged lanes rewrite their own
            // value, lanes past the end stay 0).
            let es = self.ent.as_mut_ptr().add(lb.base as usize);
            vst1q_u16(es, vmovl_u8(vget_low_u8(vn)));
            vst1q_u16(es.add(8), vmovl_high_u8(vn));
            vst1q_u8(lb.head.as_mut_ptr(), vn);
            vst1q_u8(lb.text.as_mut_ptr(), vn);
            self.stats[0] += 1;
            true
        }
    }

    #[cold]
    #[inline(never)]
    fn slow<const D: bool, P: Marks + ?Sized>(&mut self, sh: &Shaper, cache: &mut GlyphCache, lb: &mut Label, new: &str, plane: &mut P) -> bool {
        let strike = lb.strike as StrikeId;
        loop {
            let ts = cache.tok(strike);
            let s = new.as_bytes();
            let n = s.len();
            let n_old = lb.nbytes as usize;
            let cap = lb.cap as usize;
            let o = if lb.flags == IDENTITY && n <= cap && s.is_ascii() {
                let p = first_diff(&lb.text[..n_old], s);
                if p == n && n == n_old {
                    return true;
                }
                match self.window::<D, P>(sh, lb, s, p, plane) {
                    Some(o) => o,
                    None => self.replace::<D, P>(sh, ts, lb, new, plane),
                }
            } else if lb.flags != IDENTITY && lb.text() == s {
                Outcome::Done
            } else {
                self.replace::<D, P>(sh, ts, lb, new, plane)
            };
            match o {
                Outcome::Done => return true,
                Outcome::Refused => return false,
                Outcome::Miss => self.prepare(sh, cache, strike, new),
            }
        }
    }

    #[cold]
    #[inline(never)]
    fn prepare(&mut self, sh: &Shaper, cache: &mut GlyphCache, strike: StrikeId, new: &str) {
        let tt = sh.script.tokens.as_ref().expect("token model");
        let n = new.len();
        self.se.clear();
        self.se.resize(n, 0);
        self.sc.clear();
        self.sc.resize(n, 0);
        // SAFETY: both scratch buffers hold `n` elements; `new` is UTF-8.
        let j = unsafe { tokenize(tt, new.as_bytes(), self.se.as_mut_ptr(), self.sc.as_mut_ptr()) }.unwrap_or(0);
        let toks: Vec<u16> = self.se[..j].to_vec();
        cache.prepare_tokens(strike, &tt.gid, toks.into_iter());
    }

    /// Mark the line's tiles over pens `[pen0, pen1]` (font units, run-relative), O(1).
    #[inline]
    fn mark_line<P: Marks + ?Sized>(&self, lb: &Label, pen0: i32, pen1: i32, plane: &mut P) {
        let sp = self.spaces[lb.space as usize];
        let oy = lb.oy + sp[1];
        let b = lb.bounds;
        mark(
            plane,
            pen_px(pen0, lb.scale, lb.oxh) + sp[0] + b.left,
            oy + b.top,
            pen_px(pen1, lb.scale, lb.oxh) + sp[0] + b.over,
            oy + b.bottom,
        );
    }

    /// Identity label, ASCII text that fits the block: recompute steps from the first changed
    /// byte `p` (the slot before it keeps its `x`; only its step is re-read). A short changed
    /// region of an equal-length text runs scalar and stops at the last changed byte if the
    /// pen there comes out unchanged (the suffix keeps its slots); anything longer runs the
    /// NEON slot kernel to the end. Damage is O(1). `None` if the new text has a trigger (the
    /// pool is restored first).
    fn window<const D: bool, P: Marks + ?Sized>(&mut self, sh: &Shaper, lb: &mut Label, s: &[u8], p: usize, plane: &mut P) -> Option<Outcome> {
        let n = s.len();
        let n_old = lb.nbytes as usize;
        // Equal lengths: bytes from `stop` on are unchanged.
        let stop = if n == n_old { n - common_suffix(&lb.text[p..n], &s[p..]) } else { n };
        let dense = sh.dense();
        let mut acc = 0u32;
        let mut stopped = false;
        let cap = lb.cap as usize;
        // SAFETY: slot indices are < cap (n <= cap; the NEON kernel's 16-lane tail is checked
        // against cap below); bytes are ASCII so dense indices are < 128 * 128.
        let (x_p_old, x_p_new, x_end) = unsafe {
            let xs = self.x.as_mut_ptr().add(lb.base as usize);
            let es = self.ent.as_mut_ptr().add(lb.base as usize);
            let x_p_old = if p < n_old { *xs.add(p) } else { lb.width };
            let mut x = if p == 0 {
                0
            } else {
                let q = p - 1;
                let c = if p < n { s[p] as usize } else { 0 };
                let st = *dense.get_unchecked((s[q] as usize) << 7 | c) as u32;
                acc |= st;
                *xs.add(q) + st as i32
            };
            let x_p_new = x;
            let mut i = p;
            if stop - p <= 16 {
                while i < n {
                    if i == stop && x == *xs.add(i) {
                        stopped = true; // the unchanged suffix keeps its pens
                        break;
                    }
                    if i >= stop + 1 {
                        break; // pens moved: the rest goes to the bulk kernel
                    }
                    let b = *s.get_unchecked(i) as usize;
                    let c = if i + 1 < n { *s.get_unchecked(i + 1) as usize } else { 0 };
                    let st = *dense.get_unchecked(b << 7 | c) as u32;
                    acc |= st;
                    *xs.add(i) = x;
                    *es.add(i) = b as u16;
                    x += st as i32;
                    i += 1;
                }
            }
            if !stopped && i < n {
                let rest = &s[i..];
                let (w, a) = if i + rest.len().next_multiple_of(16) <= cap {
                    crate::window::slots(sh, rest, 0, x, xs.add(i), es.add(i))
                } else {
                    crate::shape::scalar_slots(dense, rest, 0, x, xs.add(i), es.add(i))
                };
                acc |= a;
                x = w;
            }
            if acc & 0x8000 != 0 {
                self.restore_identity(sh, lb);
                return None;
            }
            if n <= 16 {
                // Pens are maintained only while the text fits 16 slots; coming back from a
                // longer text they are all stale.
                let from = if n_old > 16 { 0 } else { p };
                for k in from..n {
                    lb.pen[k] = pen_px(*xs.add(k), lb.scale, lb.oxh) as i16;
                }
                lb.tspace = [i32::MIN, i32::MIN]; // pens moved: tiles are stale
            }
            (x_p_old, x_p_new, x)
        };
        // Ran to the end: the last pen is the new width. Stopped: the width is unchanged.
        let new_width = if stopped { lb.width } else { x_end };
        if D && !(p >= n && p >= n_old) {
            // O(1): everything between the first change and the end (or the unchanged suffix).
            let right = if stopped { x_end } else { lb.width.max(new_width) };
            self.mark_line(lb, x_p_old.min(x_p_new), right, plane);
        }
        lb.width = new_width;
        store_text(lb, s, n_old);
        lb.nbytes = n as u32;
        // SAFETY: `lb.run` was created by this pool.
        unsafe { self.runs.get_unchecked_mut(lb.run as usize).len = n as u32 };
        self.stats[1 + stopped as usize] += 1;
        Some(Outcome::Done)
    }

    /// Undo a partial `window` write: the label's old text was a valid identity run, so
    /// re-shaping it restores `x` exactly and `ent` is its bytes.
    #[cold]
    fn restore_identity(&mut self, sh: &Shaper, lb: &Label) {
        let h = self.runs[lb.run as usize];
        let n_old = lb.nbytes as usize;
        let t = &lb.text[..n_old];
        let (w, acc) = ascii_slots(sh.dense(), t, &mut self.x[h.base as usize..], &mut self.ent[h.base as usize..]);
        debug_assert!(acc & 0x8000 == 0 && w == lb.width);
    }

    /// Full replacement (new text, growth, token path): ASCII is shaped straight into the
    /// block by the slot kernels (NEON from 16 bytes); ligatures / Latin-1 go through the token
    /// path into scratch. Damage: old line ∪ new line, O(1). A refused text leaves the label
    /// exactly as it was.
    #[inline(never)]
    fn replace<const D: bool, P: Marks + ?Sized>(&mut self, sh: &Shaper, ts: &TokStrike, lb: &mut Label, new: &str, plane: &mut P) -> Outcome {
        let s = new.as_bytes();
        let n = s.len();
        let h = self.runs[lb.run as usize];
        let n_old = h.len as usize;
        // Block: grow first if the text cannot fit (the old block stays until success).
        let (base, cap) = if n > lb.cap as usize {
            let cap = n.next_power_of_two() as u32;
            (self.alloc_block(cap as usize, lb.run), cap)
        } else {
            (lb.base, lb.cap)
        };
        let bs = base as usize;
        let mut wrote_block = false;
        let mut done = None;
        if s.is_ascii() {
            // SAFETY: ASCII; the block holds `cap >= n.next_multiple_of(16)` slots.
            let (w, acc) = unsafe {
                let (xp, ep) = (self.x.as_mut_ptr().add(bs), self.ent.as_mut_ptr().add(bs));
                #[cfg(target_arch = "aarch64")]
                let r = if n >= 16 { crate::shape::simd_slots(sh.dense(), s, 0, 0, xp, ep) } else { crate::shape::scalar_slots(sh.dense(), s, 0, 0, xp, ep) };
                #[cfg(not(target_arch = "aarch64"))]
                let r = crate::shape::scalar_slots(sh.dense(), s, 0, 0, xp, ep);
                r
            };
            wrote_block = true;
            if acc & 0x8000 == 0 {
                done = Some((n, w, true));
            }
        }
        if done.is_none() {
            let r = self.tokens_to_scratch(sh, ts, s);
            match r {
                Ok((j, w)) => {
                    self.x[bs..bs + j].copy_from_slice(&self.sx[..j]);
                    self.ent[bs..bs + j].copy_from_slice(&self.se[..j]);
                    done = Some((j, w, false));
                }
                Err(o) => {
                    if base != lb.base {
                        self.free_block(base, cap);
                    } else if wrote_block {
                        self.reshape_current(sh, lb);
                    }
                    return o;
                }
            }
        }
        let (glyphs, width, identity) = done.unwrap();
        if D {
            let w = match (n_old > 0, glyphs > 0) {
                (true, true) => Some(lb.width.max(width)),
                (true, false) => Some(lb.width),
                (false, true) => Some(width),
                (false, false) => None,
            };
            if let Some(w) = w {
                self.mark_line(lb, 0, w, plane);
            }
        }
        if base != lb.base {
            self.free_block(lb.base, lb.cap);
        }
        let r = &mut self.runs[lb.run as usize];
        r.base = base;
        r.len = glyphs as u32;
        if lb.text.len() < cap as usize + 16 {
            lb.text = vec![0u8; cap as usize + 16].into_boxed_slice();
            lb.text[..n].copy_from_slice(s);
            lb.head.copy_from_slice(&lb.text[..16]);
        } else {
            store_text(lb, s, lb.nbytes as usize);
        }
        lb.nbytes = n as u32;
        lb.base = base;
        lb.cap = cap;
        lb.flags = if identity { IDENTITY } else { 0 };
        lb.width = width;
        if glyphs <= 16 {
            let xs = &self.x[bs..bs + glyphs];
            for k in 0..glyphs {
                lb.pen[k] = pen_px(xs[k], lb.scale, lb.oxh) as i16;
            }
            lb.tspace = [i32::MIN, i32::MIN];
        }
        self.stats[3] += 1;
        Outcome::Done
    }

    /// Token path into scratch: `(glyphs, width)` in `sx`/`se`, or why not.
    #[cold]
    fn tokens_to_scratch(&mut self, sh: &Shaper, ts: &TokStrike, s: &[u8]) -> Result<(usize, i32), Outcome> {
        let n = s.len();
        let Some(tt) = sh.script.tokens.as_ref() else { return Err(Outcome::Refused) };
        self.sx.clear();
        self.sx.resize(n + 1, 0);
        self.se.clear();
        self.se.resize(n + 1, 0);
        self.sc.clear();
        self.sc.resize(n + 1, 0);
        // SAFETY: scratch holds n + 1 >= n elements; `s` is UTF-8 (from a &str).
        let Some(j) = (unsafe { tokenize(tt, s, self.se.as_mut_ptr(), self.sc.as_mut_ptr()) }) else {
            return Err(Outcome::Refused);
        };
        let mut x = 0i32;
        let mut acc = 0u32;
        for k in 0..j {
            let t = self.se[k] as usize;
            let u = if k + 1 < j { self.se[k + 1] as usize } else { 0 };
            let st = tt.step[t << 8 | u] as u32;
            acc |= st;
            self.sx[k] = x;
            x += st as i32;
        }
        if acc & 0x8000 != 0 {
            return Err(Outcome::Refused);
        }
        if self.se[..j].iter().any(|&t| ts.ready[t as usize] == 0) {
            return Err(Outcome::Miss);
        }
        Ok((j, x))
    }

    /// Re-shape the label's current text into its block (undo of a failed in-place attempt).
    #[cold]
    fn reshape_current(&mut self, sh: &Shaper, lb: &Label) {
        let bs = lb.base as usize;
        let t = lb.text[..lb.nbytes as usize].to_vec();
        if lb.flags == IDENTITY {
            // SAFETY: identity text is ASCII; the block holds `cap` slots.
            unsafe { crate::shape::scalar_slots(sh.dense(), &t, 0, 0, self.x.as_mut_ptr().add(bs), self.ent.as_mut_ptr().add(bs)) };
        } else {
            let tt = sh.script.tokens.as_ref().expect("token label without a token model");
            let mut tok = vec![0u16; t.len() + 1];
            let mut cl = vec![0u32; t.len() + 1];
            // SAFETY: buffers hold len + 1 elements; the stored text is UTF-8.
            let j = unsafe { tokenize(tt, &t, tok.as_mut_ptr(), cl.as_mut_ptr()) }.expect("stored text tokenizes");
            let mut x = 0i32;
            for k in 0..j {
                let u = if k + 1 < j { tok[k + 1] as usize } else { 0 };
                self.x[bs + k] = x;
                self.ent[bs + k] = tok[k];
                x += tt.step[(tok[k] as usize) << 8 | u] as i32;
            }
        }
    }

    /// The oracle / the vertex shader's arithmetic on the CPU: the 48-byte instances of a
    /// run's visible glyphs, in slot order (empty glyphs skipped, like the reference writer).
    pub fn expand(&self, cache: &GlyphCache, run: u32, out: &mut Vec<Instance>) {
        let h = self.runs[run as usize];
        let ts = cache.tok(h.strike as StrikeId);
        for k in 0..h.len as usize {
            let s = h.base as usize + k;
            let e = &ts.entry[self.ent[s] as usize];
            if e.uv[2] == 0 {
                continue;
            }
            let fx = (self.x[s].wrapping_sub(h.x0) as f32).mul_add(h.scale, h.origin[0] + 0.5).floor();
            out.push(Instance {
                xy: [fx + e.geo[0], h.origin[1] + e.geo[1]],
                wh: [e.geo[2], e.geo[3]],
                uv: e.uv,
                color: h.color,
                kind: KIND_GLYPH,
                space: h.space,
                aux: [0; 4],
            });
        }
    }

    /// Per-slot rendered boxes `(ent, box)` of a run, for the damage oracle (computed from the
    /// float entry geometry, independently of `ibox`), space-relative.
    pub fn slot_boxes(&self, cache: &GlyphCache, run: u32) -> Vec<Option<(u16, NRect)>> {
        let h = self.runs[run as usize];
        let ts = cache.tok(h.strike as StrikeId);
        (0..h.len as usize)
            .map(|k| {
                let s = h.base as usize + k;
                let e = &ts.entry[self.ent[s] as usize];
                if e.uv[2] == 0 {
                    return None;
                }
                let fx = (self.x[s].wrapping_sub(h.x0) as f32).mul_add(h.scale, h.origin[0] + 0.5).floor();
                let (x0, y0) = (fx + e.geo[0], h.origin[1] + e.geo[1]);
                Some((self.ent[s], [x0 as i32, y0 as i32, -((x0 + e.geo[2]) as i32), -((y0 + e.geo[3]) as i32)]))
            })
            .collect()
    }
}

/// Recompute a label's cached digit-cell tiles for space translation `sp`.
#[cold]
#[inline(never)]
fn retile(lb: &mut Label, sp: [i32; 2]) {
    let c = lb.dcell;
    for k in 0..16 {
        let px = lb.pen[k] as i32 + sp[0];
        let (x0, x1) = (px + c[0], px + c[2]);
        if x1 <= x0 || x1 <= 0 || x0 >= 128 * 32 {
            lb.t0[k] = 0xFF;
            lb.t1[k] = 0;
        } else {
            lb.t0[k] = (x0 >> 5).clamp(0, 127) as u8;
            lb.t1[k] = ((x1 - 1) >> 5).clamp(0, 127) as u8;
        }
    }
    let oy = lb.oy + sp[1];
    lb.trow = ((oy + c[1]) >> 5).max(0);
    lb.trow_end = (oy + c[3] - 1) >> 5;
    lb.tspace = sp;
}

/// Store the new text `s` (replacing `n_old` bytes) into `lb.text` and `lb.head`, without
/// library calls for short texts.
#[inline(always)]
fn store_text(lb: &mut Label, s: &[u8], n_old: usize) {
    let n = s.len();
    #[cfg(target_arch = "aarch64")]
    if n <= 16 && n_old <= 16 {
        // SAFETY: n <= 16 readable bytes (an empty slice may dangle, so it is not read);
        // `text` holds at least 32 bytes.
        unsafe {
            use core::arch::aarch64::*;
            let v = if n == 0 { vdupq_n_u8(0) } else { load16(s.as_ptr(), n) };
            vst1q_u8(lb.text.as_mut_ptr(), v);
            vst1q_u8(lb.head.as_mut_ptr(), v);
        }
        return;
    }
    lb.text[..n].copy_from_slice(s);
    for b in &mut lb.text[n..n_old.max(n)] {
        *b = 0;
    }
    lb.head.copy_from_slice(&lb.text[..16]);
}

/// Length of the longest common suffix of `a` and `b` (equal lengths).
#[inline]
fn common_suffix(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut k = 0;
    while k < n && a[a.len() - 1 - k] == b[b.len() - 1 - k] {
        k += 1;
    }
    k
}

/// Index of the first byte where `a` and `b` differ (`min(len)` if one is a prefix).
#[inline]
fn first_diff(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + 8 <= n {
        let x = u64::from_le_bytes(a[i..i + 8].try_into().unwrap()) ^ u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        if x != 0 {
            return i + (x.trailing_zeros() / 8) as usize;
        }
        i += 8;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

/// Scalar dense-table kernel writing pool slots: `x[k]` and `ent[k] = s[k]`. Returns the width
/// and the OR of all steps (bit 15 = trigger). `s` must be ASCII and the outputs at least
/// `s.len()` long.
pub fn ascii_slots(dense: &[u16], s: &[u8], xo: &mut [i32], eo: &mut [u16]) -> (i32, u32) {
    let n = s.len();
    assert!(xo.len() >= n && eo.len() >= n && dense.len() == 128 * 128);
    let mut x = 0i32;
    let mut acc = 0u32;
    // SAFETY: bounds asserted above; bytes are masked to 7 bits so dense indices stay in range
    // (callers pass ASCII).
    unsafe {
        let (xp, ep, dp) = (xo.as_mut_ptr(), eo.as_mut_ptr(), dense.as_ptr());
        for i in 0..n {
            let b = *s.get_unchecked(i) as usize;
            let c = if i + 1 < n { *s.get_unchecked(i + 1) as usize } else { 0 };
            let st = *dp.add((b & 127) << 7 | (c & 127)) as u32;
            acc |= st;
            *xp.add(i) = x;
            *ep.add(i) = b as u16;
            x += st as i32;
        }
    }
    (x, acc)
}
