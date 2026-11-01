//! "No regression" for the parley path, defined before any graft: the vendored parley (with every graft) lays out
//! exactly like unmodified upstream parley 0.11.1 on the corpora bough-text's own gates used (`exact`, `slots`,
//! `lines`, `para`, `variation`), plus the inputs the grafts must refuse (bidi, CJK, emoji, marks, controls,
//! features, spacing, locales). See `common/twin.rs` for what a dump holds.
#[path = "../examples/common/corpus.rs"]
mod corpus;
mod common {
    pub mod twin;
}
use common::twin::{dejavu, Case, Prop, Twin};

const SANS: &str = "DejaVu Sans";
const MONO: &str = "DejaVu Sans Mono";

/// The table's covered set: ASCII 0x20..=0x7E and Latin-1 0xA0..=0xFF without U+00AD.
fn covered() -> Vec<char> {
    (0x20u32..=0x7E).chain((0xA0..=0xFF).filter(|&c| c != 0xAD)).map(|c| char::from_u32(c).unwrap()).collect()
}

/// Every string of one or two covered characters (36,290), unwrapped, at 14 px.
#[test]
fn exhaustive_pairs() {
    let mut t = Twin::new(&dejavu(), false);
    let cs = covered();
    let mut s = String::new();
    for &a in &cs {
        s.clear();
        s.push(a);
        t.check(&Case::plain(&s, SANS, 14.0, None));
        for &b in &cs {
            s.truncate(a.len_utf8());
            s.push(b);
            t.check(&Case::plain(&s, SANS, 14.0, None));
        }
    }
    let [h, f] = bough_text::parley::bough::segments();
    eprintln!("exhaustive pairs: {} cases equal; font segments: {f} by the table, {h} by harfrust", t.cases);
    assert!(f > 2 * h, "the table must shape most covered pairs");
}

/// Every string of up to five characters over an alphabet of ligature letters, break classes and Latin-1, at two
/// widths (unwrapped and 1 px, where every opportunity breaks).
#[test]
fn exhaustive_short_breaks() {
    let mut t = Twin::new(&dejavu(), false);
    let alpha: Vec<char> = "fil T1,.-/'(é%".chars().collect();
    let mut idx = vec![0usize; 5];
    let mut s = String::new();
    for len in 1..=4 {
        idx.iter_mut().for_each(|i| *i = 0);
        loop {
            s.clear();
            for &i in &idx[..len] {
                s.push(alpha[i]);
            }
            t.check(&Case::plain(&s, SANS, 14.0, None));
            t.check(&Case::plain(&s, SANS, 14.0, Some(1.0)));
            let mut k = 0;
            while k < len {
                idx[k] += 1;
                if idx[k] < alpha.len() {
                    break;
                }
                idx[k] = 0;
                k += 1;
            }
            if k == len {
                break;
            }
        }
    }
    eprintln!("short strings: {} cases equal", t.cases);
}

/// Every workload string bough's scenes draw (heavy, lab, vlist: labels, numbers, log lines, paragraphs) at the
/// sizes they use, unwrapped and wrapped at 120 / 300 px.
#[test]
fn workload_strings() {
    let mut t = Twin::new(&dejavu(), false);
    let mut all = corpus::all_workload_strings();
    all.sort();
    all.dedup();
    for (k, s) in all.iter().enumerate() {
        t.check(&Case::plain(s, SANS, 12.0, None));
        if k % 5 == 0 {
            t.check(&Case::plain(s, SANS, 14.0, Some(300.0)));
            t.check(&Case::plain(s, SANS, 11.0, Some(120.0)));
        }
    }
    eprintln!("workload strings: {} cases equal", t.cases);
}

const WORDS: &[&str] = &[
    "the", "renderer", "keeps", "every", "glyph", "retained", "while", "streaming", "tokens", "arrive", "layout", "measures", "paragraphs",
    "at", "two", "widths", "per", "pass", "and", "virtualized", "transcripts", "need", "exact", "heights", "before", "mounting", "views",
    "fonts", "shape", "once", "wrapping", "is", "arithmetic", "over", "cached", "advances",
];

/// zgx text_bench's paragraph: runs of 1-4 words, every 11th run bold, every 11th + 4 mono (0 body, 1 bold, 2 code).
pub fn text_bench_paragraph(seed: usize, chars: usize) -> (String, Vec<(usize, u8)>) {
    let (mut text, mut runs) = (String::new(), Vec::<(usize, u8)>::new());
    let mut i = seed;
    while text.len() < chars {
        let font = match i % 11 {
            3 => 1,
            7 => 2,
            _ => 0,
        };
        if runs.last().is_none_or(|r| r.1 != font) {
            runs.push((text.len(), font));
        }
        for _ in 0..(1 + i % 4) {
            text.push_str(WORDS[(i * 7 + seed) % WORDS.len()]);
            text.push(' ');
            i += 1;
        }
    }
    (text, runs)
}

pub fn rich_case<'a>(text: &'a str, runs: &[(usize, u8)], width: Option<f32>) -> Case<'a> {
    let mut c = Case::plain(text, SANS, 14.0, width);
    for (k, &(start, font)) in runs.iter().enumerate() {
        let end = runs.get(k + 1).map_or(text.len(), |r| r.0);
        match font {
            1 => c.spans.push((start..end, Prop::Weight(700.0))),
            2 => c.spans.push((start..end, Prop::Family(MONO))),
            _ => {}
        }
    }
    c
}

/// text_bench's rich paragraphs (three faces) at its widths, and its stream (a paragraph growing 6 bytes a step).
#[test]
fn text_bench_paragraphs() {
    let mut t = Twin::new(&dejavu(), false);
    for seed in 0..300 {
        let (text, runs) = text_bench_paragraph(seed, 400);
        for w in [Some(736.0), Some(700.0), Some(240.0), Some(37.0), None] {
            t.check(&rich_case(&text, &runs, w));
        }
    }
    let (text, runs) = text_bench_paragraph(7, 3000);
    let mut end = 6;
    while end < text.len() {
        let r: Vec<_> = runs.iter().copied().filter(|r| r.0 < end).collect();
        t.check(&rich_case(&text[..end], &r, Some(736.0)));
        end += 6 * 17;
    }
    let [h, f] = bough_text::parley::bough::segments();
    eprintln!("text_bench: {} cases equal; font segments: {f} by the table, {h} by harfrust", t.cases);
    assert!(f > 0 && h < f / 50, "text_bench's segments are all covered");
}

fn rng(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

/// Random strings over everything the grafts must refuse or pass through (CJK, Hebrew and Arabic for bidi, emoji,
/// combining marks, controls, newlines, tabs, soft hyphens) mixed with covered text, with system fallback fonts.
#[test]
fn random_mixed() {
    // System fonts only: DejaVu is installed system-wide, and registering a second copy of the same family lets the
    // two collections pick different duplicates (hash order), which is not a graft difference.
    let mut t = Twin::new(&[], true);
    let pool: Vec<&str> = vec![
        "a", "b", "f", "i", "l", "fi", "ffl", " ", " ", "  ", "\t", "\n", "\r\n", "\u{AD}", "é", "ß", "Ä", "ñ", "1", "2", ",", ".", "-",
        "/", "(", ")", "'", "\"", "%", "$", "日本", "語", "中文", "שלום", "עברית", "مرحبا", "سلام", "😀", "👍🏽", "e\u{301}", "\u{300}",
        "Ω", "Ж", "δ", "\u{200B}", "\u{A0}", "\u{2028}", "word", "streaming", "Hello", "World",
    ];
    let mut seed = 0x9E3779B97F4A7C15u64;
    for _ in 0..3000 {
        let n = 1 + rng(&mut seed) as usize % 24;
        let mut s = String::new();
        for _ in 0..n {
            s.push_str(pool[rng(&mut seed) as usize % pool.len()]);
        }
        let w = match rng(&mut seed) % 3 {
            0 => None,
            1 => Some(80.0),
            _ => Some(300.0),
        };
        t.check(&Case::plain(&s, SANS, 14.0, w));
    }
    let [h, f] = bough_text::parley::bough::segments();
    eprintln!("random mixed: {} cases equal; font segments: {f} by the table, {h} by harfrust", t.cases);
}

/// Style variants the shaping fast path must refuse (features, locale) or that act after shaping (spacing), sizes,
/// and mixed sizes inside a line.
#[test]
fn style_variants() {
    let mut t = Twin::new(&dejavu(), false);
    let texts = ["office affinity fly", "12,345 req/s", "The quick brown fox jumps", "Ünïcödé façade", "a-b c/d (e) 'f'"];
    for s in texts {
        let base = Case::plain(s, SANS, 14.0, Some(90.0));
        let variants: Vec<Vec<Prop>> = vec![
            vec![Prop::Features("'liga' 0")],
            vec![Prop::Features("'kern' 0")],
            vec![Prop::Locale("tr")],
            vec![Prop::Locale("en-US")],
            vec![Prop::LetterSpacing(1.5)],
            vec![Prop::WordSpacing(3.0)],
            vec![Prop::Size(9.5)],
            vec![Prop::Size(33.0)],
            vec![Prop::Weight(700.0)],
            vec![Prop::Family(MONO)],
        ];
        for v in variants {
            let mut c = base.clone();
            c.defaults.extend(v);
            t.check(&c);
        }
        let mut c = base.clone();
        c.spans.push((2..5.min(s.len()), Prop::Size(20.0)));
        t.check(&c);
    }
    eprintln!("style variants: {} cases equal", t.cases);
}

/// SF NS (a variable font) at text_bench's instances and at explicit `wght` variations; skipped without the font.
#[test]
fn variable_font() {
    let Some(sf) = common::twin::read("/System/Library/Fonts/SFNS.ttf") else { return };
    let mut t = Twin::new(&[sf], false);
    for seed in 0..60 {
        let (text, _) = text_bench_paragraph(seed, 300);
        for v in [None, Some("'wght' 700"), Some("'wght' 400, 'opsz' 17")] {
            let mut c = Case::plain(&text, "System Font", 14.0, Some(500.0));
            if let Some(v) = v {
                c.defaults.push(Prop::Variations(v));
            }
            t.check(&c);
        }
    }
    let [h, f] = bough_text::parley::bough::segments();
    eprintln!("variable font: {} cases equal; font segments: {f} by the table, {h} by harfrust", t.cases);
}

/// Font stacks whose first family lacks some covered characters (braille, symbol, dingbat, monospace and
/// calligraphic fonts first), so upstream's per-cluster selection picks different fonts within a run: graft 2 must
/// find the same segments (it takes its one-selection-per-run path only when the run's font is the first candidate
/// and covers the run). The fonts are registered explicitly, without system fallback: with system fonts, upstream
/// parley itself is not deterministic when a cluster falls through to fontique's fallback with several equal
/// candidates (`upstream_self_consistency`).
#[test]
fn font_stacks() {
    let mut fonts = dejavu();
    for f in [
        "/System/Library/Fonts/Apple Braille.ttf",
        "/System/Library/Fonts/Symbol.ttf",
        "/System/Library/Fonts/Supplemental/Webdings.ttf",
        "/System/Library/Fonts/Supplemental/Wingdings.ttf",
        "/System/Library/Fonts/Supplemental/Courier New.ttf",
        "/System/Library/Fonts/Supplemental/Zapfino.ttf",
    ] {
        fonts.extend(common::twin::read(f));
    }
    let mut t = Twin::new(&fonts, false);
    let latin1: String = (0xA0u32..=0xFF).filter(|&c| c != 0xAD).map(|c| char::from_u32(c).unwrap()).collect();
    let texts = ["office affinity fly", "12,345 req/s", "The quick brown fox jumps", "Ünïcödé façade", "a-b c/d (e) 'f'", &latin1];
    let stacks = [
        "'Apple Braille', 'DejaVu Sans'",
        "'Symbol', 'DejaVu Sans'",
        "'Webdings', 'DejaVu Sans'",
        "'Wingdings', 'DejaVu Sans Mono'",
        "'Zapfino', 'DejaVu Sans'",
        "'Courier New', 'DejaVu Sans Mono'",
        "'DejaVu Sans Mono', 'DejaVu Sans'",
    ];
    for stack in stacks {
        for s in texts {
            let mut c = Case::plain(s, SANS, 14.0, Some(120.0));
            c.defaults.push(Prop::Families(stack));
            t.check(&c);
            let e = 4.min(s.len());
            if s.is_char_boundary(1) && s.is_char_boundary(e) {
                c.spans.push((1..e, Prop::Family(SANS)));
                t.check(&c);
            }
        }
    }
    let [h, f] = bough_text::parley::bough::segments();
    eprintln!("font stacks: {} cases equal; font segments: {f} by the table, {h} by harfrust", t.cases);
}

/// Probe: is upstream parley deterministic across two font collections with system fonts on this case?
#[test]
#[ignore]
fn upstream_self_consistency() {
    let mut a = common::twin::upstream::Eng::new(&[], true);
    let mut b = common::twin::upstream::Eng::new(&[], true);
    let mut c = Case::plain("office affinity fly", SANS, 14.0, Some(120.0));
    c.defaults.push(Prop::Families("'Apple Braille', 'DejaVu Sans'"));
    let (x, y) = (a.dump(&c), b.dump(&c));
    for l in x.lines().zip(y.lines()).filter(|(p, q)| p != q).take(2) {
        eprintln!("A {}\nB {}", &l.0[..l.0.len().min(90)], &l.1[..l.1.len().min(90)]);
    }
    assert_eq!(x, y, "upstream vs upstream");
}
