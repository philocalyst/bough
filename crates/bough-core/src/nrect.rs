//! `NRect`: device-pixel rectangles in negated-max form `(x0, y0, -x1, -y1)`, half-open.
//!
//! With `r = (x0, y0, -x1, -y1)`:
//!
//! * `intersect(a, b) = lane-wise max`, `union(a, b) = lane-wise min`
//! * `empty(r) <=> x0 + r[2] >= 0 || y0 + r[3] >= 0`
//!
//! # Domain
//!
//! Coordinates (normal form) live in `[-32766, 32766]`. Inside that domain no lane of a
//! non-empty rect can equal `i16::MAX`, so the canonical empty rect `EMPTY = [MAX; 4]` is
//! distinguishable lane-by-lane from every non-empty rect, is the identity of `union`
//! (lane-wise min), and is absorbed by `intersect` (lane-wise max). `union` is only the
//! normal-form union when both operands are non-empty or canonical-empty; every rect that is
//! *stored* in the tree is canonicalised on the way in (`NRect::canon`), and `intersect`
//! results are only ever tested for emptiness.
//!
//! Containment/intersection tests against a query use a threshold vector (`Thresh`) so that
//! one `lane < thresh` compare answers all four edge conditions at once (and the canonical
//! empty rect fails it automatically, which is what makes padding slots free).

#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

pub const MAXC: i16 = i16::MAX;
pub const COORD_MIN: i32 = -32766;
pub const COORD_MAX: i32 = 32766;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NRect(pub [i16; 4]);

impl core::fmt::Debug for NRect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_empty() {
            write!(f, "NRect(empty:{:?})", self.0)
        } else {
            let [a, b, c, d] = self.0;
            write!(f, "NRect[{a},{b} .. {},{}]", -(c as i32), -(d as i32))
        }
    }
}

/// `lane < thresh` for all four lanes <=> the (non-empty, canonical) rect intersects the query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Thresh(pub [i16; 4]);

impl Thresh {
    /// Threshold for "ink intersects non-empty query `q`".
    #[inline(always)]
    pub fn of_rect(q: NRect) -> Thresh {
        let [x0, y0, nx1, ny1] = q.0;
        Thresh([-nx1, -ny1, -x0, -y0])
    }
    /// Threshold for "ink contains pixel (x, y)"; `None` if the point is outside the domain.
    #[inline(always)]
    pub fn of_point(x: i32, y: i32) -> Option<Thresh> {
        if !(COORD_MIN..=COORD_MAX).contains(&x) || !(COORD_MIN..=COORD_MAX).contains(&y) {
            return None;
        }
        Some(Thresh([(x + 1) as i16, (y + 1) as i16, (-x) as i16, (-y) as i16]))
    }
    /// The threshold for inks stored relative to an offset `(dx, dy)` (absolute = stored + offset):
    /// `stored + off < t  <=>  stored < t - off`, lanes `(dx, dy, -dx, -dy)`, saturating (a saturated
    /// lane is "always" / "never", which is what the unsaturated value meant for in-domain inks).
    #[inline(always)]
    pub fn shift(self, dx: i32, dy: i32) -> Thresh {
        let t = self.0;
        let c = |v: i32| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        Thresh([c(t[0] as i32 - dx), c(t[1] as i32 - dy), c(t[2] as i32 + dx), c(t[3] as i32 + dy)])
    }
    /// `shift` with the offset given as lanes `(dx, dy, -dx, -dy)`: one saturating lane subtract.
    #[inline(always)]
    pub fn shift_lanes(self, o: [i16; 4]) -> Thresh {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            let r = vqsub_s16(vld1_s16(self.0.as_ptr()), vld1_s16(o.as_ptr()));
            Thresh(core::mem::transmute::<int16x4_t, [i16; 4]>(r))
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            let t = self.0;
            Thresh([t[0].saturating_sub(o[0]), t[1].saturating_sub(o[1]), t[2].saturating_sub(o[2]), t[3].saturating_sub(o[3])])
        }
    }
    #[inline(always)]
    pub fn matches(self, r: NRect) -> bool {
        let (a, b) = (r.0, self.0);
        (a[0] < b[0]) & (a[1] < b[1]) & (a[2] < b[2]) & (a[3] < b[3])
    }
    #[cfg(target_arch = "aarch64")]
    #[inline(always)]
    fn bits(self) -> u64 {
        u64::from_ne_bytes(unsafe { core::mem::transmute::<[i16; 4], [u8; 8]>(self.0) })
    }
}

impl Default for NRect {
    /// The empty rect (the identity of `union`).
    fn default() -> Self {
        NRect::EMPTY
    }
}

impl NRect {
    pub const EMPTY: NRect = NRect([MAXC; 4]);

    /// From normal-form half-open `[x0, x1) x [y0, y1)`; clamps to the domain, canonical if empty.
    #[inline]
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> NRect {
        let c = |v: i32| v.clamp(COORD_MIN, COORD_MAX);
        let (x0, y0, x1, y1) = (c(x0), c(y0), c(x1), c(y1));
        if x0 >= x1 || y0 >= y1 {
            NRect::EMPTY
        } else {
            NRect([x0 as i16, y0 as i16, (-x1) as i16, (-y1) as i16])
        }
    }
    /// Normal form `(x0, y0, x1, y1)`. For an empty rect the values are unspecified but satisfy `x0 >= x1 || y0 >= y1`.
    #[inline]
    pub fn to_xyxy(self) -> [i32; 4] {
        let [a, b, c, d] = self.0;
        [a as i32, b as i32, -(c as i32), -(d as i32)]
    }
    #[inline(always)]
    pub fn is_empty(self) -> bool {
        let a = self.0;
        (a[0] as i32 + a[2] as i32 >= 0) | (a[1] as i32 + a[3] as i32 >= 0)
    }
    #[inline(always)]
    pub fn canon(self) -> NRect {
        if self.is_empty() { NRect::EMPTY } else { self }
    }
    /// Lane-wise max.
    #[inline(always)]
    pub fn intersect(self, o: NRect) -> NRect {
        let (a, b) = (self.0, o.0);
        NRect([a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2]), a[3].max(b[3])])
    }
    /// Lane-wise min. Precondition: both operands non-empty or canonical-empty.
    #[inline(always)]
    pub fn union(self, o: NRect) -> NRect {
        let (a, b) = (self.0, o.0);
        NRect([a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2]), a[3].min(b[3])])
    }
    #[inline(always)]
    pub fn intersects(self, o: NRect) -> bool {
        !self.intersect(o).is_empty()
    }
    #[inline(always)]
    pub fn contains_point(self, x: i32, y: i32) -> bool {
        let [a, b, c, d] = self.0;
        (a as i32 <= x) & (y >= b as i32) & (x < -(c as i32)) & (y < -(d as i32))
    }
    /// Lane-wise wrapping add of `dl = (dx, dy, -dx, -dy)` (as i16): the translation of a
    /// NON-EMPTY rect whose result is known to lie in the domain (wrapping makes an offset outside
    /// i16 still exact, since only the in-domain sum is ever formed).
    #[inline(always)]
    pub fn add_lanes(self, dl: [i16; 4]) -> NRect {
        let a = self.0;
        NRect([a[0].wrapping_add(dl[0]), a[1].wrapping_add(dl[1]), a[2].wrapping_add(dl[2]), a[3].wrapping_add(dl[3])])
    }
    /// Translate, or `None` if a coordinate would leave the domain (EMPTY stays EMPTY).
    #[inline(always)]
    pub fn translate_checked(self, dx: i32, dy: i32) -> Option<NRect> {
        if self.0[0] == MAXC {
            return Some(NRect::EMPTY);
        }
        let [x0, y0, x1, y1] = self.to_xyxy();
        let d = (COORD_MIN..=COORD_MAX).contains(&(x0 + dx))
            & (COORD_MIN..=COORD_MAX).contains(&(x1 + dx))
            & (COORD_MIN..=COORD_MAX).contains(&(y0 + dy))
            & (COORD_MIN..=COORD_MAX).contains(&(y1 + dy));
        d.then(|| NRect([(x0 + dx) as i16, (y0 + dy) as i16, (-(x1 + dx)) as i16, (-(y1 + dy)) as i16]))
    }
    /// Translate by `(dx, dy)`, or `None` if a coordinate would leave the domain or the delta is beyond
    /// i16 (EMPTY stays EMPTY). `translate_checked` in one saturating lane add and one lane compare: a
    /// lane of a non-empty in-domain rect lies in `[-32766, 32766]`, and a sum that left it saturates or
    /// lands on a value of larger magnitude.
    #[inline(always)]
    pub fn translate_in_domain(self, dx: i32, dy: i32) -> Option<NRect> {
        if self.0[0] == MAXC {
            return Some(NRect::EMPTY);
        }
        if (dx.unsigned_abs() | dy.unsigned_abs()) > 32766 {
            return None;
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            let d = [dx as i16, dy as i16, (-dx) as i16, (-dy) as i16];
            let r = vqadd_s16(vld1_s16(self.0.as_ptr()), vld1_s16(d.as_ptr()));
            let out = vcgt_s16(vqabs_s16(r), vdup_n_s16(COORD_MAX as i16));
            if vget_lane_u64::<0>(vreinterpret_u64_u16(out)) != 0 {
                return None;
            }
            Some(NRect(core::mem::transmute::<int16x4_t, [i16; 4]>(r)))
        }
        #[cfg(not(target_arch = "aarch64"))]
        self.translate_checked(dx, dy)
    }
    /// Translate; the canonical empty rect stays canonical. Coordinates must stay in the domain.
    #[inline(always)]
    pub fn translate(self, dx: i32, dy: i32) -> NRect {
        if self.0[0] == MAXC {
            return NRect::EMPTY;
        }
        let [x0, y0, x1, y1] = self.to_xyxy();
        debug_assert!((COORD_MIN..=COORD_MAX).contains(&(x0 + dx)));
        debug_assert!((COORD_MIN..=COORD_MAX).contains(&(y0 + dy)));
        debug_assert!((COORD_MIN..=COORD_MAX).contains(&(x1 + dx)));
        debug_assert!((COORD_MIN..=COORD_MAX).contains(&(y1 + dy)));
        NRect([(x0 + dx) as i16, (y0 + dy) as i16, (-(x1 + dx)) as i16, (-(y1 + dy)) as i16])
    }
}

// ---------------------------------------------------------------------------------------------
// Two rects per 128-bit operation (NEON), scalar twins for reference and non-aarch64 targets.
// ---------------------------------------------------------------------------------------------

#[inline(always)]
pub fn intersect2_scalar(a: [NRect; 2], b: [NRect; 2]) -> [NRect; 2] {
    [a[0].intersect(b[0]), a[1].intersect(b[1])]
}
#[inline(always)]
pub fn union2_scalar(a: [NRect; 2], b: [NRect; 2]) -> [NRect; 2] {
    [a[0].union(b[0]), a[1].union(b[1])]
}
#[inline(always)]
pub fn is_empty2_scalar(a: [NRect; 2]) -> [bool; 2] {
    [a[0].is_empty(), a[1].is_empty()]
}

#[cfg(target_arch = "aarch64")]
mod simd2 {
    use super::*;
    #[inline(always)]
    fn ld(a: &[NRect; 2]) -> int16x8_t {
        unsafe { vld1q_s16(a.as_ptr() as *const i16) }
    }
    #[inline(always)]
    fn st(v: int16x8_t) -> [NRect; 2] {
        let mut o = [NRect::EMPTY; 2];
        unsafe { vst1q_s16(o.as_mut_ptr() as *mut i16, v) };
        o
    }
    #[inline(always)]
    pub fn intersect2(a: [NRect; 2], b: [NRect; 2]) -> [NRect; 2] {
        unsafe { st(vmaxq_s16(ld(&a), ld(&b))) }
    }
    #[inline(always)]
    pub fn union2(a: [NRect; 2], b: [NRect; 2]) -> [NRect; 2] {
        unsafe { st(vminq_s16(ld(&a), ld(&b))) }
    }
    /// Per rect: saturating `(x0 + -x1, y0 + -y1)` via a 32-bit-pair swap, then sign tests.
    #[inline(always)]
    pub fn is_empty2(a: [NRect; 2]) -> [bool; 2] {
        unsafe {
            let v = ld(&a);
            let sw = vreinterpretq_s16_s32(vrev64q_s32(vreinterpretq_s32_s16(v)));
            let s = vqaddq_s16(v, sw); // lanes 0,1 of each rect: x0-x1, y0-y1 (saturated)
            let nonneg = vcgezq_s16(s);
            // low two 16-bit lanes of each 64-bit half
            let w = vreinterpretq_u32_u16(nonneg);
            let any0 = vgetq_lane_u32::<0>(w) != 0;
            let any1 = vgetq_lane_u32::<2>(w) != 0;
            [any0, any1]
        }
    }
}
#[cfg(target_arch = "aarch64")]
pub use simd2::{intersect2, is_empty2, union2};
#[cfg(not(target_arch = "aarch64"))]
pub use self::{intersect2_scalar as intersect2, is_empty2_scalar as is_empty2, union2_scalar as union2};

// ---------------------------------------------------------------------------------------------
// Slice kernels used by pages and the directory.
// ---------------------------------------------------------------------------------------------

/// Bit `i` set iff `thresh.matches(ink[i])`, for `n <= 64` rects at `p`. Scalar reference.
///
/// # Safety
/// `p` must be valid for reads of `n` `NRect`s.
#[inline(always)]
pub unsafe fn lt_mask_scalar(p: *const NRect, n: usize, t: Thresh) -> u64 {
    let mut m = 0u64;
    for i in 0..n {
        m |= (t.matches(unsafe { *p.add(i) }) as u64) << i;
    }
    m
}

/// SIMD `lt_mask`.
///
/// Per 8 rects: 4 vector compares (`rect_lane < thresh_lane`), 3 pairwise-min folds so that one
/// u16 lane per rect says "all four edge tests passed". For a full page (`n == 64`) the eight
/// resulting vectors are narrowed to bytes (`uzp1`), weighted with bit values and folded with four
/// `addp` into the 64-bit mask: no per-block horizontal add. Other lengths use one `addv` per block.
///
/// # Safety
/// `p` must be valid for reads of `n` `NRect`s, `n <= 64`.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub unsafe fn lt_mask(p: *const NRect, n: usize, t: Thresh) -> u64 {
    unsafe {
        let tv = vreinterpretq_s16_u64(vdupq_n_u64(t.bits()));
        #[inline(always)]
        unsafe fn block(q: *const i16, tv: int16x8_t) -> uint16x8_t {
            unsafe {
                let l0 = vcltq_s16(vld1q_s16(q), tv);
                let l1 = vcltq_s16(vld1q_s16(q.add(8)), tv);
                let l2 = vcltq_s16(vld1q_s16(q.add(16)), tv);
                let l3 = vcltq_s16(vld1q_s16(q.add(24)), tv);
                vpminq_u16(vpminq_u16(l0, l1), vpminq_u16(l2, l3))
            }
        }
        if n == 64 {
            const W: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
            let w = vld1q_u8(W.as_ptr());
            let q = p as *const i16;
            let mut m = [vdupq_n_u8(0); 4];
            for i in 0..4 {
                let a = block(q.add(i * 64), tv);
                let b = block(q.add(i * 64 + 32), tv);
                let packed = vuzp1q_u8(vreinterpretq_u8_u16(a), vreinterpretq_u8_u16(b));
                m[i] = vandq_u8(packed, w);
            }
            let x = vpaddq_u8(m[0], m[1]);
            let y = vpaddq_u8(m[2], m[3]);
            let z = vpaddq_u8(x, y);
            let r = vpaddq_u8(z, z);
            return vgetq_lane_u64::<0>(vreinterpretq_u64_u8(r));
        }
        const W: [u16; 8] = [1, 2, 4, 8, 16, 32, 64, 128];
        let w = vld1q_u16(W.as_ptr());
        let mut out = 0u64;
        let full = n / 8;
        for i in 0..full {
            let all = block(p.add(i * 8) as *const i16, tv);
            out |= (vaddvq_u16(vandq_u16(all, w)) as u64) << (i * 8);
        }
        for i in full * 8..n {
            out |= (t.matches(*p.add(i)) as u64) << i;
        }
        out
    }
}
#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
pub unsafe fn lt_mask(p: *const NRect, n: usize, t: Thresh) -> u64 {
    unsafe { lt_mask_scalar(p, n, t) }
}

/// Union (lane-wise min) of `n` canonical rects. Scalar reference.
#[inline(always)]
pub fn union_all_scalar(r: &[NRect]) -> NRect {
    r.iter().fold(NRect::EMPTY, |a, &b| a.union(b))
}

/// Union (lane-wise min) of canonical rects: one `smin` per 2 rects.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub fn union_all(r: &[NRect]) -> NRect {
    unsafe {
        let p = r.as_ptr() as *const i16;
        let pairs = r.len() / 2;
        let mut acc = vdupq_n_s16(MAXC);
        for i in 0..pairs {
            acc = vminq_s16(acc, vld1q_s16(p.add(i * 8)));
        }
        let m = vmin_s16(vget_low_s16(acc), vget_high_s16(acc));
        let mut o = NRect(core::mem::transmute::<int16x4_t, [i16; 4]>(m));
        if r.len() & 1 == 1 {
            o = o.union(r[r.len() - 1]);
        }
        o
    }
}
#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
pub fn union_all(r: &[NRect]) -> NRect {
    union_all_scalar(r)
}

/// Translate `n` canonical rects in place; returns (union of old, union of new). Scalar reference.
///
/// # Safety
/// `p` valid for reads and writes of `n` rects.
pub unsafe fn translate_union_scalar(p: *mut NRect, n: usize, dx: i32, dy: i32) -> (NRect, NRect) {
    let (mut o, mut w) = (NRect::EMPTY, NRect::EMPTY);
    for i in 0..n {
        let r = unsafe { *p.add(i) };
        let t = r.translate(dx, dy);
        unsafe { *p.add(i) = t };
        o = o.union(r);
        w = w.union(t);
    }
    (o, w)
}

/// SIMD translate: per pair `bsl(v == MAX, MAX, qadd(v, d))`, fused with both union reductions.
///
/// # Safety
/// `p` valid for reads and writes of `n` rects; translated rects stay inside the domain.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub unsafe fn translate_union(p: *mut NRect, n: usize, dx: i32, dy: i32) -> (NRect, NRect) {
    unsafe {
        let d = [dx as i16, dy as i16, (-dx) as i16, (-dy) as i16, dx as i16, dy as i16, (-dx) as i16, (-dy) as i16];
        let dv = vld1q_s16(d.as_ptr());
        let maxv = vdupq_n_s16(MAXC);
        let mut old = maxv;
        let mut new = maxv;
        let pairs = n / 2;
        let q = p as *mut i16;
        for i in 0..pairs {
            let v = vld1q_s16(q.add(i * 8));
            // wrapping: equal to the saturating add whenever the result is in the domain, and exactly
            // undone by the opposite translation when a caller finds afterwards that it was not
            let t = vaddq_s16(v, dv);
            let is_empty = vceqq_s16(v, maxv);
            let r = vbslq_s16(is_empty, maxv, t);
            vst1q_s16(q.add(i * 8), r);
            old = vminq_s16(old, v);
            new = vminq_s16(new, r);
        }
        let fold = |a: int16x8_t| NRect(core::mem::transmute::<int16x4_t, [i16; 4]>(vmin_s16(vget_low_s16(a), vget_high_s16(a))));
        let (mut o, mut w) = (fold(old), fold(new));
        if n & 1 == 1 {
            let r = *p.add(n - 1);
            let t = if r.0[0] == MAXC { r } else { r.add_lanes([dx as i16, dy as i16, (-dx) as i16, (-dy) as i16]) };
            *p.add(n - 1) = t;
            o = o.union(r);
            w = w.union(t);
        }
        (o, w)
    }
}
#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
pub unsafe fn translate_union(p: *mut NRect, n: usize, dx: i32, dy: i32) -> (NRect, NRect) {
    unsafe { translate_union_scalar(p, n, dx, dy) }
}

// ---------------------------------------------------------------------------------------------
// 8-rect group kernels (one record group, or the 8 group summaries of a page).
// ---------------------------------------------------------------------------------------------

/// Bit `i` (0..8) set iff `t.matches(p[i])`. Scalar reference.
///
/// # Safety
/// `p` valid for reads of 8 rects.
#[inline(always)]
pub unsafe fn lt8_scalar(p: *const NRect, t: Thresh) -> u32 {
    let mut m = 0u32;
    for i in 0..8 {
        m |= (t.matches(unsafe { *p.add(i) }) as u32) << i;
    }
    m
}

/// `lt8` with NEON: 4 loads, 4 compares (2 rects per register, all four edge conditions at once),
/// 3 pairwise-min folds to one lane per rect, then a byte-narrow + weighted horizontal add.
///
/// # Safety
/// `p` valid for reads of 8 rects.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub unsafe fn lt8(p: *const NRect, t: Thresh) -> u32 {
    unsafe {
        let tv = vreinterpretq_s16_u64(vdupq_n_u64(t.bits()));
        let q = p as *const i16;
        let l0 = vcltq_s16(vld1q_s16(q), tv);
        let l1 = vcltq_s16(vld1q_s16(q.add(8)), tv);
        let l2 = vcltq_s16(vld1q_s16(q.add(16)), tv);
        let l3 = vcltq_s16(vld1q_s16(q.add(24)), tv);
        let all = vpminq_u16(vpminq_u16(l0, l1), vpminq_u16(l2, l3));
        const W: [u8; 8] = [1, 2, 4, 8, 16, 32, 64, 128];
        vaddv_u8(vand_u8(vmovn_u16(all), vld1_u8(W.as_ptr()))) as u32
    }
}
#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
pub unsafe fn lt8(p: *const NRect, t: Thresh) -> u32 {
    unsafe { lt8_scalar(p, t) }
}

/// Union (lane-wise min) of 8 canonical rects.
///
/// # Safety
/// `p` valid for reads of 8 rects.
#[inline(always)]
pub unsafe fn union8(p: *const NRect) -> NRect {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let q = p as *const i16;
        let a = vminq_s16(vld1q_s16(q), vld1q_s16(q.add(8)));
        let b = vminq_s16(vld1q_s16(q.add(16)), vld1q_s16(q.add(24)));
        let c = vminq_s16(a, b);
        NRect(core::mem::transmute::<int16x4_t, [i16; 4]>(vmin_s16(vget_low_s16(c), vget_high_s16(c))))
    }
    #[cfg(not(target_arch = "aarch64"))]
    unsafe {
        union_all_scalar(core::slice::from_raw_parts(p, 8))
    }
}


// ---------------------------------------------------------------------------------------------
// Exact rectangles.
// ---------------------------------------------------------------------------------------------

/// An exact device-pixel rectangle `[x0, x1) x [y0, y1)` in i32 (coordinates within `±WRect::LIMIT`).
/// `NRect` is the 16-bit form the pages store *relative to their offset* and the form every window test
/// uses; a `WRect` is what a record's ink *is*. `to_nrect` is the clamped view (exact inside the domain,
/// and a rect wholly outside it becomes EMPTY): it may be tested and marked, never translated again.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WRect(pub [i32; 4]);

impl WRect {
    pub const EMPTY: WRect = WRect([0; 4]);
    pub const LIMIT: i32 = 1 << 29;
    #[inline]
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> WRect {
        let c = |v: i32| v.clamp(-Self::LIMIT, Self::LIMIT);
        let (x0, y0, x1, y1) = (c(x0), c(y0), c(x1), c(y1));
        if x0 >= x1 || y0 >= y1 { WRect::EMPTY } else { WRect([x0, y0, x1, y1]) }
    }
    #[inline(always)]
    pub fn is_empty(self) -> bool {
        self.0[0] >= self.0[2] || self.0[1] >= self.0[3]
    }
    #[inline]
    pub fn translate(self, dx: i32, dy: i32) -> WRect {
        if self.is_empty() {
            return WRect::EMPTY;
        }
        let [x0, y0, x1, y1] = self.0;
        WRect::new(x0 + dx, y0 + dy, x1 + dx, y1 + dy)
    }
    #[inline]
    pub fn union(self, o: WRect) -> WRect {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return self;
        }
        let (a, b) = (self.0, o.0);
        WRect([a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
    }
    /// The clamped 16-bit view.
    #[inline]
    pub fn to_nrect(self) -> NRect {
        if self.is_empty() {
            return NRect::EMPTY;
        }
        NRect::new(self.0[0], self.0[1], self.0[2], self.0[3])
    }
    /// Is the rect representable exactly as an `NRect` (inside the domain, or empty)?
    #[inline]
    pub fn in_domain(self) -> bool {
        self.is_empty() || self.0.iter().all(|v| (COORD_MIN..=COORD_MAX).contains(v))
    }
}

impl From<NRect> for WRect {
    #[inline]
    fn from(r: NRect) -> WRect {
        if r.is_empty() {
            return WRect::EMPTY;
        }
        WRect(r.to_xyxy())
    }
}
