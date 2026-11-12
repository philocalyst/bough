//! The gate: the prepared image places and covers exactly as the real shaper and rasteriser do.
//!
//! Needs the face the image was baked from: `BOUGH_GLYPH_FONT=/path/to/DejaVuSans.ttf` (2.37, sha256 7da195a7..;
//! `data/README.md`). Without it the test says so and passes vacuously, so the workspace suite does not depend on
//! a font file; the lane's gate runs set it.
//!
//! Negative control: `BOUGH_GLYPH_MUTANT=kern` drops the kerning (the placement must then differ from the
//! shaper's on some string and the test must fail); `=cov` flips one coverage byte.
use bough_glyph::{DEJAVU_SANS_14 as IMG, NONE};
use swash::scale::{Render, ScaleContext, Source};
use swash::shape::ShapeContext;
use swash::text::Script;
use swash::zeno::Format;
use swash::FontRef;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn placement_and_coverage_equal_the_shaper() {
    let Ok(path) = std::env::var("BOUGH_GLYPH_FONT") else {
        eprintln!("BOUGH_GLYPH_FONT not set: the glyph image is not checked against the shaper in this run");
        return;
    };
    let mutant = std::env::var("BOUGH_GLYPH_MUTANT").unwrap_or_default();
    let data = std::fs::read(path).unwrap();
    let font = FontRef::from_index(&data, 0).unwrap();
    let upem = font.metrics(&[]).units_per_em as f32;
    assert_eq!(upem as u32, IMG.upem);
    let mut cx = ShapeContext::new();
    let chars: Vec<char> = (0x20u32..0x7f).chain(0xa0..0x100).filter_map(char::from_u32).filter(|&c| IMG.index(c as u32) != NONE).collect();
    assert_eq!(chars.len(), 190);
    let words = ["office", "waffle", "fifty", "AVATAR", "To Wy", "Row 00042", "1000 rows \u{b7} tick 7", "inexpensive purple sandwich !!!", "ff", "ffi", "fflfi", "f f", "T\u{e9}l\u{e9}"];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut strings: Vec<String> = words.iter().map(|s| s.to_string()).collect();
    for _ in 0..4000 {
        let n = 1 + rng.next() % 24;
        strings.push((0..n).map(|_| if rng.next() % 5 == 0 { ['f', 'i', 'l', 'A', 'V', 'T', 'o', ' '][(rng.next() % 8) as usize] } else { chars[(rng.next() % chars.len() as u64) as usize] }).collect());
    }
    let (mut glyphs, mut kerned, mut ligs) = (0usize, 0usize, 0usize);
    for s in &strings {
        let mut want: Vec<(u16, i32)> = Vec::new();
        let mut sh = cx.builder(font).script(Script::Latin).size(upem).build();
        sh.add_str(s);
        let mut pen = 0f32;
        sh.shape_with(|c| {
            for g in c.glyphs {
                assert!(g.x == 0.0 && g.y == 0.0, "an offset glyph in {s:?}");
                want.push((g.id, pen.round() as i32));
                pen += g.advance;
            }
        });
        let mut got: Vec<(u16, i32)> = Vec::new();
        let mut plain = 0;
        let adv = IMG.place(s.as_bytes(), |g, x| {
            got.push((IMG.gid(g), if mutant == "kern" { plain } else { x }));
            plain += IMG.glyph(g).adv;
        });
        assert!(IMG.covers(s.as_bytes()));
        assert_eq!(got, want, "placement of {s:?}");
        assert_eq!(adv, pen.round() as i32, "advance of {s:?}");
        glyphs += got.len();
        ligs += s.chars().count() - got.len();
        kerned += (plain != adv) as usize;
    }
    assert!(ligs > 100 && kerned > 500, "the strings exercise ligatures ({ligs}) and kerning ({kerned})");
    // coverage: every glyph of the image against the rasteriser
    let mut scx = ScaleContext::new();
    let mut sc = scx.builder(font).size(IMG.size).hint(false).build();
    for i in 0..IMG.len() as u8 {
        let g = IMG.glyph(i);
        let img = Render::new(&[Source::Outline]).format(Format::Alpha).render(&mut sc, IMG.gid(i)).unwrap();
        assert_eq!((g.left, g.top, g.w, g.h), (img.placement.left, img.placement.top, img.placement.width, img.placement.height), "box of glyph {i}");
        let mut cov = g.cov.to_vec();
        if mutant == "cov" && i == 40 && !cov.is_empty() {
            cov[0] ^= 1;
        }
        assert_eq!(cov, img.data, "coverage of glyph {i}");
    }
    eprintln!("{} strings, {glyphs} glyphs placed, {ligs} characters folded into ligatures, {kerned} strings kerned; {} glyphs' coverage equal", strings.len(), IMG.len());
}

#[test]
fn snap_rounds_half_up() {
    // 14 px at 2048 units per em: 1 px is 146.29 units
    assert_eq!(IMG.snap(0), 0);
    assert_eq!(IMG.snap(73), 0);
    assert_eq!(IMG.snap(74), 1);
    assert_eq!(IMG.snap(2048), 14);
    assert_eq!(IMG.snap(-74), -1);
}
