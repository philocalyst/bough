//! The line record's glyphs, in the two forms `TEXT-SEAM.md` B2 leaves open, and a reference kernel for each.
//!
//! * **Placed** ([`GlyphW`], three words per glyph): position in whole device pixels from the line's origin,
//!   the atlas texel and the box, a style index and a justification gap ordinal. Self-contained: the kernel
//!   loads nothing else per glyph. It must be produced (placed) whenever a line is mounted, re-broken or
//!   shown at another size.
//! * **Shaped** (the memo's own lanes: a pen in font units and a glyph id, 6 bytes per glyph): the kernel
//!   computes the pixel position (one multiply) and loads the glyph's slot from a per-strike table. Nothing is
//!   produced at mount, at a re-break or at another size: the line record names a range of the memo.
//!
//! The kernels emit (x, y, w, h, texel) per glyph; the blit (`cover`) is the same for both.
use crate::atlas::{GlyphCache, StrikeId};

/// A placed glyph: 12 bytes.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct GlyphW {
    pub dx: i16,
    pub dy: i16,
    pub texel: u32,
    pub w: u8,
    pub h: u8,
    pub style: u8,
    pub gap: u8,
}
const _: () = assert!(size_of::<GlyphW>() == 12);

/// A glyph of a strike: where its coverage is and how it sits on the pen. 8 bytes, indexed by glyph id.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Slot {
    pub texel: u32,
    pub left: i8,
    pub top: i8,
    pub w: u8,
    pub h: u8,
}

/// A face at a size: device pixels per font unit in Q32, and a slot per glyph id.
pub struct Strike {
    pub scale: u64,
    pub slots: Vec<Slot>,
}

impl Strike {
    /// Rasterise `gids` of `strike` (through the façade's cache: unhinted, linear coverage, whole-pixel pen).
    pub fn build(cache: &mut GlyphCache, strike: StrikeId, upem: u16, gids: impl Iterator<Item = u16>) -> Strike {
        let size = cache.strike_size(strike);
        let mut slots = Vec::new();
        for g in gids {
            let e = cache.entry(strike, g);
            if slots.len() <= g as usize {
                slots.resize(g as usize + 1, Slot::default());
            }
            slots[g as usize] = Slot { texel: e.uv[1] as u32 * cache.atlas.width + e.uv[0] as u32, left: e.geo[0] as i8, top: e.geo[1] as i8, w: e.uv[2] as u8, h: e.uv[3] as u8 };
        }
        Strike { scale: ((size as f64 / upem as f64) * 4294967296.0) as u64, slots }
    }
    /// A pen in font units as whole device pixels: the one rounding rule, in integers.
    #[inline(always)]
    pub fn px(&self, units: i32) -> i32 {
        ((units as i64 * self.scale as i64 + (1 << 31)) >> 32) as i32
    }
}

/// Place glyphs `g0..g0 + n` of the shaped lanes, relative to the pen `x0` (the line's start), into `out`.
pub fn place(st: &Strike, gid: &[u16], x: &[i32], x0: i32, style: u8, out: &mut Vec<GlyphW>) {
    for (&g, &px) in gid.iter().zip(x) {
        let s = st.slots[g as usize];
        if s.w == 0 {
            continue;
        }
        out.push(GlyphW { dx: (st.px(px - x0) + s.left as i32) as i16, dy: s.top as i16, texel: s.texel, w: s.w, h: s.h, style, gap: 0 });
    }
}

/// The placed form's kernel: every glyph's box and texel, from the glyph words alone.
#[inline(always)]
pub fn each_placed(gl: &[GlyphW], org: (i32, i32), mut f: impl FnMut(i32, i32, u32, u32, u32)) {
    for g in gl {
        f(org.0 + g.dx as i32, org.1 + g.dy as i32, g.w as u32, g.h as u32, g.texel);
    }
}

/// The shaped form's kernel: every glyph's box and texel, from the memo's lanes and the strike's slots.
#[inline(always)]
pub fn each_shaped(st: &Strike, gid: &[u16], x: &[i32], x0: i32, org: (i32, i32), mut f: impl FnMut(i32, i32, u32, u32, u32)) {
    for (&g, &px) in gid.iter().zip(x) {
        // SAFETY: a glyph id in the memo came from this face's shaper, and the strike has a slot for every
        // glyph id the shaper can produce (`Strike::build` is given them all).
        let s = unsafe { *st.slots.get_unchecked(g as usize) };
        if s.w != 0 {
            f(org.0 + st.px(px - x0) + s.left as i32, org.1 + s.top as i32, s.w as u32, s.h as u32, s.texel);
        }
    }
}

/// Lay a glyph's coverage into an R8 target (`tw` wide), cut to the target. The reference blit.
#[inline(always)]
pub fn cover(target: &mut [u8], tw: i32, atlas: &[u8], aw: u32, x: i32, y: i32, w: u32, h: u32, texel: u32) {
    let th = target.len() as i32 / tw;
    for r in 0..h as i32 {
        let ty = y + r;
        if ty < 0 || ty >= th {
            continue;
        }
        let src = (texel + r as u32 * aw) as usize;
        for c in 0..w as i32 {
            let tx = x + c;
            if tx < 0 || tx >= tw {
                continue;
            }
            let d = &mut target[(ty * tw + tx) as usize];
            *d = (*d).max(atlas[src + c as usize]);
        }
    }
}
