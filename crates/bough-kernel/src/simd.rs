//! The fast executor's inner loops as explicit vectors (portable SIMD: NEON on aarch64, SSE/AVX on x86-64).
//!
//! Every function here is the kernel's arithmetic applied lane-wise: the lanes are 16-bit accumulator
//! channels, widened to 32 bits for the Q15 product `(a * b + HALF) >> 15` and narrowed back, which is
//! exactly `k::mul15` (integers: a lane cannot round differently from the scalar). The scalar kernel stays
//! the specification; `tests/exact.rs` holds these equal to it on every scene, and `tests/simd.rs` on random
//! lanes including the extremes (0, ONE).
//!
//! Shape of a row: a kind produces a *coverage row* (Q15 per pixel) or a colour row, and one of the blends
//! below applies it. Four pixels (16 lanes) per step. (A colour row under a coverage row, for gradients and user
//! nodes, was tried and dropped: slower than the kernel's scalar form.)
use crate::k;
use std::simd::prelude::*;

type V = Simd<u16, 16>;
type W = Simd<u32, 16>;

/// `k::mul15` on 16 lanes.
#[inline(always)]
fn mul15(a: V, b: V) -> V {
    ((a.cast::<u32>() * b.cast::<u32>() + W::splat(k::HALF)) >> W::splat(15)).cast::<u16>()
}
/// Each of four values repeated over its pixel's four channels.
#[inline(always)]
fn per_px(c: [u16; 4]) -> V {
    simd_swizzle!(Simd::<u16, 4>::from_array(c), [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3])
}
/// One colour repeated over four pixels.
#[inline(always)]
fn per_ch(c: [u16; 4]) -> V {
    simd_swizzle!(Simd::<u16, 4>::from_array(c), [0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3])
}
/// `k::blend` of four pixels: `sh + mul15(acc, ONE - sh.a)`.
#[inline(always)]
fn over4(acc: V, sh: V) -> V {
    let a = simd_swizzle!(sh, [3, 3, 3, 3, 7, 7, 7, 7, 11, 11, 11, 11, 15, 15, 15, 15]);
    sh + mul15(acc, V::splat(k::ONE as u16) - a)
}
#[inline(always)]
fn ld1(p: &[u16; 4]) -> wgsl_rs::std::Vec4u {
    wgsl_rs::std::Vec4 { x: p[0] as u32, y: p[1] as u32, z: p[2] as u32, w: p[3] as u32 }
}
#[inline(always)]
fn st1(v: wgsl_rs::std::Vec4u) -> [u16; 4] {
    [v.x as u16, v.y as u16, v.z as u16, v.w as u16]
}

/// `acc = blend(acc, sh)` over a span, `sh` constant and not opaque.
#[inline(always)]
pub fn blend_const(row: &mut [[u16; 4]], sh: [u16; 4]) {
    let (quads, rest) = row.as_chunks_mut::<4>();
    let s = per_ch(sh);
    let kk = V::splat((k::ONE - sh[3] as u32) as u16);
    for q in quads {
        let a = V::from_array(*q.as_flattened().first_chunk().unwrap());
        q.as_flattened_mut().copy_from_slice(&(s + mul15(a, kk)).to_array());
    }
    for p in rest {
        *p = st1(k::blend(ld1(p), ld1(&sh)));
    }
}

/// `acc = blend(acc, scale(pm, cov[x]))` over a span: one colour under a coverage row (glyphs, rounded
/// edges, shadows).
#[inline(always)]
pub fn blend_cov(row: &mut [[u16; 4]], pm: [u16; 4], cov: &[u16]) {
    debug_assert_eq!(row.len(), cov.len());
    let (quads, rest) = row.as_chunks_mut::<4>();
    let (cq, cr) = cov.as_chunks::<4>();
    let p = per_ch(pm);
    for (q, c) in quads.iter_mut().zip(cq) {
        let a = V::from_array(*q.as_flattened().first_chunk().unwrap());
        q.as_flattened_mut().copy_from_slice(&over4(a, mul15(p, per_px(*c))).to_array());
    }
    for (px, &c) in rest.iter_mut().zip(cr) {
        *px = st1(k::blend(ld1(px), k::scale(ld1(&pm), c as u32)));
    }
}

/// `acc = blend(acc, scale(pm, h1[x]) + scale(pm2, h2[x]))`: a border's ring and fill.
#[inline(always)]
pub fn blend_cov2(row: &mut [[u16; 4]], pm: [u16; 4], h1: &[u16], pm2: [u16; 4], h2: &[u16]) {
    let (quads, rest) = row.as_chunks_mut::<4>();
    let ((c1, r1), (c2, r2)) = (h1.as_chunks::<4>(), h2.as_chunks::<4>());
    let (p, p2) = (per_ch(pm), per_ch(pm2));
    for ((q, a1), a2) in quads.iter_mut().zip(c1).zip(c2) {
        let a = V::from_array(*q.as_flattened().first_chunk().unwrap());
        let sh = mul15(p, per_px(*a1)) + mul15(p2, per_px(*a2));
        q.as_flattened_mut().copy_from_slice(&over4(a, sh).to_array());
    }
    for ((px, &a1), &a2) in rest.iter_mut().zip(r1).zip(r2) {
        *px = st1(k::blend(ld1(px), k::scale(ld1(&pm), a1 as u32) + k::scale(ld1(&pm2), a2 as u32)));
    }
}

/// `out[x] = mul15(a[x], b)`: a separable shadow's column profile times its row's value.
#[inline(always)]
pub fn scale_row(a: &[u16], b: u16, out: &mut [u16]) {
    let ((aq, ar), (oq, or)) = (a.as_chunks::<16>(), out.as_chunks_mut::<16>());
    for (o, a) in oq.iter_mut().zip(aq) {
        *o = mul15(V::from_array(*a), V::splat(b)).to_array();
    }
    for (o, &a) in or.iter_mut().zip(ar) {
        *o = k::mul15(a as u32, b as u32) as u16;
    }
}

/// `out[x] = store8(acc[x])`: Q15 channels to 8 bits, RGBA to BGRA words.
#[inline(always)]
pub fn store_row(acc: &[[u16; 4]], out: &mut [u32]) {
    let ((aq, ar), (oq, or)) = (acc.as_chunks::<4>(), out.as_chunks_mut::<4>());
    for (o, a) in oq.iter_mut().zip(aq) {
        let v = V::from_array(*a.as_flattened().first_chunk().unwrap());
        let b = ((v.cast::<u32>() * W::splat(255) + W::splat(k::HALF)) >> W::splat(15)).cast::<u8>();
        let b = simd_swizzle!(b, [2, 1, 0, 3, 6, 5, 4, 7, 10, 9, 8, 11, 14, 13, 12, 15]).to_array();
        for (w, c) in o.iter_mut().zip(b.as_chunks::<4>().0) {
            *w = u32::from_le_bytes(*c);
        }
    }
    for (o, p) in or.iter_mut().zip(ar) {
        *o = k::store8(ld1(p));
    }
}

/// `out[x] = q15(t[x])`: a row of coverage texels to Q15. Returns false if every texel is 0 (nothing to
/// blend).
#[inline(always)]
pub fn q15_row(t: &[u8], out: &mut [u16]) -> bool {
    let mut any = 0u8;
    for (o, &t) in out.iter_mut().zip(t) {
        any |= t;
        let t = t as u16;
        // k::q15 in 16-bit lanes (255 -> 32768 fits)
        *o = (t << 7) + (t >> 1) + (t >> 7);
    }
    any != 0
}
