//! Round 3 gate: line-local editing and convergent re-wrap (`para::Para`).
//!
//! * random edits (insert / delete / replace words and characters, spaces, ligature words, Latin-1, kerning
//!   pairs, empty text) applied with `edit` leave slots, width, word table and the cached wraps at two widths
//!   **equal to the paragraph set from scratch** and wrapped by the full greedy loop;
//! * a rich paragraph streamed 6 bytes at a time across font runs (zgx `text_bench`'s stream) with `append`
//!   equals `set` of the same text and runs at every step, at both of the benchmark's widths.
use bough_text::para::{Para, Run};
use bough_text::pool::GlyphPool;
use bough_text::{Shaper, SimpleScript};

fn shaper(b: &[u8]) -> Shaper {
    Shaper::new(SimpleScript::build(b, 0).unwrap())
}

fn bold() -> Vec<u8> {
    let p = "/Library/Fonts/Nix Fonts/vl9mbzk97wmh1s1yj3766b4zai8msjq6-dejavu-fonts-2.37/share/fonts/truetype/DejaVuSans-Bold.ttf";
    std::fs::read(std::env::var("BOUGH_BOLD_FONT").unwrap_or(p.into())).unwrap_or_else(|_| zgx_workload::FONT_BYTES.to_vec())
}

/// Everything a paragraph exposes, slot pens relative to slot 0.
fn state(p: &Para, pool: &GlyphPool) -> (Vec<i32>, Vec<u16>, i32, Vec<bough_text::para::Word>) {
    let n = p.text.len();
    let b = p.base as usize;
    (pool.x[b..b + n].to_vec(), pool.ent[b..b + n].to_vec(), p.width, p.words.clone())
}

fn fresh_wrap(fonts: &[&Shaper], text: &str, runs: &[Run], mu: i32) -> bough_text::para::Wrap {
    let mut pool = GlyphPool::new();
    let mut q = Para::default();
    q.set(&mut pool, fonts, text, runs).unwrap();
    q.wrap(mu).clone()
}

#[test]
fn random_edits_equal_a_fresh_paragraph_and_a_full_rewrap() {
    let sh = shaper(zgx_workload::FONT_BYTES);
    let fonts = [&sh];
    let vocab = ["the", "renderer", "keeps", "office", "fluffy", "café", "naïve", "AVATAR", "Ta", "Wo", "x", "1234", "ffi", "æble", "glyph", "retained", "—"];
    let vocab: Vec<&str> = vocab.into_iter().filter(|w| !w.contains('—')).collect();
    let mut seed = 0x51_7cc1_b727_220au64;
    let mut rnd = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n.max(1) as u64) as usize
    };
    let mut pool = GlyphPool::new();
    let mut p = Para::default();
    let mut text: String = (0..40).map(|i| vocab[i % vocab.len()]).collect::<Vec<_>>().join(" ");
    p.set(&mut pool, &fonts, &text, &[]).unwrap();
    let widths = [38_000, 61_000];
    for &w in &widths {
        p.wrap(w);
    }
    let reps = if cfg!(debug_assertions) { 1500 } else { 15_000 };
    let (mut conv, mut shaped) = (0u64, 0u64);
    for step in 0..reps {
        // an edit at a character boundary
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let at = |k: usize| chars.get(k).map_or(text.len(), |c| c.0);
        let n = chars.len();
        let mut t = text.clone();
        match rnd(8) {
            0 => {
                let k = at(rnd(n + 1));
                t.insert_str(k, &format!("{} ", vocab[rnd(vocab.len())]));
            }
            1 if n > 0 => {
                let (i, j) = (rnd(n), rnd(n));
                let (i, j) = (i.min(j), i.max(j).min(i + 12));
                t.replace_range(at(i)..at(j), "");
            }
            2 if n > 0 => {
                let k = rnd(n);
                t.replace_range(at(k)..at(k + 1), ["a", "f", "i", "é", " ", "V", "  "][rnd(7)]);
            }
            3 => t.push_str(&format!(" {}", vocab[rnd(vocab.len())])),
            4 if n > 3 => {
                let k = rnd(n);
                t.truncate(at(k));
            }
            5 => {
                let k = at(rnd(n + 1));
                t.insert(k, ' ');
            }
            6 => t = format!("{} {}", vocab[rnd(vocab.len())], t),
            _ => {}
        }
        if t.len() > 900 {
            t.truncate(at(150.min(n)));
        }
        let before = p.stats;
        let prev = text.clone();
        p.edit(&mut pool, &fonts, &t).unwrap();
        shaped += p.stats.bytes_shaped - before.bytes_shaped;
        text = t;
        let mut fp = GlyphPool::new();
        let mut q = Para::default();
        q.set(&mut fp, &fonts, &text, &[]).unwrap();
        let (x, e, w, words) = state(&p, &pool);
        let (fx, fe, fw, fwords) = state(&q, &fp);
        assert_eq!(e, fe, "step {step}: tokens of {text:?} (was {prev:?})");
        assert_eq!(x, fx, "step {step}: pens of {text:?}");
        assert_eq!((w, &words), (fw, &fwords), "step {step}: width / words of {text:?}");
        for &mu in &widths {
            let got = p.wrap(mu).clone();
            assert_eq!(got, fresh_wrap(&fonts, &text, &[], mu), "step {step}: wrap at {mu} of {text:?}");
        }
        conv = p.stats.converged;
    }
    println!("{reps} edits: {shaped} bytes shaped in total, {conv} re-wraps converged early, {} words wrapped", p.stats.words_wrapped);
    assert!(conv > 0);
}

/// The convergence is what makes an edit line-local: a long paragraph (~2,000 bytes, ~35 lines) edited word by
/// word in its middle re-wraps a few lines per edit, not the paragraph.
#[test]
fn mid_paragraph_edits_rewrap_a_few_lines() {
    let sh = shaper(zgx_workload::FONT_BYTES);
    let fonts = [&sh];
    let vocab = ["the", "renderer", "keeps", "every", "glyph", "retained", "while", "streaming", "tokens", "arrive"];
    let mut h = 0x9E37_79B9u64;
    let mut pick = move || {
        h = h.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (h >> 33) as usize
    };
    let mut words: Vec<String> = (0..330).map(|_| vocab[pick() % vocab.len()].to_string()).collect();
    let mut pool = GlyphPool::new();
    let mut p = Para::default();
    p.set(&mut pool, &fonts, &words.join(" "), &[]).unwrap();
    let mu = 100_000;
    let lines = p.wrap(mu).starts.len();
    let before = p.stats;
    for k in 0..200 {
        let i = 100 + (k * 37) % 130;
        words[i] = vocab[pick() % vocab.len()].into();
        let t = words.join(" ");
        p.edit(&mut pool, &fonts, &t).unwrap();
        assert_eq!(p.wrap(mu).clone(), fresh_wrap(&fonts, &t, &[], mu));
    }
    let per_edit = (p.stats.words_wrapped - before.words_wrapped) as f64 / 200.0;
    let shaped = (p.stats.bytes_shaped - before.bytes_shaped) as f64 / 200.0;
    println!("{lines} lines, 330 words: {per_edit:.1} words re-wrapped and {shaped:.1} bytes shaped per edit, {} converged", p.stats.converged);
    // greedy wrapping can ripple (a moved break moves the next one); on average it re-syncs within a few lines
    assert!(per_edit < 330.0 * 0.3 && shaped < 32.0);
}

#[test]
fn rich_stream_equals_the_paragraph_set_at_once() {
    let (a, b) = (zgx_workload::FONT_BYTES.to_vec(), bold());
    let (s0, s1) = (shaper(&a), shaper(&b));
    let fonts = [&s0, &s1];
    // text_bench's paragraph shape: runs of 1-4 words, every 11th bold
    let words = ["the", "renderer", "keeps", "every", "glyph", "retained", "while", "streaming", "tokens", "arrive", "layout", "fine", "efficient", "café"];
    let (mut text, mut runs) = (String::new(), Vec::<Run>::new());
    let mut i = 7usize;
    while text.len() < 3000 {
        let font = (i % 11 == 3) as u8;
        if runs.last().is_none_or(|r| r.font != font) {
            runs.push(Run { start: text.len() as u32, font });
        }
        for _ in 0..(1 + i % 4) {
            text.push_str(words[(i * 7 + 3) % words.len()]);
            text.push(' ');
            i += 1;
        }
    }
    let mut pool = GlyphPool::new();
    let mut p = Para::default();
    let (w1, w2) = (736.0 / 14.0 * 2048.0, 968.0 / 14.0 * 2048.0);
    let (w1, w2) = (w1 as i32, w2 as i32);
    let mut end = 0usize;
    let mut steps = 0;
    while end < text.len() {
        let mut next = (end + 6).min(text.len());
        while !text.is_char_boundary(next) {
            next += 1;
        }
        // append run by run
        let mut k = end;
        while k < next {
            let r = runs.partition_point(|r| r.start as usize <= k) - 1;
            let stop = runs.get(r + 1).map_or(next, |r| (r.start as usize).min(next));
            p.append(&mut pool, &fonts, &text[k..stop], runs[r].font).unwrap();
            k = stop;
        }
        end = next;
        let now: Vec<Run> = runs.iter().copied().filter(|r| (r.start as usize) < end).collect();
        let mut fp = GlyphPool::new();
        let mut q = Para::default();
        q.set(&mut fp, &fonts, &text[..end], &now).unwrap();
        assert_eq!(state(&p, &pool), state(&q, &fp), "after {end} bytes");
        for mu in [w1, w2] {
            assert_eq!(p.wrap(mu).clone(), q.wrap(mu).clone(), "wrap at {mu} after {end} bytes");
        }
        steps += 1;
    }
    println!("{steps} steps: {} bytes shaped (text {}), {} words wrapped, {} early convergences", p.stats.bytes_shaped, text.len(), p.stats.words_wrapped, p.stats.converged);
    assert!(p.stats.bytes_shaped < 3 * text.len() as u64);
}
