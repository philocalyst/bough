//! Software hash framebuffer: the correctness oracle's renderer.
//!
//! Every primitive writes a hash colour. Pixel functions per kind:
//! - SOLID: `key` (opaque, uniform)
//! - TINT: `mix(below, key)` (translucent, uniform)
//! - ROUND: interior `mix(below, key)`, corner squares (r x r at each ink
//!   corner) `mix(below, key ^ h(dx, dy))` with (dx, dy) the offsets from the
//!   nearest ink edges: the AA/rounding depends on where the corner is
//! - GLYPH: `mix(below, key ^ h(x - ink.x0, y - ink.y0))`: content anchored to
//!   the ink origin, blended over what is below.
//! `mix` is order-sensitive, so any missing, extra, duplicated or reordered
//! primitive in a repair changes the pixel.

use crate::geom::R;
use crate::trace::{GLYPH, PState, ROUND, SOLID, TINT, hk};

pub const BG: u32 = 0x1234_5678;

#[inline(always)]
pub fn mix(below: u32, v: u32) -> u32 {
    (below.rotate_left(5) ^ v).wrapping_mul(0x9E37_79B1) ^ 0x5bd1_e995
}

#[derive(Clone)]
pub struct Fb {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Fb {
    pub fn new(w: i32, h: i32) -> Fb {
        Fb { w: w as usize, h: h as usize, px: vec![BG; (w * h) as usize] }
    }
    pub fn clear(&mut self) {
        self.px.fill(BG);
    }
    pub fn fill(&mut self, r: R, v: u32) {
        for y in r.y0..r.y1 {
            let row = y as usize * self.w;
            self.px[row + r.x0 as usize..row + r.x1 as usize].fill(v);
        }
    }
}

/// Per-pixel region mask (1 = in region).
pub struct Mask {
    pub w: usize,
    pub m: Vec<u8>,
}

impl Mask {
    pub fn new(w: i32, h: i32) -> Mask {
        Mask { w: w as usize, m: vec![0; (w * h) as usize] }
    }
    /// Set `r`; returns how many pixels were newly set (union area).
    pub fn set(&mut self, r: R) -> u64 {
        let mut n = 0u64;
        for y in r.y0..r.y1 {
            let row = y as usize * self.w;
            for b in &mut self.m[row + r.x0 as usize..row + r.x1 as usize] {
                n += (*b == 0) as u64;
                *b = 1;
            }
        }
        n
    }
    pub fn clear_rect(&mut self, r: R) {
        for y in r.y0..r.y1 {
            let row = y as usize * self.w;
            self.m[row + r.x0 as usize..row + r.x1 as usize].fill(0);
        }
    }
    pub fn count(&self) -> u64 {
        self.m.iter().map(|&b| b as u64).sum()
    }
}

#[inline(always)]
fn shade_row<const M: bool>(px: &mut [u32], m: &[u8], s: &PState, x0: i32, y: i32) -> u64 {
    let mut n = 0u64;
    macro_rules! each {
        (|$i:ident, $below:ident| $e:expr) => {
            for $i in 0..px.len() {
                if M && m[$i] == 0 {
                    continue;
                }
                n += 1;
                let $below = px[$i];
                px[$i] = $e;
            }
        };
    }
    match s.kind {
        SOLID => each!(|_i, _b| s.key),
        TINT => each!(|_i, b| mix(b, s.key)),
        GLYPH => {
            let ly = (y - s.ink.y0) as u32;
            let lx0 = x0 - s.ink.x0;
            each!(|i, b| mix(b, s.key ^ hk((lx0 + i as i32) as u32, ly)))
        }
        ROUND => {
            let (ink, r) = (s.ink, s.rad as i32);
            let dy = if y < ink.y0 + r {
                Some(y - ink.y0)
            } else if y >= ink.y1 - r {
                Some(ink.y1 - 1 - y)
            } else {
                None
            };
            match dy {
                None => each!(|_i, b| mix(b, s.key)),
                Some(dy) => each!(|i, b| {
                    let x = x0 + i as i32;
                    let dx = if x < ink.x0 + r {
                        x - ink.x0
                    } else if x >= ink.x1 - r {
                        ink.x1 - 1 - x
                    } else {
                        -1
                    };
                    if dx < 0 { mix(b, s.key) } else { mix(b, s.key ^ hk(dx as u32, dy as u32 | 0x8000_0000)) }
                }),
            }
        }
        _ => unreachable!(),
    }
    n
}

/// Draw `s` within `s.cov ∩ clip`, only where `mask` is set (if given).
/// Returns the number of fragments shaded.
pub fn draw(fb: &mut Fb, s: &PState, clip: R, mask: Option<&Mask>) -> u64 {
    let c = s.cov.inter(clip);
    if c.is_empty() {
        return 0;
    }
    let mut n = 0u64;
    let w = fb.w;
    for y in c.y0..c.y1 {
        let (a, b) = (y as usize * w + c.x0 as usize, y as usize * w + c.x1 as usize);
        n += match mask {
            None => shade_row::<false>(&mut fb.px[a..b], &[], s, c.x0, y),
            Some(m) => shade_row::<true>(&mut fb.px[a..b], &m.m[a..b], s, c.x0, y),
        };
    }
    n
}

pub fn render_full(fb: &mut Fb, st: &[PState]) {
    fb.clear();
    let vp = R::new(0, 0, fb.w as i32, fb.h as i32);
    for s in st {
        if !s.cov.is_empty() {
            draw(fb, s, vp, None);
        }
    }
}
