//! Bake a glyph image: `cargo run --release -p bough-glyph --example bake -- FONT.ttf SIZE_PX OUT.bgi`.
//!
//! The shaper and the rasteriser are swash's (unhinted alpha coverage at whole-pixel positions), as in
//! `bough-text`. Advances and kerning are taken in font units by shaping at `size = units per em`; the kerning
//! of a pair is the first glyph's advance inside the pair minus its advance alone, so whatever the font does
//! between two glyphs (GPOS pairs, classes, the legacy `kern` table) is in the table. A pair or triple that the
//! shaper turns into one glyph is a ligature and gets a glyph of its own; a pair it changes in any other way is
//! reported and makes the bake fail, because the image could not reproduce it.
use std::collections::BTreeMap;
use swash::scale::{Render, ScaleContext, Source};
use swash::shape::ShapeContext;
use swash::text::Script;
use swash::zeno::Format;
use swash::FontRef;

fn shape(cx: &mut ShapeContext, font: FontRef, size: f32, s: &str) -> Vec<(u16, f32, f32)> {
    let mut sh = cx.builder(font).script(Script::Latin).size(size).build();
    sh.add_str(s);
    let mut out = Vec::new();
    sh.shape_with(|c| {
        for g in c.glyphs {
            out.push((g.id, g.advance, g.x));
        }
    });
    out
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (path, size, out) = (&a[1], a[2].parse::<f32>().unwrap(), &a[3]);
    let data = std::fs::read(path).unwrap();
    let font = FontRef::from_index(&data, 0).unwrap();
    let m = font.metrics(&[]);
    let upem = m.units_per_em as f32;
    let mut cx = ShapeContext::new();
    let chars: Vec<char> = (0x20u32..0x7f).chain(0xa0..0x100).filter_map(char::from_u32).collect();

    // entries: (source text, glyph id, advance in units)
    let mut ents: Vec<(String, u16, i32)> = Vec::new();
    let mut cmap = [0xffu8; 256];
    for &c in &chars {
        let g = shape(&mut cx, font, upem, &c.to_string());
        if g.len() != 1 || g[0].0 == 0 {
            continue; // the face has no single glyph for it (the soft hyphen shapes to nothing)
        }
        cmap[c as usize] = ents.len() as u8;
        ents.push((c.to_string(), g[0].0, g[0].1.round() as i32));
    }
    let nchar = ents.len();
    // ligatures of two, then of three starting with a pair that ligates
    let mut ligs: Vec<(String, u8)> = Vec::new();
    let mut odd = Vec::new();
    let single = |ents: &Vec<(String, u16, i32)>, s: &str| ents.iter().position(|e| e.0 == s).unwrap();
    let mut pairs_lig = Vec::new();
    for i in 0..nchar {
        for j in 0..nchar {
            let s = format!("{}{}", ents[i].0, ents[j].0);
            let g = shape(&mut cx, font, upem, &s);
            if g.len() == 1 {
                pairs_lig.push((i, j, g[0]));
            } else if g.len() != 2 || g[0].0 != ents[i].1 || g[1].0 != ents[j].1 || g[0].2 != 0.0 || g[1].2 != 0.0 {
                odd.push(s);
            }
        }
    }
    let mut seqs: Vec<(String, (u16, f32, f32))> = pairs_lig.iter().map(|&(i, j, g)| (format!("{}{}", ents[i].0, ents[j].0), g)).collect();
    for &(i, j, _) in &pairs_lig {
        for k in 0..nchar {
            let s = format!("{}{}{}", ents[i].0, ents[j].0, ents[k].0);
            let g = shape(&mut cx, font, upem, &s);
            if g.len() == 1 {
                seqs.push((s, g[0]));
            }
        }
    }
    // longest first: the placer takes the first match
    seqs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
    for (s, g) in &seqs {
        assert!(s.is_ascii() && s.len() <= 4, "ligature {s:?} is not short ASCII");
        ligs.push((s.clone(), ents.len() as u8));
        ents.push((s.clone(), g.0, g.1.round() as i32));
    }
    assert!(ents.len() < 255, "too many glyphs for a byte index");
    let n = ents.len();
    // kerning between every two entries: the advance of the first inside the pair minus its advance alone
    let mut kern = vec![0i16; n * n];
    let mut nk = 0;
    for i in 0..n {
        for j in 0..n {
            let s = format!("{}{}", ents[i].0, ents[j].0);
            let g = shape(&mut cx, font, upem, &s);
            if g.len() == 2 && g[0].0 == ents[i].1 && g[1].0 == ents[j].1 {
                let k = g[0].1.round() as i32 - ents[i].2;
                kern[i * n + j] = k as i16;
                nk += (k != 0) as usize;
            } else if i < nchar && j < nchar && g.len() != 1 {
                // already in `odd`
            }
        }
    }
    let _ = single;
    if !odd.is_empty() {
        eprintln!("{} pairs are shaped in a way the image cannot hold, e.g. {:?}", odd.len(), &odd[..odd.len().min(8)]);
        std::process::exit(1);
    }
    // coverage
    let mut scx = ScaleContext::new();
    let mut sc = scx.builder(font).size(size).hint(false).build();
    let mut cov: Vec<u8> = Vec::new();
    let mut memo: BTreeMap<u16, (i16, i16, i16, i16, u32)> = BTreeMap::new();
    let mut grecs = Vec::new();
    for e in &ents {
        let r = *memo.entry(e.1).or_insert_with(|| {
            let img = Render::new(&[Source::Outline]).format(Format::Alpha).render(&mut sc, e.1).expect("glyph renders");
            let off = cov.len() as u32;
            cov.extend_from_slice(&img.data);
            assert!(img.data.len() == (img.placement.width * img.placement.height) as usize);
            (img.placement.left as i16, img.placement.top as i16, img.placement.width as i16, img.placement.height as i16, off)
        });
        grecs.push((e.2, r, e.1));
    }
    let mut b: Vec<u8> = Vec::new();
    let w32 = |b: &mut Vec<u8>, v: u32| b.extend_from_slice(&v.to_le_bytes());
    w32(&mut b, 0x3149_4742);
    w32(&mut b, size.to_bits());
    w32(&mut b, m.units_per_em as u32);
    w32(&mut b, m.ascent.round() as i32 as u32);
    w32(&mut b, m.descent.round() as i32 as u32);
    w32(&mut b, m.leading.round() as i32 as u32);
    w32(&mut b, 0);
    w32(&mut b, n as u32);
    w32(&mut b, ligs.len() as u32);
    w32(&mut b, cov.len() as u32);
    b.extend_from_slice(&cmap);
    for (adv, (l, t, w, h, off), gid) in &grecs {
        b.extend_from_slice(&adv.to_le_bytes());
        for v in [l, t, w, h] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&off.to_le_bytes());
        b.extend_from_slice(&gid.to_le_bytes());
        b.extend_from_slice(&[0, 0]);
    }
    for k in &kern {
        b.extend_from_slice(&k.to_le_bytes());
    }
    for (s, g) in &ligs {
        let mut r = [0u8; 8];
        r[0] = s.len() as u8;
        r[1..1 + s.len()].copy_from_slice(s.as_bytes());
        r[5] = *g;
        b.extend_from_slice(&r);
    }
    b.extend_from_slice(&cov);
    std::fs::write(out, &b).unwrap();
    println!("{out}: {} bytes; {} characters, {} ligatures {:?}, {} kerned pairs, {} coverage bytes; upem {}, ascent {}, descent {}, gap {}", b.len(), nchar, ligs.len(), ligs.iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), nk, cov.len(), m.units_per_em, m.ascent, m.descent, m.leading);
}
