//! The fast shapers: NEON table lookup + SIMD prefix sum, and a scalar twin.

use std::prelude::rust_2024::*;
use std::vec;
use super::script::{SimpleScript, TRIG, TokenTable, TOK_PAD};

/// A shaped run in font design units. Everything is an exact integer; the pixel scale is
/// applied only when instances are generated (`scale = size / upem`).
///
/// `x[i]` is the pen position of glyph `i` relative to the run origin; the advance of glyph
/// `i` (kerning included, as HarfBuzz reports it) is `x[i + 1] - x[i]` and `width - x[n - 1]`
/// for the last one. Offsets are always zero on the fast path (a pair with an offset is a
/// trigger), so none are stored.
#[derive(Default, Clone, Debug)]
pub struct GlyphRun {
    pub gid: Vec<u16>,
    pub x: Vec<i32>,
    /// UTF-8 byte offset of each glyph's cluster. Empty when `identity_clusters`
    /// (pure ASCII: cluster `i` is byte `i`).
    pub cluster: Vec<u32>,
    pub identity_clusters: bool,
    pub width: i32,
    pub upem: u16,
    pub size: f32,
}

impl GlyphRun {
    pub fn len(&self) -> usize {
        self.gid.len()
    }
    pub fn is_empty(&self) -> bool {
        self.gid.is_empty()
    }
    #[inline]
    pub fn advance(&self, i: usize) -> i32 {
        let next = if i + 1 < self.x.len() { self.x[i + 1] } else { self.width };
        next - self.x[i]
    }
    #[inline]
    pub fn cluster_of(&self, i: usize) -> u32 {
        if self.identity_clusters { i as u32 } else { self.cluster[i] }
    }
    /// Pixels per font unit.
    #[inline]
    pub fn scale(&self) -> f32 {
        self.size / self.upem as f32
    }
    fn reset(&mut self, upem: u16, size: f32) {
        self.gid.clear();
        self.x.clear();
        self.cluster.clear();
        self.identity_clusters = true;
        self.width = 0;
        self.upem = upem;
        self.size = size;
    }
}

/// Per-byte record for the scalar kernels: everything one input byte contributes, in 8 bytes.
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Rec {
    gid: u16,
    adv: u16,
    /// `class1 * n2`: this byte's row offset into the pair matrix.
    row_off: u16,
    /// `class2`: this byte's column.
    col: u16,
}

/// 128-entry byte tables for the NEON path (glyph ids; steps come from the dense table).
#[repr(C, align(64))]
struct SimdTables {
    gid_lo: [u8; 128],
    gid_hi: [u8; 128],
}

pub struct Shaper {
    pub script: SimpleScript,
    t: SimdTables,
    /// Codepoints 0..=255 (Latin-1), for the UTF-8 decoding path.
    cp_recs: [Rec; 256],
    /// Every covered ASCII gid fits a byte: the SIMD kernel skips the high-byte lookup.
    narrow_gid: bool,
    /// Dense expansion of the class-pair table for ASCII: `dense[a << 7 | b]` is the full step
    /// `adv(a) + kern(a, b)` of glyph `a` followed by `b`, or `0xFFFF` for "needs full
    /// shaping" (row or pair trigger, uncovered byte). Column 0 is the end-of-string column:
    /// `dense[a << 7]` is `adv(a)` (a NUL byte is uncovered, so its own row is `0xFFFF` and the
    /// column is free to carry the pad). 32 KiB.
    dense: Box<[u16]>,
    gid128: [u16; 128],
    /// Strings shorter than this take the scalar kernel in `shape_into`.
    pub simd_min_len: usize,
    /// Position-equivalence class of each ASCII byte: `a` and `b` share a class iff
    /// `dense[a][*] == dense[b][*]` and `dense[*][a] == dense[*][b]` over all 128 rows and
    /// columns. Substituting bytes within their classes leaves every pen step of every
    /// string unchanged (each step `dense[s_i][s_i+1]` keeps its row and column), so pen
    /// positions, width and eligibility are unchanged; only glyph ids differ. In DejaVu Sans
    /// the ten digits are one class.
    pub pos_class: [u8; 128],
}

impl Shaper {
    /// Dense step table and ASCII gid table, for the fused shape-to-instances kernel.
    pub fn dense_tables(&self) -> (&[u16], &[u16; 128]) {
        (&self.dense, &self.gid128)
    }

    pub fn new(script: SimpleScript) -> Self {
        let mut t = SimdTables {
            gid_lo: [0; 128],
            gid_hi: [0; 128],
        };
        let n2 = script.n2;
        let mut cp_recs = [Rec::default(); 256];
        for c in 0..256usize {
            cp_recs[c] = Rec {
                gid: script.gid[c],
                adv: script.adv[c],
                row_off: (script.class1[c] as usize * n2) as u16,
                col: script.class2[c] as u16,
            };
            if c < 128 {
                t.gid_lo[c] = script.gid[c] as u8;
                t.gid_hi[c] = (script.gid[c] >> 8) as u8;
            }
        }
        let narrow_gid = (0..128).all(|c| script.gid[c] < 256);
        let mut dense = vec![0xFFFFu16; 128 * 128].into_boxed_slice();
        for a in 0..128usize {
            for b in 0..128usize {
                // Column 0 is the pad column; real second bytes use their class column.
                let col = if b == 0 { 0 } else { script.class2[b] as usize };
                let k = script.matrix[script.class1[a] as usize * n2 + col];
                dense[a << 7 | b] = if k == TRIG || script.class1[a] == 0 {
                    0xFFFF
                } else {
                    (script.adv[a] as i32 + k as i32) as u16
                };
            }
        }
        let mut gid128 = [0u16; 128];
        gid128.copy_from_slice(&script.gid[..128]);
        let mut pos_class = [0u8; 128];
        let mut reps: Vec<usize> = vec![];
        for a in 0..128usize {
            let same = |r: usize| (0..128).all(|k| dense[a << 7 | k] == dense[r << 7 | k] && dense[k << 7 | a] == dense[k << 7 | r]);
            pos_class[a] = match reps.iter().position(|&r| same(r)) {
                Some(c) => c as u8,
                None => {
                    reps.push(a);
                    (reps.len() - 1) as u8
                }
            };
        }
        Shaper { script, t, cp_recs, narrow_gid, dense, gid128, simd_min_len: 16, pos_class }
    }

    /// The dense ASCII step table (`dense[a << 7 | b]`), for the glyph pool.
    #[inline]
    pub fn dense(&self) -> &[u16] {
        &self.dense
    }

    /// Convenience allocation wrapper. Returns `None` if the string needs full shaping.
    pub fn shape_fast(&self, text: &str, font_size: f32) -> Option<GlyphRun> {
        let mut r = GlyphRun::default();
        self.shape_into(text, font_size, &mut r).then_some(r)
    }

    /// The hot entry point: shapes into `out` (reusing its allocations). `false` means the
    /// string contains a trigger (or a codepoint outside the covered set) and `out` is
    /// unspecified; the caller must use the full shaper.
    ///
    /// Strings shorter than `simd_min_len` take the scalar kernel (its fixed cost is a third of
    /// the NEON kernel's); longer ones the NEON kernel. Non-ASCII text decodes UTF-8.
    #[inline]
    pub fn shape_into(&self, text: &str, font_size: f32, out: &mut GlyphRun) -> bool {
        out.reset(self.script.upem, font_size);
        if !text.is_ascii() {
            return self.shape_tokens(text, out);
        }
        let b = text.as_bytes();
        // SAFETY: `b` is ASCII (checked above).
        let ok = unsafe {
            #[cfg(target_arch = "aarch64")]
            if b.len() >= self.simd_min_len {
                self.simd_ascii(b, out)
            } else {
                self.scalar_ascii(b, out)
            }
            #[cfg(not(target_arch = "aarch64"))]
            self.scalar_ascii(b, out)
        };
        ok || self.shape_tokens(text, out)
    }

    /// Always the scalar kernel (ASCII loop; the token path for the rest).
    pub fn shape_scalar(&self, text: &str, font_size: f32, out: &mut GlyphRun) -> bool {
        out.reset(self.script.upem, font_size);
        if !text.is_ascii() {
            return self.shape_tokens(text, out);
        }
        // SAFETY: ASCII checked.
        unsafe { self.scalar_ascii(text.as_bytes(), out) || self.shape_tokens(text, out) }
    }

    /// Always the NEON kernel (aarch64 only), for benchmarks and tests.
    #[cfg(target_arch = "aarch64")]
    pub fn shape_simd(&self, text: &str, font_size: f32, out: &mut GlyphRun) -> bool {
        out.reset(self.script.upem, font_size);
        if !text.is_ascii() {
            return self.shape_tokens(text, out);
        }
        // SAFETY: ASCII checked.
        unsafe { self.simd_ascii(text.as_bytes(), out) || self.shape_tokens(text, out) }
    }

    /// The prototype's Latin-1 path (`char_indices` + class matrix + `Vec::push`), kept only as
    /// the "before" of the token path in the benchmarks. Ligature strings are refused.
    pub fn shape_latin1_legacy(&self, text: &str, font_size: f32, out: &mut GlyphRun) -> bool {
        out.reset(self.script.upem, font_size);
        self.scalar_utf8(text, out)
    }

    /// The token path: ASCII + Latin-1 UTF-8 with the font's ligatures replayed, then the
    /// same pair-step prefix sum as the ASCII kernels over the 256-token table. `false` for
    /// codepoints outside the covered set (or a font without a token model).
    ///
    /// Pass 1 writes one token per glyph (and its cluster = UTF-8 offset of its first byte);
    /// pass 2 turns tokens into gids and pen positions in place.
    pub fn shape_tokens(&self, text: &str, out: &mut GlyphRun) -> bool {
        let Some(tt) = self.script.tokens.as_ref() else { return false };
        let s = text.as_bytes();
        let n = s.len();
        out.gid.clear();
        out.x.clear();
        out.cluster.clear();
        out.identity_clusters = false;
        out.gid.reserve(n);
        out.x.reserve(n);
        out.cluster.reserve(n);
        let tok = out.gid.as_mut_ptr();
        let cl = out.cluster.as_mut_ptr();
        // SAFETY: at most one token per input byte, so `j <= i < n` stays within the reserved
        // capacity; `s` is valid UTF-8 (a `&str`), so a lead byte is followed by its
        // continuation byte (`i + 1 < n`).
        let j = unsafe { tokenize(tt, s, tok, cl) };
        let Some(j) = j else { return false };
        let step = tt.step.as_ptr();
        let gid = tt.gid.as_ptr();
        let xp = out.x.as_mut_ptr();
        let mut x = 0i32;
        let mut acc = 0u32;
        // SAFETY: tokens are < 256 so `t << 8 | u < 65536 = step.len()`; k < j <= capacity.
        unsafe {
            for k in 0..j {
                let t = *tok.add(k) as usize;
                let u = if k + 1 < j { *tok.add(k + 1) as usize } else { TOK_PAD as usize };
                let st = *step.add(t << 8 | u) as u32;
                acc |= st;
                *tok.add(k) = *gid.add(t);
                *xp.add(k) = x;
                x += st as i32;
            }
            out.width = x;
            if acc & 0x8000 != 0 {
                return false;
            }
            out.gid.set_len(j);
            out.x.set_len(j);
            out.cluster.set_len(j);
        }
        true
    }

    /// Scalar kernel over ASCII bytes: per glyph one byte load, one 16-bit step load from the
    /// dense pair table, one gid load and two stores; no bounds checks or `Vec::push`.
    ///
    /// # Safety
    /// `s` must be ASCII (all indices below are `< 128`), which `shape_*` check.
    #[inline(never)]
    unsafe fn scalar_ascii(&self, s: &[u8], out: &mut GlyphRun) -> bool {
        let n = s.len();
        out.gid.reserve(n);
        out.x.reserve(n);
        let gp = out.gid.as_mut_ptr();
        let xp = out.x.as_mut_ptr();
        let dense = self.dense.as_ptr();
        let gids = self.gid128.as_ptr();
        let mut x = 0i32;
        let mut acc = 0u32;
        // SAFETY: bytes are < 128, so every index is < 128 * 128 = dense.len() and < 128 for
        // `gids`; i < n <= reserved capacity; reads of `s` stay below n.
        unsafe {
            let mut i = 0usize;
            if n > 0 {
                let mut a = *s.get_unchecked(0) as usize;
                // Two glyphs per iteration (a, b) and (b, c): no register shuffling.
                while i + 2 < n {
                    let b = *s.get_unchecked(i + 1) as usize;
                    let c = *s.get_unchecked(i + 2) as usize;
                    let s1 = *dense.add(a << 7 | b) as u32;
                    let s2 = *dense.add(b << 7 | c) as u32;
                    acc |= s1 | s2;
                    *gp.add(i) = *gids.add(a);
                    *gp.add(i + 1) = *gids.add(b);
                    *xp.add(i) = x;
                    *xp.add(i + 1) = x + s1 as i32;
                    x += (s1 + s2) as i32;
                    a = c;
                    i += 2;
                }
                // One or two glyphs left; the byte after the last is the pad column (0).
                while i < n {
                    let b = if i + 1 < n { *s.get_unchecked(i + 1) as usize } else { 0 };
                    let step = *dense.add(a << 7 | b) as u32;
                    acc |= step;
                    *gp.add(i) = *gids.add(a);
                    *xp.add(i) = x;
                    x += step as i32;
                    a = b;
                    i += 1;
                }
            }
        }
        // A trigger step is 0xFFFF; real steps are < 0x1000, so bit 15 is set only by triggers
        // (x is garbage then).
        out.width = x;
        if acc & 0x8000 != 0 {
            return false;
        }
        // SAFETY: the first n slots of both vectors were written above.
        unsafe {
            out.gid.set_len(n);
            out.x.set_len(n);
        }
        true
    }

    fn scalar_utf8(&self, text: &str, out: &mut GlyphRun) -> bool {
        out.gid.clear();
        out.x.clear();
        out.cluster.clear();
        out.identity_clusters = false;
        let recs = &self.cp_recs;
        let mat = &self.script.matrix;
        let mut x = 0i32;
        let mut prev: Option<(Rec, u32)> = None;
        for (off, ch) in text.char_indices() {
            let cp = ch as u32;
            if cp > 255 {
                return false;
            }
            let cur = recs[cp as usize];
            if let Some((p, cl)) = prev {
                let k = mat[p.row_off as usize + cur.col as usize];
                if k == TRIG {
                    return false;
                }
                out.gid.push(p.gid);
                out.x.push(x);
                out.cluster.push(cl);
                x += p.adv as i32 + k as i32;
            }
            prev = Some((cur, off as u32));
        }
        if let Some((p, cl)) = prev {
            let k = mat[p.row_off as usize];
            if k == TRIG {
                return false;
            }
            out.gid.push(p.gid);
            out.x.push(x);
            out.cluster.push(cl);
            x += p.adv as i32 + k as i32;
        }
        out.width = x;
        true
    }

    /// NEON: 16 ASCII bytes per iteration.
    ///
    /// # Safety
    /// `s` must be ASCII.
    #[cfg(target_arch = "aarch64")]
    #[inline]
    unsafe fn simd_ascii(&self, s: &[u8], out: &mut GlyphRun) -> bool {
        // SAFETY: NEON is baseline on aarch64; the kernel's invariants are on `simd_kernel`.
        unsafe {
            if self.narrow_gid {
                self.simd_kernel::<true>(s, out)
            } else {
                self.simd_kernel::<false>(s, out)
            }
        }
    }

    /// Per 16 bytes: `tbl`/`tbx` map bytes to glyph ids; the pair step `adv(a) + kern(a, b)` of
    /// each lane is gathered from the dense table by `a << 7 | b` (the second operand is the
    /// same chunk shifted by one byte, so no cross-lane work is needed); a 16-lane prefix sum
    /// gives pen positions.
    ///
    /// # Safety
    /// `s` must be ASCII, so every gather index is `< 128 * 128 = dense.len()` (lanes past the
    /// end read zero bytes, also in range). `16 * max_step < 65536` (checked by
    /// `SimpleScript::build`) so the u16 prefix sums cannot wrap: triggers (0xFFFF steps) are
    /// OR-ed aside before summing, so their wrap is harmless. With `NARROW`, every ASCII gid is
    /// `< 256`.
    #[cfg(target_arch = "aarch64")]
    #[inline(never)]
    pub unsafe fn simd_kernel<const NARROW: bool>(&self, s: &[u8], out: &mut GlyphRun) -> bool {
        use core::arch::aarch64::*;
        unsafe {
            let n = s.len();
            out.gid.reserve(n + 16);
            out.x.reserve(n + 16);
            let gp = out.gid.as_mut_ptr();
            let xp = out.x.as_mut_ptr();
            let dense = self.dense.as_ptr();
            let t = &self.t;

            // One 128-entry lookup: bytes < 64 from the first table via tbl, 64..127 from the
            // second via tbx on (b - 64). The table pointer goes through black_box so the loads
            // stay in the loop instead of the compiler pinning the tables in registers (it
            // spilled them to the stack every call).
            #[inline(always)]
            unsafe fn look(t: *const u8, i: uint8x16_t, ih: uint8x16_t) -> uint8x16_t {
                unsafe {
                    let t = core::hint::black_box(t);
                    let a = vld1q_u8_x4(t);
                    let b = vld1q_u8_x4(t.add(64));
                    vqtbx4q_u8(vqtbl4q_u8(a, i), b, ih)
                }
            }

            const IOTA: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
            let iota = vld1q_u8(IOTA.as_ptr());
            let c64 = vdupq_n_u8(64);
            let zero16 = vdupq_n_u16(0);
            let zero8 = vdupq_n_u8(0);
            let mut base = 0i32;
            let mut trig = vdupq_n_u16(0);
            let mut i = 0usize;
            while i < n {
                let rem = n - i;
                let (b, nx) = if rem >= 17 {
                    (vld1q_u8(s.as_ptr().add(i)), vld1q_u8(s.as_ptr().add(i + 1)))
                } else {
                    let b = load_partial(s.as_ptr().add(i), rem);
                    (b, vextq_u8::<1>(b, zero8))
                };
                let valid = vcltq_u8(iota, vdupq_n_u8(if rem >= 16 { 16 } else { rem as u8 }));
                let ib = vsubq_u8(b, c64);

                // glyph ids
                let glo = look(t.gid_lo.as_ptr(), b, ib);
                let g = gp.add(i);
                if NARROW {
                    vst1q_u16(g, vmovl_u8(vget_low_u8(glo)));
                    vst1q_u16(g.add(8), vmovl_high_u8(glo));
                } else {
                    let ghi = look(t.gid_hi.as_ptr(), b, ib);
                    vst1q_u16(g, vreinterpretq_u16_u8(vzip1q_u8(glo, ghi)));
                    vst1q_u16(g.add(8), vreinterpretq_u16_u8(vzip2q_u8(glo, ghi)));
                }

                // steps: gather dense[a << 7 | b]
                let i0 = vaddw_u8(vshll_n_u8::<7>(vget_low_u8(b)), vget_low_u8(nx));
                let i1 = vaddw_high_u8(vshll_high_n_u8::<7>(b), nx);
                let mut st = [0u16; 16];
                macro_rules! lane {
                    ($v:ident, $off:literal, $($l:literal)*) => { $(
                        *st.get_unchecked_mut($off + $l) = *dense.add(vgetq_lane_u16::<$l>($v) as usize);
                    )* };
                }
                lane!(i0, 0, 0 1 2 3 4 5 6 7);
                lane!(i1, 8, 0 1 2 3 4 5 6 7);
                let m8 = vreinterpretq_s8_u8(valid);
                let m0 = vreinterpretq_u16_s16(vmovl_s8(vget_low_s8(m8)));
                let m1 = vreinterpretq_u16_s16(vmovl_high_s8(m8));
                let st0 = vandq_u16(vld1q_u16(st.as_ptr()), m0);
                let st1 = vandq_u16(vld1q_u16(st.as_ptr().add(8)), m1);
                trig = vorrq_u16(trig, vorrq_u16(st0, st1));

                // Exclusive prefix sum of the 16 steps.
                let mut s0 = vaddq_u16(st0, vextq_u16::<7>(zero16, st0));
                s0 = vaddq_u16(s0, vextq_u16::<6>(zero16, s0));
                s0 = vaddq_u16(s0, vextq_u16::<4>(zero16, s0));
                let mut s1 = vaddq_u16(st1, vextq_u16::<7>(zero16, st1));
                s1 = vaddq_u16(s1, vextq_u16::<6>(zero16, s1));
                s1 = vaddq_u16(s1, vextq_u16::<4>(zero16, s1));
                s1 = vaddq_u16(s1, vdupq_laneq_u16::<7>(s0));
                let e0 = vsubq_u16(s0, st0);
                let e1 = vsubq_u16(s1, st1);
                let bv = vdupq_n_u32(base as u32);
                let x = xp.add(i) as *mut u32;
                vst1q_u32(x, vaddw_u16(bv, vget_low_u16(e0)));
                vst1q_u32(x.add(4), vaddw_high_u16(bv, e0));
                vst1q_u32(x.add(8), vaddw_u16(bv, vget_low_u16(e1)));
                vst1q_u32(x.add(12), vaddw_high_u16(bv, e1));
                base += vgetq_lane_u16::<7>(s1) as i32;
                i += 16;
            }
            out.width = base;
            if vmaxvq_u16(trig) & 0x8000 != 0 {
                return false;
            }
            out.gid.set_len(n);
            out.x.set_len(n);
            true
        }
    }
}

/// Load `rem` (1..=16) bytes into the low lanes of a vector, zero elsewhere, without reading
/// past `p + rem`. Overlapping loads, no memory round trip for the combine.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub unsafe fn load_partial(p: *const u8, rem: usize) -> core::arch::aarch64::uint8x16_t {
    use core::arch::aarch64::*;
    unsafe {
        if rem >= 8 {
            // Bytes [0,8) and [rem-8,rem); re-index so lane j holds s[j].
            let lo = (p as *const u64).read_unaligned();
            let hi = (p.add(rem - 8) as *const u64).read_unaligned();
            let v = vcombine_u8(vcreate_u8(lo), vcreate_u8(hi));
            const IOTA: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
            let iota = vld1q_u8(IOTA.as_ptr());
            // lane j >= 8 comes from v[j + 16 - rem]; lanes >= rem are zeroed (index 0xFF).
            let shift = vdupq_n_u8((16 - rem) as u8);
            let idx = vbslq_u8(vcltq_u8(iota, vdupq_n_u8(8)), iota, vaddq_u8(iota, shift));
            let idx = vorrq_u8(idx, vcgeq_u8(iota, vdupq_n_u8(rem as u8)));
            vqtbl1q_u8(v, idx)
        } else {
            let x: u64 = if rem >= 4 {
                let lo = (p as *const u32).read_unaligned() as u64;
                let hi = (p.add(rem - 4) as *const u32).read_unaligned() as u64;
                lo | (hi << ((rem - 4) * 8))
            } else if rem >= 2 {
                let lo = (p as *const u16).read_unaligned() as u64;
                let hi = (p.add(rem - 2) as *const u16).read_unaligned() as u64;
                lo | (hi << ((rem - 2) * 8))
            } else {
                *p as u64
            };
            vcombine_u8(vcreate_u8(x), vdup_n_u8(0))
        }
    }
}

/// Rewrite UTF-8 (ASCII + Latin-1) into glyph tokens, replaying the font's ligatures exactly as
/// HarfBuzz applies a ligature lookup: left to right, at each glyph the ligatures that start
/// with it in the font's order, first match wins, matching resumes after the ligature.
/// Writes `tok[j]` and `cl[j]` (UTF-8 offset of the glyph's first byte); returns the glyph
/// count, or `None` for a codepoint outside ASCII printable / Latin-1.
///
/// # Safety
/// `tok` and `cl` must be valid for `s.len()` writes; `s` must be valid UTF-8.
#[inline]
pub unsafe fn tokenize(tt: &TokenTable, s: &[u8], tok: *mut u16, cl: *mut u32) -> Option<usize> {
    let n = s.len();
    let (mut i, mut j) = (0usize, 0usize);
    unsafe {
        while i < n {
            let b = *s.get_unchecked(i);
            let (t, len) = if b < 0x80 {
                if *tt.lig_first.get_unchecked(b as usize) {
                    let mut hit = (b, 1usize);
                    for (first, rest, lt) in &tt.ligs {
                        if *first == b && s.len() - i - 1 >= rest.len() && s[i + 1..i + 1 + rest.len()] == rest[..] {
                            hit = (*lt, 1 + rest.len());
                            break;
                        }
                    }
                    hit
                } else {
                    // Control bytes (and DEL) have all-TRIG rows; they must not alias the
                    // internal tokens below 0x20.
                    if b < 0x20 {
                        return None;
                    }
                    (b, 1)
                }
            } else if b == 0xC3 {
                (*s.get_unchecked(i + 1) + 0x40, 2)
            } else if b == 0xC2 {
                let c = *s.get_unchecked(i + 1);
                if c < 0xA0 {
                    return None; // C1 controls
                }
                (c, 2)
            } else {
                return None; // beyond Latin-1
            };
            *tok.add(j) = t as u16;
            *cl.add(j) = i as u32;
            j += 1;
            i += len;
        }
    }
    Some(j)
}

/// NEON slot kernel for the glyph pool: per 16 ASCII bytes, the entry ids (the bytes, widened:
/// no glyph-id lookup) and the pen x of every slot (dense-table gather + 16-lane prefix sum),
/// starting at pen `x_base`. Returns `(end pen, OR of all steps)`; bit 15 of the OR = trigger
/// (outputs are then unspecified). `next` is the byte after `s` (0 = end of run), so a chunk
/// appended to a stream gets the right last step.
///
/// # Safety
/// `s` must be ASCII; `xp`/`ep` valid for `s.len().next_multiple_of(16)` writes.
#[cfg(target_arch = "aarch64")]
#[inline(never)]
pub unsafe fn simd_slots(dense: &[u16], s: &[u8], next: u8, x_base: i32, xp: *mut i32, ep: *mut u16) -> (i32, u32) {
    use core::arch::aarch64::*;
    debug_assert_eq!(dense.len(), 128 * 128);
    unsafe {
        let n = s.len();
        let dense = core::hint::black_box(dense.as_ptr());
        const IOTA: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let iota = vld1q_u8(IOTA.as_ptr());
        let zero16 = vdupq_n_u16(0);
        let mut base = x_base;
        let mut trig = vdupq_n_u16(0);
        let mut i = 0usize;
        while i < n {
            let rem = n - i;
            let (b, nx) = if rem >= 17 {
                (vld1q_u8(s.as_ptr().add(i)), vld1q_u8(s.as_ptr().add(i + 1)))
            } else {
                let b = load_partial(s.as_ptr().add(i), rem);
                // the byte after the last one is `next`
                let nb = vsetq_lane_u8::<15>(next, vdupq_n_u8(0));
                let shifted = vextq_u8::<1>(b, vdupq_n_u8(0));
                let at = vceqq_u8(iota, vdupq_n_u8((rem - 1) as u8));
                let nbv = vdupq_laneq_u8::<15>(nb);
                (b, vbslq_u8(at, nbv, shifted))
            };
            let valid = vcltq_u8(iota, vdupq_n_u8(if rem >= 16 { 16 } else { rem as u8 }));
            vst1q_u16(ep.add(i), vmovl_u8(vget_low_u8(b)));
            vst1q_u16(ep.add(i + 8), vmovl_high_u8(b));
            let i0 = vaddw_u8(vshll_n_u8::<7>(vget_low_u8(b)), vget_low_u8(nx));
            let i1 = vaddw_high_u8(vshll_high_n_u8::<7>(b), nx);
            let mut st = [0u16; 16];
            macro_rules! lane {
                ($v:ident, $off:literal, $($l:literal)*) => { $(
                    *st.get_unchecked_mut($off + $l) = *dense.add(vgetq_lane_u16::<$l>($v) as usize);
                )* };
            }
            lane!(i0, 0, 0 1 2 3 4 5 6 7);
            lane!(i1, 8, 0 1 2 3 4 5 6 7);
            let m8 = vreinterpretq_s8_u8(valid);
            let m0 = vreinterpretq_u16_s16(vmovl_s8(vget_low_s8(m8)));
            let m1 = vreinterpretq_u16_s16(vmovl_high_s8(m8));
            let st0 = vandq_u16(vld1q_u16(st.as_ptr()), m0);
            let st1 = vandq_u16(vld1q_u16(st.as_ptr().add(8)), m1);
            trig = vorrq_u16(trig, vorrq_u16(st0, st1));
            let mut s0 = vaddq_u16(st0, vextq_u16::<7>(zero16, st0));
            s0 = vaddq_u16(s0, vextq_u16::<6>(zero16, s0));
            s0 = vaddq_u16(s0, vextq_u16::<4>(zero16, s0));
            let mut s1 = vaddq_u16(st1, vextq_u16::<7>(zero16, st1));
            s1 = vaddq_u16(s1, vextq_u16::<6>(zero16, s1));
            s1 = vaddq_u16(s1, vextq_u16::<4>(zero16, s1));
            s1 = vaddq_u16(s1, vdupq_laneq_u16::<7>(s0));
            let e0 = vsubq_u16(s0, st0);
            let e1 = vsubq_u16(s1, st1);
            let bv = vdupq_n_u32(base as u32);
            let x = xp.add(i) as *mut u32;
            vst1q_u32(x, vaddw_u16(bv, vget_low_u16(e0)));
            vst1q_u32(x.add(4), vaddw_high_u16(bv, e0));
            vst1q_u32(x.add(8), vaddw_u16(bv, vget_low_u16(e1)));
            vst1q_u32(x.add(12), vaddw_high_u16(bv, e1));
            base = base.wrapping_add(vgetq_lane_u16::<7>(s1) as i32);
            i += 16;
        }
        (base, vmaxvq_u16(trig) as u32)
    }
}

/// Scalar twin of [`simd_slots`] (also the short-run kernel).
///
/// # Safety
/// `s` must be ASCII; `xp`/`ep` valid for `s.len()` writes.
#[inline]
pub unsafe fn scalar_slots(dense: &[u16], s: &[u8], next: u8, x_base: i32, xp: *mut i32, ep: *mut u16) -> (i32, u32) {
    let n = s.len();
    let mut x = x_base;
    let mut acc = 0u32;
    unsafe {
        let dp = dense.as_ptr();
        for i in 0..n {
            let b = *s.get_unchecked(i) as usize;
            let c = if i + 1 < n { *s.get_unchecked(i + 1) as usize } else { next as usize };
            let st = *dp.add((b & 127) << 7 | (c & 127)) as u32;
            acc |= st;
            *xp.add(i) = x;
            *ep.add(i) = b as u16;
            x = x.wrapping_add(st as i32);
        }
    }
    (x, acc)
}
