//! The baked face image (`stage::bake`, read by `bough_face`) against what it was made from: the table shaper
//! (itself held to a full OpenType shaper by `tests/exact.rs`) and the rasteriser.
//!
//! Needs a font: `BOUGH_FONT`, else DejaVu Sans where the twin tests find it, else the test is skipped with a
//! note (a gate that cannot run says so, it does not pass silently).
use bough_face::{Face, Glyphs, num::Num};
use bough_text::atlas::GlyphCache;
use bough_text::stage::{bake, digits};
use bough_text::{GlyphRun, Shaper, SimpleScript};

fn font() -> Option<Vec<u8>> {
    let dir = std::env::var("BOUGH_DEJAVU_DIR").unwrap_or_else(|_| "/Library/Fonts/Nix Fonts/vl9mbzk97wmh1s1yj3766b4zai8msjq6-dejavu-fonts-2.37/share/fonts/truetype".into());
    let cands = [std::env::var("BOUGH_FONT").ok(), Some(format!("{dir}/DejaVuSans.ttf")), Some("/System/Library/Fonts/Supplemental/Arial.ttf".into())];
    cands.into_iter().flatten().find_map(|p| std::fs::read(p).ok())
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Strings that exercise what the image models: ASCII, Latin-1, ligature sequences, and what it must refuse.
fn strings() -> Vec<String> {
    let mut v: Vec<String> = ["", " ", "Row 00042", "12 rows \u{b7} tick 7", "office fluffy affine fjord", "fi", "ffi", "f", "if", "AVATAR To. Ty,", "na\u{ef}ve caf\u{e9} \u{c5}ngstr\u{f6}m", "tab\there", "\u{4e2d}\u{6587}", "emoji \u{1f600}", "nbsp\u{a0}x", "c1 \u{85}"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut r = Rng(0x1234_5678_9abc_def1);
    let pool: Vec<char> = (0x20u32..0x7f).chain(0xa0..0x100).filter_map(char::from_u32).collect();
    for _ in 0..4000 {
        let n = r.below(40);
        v.push((0..n).map(|_| if r.below(8) == 0 { ['f', 'i', 'l', 'f'][r.below(4)] } else { pool[r.below(pool.len())] }).collect());
    }
    v
}

#[test]
fn image_is_the_shaper_and_the_rasteriser() {
    let Some(bytes) = font() else {
        eprintln!("SKIPPED: no font (set BOUGH_FONT)");
        return;
    };
    let sc = SimpleScript::build(&bytes, 0).expect("simple-script table");
    let sh = Shaper::new(sc);
    let tt = sh.script.tokens.as_ref().expect("token model");
    for size in [14.0f32, 26.0] {
        let img: &'static [u8] = Box::leak(bake::bake(&bytes, size).expect("bake").into_boxed_slice());
        let face = Face::new(img);
        assert_eq!(face.size(), size);
        let mut cache = GlyphCache::new(&bytes);
        let strike = cache.strike(size);
        let mut run = GlyphRun::default();
        let (mut covered, mut refused) = (0, 0);
        for s in strings() {
            let ok = sh.shape_into(&s, size, &mut run);
            let (mut toks, mut pens, mut at) = (Vec::new(), Vec::new(), Vec::new());
            let w = face.shape(s.as_bytes(), |t, x, i| {
                toks.push(t);
                pens.push(x);
                at.push(i);
            });
            assert_eq!(w.is_some(), ok, "covered: {s:?}");
            let Some(w) = w else {
                refused += 1;
                continue;
            };
            covered += 1;
            assert_eq!(w, run.width, "{s:?}");
            assert_eq!(pens, run.x, "{s:?}");
            assert_eq!(toks.iter().map(|&t| tt.gid[t as usize]).collect::<Vec<_>>(), run.gid, "{s:?}");
            for (k, &i) in at.iter().enumerate() {
                assert_eq!(i, run.cluster_of(k), "{s:?}");
            }
            // every glyph's coverage and placement are the rasteriser's
            for &t in &toks {
                let e = cache.entry(strike, tt.gid[t as usize]);
                let sl = face.slot(t as u16);
                assert_eq!((sl.left, sl.top, sl.w, sl.h), (e.geo[0] as i32, e.geo[1] as i32, e.uv[2] as u32, e.uv[3] as u32));
                let aw = cache.atlas.width as usize;
                for r in 0..sl.h {
                    let a = (e.uv[1] as usize + r as usize) * aw + e.uv[0] as usize;
                    assert_eq!(face.row(sl, r), &cache.atlas.pixels[a..a + sl.w as usize]);
                }
            }
        }
        assert!(covered > 3000 && refused >= 4, "{covered} covered, {refused} refused");
        // the digit cell is the font's, and a number's glyphs and pens are the shaper's where it does not kern
        assert_eq!(face.cell(), digits::cell_of(&sh.script).ok());
        if let Some(c) = face.cell() {
            let mut s = String::new();
            for (v, d, pad) in [(0, 0, 0), (42, 0, 5), (-1234567, 2, 0), (99999, 3, 0), (7, 2, 0), (i32::MAX, 0, 0)] {
                digits::format(v, d, pad, &mut s);
                let num = Num::new(v, d, pad);
                let mut pen = 0;
                for (g, &ch) in s.as_bytes().iter().enumerate() {
                    let k = num.count() - 1 - g as u32;
                    assert_eq!((num.glyph(k), num.pen(&c, k)), (ch, pen), "{s}");
                    pen += sh.script.adv[ch as usize] as i32;
                }
                assert_eq!(num.width(&c), pen);
            }
        }
    }
}

/// The spans of a line through the memo are the glyphs' coverage where the rounding rule puts them, cut to
/// the clip; asking again finds the same run.
#[test]
fn lines_are_stable_and_spans_cover_what_the_glyphs_cover() {
    let Some(bytes) = font() else {
        eprintln!("SKIPPED: no font (set BOUGH_FONT)");
        return;
    };
    let face = Face::new(Box::leak(bake::bake(&bytes, 14.0).expect("bake").into_boxed_slice()));
    let mut g = Glyphs::new();
    let texts: Vec<String> = (0..3000).map(|i| format!("quick amber river {i}")).collect();
    let runs: Vec<_> = texts.iter().map(|t| g.line(0, &face, t).expect("covered")).collect();
    for (t, r) in texts.iter().zip(&runs) {
        assert_eq!(g.line(0, &face, t).unwrap(), *r);
    }
    assert_eq!((g.shaped, g.found), (3000, 3000));
    assert!(g.line(0, &face, "\u{4e2d}").is_none());
    assert_eq!(g.lanes().0.len(), texts.iter().map(|t| t.len()).sum::<usize>(), "a refused line leaves nothing behind");
    // paint one line into a small target twice: whole, and tile by tile; both equal a per-glyph reference
    let (w, h) = (200i32, 24i32);
    let (run, _) = runs[1234];
    let (slot, x) = g.of(run);
    let org = (3, face.baseline(h));
    let mut whole = vec![0u8; (w * h) as usize];
    face.spans(slot, x, 0, org, [0, 0, w, h], |px, py, c| whole[(py * w + px) as usize..][..c.len()].copy_from_slice(c));
    let mut tiled = vec![0u8; (w * h) as usize];
    for ty in (0..h).step_by(7) {
        for tx in (0..w).step_by(13) {
            face.spans(slot, x, 0, org, [tx, ty, (tx + 13).min(w), (ty + 7).min(h)], |px, py, c| tiled[(py * w + px) as usize..][..c.len()].copy_from_slice(c));
        }
    }
    let mut reference = vec![0u8; (w * h) as usize];
    for (&t, &pen) in slot.iter().zip(x) {
        let s = face.slot(t);
        for r in 0..s.h as i32 {
            let cov = face.row(s, r as u32);
            for c in 0..s.w as i32 {
                let (px, py) = (org.0 + face.px(pen) + s.left + c, org.1 + s.top + r);
                if (0..w).contains(&px) && (0..h).contains(&py) {
                    reference[(py * w + px) as usize] = cov[c as usize];
                }
            }
        }
    }
    assert!(reference.iter().any(|&c| c == 255) && reference.iter().filter(|&&c| c != 0).count() > 300);
    assert_eq!(whole, reference);
    assert_eq!(tiled, reference);
    // the expansion into one box per glyph (what a glyph-record executor draws: texels read from the atlas by
    // index and stride) inks exactly what the line inks, cut to a box smaller than the line included
    for cut in [[0, 0, w, h], [10, 6, 150, 17], [0, 0, 1, 1]] {
        let mut by_line = vec![0u8; (w * h) as usize];
        face.spans(slot, x, 0, org, cut, |px, py, c| by_line[(py * w + px) as usize..][..c.len()].copy_from_slice(c));
        let mut by_glyph = vec![0u8; (w * h) as usize];
        let (atlas, stride) = (face.atlas(), face.stride());
        let mut boxes = 0;
        face.expand(slot, x, 0, org, cut, |b, texel| {
            boxes += 1;
            for py in b[1]..b[3] {
                for px in b[0]..b[2] {
                    by_glyph[(py * w + px) as usize] = atlas[texel as usize + (py - b[1]) as usize * stride + (px - b[0]) as usize];
                }
            }
        });
        assert_eq!(by_glyph, by_line, "cut {cut:?}");
        assert!(boxes <= slot.len());
    }
}
