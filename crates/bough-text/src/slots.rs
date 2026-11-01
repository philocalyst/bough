//! Round 3: **one slot per byte** for every covered text, so the token table (ligatures, Latin-1) runs through
//! the same window mechanism as ASCII.
//!
//! Round 2 wrote one slot per *glyph* on the token path, so a slot index stopped being a byte index as soon as a
//! line held a ligature or a two-byte Latin-1 character: windows (wrapped lines, stream lines) could not point
//! into such a block, and multi-line text fell back to rustybuzz glyph instances. Here every byte owns a slot:
//!
//! * a glyph sits on the first byte of its cluster (`ent` = its token, `x` = its pen);
//! * the other bytes of the cluster (a UTF-8 continuation byte, the consumed components of a ligature) are
//!   **phantom** slots: `ent = TOK_PH`, which draws nothing (its strike entry is empty), and `x` = the pen after
//!   the glyph (their step is 0: row `TOK_PH` of the token table is zero).
//!
//! So a slot index is a byte index for all covered text, and everything built on that (windows, the word
//! table, wrapping, streams, line rings) works for ligatures and Latin-1 unchanged. The pen of a glyph depends
//! on its token and the *next real* token (the next non-phantom slot): `step[t << 8 | next]`.
//!
//! # The kernel: wide classification, one gather, one prefix sum
//!
//! [`any_slots`] reads 16 bytes per step and classifies them at once (NEON):
//!
//! * the 16 bytes and the byte after them are ASCII (one `umaxv`): the round-2 ASCII lanes, glyph = byte,
//!   `dense[a << 7 | b]` (32 KB table);
//! * otherwise the token lanes: each lane decodes its token from its byte and the next one, lane-locally
//!   (`b < 0x80`: the byte; a continuation byte: `TOK_PH`; `C3 c`: `c + 0x40`; `C2 c`: `c`); the next real
//!   token of each lane is a `bsl` of the token vectors shifted by one and by two (Latin-1 has at most one
//!   phantom in a row); the step is gathered from the token table at `t << 8 | next`.
//!
//! Both lane kinds produce the same `(x, ent)` slots and the same trigger bit, so they mix freely chunk by
//! chunk. Anything the lanes cannot express (a ligature, whose replay is a left-to-right parse; a control byte;
//! a codepoint beyond Latin-1) shows up as a trigger or a refusal bit of its chunk: ligature text is redone by
//! the scalar byte-slot tokenizer ([`token_slots_scalar`], same output), anything else is refused.

use crate::script::{TokenTable, TOK_PH};
use crate::shape::Shaper;

/// Why [`any_slots`] could not shape a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A codepoint outside the covered set (beyond Latin-1, a control character, U+00AD...) or a pair the
    /// token model does not cover: the caller needs the full shaper.
    Uncovered,
}

/// Scalar byte-slot shaping of UTF-8 `s` (ASCII, Latin-1, the font's ligatures): `x[i]`, `ent[i]` for every
/// byte (phantoms for continuation bytes and consumed ligature components). Returns the pen after the last
/// glyph, or `None` when the text is not covered.
///
/// # Safety
/// `s` must be valid UTF-8; `xp`/`ep` valid for `s.len()` writes.
pub unsafe fn token_slots_scalar(tt: &TokenTable, s: &[u8], x_base: i32, xp: *mut i32, ep: *mut u16) -> Option<i32> {
    let n = s.len();
    unsafe {
        // Pass 1: one token per byte (the ligature replay of `shape::tokenize`, phantoms in place).
        let mut i = 0usize;
        while i < n {
            let b = *s.get_unchecked(i);
            if b < 0x80 {
                if *tt.lig_first.get_unchecked(b as usize) {
                    let mut hit = 0usize;
                    for (first, rest, lt) in &tt.ligs {
                        if *first == b && n - i - 1 >= rest.len() && s[i + 1..i + 1 + rest.len()] == rest[..] {
                            *ep.add(i) = *lt as u16;
                            for k in 1..=rest.len() {
                                *ep.add(i + k) = TOK_PH as u16;
                            }
                            hit = 1 + rest.len();
                            break;
                        }
                    }
                    if hit > 0 {
                        i += hit;
                        continue;
                    }
                }
                if b < 0x20 {
                    return None; // control bytes would alias the internal tokens
                }
                *ep.add(i) = b as u16;
                i += 1;
            } else if b == 0xC3 {
                *ep.add(i) = (*s.get_unchecked(i + 1) + 0x40) as u16;
                *ep.add(i + 1) = TOK_PH as u16;
                i += 2;
            } else if b == 0xC2 {
                let c = *s.get_unchecked(i + 1);
                if c < 0xA0 {
                    return None; // C1 controls
                }
                *ep.add(i) = c as u16;
                *ep.add(i + 1) = TOK_PH as u16;
                i += 2;
            } else {
                return None; // beyond Latin-1
            }
        }
        // Pass 2: pens. A glyph's step reads the next real token; its phantoms take the pen after it.
        let step = tt.step.as_ptr();
        let mut x = x_base;
        let mut acc = 0u32;
        let mut i = 0usize;
        while i < n {
            let t = *ep.add(i) as usize;
            let mut j = i + 1;
            while j < n && *ep.add(j) == TOK_PH as u16 {
                j += 1;
            }
            let u = if j < n { *ep.add(j) as usize } else { 0 };
            let st = *step.add(t << 8 | u) as u32;
            acc |= st;
            *xp.add(i) = x;
            x = x.wrapping_add(st as i32);
            for k in i + 1..j {
                *xp.add(k) = x;
            }
            i = j;
        }
        if acc & 0x8000 != 0 {
            return None;
        }
        Some(x)
    }
}

/// Shape UTF-8 `s` into byte slots starting at pen `x_base` (see the module docs): the NEON classifying kernel,
/// with the scalar byte-slot tokenizer for chunks with a ligature. Returns the pen after the last glyph and
/// whether any token beyond ASCII was written (a Latin-1 glyph or a ligature: the caller makes sure its strike
/// has rasterized it, a cold path; ASCII strikes are complete from their creation).
///
/// # Safety
/// `s` must be valid UTF-8; `xp`/`ep` valid for `s.len().next_multiple_of(16)` writes.
#[inline]
pub unsafe fn any_slots(sh: &Shaper, s: &[u8], x_base: i32, xp: *mut i32, ep: *mut u16) -> Result<(i32, bool), Refusal> {
    let Some(tt) = sh.script.tokens.as_ref() else {
        // No token model: ASCII through the dense table only.
        if !s.is_ascii() {
            return Err(Refusal::Uncovered);
        }
        let (w, acc) = unsafe { crate::window::slots(sh, s, 0, x_base, xp, ep) };
        return if acc & 0x8000 == 0 { Ok((w, false)) } else { Err(Refusal::Uncovered) };
    };
    #[cfg(target_arch = "aarch64")]
    {
        if s.len() >= 16 {
            return match unsafe { neon_any(sh.dense(), tt, s, x_base, xp, ep) } {
                Lanes::Done(w, exotic) => Ok((w, exotic)),
                Lanes::Refused => Err(Refusal::Uncovered),
                Lanes::Ligature => unsafe { token_slots_scalar(tt, s, x_base, xp, ep) }.map(|w| (w, true)).ok_or(Refusal::Uncovered),
            };
        }
    }
    // Short texts: the scalar ASCII loop when ASCII without a trigger, else the scalar byte-slot tokenizer.
    if s.is_ascii() {
        let (w, acc) = unsafe { crate::shape::scalar_slots(sh.dense(), s, 0, x_base, xp, ep) };
        if acc & 0x8000 == 0 {
            return Ok((w, false));
        }
    }
    unsafe { token_slots_scalar(tt, s, x_base, xp, ep) }.map(|w| (w, true)).ok_or(Refusal::Uncovered)
}

/// Test hook: which way the NEON kernel finished a text of at least 16 bytes (0: done in the lanes, 1: refused,
/// 2: a trigger sent it to the scalar tokenizer; 3: not run). The gate requires 0 for every covered text
/// without a ligature, so a lane bug cannot hide behind the scalar path.
#[doc(hidden)]
pub fn lanes_outcome(sh: &Shaper, s: &str) -> u8 {
    #[cfg(target_arch = "aarch64")]
    if let (Some(tt), true) = (sh.script.tokens.as_ref(), s.len() >= 16) {
        let n = s.len().next_multiple_of(16);
        let (mut x, mut e) = (vec![0i32; n], vec![0u16; n]);
        // SAFETY: buffers of n slots; `s` is UTF-8 of at least 16 bytes.
        return match unsafe { neon_any(sh.dense(), tt, s.as_bytes(), 0, x.as_mut_ptr(), e.as_mut_ptr()) } {
            Lanes::Done(..) => 0,
            Lanes::Refused => 1,
            Lanes::Ligature => 2,
        };
    }
    let _ = (sh, s);
    3
}

#[cfg(target_arch = "aarch64")]
enum Lanes {
    Done(i32, bool),
    Refused,
    Ligature,
}

/// The NEON classifying kernel (see the module docs).
///
/// # Safety
/// `s` valid UTF-8 of at least 16 bytes; outputs valid for `s.len().next_multiple_of(16)` writes.
#[cfg(target_arch = "aarch64")]
#[inline(never)]
unsafe fn neon_any(dense: &[u16], tt: &TokenTable, s: &[u8], x_base: i32, xp: *mut i32, ep: *mut u16) -> Lanes {
    use core::arch::aarch64::*;
    unsafe {
        let n = s.len();
        let dense = core::hint::black_box(dense.as_ptr());
        let step = core::hint::black_box(tt.step.as_ptr());
        const IOTA: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let iota = vld1q_u8(IOTA.as_ptr());
        let zero16 = vdupq_n_u16(0);
        let mut base = x_base;
        let mut trig = vdupq_n_u16(0);
        let mut bad = vdupq_n_u8(0);
        let mut exotic = false;
        let mut i = 0usize;
        while i < n {
            let rem = n - i;
            // b0..b3: the chunk and the chunk shifted by 1, 2, 3 bytes. Inside the text: four overlapping loads.
            // At its end: the bytes after the text read as zero (the pad token), built in registers from
            // `load_partial` (no buffer, no read past the end).
            let (b0, b1, b2, b3) = if rem >= 19 {
                let p = s.as_ptr().add(i);
                (vld1q_u8(p), vld1q_u8(p.add(1)), vld1q_u8(p.add(2)), vld1q_u8(p.add(3)))
            } else {
                let p = s.as_ptr().add(i);
                let (lo, hi) = if rem > 16 { (vld1q_u8(p), crate::shape::load_partial(p.add(16), rem - 16)) } else { (crate::shape::load_partial(p, rem), vdupq_n_u8(0)) };
                (lo, vextq_u8::<1>(lo, hi), vextq_u8::<2>(lo, hi), vextq_u8::<3>(lo, hi))
            };
            let valid = vcltq_u8(iota, vdupq_n_u8(if rem >= 16 { 16 } else { rem as u8 }));
            let (t0, idx0, idx1, tbl) = if vmaxvq_u8(vorrq_u8(b0, b1)) < 0x80 {
                // ASCII lanes: glyph = byte, step = dense[a << 7 | b].
                let i0 = vaddw_u8(vshll_n_u8::<7>(vget_low_u8(b0)), vget_low_u8(b1));
                let i1 = vaddw_high_u8(vshll_high_n_u8::<7>(b0), b1);
                (b0, i0, i1, dense)
            } else {
                // Token lanes: decode each lane's token from its byte and the next one.
                #[inline(always)]
                unsafe fn tok(b: uint8x16_t, nx: uint8x16_t, bad: &mut uint8x16_t, valid: uint8x16_t) -> uint8x16_t {
                    unsafe {
                        let ascii = vcltq_u8(b, vdupq_n_u8(0x80));
                        let cont = vceqq_u8(vandq_u8(b, vdupq_n_u8(0xC0)), vdupq_n_u8(0x80));
                        let c3 = vceqq_u8(b, vdupq_n_u8(0xC3));
                        let c2 = vceqq_u8(b, vdupq_n_u8(0xC2));
                        let lead = vaddq_u8(nx, vandq_u8(c3, vdupq_n_u8(0x40)));
                        let t = vbslq_u8(ascii, b, vbslq_u8(cont, vdupq_n_u8(TOK_PH), lead));
                        // beyond Latin-1, a C1 control, an ASCII control byte (aliases internal tokens)
                        let other = vmvnq_u8(vorrq_u8(vorrq_u8(ascii, cont), vorrq_u8(c2, c3)));
                        let c1 = vandq_u8(c2, vcltq_u8(nx, vdupq_n_u8(0xA0)));
                        let ctl = vandq_u8(ascii, vcltq_u8(b, vdupq_n_u8(0x20)));
                        *bad = vorrq_u8(*bad, vandq_u8(valid, vorrq_u8(vorrq_u8(other, c1), ctl)));
                        t
                    }
                }
                let mut ign = vdupq_n_u8(0);
                let t0 = tok(b0, b1, &mut bad, valid);
                // lanes past the end decode the zero padding (token 0 = pad), never "bad"
                let t1 = tok(b1, b2, &mut ign, valid);
                let t2 = tok(b2, b3, &mut ign, valid);
                // the next real token: one phantom at most between two Latin-1 glyphs
                let nx = vbslq_u8(vceqq_u8(t1, vdupq_n_u8(TOK_PH)), t2, t1);
                let i0 = vorrq_u16(vshll_n_u8::<8>(vget_low_u8(t0)), vmovl_u8(vget_low_u8(nx)));
                let i1 = vorrq_u16(vshll_high_n_u8::<8>(t0), vmovl_high_u8(nx));
                exotic = true;
                (t0, i0, i1, step)
            };
            vst1q_u16(ep.add(i), vmovl_u8(vget_low_u8(t0)));
            vst1q_u16(ep.add(i + 8), vmovl_high_u8(t0));
            let mut st = [0u16; 16];
            macro_rules! lane {
                ($v:ident, $off:literal, $($l:literal)*) => { $(
                    *st.get_unchecked_mut($off + $l) = *tbl.add(vgetq_lane_u16::<$l>($v) as usize);
                )* };
            }
            lane!(idx0, 0, 0 1 2 3 4 5 6 7);
            lane!(idx1, 8, 0 1 2 3 4 5 6 7);
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
        if vmaxvq_u8(bad) != 0 {
            return Lanes::Refused;
        }
        if vmaxvq_u16(trig) & 0x8000 != 0 {
            // A trigger: a ligature (redone by the scalar tokenizer) or an uncovered pair (refused there).
            return Lanes::Ligature;
        }
        Lanes::Done(base, exotic)
    }
}

/// The slots of a shaped text as a compact glyph run (glyph slots only, phantoms dropped): for the
/// exactness gates, which compare it with `Shaper::shape_tokens` and rustybuzz.
pub fn compact(tt: &TokenTable, x: &[i32], ent: &[u16], width: i32) -> (Vec<u16>, Vec<i32>, Vec<u32>, i32) {
    let (mut gid, mut xs, mut cl) = (Vec::new(), Vec::new(), Vec::new());
    for (i, (&e, &p)) in ent.iter().zip(x).enumerate() {
        if e == TOK_PH as u16 {
            continue;
        }
        gid.push(tt.gid[e as usize]);
        xs.push(p);
        cl.push(i as u32);
    }
    (gid, xs, cl, width)
}
