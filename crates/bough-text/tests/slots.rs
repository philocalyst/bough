//! Round 3 gate: the byte-slot kernel (`slots::any_slots`: NEON classification, ASCII and token lanes, the scalar
//! byte-slot tokenizer for ligatures) against the token path (`Shaper::shape_tokens`, itself equal to rustybuzz
//! on the round-2 gate's whole domain) and against rustybuzz directly on corpora.
//!
//! Every glyph slot must carry the glyph id, cluster (= its byte offset) and pen of the reference; every phantom
//! must hold the pen after its glyph; acceptance must agree. Short strings are embedded at every offset inside
//! 16-byte chunks so each lane / chunk-boundary position of the NEON kernel is exercised.
#[path = "../examples/common/corpus.rs"]
mod corpus;

use bough_text::script::TOK_PH;
use bough_text::slots::{any_slots, compact};
use bough_text::{GlyphRun, Shaper, SimpleScript};

const FONT: &[u8] = zgx_workload::FONT_BYTES;

fn full() -> bool {
    !cfg!(debug_assertions) || std::env::var("BOUGH_FULL").is_ok()
}

struct H {
    sh: Shaper,
    run: GlyphRun,
    x: Vec<i32>,
    e: Vec<u16>,
    accepted: u64,
    refused: u64,
}

impl H {
    fn new() -> H {
        H { sh: Shaper::new(SimpleScript::build(FONT, 0).unwrap()), run: GlyphRun::default(), x: vec![0; 4096], e: vec![0; 4096], accepted: 0, refused: 0 }
    }

    fn check(&mut self, s: &str) {
        let n = s.len();
        if self.x.len() < n + 32 {
            self.x.resize(n + 32, 0);
            self.e.resize(n + 32, 0);
        }
        let x0 = 1000;
        // SAFETY: buffers hold n + 32 >= n.next_multiple_of(16) slots; `s` is UTF-8.
        let r = unsafe { any_slots(&self.sh, s.as_bytes(), x0, self.x.as_mut_ptr(), self.e.as_mut_ptr()) };
        let ok = self.sh.shape_tokens(s, &mut self.run);
        assert_eq!(r.is_ok(), ok, "acceptance differs for {s:?}");
        if !ok {
            self.refused += 1;
            return;
        }
        self.accepted += 1;
        if !["ff", "fi", "fl"].iter().any(|l| s.contains(l)) {
            assert!(matches!(bough_text::slots::lanes_outcome(&self.sh, s), 0 | 3), "the NEON lanes did not finish {s:?}");
        }
        let w = r.unwrap().0;
        let tt = self.sh.script.tokens.as_ref().unwrap();
        let xs: Vec<i32> = self.x[..n].iter().map(|v| v - x0).collect();
        let (gid, gx, cl, width) = compact(tt, &xs, &self.e[..n], w - x0);
        assert_eq!(gid, self.run.gid, "gids differ for {s:?}");
        assert_eq!(gx, self.run.x, "pens differ for {s:?}");
        assert_eq!(cl, self.run.cluster, "clusters differ for {s:?}");
        assert_eq!(width, self.run.width, "width differs for {s:?}");
        // phantoms hold the pen after their glyph (= the next glyph's pen, or the width)
        for i in 0..n {
            if self.e[i] == TOK_PH as u16 {
                let next = (i + 1..n).find(|&j| self.e[j] != TOK_PH as u16).map_or(w, |j| self.x[j]);
                assert_eq!(self.x[i], next, "phantom pen at {i} for {s:?}");
            }
        }
    }
}

fn covered(h: &H) -> Vec<char> {
    (0u32..256).filter(|&c| h.sh.script.covered.contains(c as u8)).map(|c| char::from_u32(c).unwrap()).collect()
}

#[test]
fn every_pair_at_every_lane() {
    let mut h = H::new();
    let cps = covered(&h);
    let pads = ["", "a", "Te", "x y", "AVAT", "lo ff", "quick", "123456", "fi é Ta", "abcdefgh", "ÿ1234567", "AV To.mm", "zzzzzzzzzzzzzzz", "éééééé", "The quick brown f"];
    let step = if full() { 1 } else { 7 };
    let mut s = String::new();
    for (ai, &a) in cps.iter().enumerate() {
        for (bi, &b) in cps.iter().enumerate() {
            if (ai * cps.len() + bi) % step != 0 {
                continue;
            }
            for k in 0..=17usize {
                let pad = pads[k % pads.len()];
                for tail in ["", "x", "é", " fl"] {
                    s.clear();
                    for i in 0..k {
                        s.push(pad.chars().nth(i % pad.chars().count().max(1)).unwrap_or('a'));
                    }
                    s.push(a);
                    s.push(b);
                    s.push_str(tail);
                    h.check(&s);
                }
            }
        }
    }
    println!("pairs at lanes: {} accepted, {} refused", h.accepted, h.refused);
    assert!(h.accepted > 1_000_000 / step as u64);
}

#[test]
fn every_short_string_alone() {
    let mut h = H::new();
    let cps = covered(&h);
    let mut s = String::new();
    let n = cps.len();
    let limit = if full() { 3 } else { 2 };
    let mut total = 0u64;
    for len in 0..=limit {
        let count = n.pow(len as u32);
        for k in 0..count {
            s.clear();
            let mut v = k;
            for _ in 0..len {
                s.push(cps[v % n]);
                v /= n;
            }
            h.check(&s);
            total += 1;
        }
    }
    println!("short strings: {total} ({} accepted, {} refused)", h.accepted, h.refused);
}

#[test]
fn ligature_alphabet_across_chunk_boundaries() {
    let mut h = H::new();
    let alpha = ['f', 'i', 'l', ' ', 'T', 'y', 'é'];
    let max = if full() { 6 } else { 4 };
    let mut s = String::new();
    for len in 1..=max {
        let count = alpha.len().pow(len as u32);
        for k in 0..count {
            for off in [0usize, 10, 13, 14, 15, 16, 17] {
                s.clear();
                for i in 0..off {
                    s.push(if i % 3 == 0 { 'a' } else { 'o' });
                }
                let mut v = k;
                for _ in 0..len {
                    s.push(alpha[v % alpha.len()]);
                    v /= alpha.len();
                }
                s.push_str(" end of it");
                h.check(&s);
            }
        }
    }
    println!("ligature alphabet: {} accepted, {} refused", h.accepted, h.refused);
}

#[test]
fn random_long_strings() {
    let mut h = H::new();
    let pools: [&[char]; 4] = [
        &['a', 'b', 'f', 'i', 'l', ' ', 'T', 'A', 'V', '.', ',', '0', '1'],
        &['é', 'è', 'ü', 'ß', 'ç', 'Ö', '×', '©', 'a', ' ', 'f', 'i'],
        &['f', 'f', 'i', 'l', 'é', ' ', 'y', '\u{a0}', 'Æ'],
        &['a', 'z', ' ', '\u{4e2d}', '\u{ad}', '\t', 'é', '\u{1}', 'x'],
    ];
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let reps = if full() { 200_000 } else { 10_000 };
    let mut s = String::new();
    for r in 0..reps {
        let pool = pools[r % pools.len()];
        let len = (rnd() % 200) as usize;
        s.clear();
        for _ in 0..len {
            s.push(pool[(rnd() % pool.len() as u64) as usize]);
        }
        h.check(&s);
    }
    println!("random: {} accepted, {} refused", h.accepted, h.refused);
    assert!(h.refused > 0 && h.accepted > reps as u64 / 2);
}

#[test]
fn workload_corpus_and_rustybuzz() {
    let mut h = H::new();
    let face = rustybuzz::Face::from_slice(FONT, 0).unwrap();
    let mut strings = corpus::all_workload_strings();
    strings.extend(corpus::heavy_log_lines(2000).into_iter().map(|l| format!("{l} — ignored")));
    strings.extend(["café crème brûlée, naïve façade", "Zürich – Straße", "affluent officials fly off", "×©®°±µ¶·¸¹º»¼½¾¿"].map(String::from));
    let mut x = vec![0i32; 4096];
    let mut e = vec![0u16; 4096];
    let sh2 = Shaper::new(SimpleScript::build(FONT, 0).unwrap());
    let tt = sh2.script.tokens.as_ref().unwrap();
    let mut direct = 0;
    for s in &strings {
        h.check(s);
        // directly against rustybuzz too
        let n = s.len();
        if x.len() < n + 32 {
            x.resize(n + 32, 0);
            e.resize(n + 32, 0);
        }
        // SAFETY: as in `check`.
        let Ok((w, _)) = (unsafe { any_slots(&sh2, s.as_bytes(), 0, x.as_mut_ptr(), e.as_mut_ptr()) }) else { continue };
        let mut b = rustybuzz::UnicodeBuffer::new();
        b.push_str(s);
        b.guess_segment_properties();
        let g = rustybuzz::shape(&face, &[], b);
        let (gid, gx, cl, width) = compact(tt, &x[..n], &e[..n], w);
        let mut pen = 0;
        assert_eq!(gid.len(), g.glyph_infos().len(), "{s:?}");
        for (k, (i, p)) in g.glyph_infos().iter().zip(g.glyph_positions()).enumerate() {
            assert_eq!((gid[k] as u32, cl[k], gx[k]), (i.glyph_id, i.cluster, pen), "{s:?} glyph {k}");
            assert_eq!((p.y_advance, p.x_offset, p.y_offset), (0, 0, 0));
            pen += p.x_advance;
        }
        assert_eq!(width, pen, "{s:?}");
        direct += 1;
    }
    println!("corpus: {} strings, {direct} against rustybuzz directly", strings.len());
}
