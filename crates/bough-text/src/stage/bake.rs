//! The build step: a face at a size as a `bough_face` image.
//!
//! Everything the first frame needs of a font, made before the program starts: the table shaper's token model
//! (its pair steps, deduplicated into row and column classes; its ligatures), the coverage of every token's
//! glyph at the size (the façade's rasteriser: unhinted outlines, linear coverage, whole-pixel pens), the
//! vertical metrics and the digit cell. A program holds the bytes as a `static` (`include_bytes!` behind an
//! alignment wrapper is not needed: the image is read bytewise) and `Face::new` is a `const fn`, so opening a
//! font at run time is no work at all: no file, no parse, no shaper.
//!
//! `tests/face.rs` holds the image to the shaper and the rasteriser it was made from.
use crate::atlas::GlyphCache;
use crate::script::SimpleScript;
use crate::stage::digits;
use std::collections::HashMap;

/// Why a face cannot be baked.
#[derive(Debug)]
pub enum NoImage {
    /// The face has no token model (its live rules are not all pair steps and ligatures): it is the full
    /// shaper's.
    NoTokenModel,
    /// A glyph at this size does not fit a slot (a box over 255 pixels, or an offset beyond 127).
    TooLarge,
    /// A ligature of more than five bytes.
    LongLigature,
}

/// The image of `font` (face 0) at `size_px` pixels per em.
pub fn bake(font: &[u8], size_px: f32) -> Result<Vec<u8>, NoImage> {
    let sc = SimpleScript::build_cached(font, 0).ok_or(NoImage::NoTokenModel)?;
    let tt = sc.tokens.as_ref().ok_or(NoImage::NoTokenModel)?;
    let tf = ttf_parser::Face::parse(font, 0).map_err(|_| NoImage::NoTokenModel)?;

    // pair steps, by classes: tokens with the same row share a row class, then the same for columns
    let mut row_of = [0u8; 256];
    let mut rows: Vec<&[u16]> = Vec::new();
    let mut seen: HashMap<&[u16], u8> = HashMap::new();
    for t in 0..256 {
        let r = &tt.step[t << 8..(t + 1) << 8];
        row_of[t] = *seen.entry(r).or_insert_with(|| {
            rows.push(r);
            (rows.len() - 1) as u8
        });
    }
    let mut col_of = [0u8; 256];
    let mut cols: Vec<Vec<u16>> = Vec::new();
    let mut seen: HashMap<Vec<u16>, u8> = HashMap::new();
    for u in 0..256 {
        let c: Vec<u16> = rows.iter().map(|r| r[u]).collect();
        col_of[u] = *seen.entry(c.clone()).or_insert_with(|| {
            cols.push(c);
            (cols.len() - 1) as u8
        });
    }

    // coverage of every token that has a glyph
    let mut cache = GlyphCache::new(font);
    let strike = cache.strike(size_px);
    let mut slots = vec![0u8; 256 * 8];
    let mut cov: Vec<u8> = Vec::new();
    for t in 2..256usize {
        // a token is live if some pair starting with it has a step
        if tt.step[t << 8..(t + 1) << 8].iter().all(|&s| s == 0xffff) {
            continue;
        }
        let e = cache.entry(strike, tt.gid[t]);
        let (left, top, w, h) = (e.geo[0] as i32, e.geo[1] as i32, e.uv[2] as usize, e.uv[3] as usize);
        if w > 255 || h > 255 || !(-128..128).contains(&left) || !(-128..128).contains(&top) {
            return Err(NoImage::TooLarge);
        }
        let at = t * 8;
        slots[at..at + 4].copy_from_slice(&(cov.len() as u32).to_le_bytes());
        slots[at + 4] = left as i8 as u8;
        slots[at + 5] = top as i8 as u8;
        slots[at + 6] = w as u8;
        slots[at + 7] = h as u8;
        let aw = cache.atlas.width as usize;
        for r in 0..h {
            let s = (e.uv[1] as usize + r) * aw + e.uv[0] as usize;
            cov.extend_from_slice(&cache.atlas.pixels[s..s + w]);
        }
    }

    let mut lig_first = [0u8; 16];
    let mut ligs: Vec<u8> = Vec::new();
    for (first, rest, tok) in &tt.ligs {
        if rest.len() > 4 {
            return Err(NoImage::LongLigature);
        }
        lig_first[*first as usize >> 3] |= 1 << (first & 7);
        let mut l = [0u8; 8];
        l[0] = *first;
        l[1] = rest.len() as u8;
        l[2..2 + rest.len()].copy_from_slice(rest);
        l[6] = *tok;
        ligs.extend_from_slice(&l);
    }

    let size_q6 = (size_px * 64.0).round() as u32;
    let scale = (((size_q6 as u128) << 32) / (64 * sc.upem as u128)) as u64;
    let mut b = Vec::with_capacity(4096 + rows.len() * cols.len() * 2 + cov.len());
    b.extend_from_slice(&bough_face::MAGIC.to_le_bytes());
    b.extend_from_slice(&size_q6.to_le_bytes());
    b.extend_from_slice(&scale.to_le_bytes());
    b.extend_from_slice(&(sc.upem as u32).to_le_bytes());
    b.extend_from_slice(&(tf.ascender() as i32).to_le_bytes());
    b.extend_from_slice(&(-(tf.descender() as i32)).to_le_bytes());
    b.extend_from_slice(&(tf.line_gap() as i32).to_le_bytes());
    b.extend_from_slice(&(rows.len() as u16).to_le_bytes());
    b.extend_from_slice(&(cols.len() as u16).to_le_bytes());
    b.extend_from_slice(&(tt.ligs.len() as u32).to_le_bytes());
    match digits::cell_of(&sc) {
        Ok(c) => {
            for v in [c.adv, c.dot, c.minus, c.kern] {
                b.extend_from_slice(&(v as u16).to_le_bytes());
            }
        }
        Err(_) => b.extend_from_slice(&[0xff; 8]),
    }
    b.extend_from_slice(&(cov.len() as u32).to_le_bytes());
    b.extend_from_slice(&[0; 12]);
    debug_assert_eq!(b.len(), 64);
    b.extend_from_slice(&row_of);
    b.extend_from_slice(&col_of);
    b.extend_from_slice(&slots);
    b.extend_from_slice(&lig_first);
    b.extend_from_slice(&ligs);
    // steps[row class][column class]
    for (ri, _) in rows.iter().enumerate() {
        for c in &cols {
            b.extend_from_slice(&c[ri].to_le_bytes());
        }
    }
    b.extend_from_slice(&cov);
    Ok(b)
}
