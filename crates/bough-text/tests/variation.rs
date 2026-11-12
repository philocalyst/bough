//! Round 3b gate: a variable font's instance (SF NS, `wght` 400 / 700, `opsz` at text_bench's 14 px) shaped by
//! the table equals rustybuzz with the same variation coordinates: every string of up to two covered characters,
//! text_bench's whole vocabulary in sentences, and random ASCII strings. Eligibility is reported (strings the
//! table refuses go to the full shaper).
use bough_text::{GlyphRun, Shaper, SimpleScript};

fn check(path: &str, coords: &[([u8; 4], f32)]) -> (u64, u64) {
    let Ok(data) = std::fs::read(path) else { return (0, 0) };
    let sh = Shaper::new(SimpleScript::build_instance(&data, 0, coords).expect("instance"));
    let mut face = rustybuzz::Face::from_slice(&data, 0).unwrap();
    for (t, v) in coords {
        face.set_variations(&[rustybuzz::Variation { tag: rustybuzz::ttf_parser::Tag::from_bytes(t), value: *v }]);
    }
    let mut vars: Vec<rustybuzz::Variation> = coords.iter().map(|(t, v)| rustybuzz::Variation { tag: rustybuzz::ttf_parser::Tag::from_bytes(t), value: *v }).collect();
    face.set_variations(&vars);
    vars.clear();
    let mut run = GlyphRun::default();
    let (mut ok, mut refused) = (0u64, 0u64);
    let mut test = |s: &str, run: &mut GlyphRun| {
        if !sh.shape_into(s, 14.0, run) {
            refused += 1;
            return;
        }
        ok += 1;
        let mut b = rustybuzz::UnicodeBuffer::new();
        b.push_str(s);
        b.guess_segment_properties();
        let g = rustybuzz::shape(&face, &[], b);
        assert_eq!(g.glyph_infos().len(), run.gid.len(), "{s:?} at {coords:?}");
        let mut pen = 0;
        for (k, (i, p)) in g.glyph_infos().iter().zip(g.glyph_positions()).enumerate() {
            assert_eq!((run.gid[k] as u32, run.cluster_of(k), run.x[k]), (i.glyph_id, i.cluster, pen), "{s:?} glyph {k} at {coords:?}");
            assert_eq!((p.y_advance, p.x_offset, p.y_offset), (0, 0, 0), "{s:?} at {coords:?}");
            pen += p.x_advance;
        }
        assert_eq!(run.width, pen, "{s:?} at {coords:?}");
    };
    let cps: Vec<char> = (0x20u32..0x7F).chain(0xA0..0x100).filter(|&c| c != 0xAD).map(|c| char::from_u32(c).unwrap()).collect();
    let mut s = String::new();
    for &a in &cps {
        for &b in &cps {
            s.clear();
            s.push(a);
            s.push(b);
            test(&s, &mut run);
        }
    }
    let words = ["the", "renderer", "keeps", "every", "glyph", "retained", "while", "streaming", "tokens", "arrive", "layout", "measures", "paragraphs", "at", "two", "widths", "per", "pass", "and", "virtualized", "transcripts", "need", "exact", "heights", "before", "mounting", "views", "fonts", "shape", "once", "wrapping", "is", "arithmetic", "over", "cached", "advances"];
    for i in 0..2000 {
        let t: Vec<&str> = (0..(1 + i % 9)).map(|k| words[(i * 7 + k * 13) % words.len()]).collect();
        test(&(t.join(" ") + " "), &mut run);
    }
    let mut seed = 0x9E37_79B9u64;
    for _ in 0..20_000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let n = (seed >> 58) as usize + 1;
        s.clear();
        for k in 0..n {
            s.push((b' ' + ((seed >> (k % 50)) as u8 % 95)) as char);
        }
        test(&s, &mut run);
    }
    println!("{path} {coords:?}: {ok} equal, {refused} refused");
    (ok, refused)
}

#[test]
fn sf_ns_instances_equal_rustybuzz() {
    for c in [&[(*b"wght", 400.0), (*b"opsz", 17.0)][..], &[(*b"wght", 700.0), (*b"opsz", 17.0)][..], &[][..]] {
        let (ok, _) = check("/System/Library/Fonts/SFNS.ttf", c);
        assert!(ok > 0 || !std::path::Path::new("/System/Library/Fonts/SFNS.ttf").exists());
    }
}

/// The same gate on another variable font with `wght` and `opsz` axes (`BOUGH_VARIABLE_FONT`, e.g. Inter 4
/// InterVariable.ttf), for platforms without SF NS. Skipped when the variable is unset; required when it is set.
#[test]
fn other_variable_font_instances_equal_rustybuzz() {
    let Ok(path) = std::env::var("BOUGH_VARIABLE_FONT") else { return };
    for c in [&[(*b"wght", 400.0), (*b"opsz", 17.0)][..], &[(*b"wght", 700.0), (*b"opsz", 17.0)][..], &[][..]] {
        let (ok, _) = check(&path, c);
        assert!(ok > 0, "{path}: no string shaped by the table");
    }
}
