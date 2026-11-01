//! Repair regions: a damage `Plane` (tile bitmap) -> the rect list the stencil mask is written
//! from (horizontal tile runs merged vertically; allocation free once the Vecs are warm).
use bough_core::damage::{TILE, TILE_SHIFT};
use bough_core::Plane;

/// Rects (device px, `[x0, y0, x1, y1]`) covering every set tile, clamped to the window.
/// Returns false (and leaves a single bounding rect) if there would be more than `cap` rects.
/// Scratch for `rects_from_plane` (kept by the caller so a frame allocates nothing).
#[derive(Default)]
pub struct RectScratch {
    prev: Vec<(u32, u32, u32)>,
    cur: Vec<(u32, u32, u32)>,
}

pub fn rects_from_plane(p: &Plane, w: i32, h: i32, cap: usize, sc: &mut RectScratch, out: &mut Vec<[i32; 4]>) -> bool {
    out.clear();
    if p.bbox.is_empty() {
        return true;
    }
    let [bx0, by0, bnx1, bny1] = p.bbox.0;
    let (ty0, ty1) = ((by0 as i32 >> TILE_SHIFT) as usize, ((-(bny1 as i32) + TILE - 1) >> TILE_SHIFT) as usize);
    let (bx1, by1) = (-(bnx1 as i32), -(bny1 as i32));
    // open runs from the previous row: (tx0, tx1, first row)
    let (prev, cur) = (&mut sc.prev, &mut sc.cur);
    prev.clear();
    let emit = |out: &mut Vec<[i32; 4]>, r: (u32, u32, u32), end_row: usize| {
        out.push([(r.0 as i32 * TILE).min(w), (r.2 as i32 * TILE).min(h), (r.1 as i32 * TILE).min(w), (end_row as i32 * TILE).min(h)]);
    };
    for ty in ty0..=ty1 {
        cur.clear();
        if ty < ty1 {
            let mut bits = p.rows[ty];
            while bits != 0 {
                let s = bits.trailing_zeros();
                let run = (bits >> s).trailing_ones();
                bits &= if run >= 128 { 0 } else { !(((1u128 << run) - 1) << s) };
                cur.push((s, s + run, ty as u32));
            }
        }
        let mut pi = 0;
        for c in cur.iter_mut() {
            while pi < prev.len() && prev[pi].0 < c.0 {
                emit(out, prev[pi], ty);
                pi += 1;
            }
            if pi < prev.len() && prev[pi].0 == c.0 {
                if prev[pi].1 == c.1 {
                    c.2 = prev[pi].2;
                } else {
                    emit(out, prev[pi], ty);
                }
                pi += 1;
            }
        }
        while pi < prev.len() {
            emit(out, prev[pi], ty);
            pi += 1;
        }
        std::mem::swap(prev, cur);
    }
    out.retain(|r| r[0] < r[2] && r[1] < r[3]);
    if out.len() > cap {
        out.clear();
        out.push([bx0 as i32, by0 as i32, bx1, by1]);
        return false;
    }
    true
}
