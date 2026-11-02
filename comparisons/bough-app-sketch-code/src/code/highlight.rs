//! zgx's Rust tokenizer (`examples/code_editor/lib/highlight.luau`, `M.rust`), line for line: given a line and
//! what the line before left open (0 nothing, 1 a block comment, 2 a string), the line's spans and what it
//! leaves the next line. Spans are `[from, to)` bytes with a class; uncovered bytes are plain text.
//!
//! [`project`] turns a line's spans into one row per colour class: a monospace font puts every byte at its
//! column, so a class's row is the line with every byte of another class blanked, and the colours of a
//! screen of code are seven plain texts drawn over each other (no run per span, no per-glyph colour).

pub const PLAIN: u8 = 0;
pub const KEYWORD: u8 = 1;
pub const STRING: u8 = 2;
pub const NUMBER: u8 = 3;
pub const COMMENT: u8 = 4;
pub const TYPE: u8 = 5;
pub const MACRO: u8 = 6;
pub const CLASSES: usize = 7;

/// `RUST_KEYWORDS`.
const KEYWORDS: [&[u8]; 38] = [
    b"as", b"async", b"await", b"break", b"const", b"continue", b"crate", b"dyn", b"else", b"enum", b"extern", b"false", b"fn", b"for", b"if", b"impl", b"in", b"let", b"loop",
    b"match", b"mod", b"move", b"mut", b"pub", b"ref", b"return", b"self", b"Self", b"static", b"struct", b"super", b"trait", b"true", b"type", b"unsafe", b"use", b"where", b"while",
];

#[inline]
fn keyword(w: &[u8]) -> bool {
    // by length first: one compare of a few bytes against at most a handful of candidates
    w.len() <= 8 && KEYWORDS.iter().any(|k| *k == w)
}
#[inline]
fn is_word(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}
/// Lua's `%w` (ASCII alphanumeric) or `_`.
#[inline]
fn is_wordc(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `closing`: from `from`, the quote ending a quoted run (a backslash skips the byte after it), or `None`.
#[inline]
fn closing(line: &[u8], from: usize, quote: u8) -> Option<usize> {
    let mut at = from;
    while at < line.len() {
        let b = line[at];
        if b == b'\\' {
            at += 2;
        } else if b == quote {
            return Some(at);
        } else {
            at += 1;
        }
    }
    None
}

/// Plain `find(line, "*/", from, true)`.
#[inline]
fn star_slash(line: &[u8], from: usize) -> Option<usize> {
    (from..line.len().saturating_sub(1)).find(|&i| line[i] == b'*' && line[i + 1] == b'/')
}

/// `M.rust(line, state)`: spans into `out` (cleared first); returns the state the line leaves.
pub fn rust(line: &[u8], state: u8, out: &mut Vec<(u32, u32, u8)>) -> u8 {
    out.clear();
    let n = line.len();
    let at = |i: usize| line.get(i).copied().unwrap_or(0);
    let mut i = 0;
    if state == 1 {
        match star_slash(line, 0) {
            None => {
                out.push((0, n as u32, COMMENT));
                return 1;
            }
            Some(e) => {
                out.push((0, e as u32 + 2, COMMENT));
                i = e + 2;
            }
        }
    } else if state == 2 {
        match closing(line, 0, b'"') {
            None => {
                out.push((0, n as u32, STRING));
                return 2;
            }
            Some(q) => {
                out.push((0, q as u32 + 1, STRING));
                i = q + 1;
            }
        }
    }
    while i < n {
        let b = line[i];
        if b == b'/' && at(i + 1) == b'/' {
            out.push((i as u32, n as u32, COMMENT));
            break;
        } else if b == b'/' && at(i + 1) == b'*' {
            match star_slash(line, i + 2) {
                None => {
                    out.push((i as u32, n as u32, COMMENT));
                    return 1;
                }
                Some(e) => {
                    out.push((i as u32, e as u32 + 2, COMMENT));
                    i = e + 2;
                }
            }
        } else if b == b'"' {
            match closing(line, i + 1, b'"') {
                None => {
                    out.push((i as u32, n as u32, STRING));
                    return 2;
                }
                Some(q) => {
                    out.push((i as u32, q as u32 + 1, STRING));
                    i = q + 1;
                }
            }
        } else if b == b'\'' {
            // a character ('a', '\n'), or a lifetime ('a) left as it is
            let esc = if at(i + 1) == b'\\' { 2 } else { 0 };
            match closing(line, i + 1, b'\'') {
                Some(q) if q - i <= 3 + esc => {
                    out.push((i as u32, q as u32 + 1, STRING));
                    i = q + 1;
                }
                _ => i += 1,
            }
        } else if b == b'#' && (at(i + 1) == b'[' || at(i + 2) == b'[') {
            let e = (i..n).find(|&j| line[j] == b']').map_or(n, |e| e + 1);
            out.push((i as u32, e as u32, MACRO));
            i = e;
        } else if is_word(b) {
            let mut e = i + 1;
            while e < n && is_wordc(line[e]) {
                e += 1;
            }
            if keyword(&line[i..e]) {
                out.push((i as u32, e as u32, KEYWORD));
            } else if at(e) == b'!' {
                out.push((i as u32, e as u32 + 1, MACRO));
                e += 1;
            } else if b.is_ascii_uppercase() {
                out.push((i as u32, e as u32, TYPE));
            }
            i = e;
        } else if b.is_ascii_digit() {
            let mut e = i + 1;
            while e < n && (is_wordc(line[e]) || line[e] == b'.') {
                e += 1;
            }
            out.push((i as u32, e as u32, NUMBER));
            i = e;
        } else {
            i += 1;
        }
    }
    0
}

/// The line's rows by class: `rows[c]` holds the bytes of class `c` at their columns (a column per char),
/// blanks elsewhere, no trailing blank. Plain spaces are blanks in every row.
pub fn project(line: &[u8], spans: &[(u32, u32, u8)], rows: &mut [Vec<u8>; CLASSES]) {
    let mut cols = [0usize; CLASSES];
    for r in rows.iter_mut() {
        r.clear();
    }
    let mut col = 0usize;
    let put = |rows: &mut [Vec<u8>; CLASSES], cols: &mut [usize; CLASSES], c: usize, bytes: &[u8], col: &mut usize| {
        for &b in bytes {
            // a column per char: UTF-8 continuation bytes ride with their lead byte
            let lead = b & 0xc0 != 0x80;
            if b == b' ' {
                *col += 1;
                continue;
            }
            if lead {
                let pad = *col - cols[c];
                rows[c].resize(rows[c].len() + pad, b' ');
                cols[c] = *col + 1;
                *col += 1;
            }
            rows[c].push(b);
        }
    };
    let mut pos = 0usize;
    for &(a, e, c) in spans {
        let (a, e) = (a as usize, e as usize);
        if a > pos {
            put(rows, &mut cols, PLAIN as usize, &line[pos..a], &mut col);
        }
        put(rows, &mut cols, c as usize, &line[a..e], &mut col);
        pos = e;
    }
    if pos < line.len() {
        put(rows, &mut cols, PLAIN as usize, &line[pos..], &mut col);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(line: &str, st: u8) -> (Vec<(u32, u32, u8)>, u8) {
        let mut v = Vec::new();
        let s = rust(line.as_bytes(), st, &mut v);
        (v, s)
    }

    #[test]
    fn the_sample_lines() {
        let (v, s) = spans("pub fn compute_7(input: &[u8], scale: f32) -> Result<Vec<u32>, Error> {", 0);
        assert_eq!(s, 0);
        assert_eq!(&v[..2], &[(0, 3, KEYWORD), (4, 6, KEYWORD)]);
        assert!(v.contains(&(46, 52, TYPE)) && v.contains(&(53, 56, TYPE)) && v.contains(&(63, 68, TYPE)));
        let (v, _) = spans("    let name = \"item 2\"; // a comment", 0);
        assert_eq!(v, vec![(4, 7, KEYWORD), (15, 23, STRING), (25, 37, COMMENT)]);
        let (v, s) = spans("    /* a block comment", 0);
        assert_eq!((v, s), (vec![(4, 22, COMMENT)], 1));
        let (v, s) = spans("       running on */", 1);
        assert_eq!((v, s), (vec![(0, 20, COMMENT)], 0));
        let (v, _) = spans("    if total > 0xff { return Err(Error::Overflow(total)); }", 0);
        assert_eq!(v, vec![(4, 6, KEYWORD), (15, 19, NUMBER), (22, 28, KEYWORD), (29, 32, TYPE), (33, 38, TYPE), (40, 48, TYPE)]);
        let (v, _) = spans("    Ok(vec![total; scale as usize])", 0);
        assert_eq!(v, vec![(4, 6, TYPE), (7, 11, MACRO), (25, 27, KEYWORD)]);
        let (v, _) = spans("#[derive(Debug)] 'a' 'lifetime \"esc\\\"q\"", 0);
        assert_eq!(v, vec![(0, 16, MACRO), (17, 20, STRING), (31, 39, STRING)]);
    }

    #[test]
    fn projection_keeps_columns() {
        let line = b"    let x = \"s\"; // c";
        let mut v = Vec::new();
        rust(line, 0, &mut v);
        let mut rows: [Vec<u8>; CLASSES] = Default::default();
        project(line, &v, &mut rows);
        // overlaying the rows gives the line back
        let mut back = vec![b' '; line.len()];
        for r in &rows {
            for (i, &b) in r.iter().enumerate() {
                if b != b' ' {
                    assert_eq!(back[i], b' ');
                    back[i] = b;
                }
            }
        }
        assert_eq!(&back[..], &line[..]);
        assert_eq!(&rows[KEYWORD as usize][..], b"    let");
        assert!(rows.iter().all(|r| r.last() != Some(&b' ')));
    }
}
