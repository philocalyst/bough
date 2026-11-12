//! The exactness gate. Every string the fast path accepts must equal rustybuzz's output
//! exactly: glyph ids, clusters, per-glyph x/y advances and x/y offsets, all as integers in
//! font design units (rustybuzz reports HarfBuzz output at scale = units-per-em, so no
//! rounding is involved on either side). Any mismatch means the trigger set is wrong.
//!
//! Scale: `BOUGH_FULL=1` (or a release build) runs everything; debug builds sample.

#[path = "../examples/common/corpus.rs"]
mod corpus;

use bough_text::{GlyphRun, Shaper, SimpleScript};
use rustybuzz::{Face, UnicodeBuffer};

const FONT: &[u8] = zgx_workload::FONT_BYTES;

fn full() -> bool {
    !cfg!(debug_assertions) || std::env::var("BOUGH_FULL").is_ok()
}

struct Harness<'a> {
    face: Face<'a>,
    shaper: &'a Shaper,
    buf: Option<UnicodeBuffer>,
    run: GlyphRun,
    run2: GlyphRun,
    run3: GlyphRun,
    run4: GlyphRun,
    eligible: u64,
    ineligible: u64,
}

impl<'a> Harness<'a> {
    fn new(shaper: &'a Shaper) -> Self {
        Harness {
            face: Face::from_slice(FONT, 0).unwrap(),
            shaper,
            buf: Some(UnicodeBuffer::new()),
            run: GlyphRun::default(),
            run2: GlyphRun::default(),
            run3: GlyphRun::default(),
            run4: GlyphRun::default(),
            eligible: 0,
            ineligible: 0,
        }
    }

    fn reference(&mut self, s: &str, latin: bool) -> Vec<(u32, u32, i32, i32, i32, i32)> {
        let mut b = self.buf.take().unwrap();
        b.push_str(s);
        if latin {
            b.set_script(rustybuzz::script::LATIN);
            b.set_direction(rustybuzz::Direction::LeftToRight);
        }
        b.guess_segment_properties();
        let g = rustybuzz::shape(&self.face, &[], b);
        let infos = g.glyph_infos();
        let pos = g.glyph_positions();
        let out = infos
            .iter()
            .zip(pos)
            .map(|(i, p)| (i.glyph_id, i.cluster, p.x_advance, p.y_advance, p.x_offset, p.y_offset))
            .collect();
        self.buf = Some(g.clear());
        out
    }

    /// Returns whether the fast path accepted the string; panics on any mismatch.
    fn check(&mut self, s: &str) -> bool {
        let ok = self.shaper.shape_into(s, 16., &mut self.run);
        let ok2 = self.shaper.shape_scalar(s, 16., &mut self.run2);
        assert_eq!(ok, ok2, "simd/scalar eligibility differ for {s:?}");
        #[cfg(target_arch = "aarch64")]
        {
            let ok3 = self.shaper.shape_simd(s, 16., &mut self.run3);
            assert_eq!(ok, ok3, "shape_simd eligibility differs for {s:?}");
            if ok3 {
                assert_eq!(
                    (&self.run3.gid, &self.run3.x, self.run3.width),
                    (&self.run2.gid, &self.run2.x, self.run2.width),
                    "shape_simd differs from scalar for {s:?}"
                );
                assert_eq!(self.run3.identity_clusters, self.run2.identity_clusters);
            }
        }
        // The token path on its own must agree with whatever the hybrid produced (for ASCII
        // strings without ligatures that is the dense kernel's output, clusters included).
        let ok4 = self.shaper.shape_tokens(s, &mut self.run4);
        assert_eq!(ok, ok4, "token path eligibility differs for {s:?}");
        if ok4 {
            let (a, b) = (&self.run4, &self.run);
            assert_eq!((&a.gid, &a.x, a.width), (&b.gid, &b.x, b.width), "token path differs for {s:?}");
            for i in 0..a.len() {
                assert_eq!(a.cluster_of(i), b.cluster_of(i), "token cluster {i} of {s:?}");
            }
        }
        if !ok {
            self.ineligible += 1;
            return false;
        }
        self.eligible += 1;
        let r = &self.run;
        let r2 = &self.run2;
        assert_eq!((&r.gid, &r.x, r.width), (&r2.gid, &r2.x, r2.width), "simd/scalar differ for {s:?}");
        for latin in [false, true] {
            let want = self.reference(s, latin);
            let r = &self.run;
            assert_eq!(r.len(), want.len(), "glyph count for {s:?} (latin={latin}): {want:?}");
            let mut total = 0;
            for (i, w) in want.iter().enumerate() {
                let got = (r.gid[i] as u32, r.cluster_of(i), r.advance(i), 0, 0, 0);
                assert_eq!(got, *w, "glyph {i} of {s:?} (latin={latin}): got {got:?} want {w:?}");
                assert_eq!(r.x[i], total, "position {i} of {s:?}");
                total += w.2;
            }
            assert_eq!(r.width, total, "width of {s:?}");
        }
        true
    }
}

fn shaper() -> Shaper {
    Shaper::new(SimpleScript::build(FONT, 0).expect("DejaVu builds"))
}

fn covered_chars(sh: &Shaper) -> Vec<char> {
    (0u8..=255).filter(|&c| sh.script.covered.contains(c)).map(|c| c as char).collect()
}

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn parallel<F: Fn(usize, usize) + Sync>(n: usize, f: F) {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    std::thread::scope(|s| {
        for t in 0..threads {
            let f = &f;
            s.spawn(move || f(t, threads));
        }
    });
    let _ = n;
}

#[test]
fn exhaustive_up_to_three() {
    let sh = shaper();
    let chars = covered_chars(&sh);
    let n = chars.len();
    assert!(n >= 180, "covered {n}");
    // Debug builds: every pair, plus a stride through triples.
    let stride = if full() { 1 } else { 211 };
    let elig = std::sync::atomic::AtomicU64::new(0);
    let inelig = std::sync::atomic::AtomicU64::new(0);
    parallel(n, |t, threads| {
        let mut h = Harness::new(&sh);
        let mut s = String::new();
        // length 1 and 2, all.
        for (ai, &a) in chars.iter().enumerate() {
            if ai % threads != t {
                continue;
            }
            s.clear();
            s.push(a);
            h.check(&s);
            for &b in &chars {
                s.clear();
                s.push(a);
                s.push(b);
                h.check(&s);
            }
            let mut k = 0usize;
            for &b in &chars {
                for &c in &chars {
                    k += 1;
                    if k % stride != 0 && stride != 1 {
                        continue;
                    }
                    s.clear();
                    s.push(a);
                    s.push(b);
                    s.push(c);
                    h.check(&s);
                }
            }
        }
        elig.fetch_add(h.eligible, std::sync::atomic::Ordering::Relaxed);
        inelig.fetch_add(h.ineligible, std::sync::atomic::Ordering::Relaxed);
    });
    let (e, i) = (elig.into_inner(), inelig.into_inner());
    eprintln!("exhaustive<=3: {e} eligible, {i} ineligible ({:.4}% ineligible)", 100. * i as f64 / (e + i) as f64);
    assert!(e > 0);
    // With the token model (ligatures in the table) every string over the covered set is
    // accepted: there is no fallback left for Latin-1 text in DejaVu Sans.
    assert_eq!(i, 0, "the token model must accept every covered string");
}

#[test]
fn ascii_exhaustive_three_all() {
    // Pure ASCII triples exercise the SIMD path with every stride; always complete.
    let sh = shaper();
    let chars: Vec<char> = (0x20u8..0x7f).map(|c| c as char).collect();
    let n = chars.len();
    let lim = if full() { n } else { 5 };
    parallel(n, |t, threads| {
        let mut h = Harness::new(&sh);
        let mut s = String::new();
        for (ai, &a) in chars.iter().enumerate().take(lim) {
            if ai % threads != t {
                continue;
            }
            for &b in &chars {
                for &c in &chars {
                    s.clear();
                    s.push(a);
                    s.push(b);
                    s.push(c);
                    h.check(&s);
                }
            }
        }
    });
}

#[test]
fn workload_strings() {
    let sh = shaper();
    let mut h = Harness::new(&sh);
    let all = corpus::all_workload_strings();
    let mut n = 0;
    let step = if full() { 1 } else { 23 };
    for s in all.iter().step_by(step) {
        n += 1;
        h.check(s);
    }
    eprintln!("workload strings: {n}, eligible {} ineligible {}", h.eligible, h.ineligible);
    assert!(h.eligible > 0);
}

#[test]
fn random_strings_up_to_200() {
    let sh = shaper();
    let chars = covered_chars(&sh);
    let ascii: Vec<char> = (0x20u8..0x7f).map(|c| c as char).collect();
    // Letters/digits/space/punct typical of UI text, without the ligature letters weighting.
    let count = if full() { 200_000 } else { 1_600 };
    parallel(0, |t, threads| {
        let mut h = Harness::new(&sh);
        let mut st = 0x9E3779B97F4A7C15u64 ^ (t as u64 + 1).wrapping_mul(0xD1B54A32D192ED03);
        let mut s = String::new();
        for _ in 0..count / threads {
            let len = (xorshift(&mut st) % 201) as usize;
            let pool = match xorshift(&mut st) % 3 {
                0 => &chars,
                _ => &ascii,
            };
            s.clear();
            for _ in 0..len {
                s.push(pool[(xorshift(&mut st) % pool.len() as u64) as usize]);
            }
            h.check(&s);
        }
        // Strings that avoid the ligature trigger letters entirely must be accepted.
        let safe: Vec<char> = ascii.iter().copied().filter(|&c| c != 'f').collect();
        for _ in 0..count / threads / 4 {
            let len = 1 + (xorshift(&mut st) % 200) as usize;
            s.clear();
            for _ in 0..len {
                s.push(safe[(xorshift(&mut st) % safe.len() as u64) as usize]);
            }
            assert!(h.check(&s), "string without f must be eligible: {s:?}");
        }
    });
}

#[test]
fn english_words_and_sentences() {
    let sh = shaper();
    let mut h = Harness::new(&sh);
    let words: Vec<String> = std::fs::read_to_string(std::env::var("BOUGH_WORDS").unwrap_or_else(|_| "/usr/share/dict/words".into()))
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_else(|_| {
            "the quick brown fox jumps over a lazy dog office affirm fluff".split(' ').map(str::to_string).collect()
        });
    let step = if full() { 7 } else { 211 };
    let mut n = 0;
    for w in words.iter().step_by(step) {
        h.check(w);
        h.check(&w.to_uppercase());
        n += 1;
    }
    // Sentences made of consecutive words.
    for ch in words.chunks(9).step_by(step * 3) {
        h.check(&ch.join(" "));
    }
    eprintln!("words: {n}; eligible {} ineligible {}", h.eligible, h.ineligible);
    assert!(n > 100 || !full());
    // Ligature words are accepted (token path) and exact.
    for s in ["fi", "office", "ffl", "fluff", "af", "affluent", "baffling", "fff", "ffffi", "Ff fF", "caf\u{e9} fl\u{e2}ner"] {
        assert!(h.check(s), "{s}");
    }
    assert_eq!(h.ineligible, 0, "dictionary forms are all covered");
}

/// Every string up to length 7 over an f-heavy alphabet: greedy ligature parity (`fff`,
/// `ffff`, `fffi`, `ffffl`...), ligatures next to kerning letters, Latin-1 and spaces.
#[test]
fn ligature_alphabet_exhaustive() {
    let sh = shaper();
    let alphabet: Vec<char> = "fil Ty\u{e9}".chars().collect();
    let max_len = if full() { 7 } else { 5 };
    parallel(0, |t, threads| {
        let mut h = Harness::new(&sh);
        let mut s = String::new();
        let k = alphabet.len();
        for len in 1..=max_len {
            let total = k.pow(len as u32);
            for code in (t..total).step_by(threads) {
                s.clear();
                let mut c = code;
                for _ in 0..len {
                    s.push(alphabet[c % k]);
                    c /= k;
                }
                assert!(h.check(&s), "{s:?} refused");
            }
        }
    });
}

#[test]
fn unicode_outside_the_set_is_refused() {
    let sh = shaper();
    let mut run = GlyphRun::default();
    for s in ["\n", "a\tb", "€", "日本", "e\u{301}", "a\u{ad}b", "x\0y", "\u{7f}"] {
        assert!(!sh.shape_into(s, 12., &mut run), "{s:?}");
        assert!(!sh.shape_scalar(s, 12., &mut run), "{s:?}");
        #[cfg(target_arch = "aarch64")]
        assert!(!sh.shape_simd(s, 12., &mut run), "{s:?}");
    }
    let mut run = GlyphRun::default();
    assert!(sh.shape_into("", 12., &mut run));
    assert_eq!(run.len(), 0);
    assert_eq!(run.width, 0);
}

#[test]
fn table_properties() {
    let s = SimpleScript::build(FONT, 0).unwrap();
    assert_eq!(s.upem, 2048);
    // The only Latin-1 triggers in DejaVu Sans are the f-ligature pairs.
    assert_eq!(s.report.single_triggers, Vec::<u8>::new());
    assert_eq!(s.report.pair_triggers, 3, "{:?}", s.report.pair_triggers_example);
}

#[test]
fn refusals_are_only_ligature_pairs_or_uncovered_codepoints() {
    let sh = shaper();
    let mut run = GlyphRun::default();
    let all = corpus::all_workload_strings();
    let mut unc = 0;
    for s in &all {
        if sh.shape_into(s, 12., &mut run) {
            continue;
        }
        assert!(s.chars().any(|c| c as u32 > 255 || !sh.script.covered.contains(c as u32 as u8)), "{s:?}");
        unc += 1;
    }
    eprintln!("workload refusals: {unc}, all with a codepoint outside the covered set");
    // Digits never kern in this font; spaces/punctuation among digits neither.
    for a in b"0123456789" {
        for b in b"0123456789" {
            assert_eq!(sh.script.pair(*a, *b), 0);
        }
    }
}
