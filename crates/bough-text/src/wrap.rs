//! Greedy word wrapping over a shaped ASCII run (the layout half of "wrapped paragraphs").

use crate::shape::GlyphRun;

/// Break `run` (shaped from the ASCII bytes `text`, glyph `i` = byte `i`) into lines no wider
/// than `max_units` font units, breaking after spaces. Appends the glyph index at which each
/// line starts to `starts` (cleared first) and returns the line count. A word wider than the
/// line is placed alone on its line (never split).
pub fn wrap_greedy(run: &GlyphRun, text: &[u8], max_units: i32, starts: &mut Vec<u32>) -> usize {
    starts.clear();
    let n = run.len();
    if n == 0 {
        return 0;
    }
    debug_assert_eq!(text.len(), n);
    starts.push(0);
    let mut line_x0 = 0i32; // x of the first glyph of the current line
    let mut i = 0usize;
    let mut line_has_word = false;
    while i < n {
        // A word is a run of non-spaces followed by its spaces.
        let ws = i;
        while i < n && text[i] != b' ' {
            i += 1;
        }
        let we = i; // end of the non-space part
        while i < n && text[i] == b' ' {
            i += 1;
        }
        if we > ws {
            // Visible right edge of the word, without the trailing space's advance.
            let end_x = run.x[we - 1] + run.advance(we - 1);
            if line_has_word && end_x - line_x0 > max_units {
                starts.push(ws as u32);
                line_x0 = run.x[ws];
            }
            line_has_word = true;
        }
    }
    starts.len()
}
