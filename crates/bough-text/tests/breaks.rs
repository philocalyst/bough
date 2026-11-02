//! Graft 3b: the boundary machines (ICU4X's own rule tables walked over a Latin-1 property map) against ICU's
//! segmenters, exactly as parley's analysis consumes them: line boundaries = the line iterator's positions without
//! the first and the last; word boundaries = the word iterator's positions; a line boundary wins over a word one.
use icu_segmenter::options::{LineBreakOptions, LineBreakWordOption, WordBreakInvariantOptions};
use icu_segmenter::{LineSegmenter, WordSegmenter};

fn oracle(s: &str) -> Vec<u8> {
    let mut opt = LineBreakOptions::default();
    opt.word_option = Some(LineBreakWordOption::Normal);
    let line: Vec<usize> = LineSegmenter::new_for_non_complex_scripts(opt).segment_str(s).collect();
    let word: Vec<usize> = WordSegmenter::new_for_non_complex_scripts(WordBreakInvariantOptions::default()).segment_str(s).collect();
    let inner = if line.len() > 2 { &line[1..line.len() - 1] } else { &[][..] };
    s.char_indices()
        .map(|(b, _)| if inner.contains(&b) { 2 } else if word.contains(&b) { 1 } else { 0 })
        .collect()
}

fn check(s: &str, n: &mut u64) {
    let got = bough_text::parley::bough::boundaries(s).expect("covered");
    assert_eq!(got, oracle(s), "boundaries of {s:?}");
    *n += 1;
}

/// Characters spanning every break class Latin-1 has (UAX #14: AL, NU, SP, BA, HY, IS, EX, QU, OP, CL, CP, PR, PO,
/// SY, GL, BB, AI; UAX #29: ALetter, Numeric, MidLetter, MidNum, MidNumLet, Single/Double quote, WSegSpace, Other).
const ALPHA: &str = " !\"#$%&'()*+,-./09:;<=>?@AZaz[\\]^_`{|}~\u{A0}¡¢£¤¥¦§¨ª«¬¯°±²³´µ¶·¸¹º»¼½¾¿ÀÆ×ßé÷ÿ";

#[test]
fn every_string_up_to_three() {
    let a: Vec<char> = ALPHA.chars().collect();
    let mut n = 0;
    let mut s = String::new();
    for &x in &a {
        s.clear();
        s.push(x);
        check(&s, &mut n);
        for &y in &a {
            s.truncate(x.len_utf8());
            s.push(y);
            check(&s, &mut n);
            for &z in &a {
                s.truncate(x.len_utf8() + y.len_utf8());
                s.push(z);
                check(&s, &mut n);
            }
        }
    }
    eprintln!("{n} strings equal");
}

#[test]
fn random_strings() {
    let a: Vec<char> = ALPHA.chars().collect();
    let words = ["the", "12", "3.5", "1,234", "a-b", "x/y", "(z)", "'q'", "\"r\"", "-7", "$9", "45%", "e.g.", "co-op", "«f»", "¿y?"];
    let mut seed = 0x2545F4914F6CDD1Du64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut n = 0;
    for _ in 0..300_000 {
        let len = 4 + next() as usize % 20;
        let mut s = String::new();
        for _ in 0..len {
            if next() % 3 == 0 {
                s.push_str(words[next() as usize % words.len()]);
            } else {
                s.push(a[next() as usize % a.len()]);
            }
        }
        check(&s, &mut n);
    }
    eprintln!("{n} strings equal");
}
