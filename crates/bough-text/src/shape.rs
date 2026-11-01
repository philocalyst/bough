//! The fast shapers: NEON table lookup + SIMD prefix sum, and a scalar twin.

use crate::script::{SimpleScript, TRIG};

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
}

impl Shaper {
    /// Dense step table and ASCII gid table, for the fused shape-to-instances kernel.
    pub(crate) fn dense_tables(&self) -> (&[u16], &[u16; 128]) {
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
        Shaper { script, t, cp_recs, narrow_gid, dense, gid128, simd_min_len: 16 }
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
            return self.scalar_utf8(text, out);
        }
        let b = text.as_bytes();
        // SAFETY: `b` is ASCII (checked above).
        unsafe {
            #[cfg(target_arch = "aarch64")]
            if b.len() >= self.simd_min_len {
                return self.simd_ascii(b, out);
            }
            self.scalar_ascii(b, out)
        }
    }

    /// Always the scalar kernel (ASCII loop, UTF-8 decoding for the rest).
    pub fn shape_scalar(&self, text: &str, font_size: f32, out: &mut GlyphRun) -> bool {
        out.reset(self.script.upem, font_size);
        if !text.is_ascii() {
            return self.scalar_utf8(text, out);
        }
        // SAFETY: ASCII checked.
        unsafe { self.scalar_ascii(text.as_bytes(), out) }
    }

    /// Always the NEON kernel (aarch64 only), for benchmarks and tests.
    #[cfg(target_arch = "aarch64")]
    pub fn shape_simd(&self, text: &str, font_size: f32, out: &mut GlyphRun) -> bool {
        out.reset(self.script.upem, font_size);
        if !text.is_ascii() {
            return self.scalar_utf8(text, out);
        }
        // SAFETY: ASCII checked.
        unsafe { self.simd_ascii(text.as_bytes(), out) }
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
unsafe fn load_partial(p: *const u8, rem: usize) -> core::arch::aarch64::uint8x16_t {
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
