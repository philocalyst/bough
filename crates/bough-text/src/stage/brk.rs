//! `block(w)`: the lines of a shaped text at a width, with the interval of widths over which they hold.
//!
//! Two forms of the same function.
//!
//! * [`block_bytes`], for text shaped one glyph per byte with spaces as its only break opportunities (what the
//!   table shaper covers without a hard break): **no break table exists**. The pen lane of the shaped run is
//!   already the prefix sum a line search needs: the last glyph that fits is found by interpolation in it, the
//!   break by looking back to the nearest space in the text, and the interval by looking on to the end of the
//!   next word. A paragraph costs what its *lines* cost, not what its words cost, and nothing is stored for it
//!   but its glyphs.
//! * [`block`], over a break table ([`build`]): for every other text (clusters that are not bytes, hard
//!   breaks, and break opportunities from a line-break machine). For opportunity `k` the table holds two
//!   prefix sums in font units: `end` (the pen at the end of segment `k`, its trailing spaces excluded) and
//!   `next` (the pen at the start of segment `k + 1`). It is also the reference the first form is held to.
//!
//! The interval is the same argument in both: every line must still fit (`w >= its width`) and must still
//! refuse its next segment (`w < the width it would need to take it`).
use crate::shape::GlyphRun;

/// One break opportunity: `[end, next]` in font units. A hard break has the top bit of `next` set
/// ([`HARD`]): the segment after it starts a line whatever the width.
pub type Brk = [i32; 2];
pub const HARD: i32 = i32::MIN;

/// What `block` answers: the number of lines at a width, the widest line, and the half-open interval
/// `[lo, hi)` of widths (font units) over which the same breaks, and so the same answer, hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Block {
    pub lines: u32,
    pub widest: i32,
    pub lo: i32,
    pub hi: i32,
}

/// Summary of a table, kept in the memo entry: the widest unbreakable segment (min-content), the widest hard
/// line (max-content).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Extent {
    pub min: i32,
    pub max: i32,
    /// The text has a hard break.
    pub hard: bool,
}

/// Append the break table of `text` shaped as `run` to `out` (and each segment's first glyph to `first`).
/// Pen positions are offset by `pen0` and glyph indices by `g0` (an append continues a table).
///
/// One glyph per byte and no hard break (nearly all text the table shaper covers): separators are classified
/// eight bytes at a time into a bitmask and only the edges between separator and text are visited, two per
/// segment. Anything else takes the glyph-by-glyph walk.
pub fn build(text: &[u8], run: &GlyphRun, pen0: i32, g0: u32, out: &mut Vec<Brk>, first: &mut Vec<u32>) -> Extent {
    if run.identity_clusters && text.len() == run.len() {
        build_bytes(text, &run.x, run.width, pen0, g0, out, first)
    } else {
        build_walk(text, &run.x, run.width, Some(&run.cluster), pen0, g0, out, first)
    }
}

/// [`build`] for a text shaped one glyph per byte: `x[i]` is the pen of byte `i`.
pub fn build_bytes(text: &[u8], x: &[i32], width: i32, pen0: i32, g0: u32, out: &mut Vec<Brk>, first: &mut Vec<u32>) -> Extent {
    let n = text.len();
    assert!(x.len() == n);
    const M7: u64 = 0x7f7f_7f7f_7f7f_7f7f;
    const ONES: u64 = 0x0101_0101_0101_0101;
    let (xs, x, end_pen) = (x, x.as_ptr(), pen0 + width);
    let mut ext = Extent::default();
    // At most one segment per two bytes: room for all of them now, so the loop stores without checking.
    let (o0, f0) = (out.len(), first.len());
    out.reserve(n / 2 + 1);
    first.reserve(n / 2 + 1);
    let (mut op, mut fp) = (o0, f0);
    // the byte before the text counts as a separator, so a segment that starts at 0 is an edge
    let mut prev = 1u64;
    // the segment begun and not yet stored: its start pen, and its end pen once its end has been seen
    let (mut begun, mut ended, mut sx, mut end) = (false, false, 0i32, 0i32);
    let mut newline = 0u64;
    let mut p = 0;
    while p < n {
        let w = if p + 8 <= n {
            // SAFETY: eight bytes from `p` are inside `text` (`p + 8 <= n == text.len()`).
            unsafe { (text.as_ptr().add(p) as *const u64).read_unaligned() }
        } else {
            // the tail, padded with separators
            let mut t = [b' '; 8];
            t[..n - p].copy_from_slice(&text[p..]);
            u64::from_le_bytes(t)
        };
        // 0x80 in every byte that is a space (exactly), and in every byte that is a newline
        let v = w ^ (ONES * b' ' as u64);
        let z = !(((v & M7) + M7) | v | M7);
        let u = w ^ (ONES * b'\n' as u64);
        newline |= !(((u & M7) + M7) | u | M7);
        let m = (z >> 7).wrapping_mul(0x0102_0408_1020_4080) >> 56;
        let mut e = (m ^ ((m << 1) | prev)) & 0xff;
        prev = m >> 7;
        while e != 0 {
            let k = e.trailing_zeros() as usize;
            e &= e - 1;
            let pos = p + k;
            if pos >= n {
                break; // the padding
            }
            // SAFETY: `pos < n == x.len()`; `op` and `fp` stay under the reserved capacity (one segment
            // needs at least two bytes: its first byte and the separator before the next).
            unsafe {
                let pen = pen0 + *x.add(pos);
                if (m >> k) & 1 != 0 {
                    // text ends here
                    end = pen;
                    ended = true;
                    ext.min = ext.min.max(end - sx);
                } else {
                    // text starts here: the segment before it now knows its `next`
                    if begun {
                        *out.as_mut_ptr().add(op) = [end, pen];
                        op += 1;
                    }
                    *first.as_mut_ptr().add(fp) = g0 + pos as u32;
                    fp += 1;
                    (begun, ended, sx) = (true, false, pen);
                }
            }
        }
        p += 8;
    }
    if newline != 0 {
        // a hard break: nothing was committed (the lengths are unchanged); walk
        return build_walk(text, xs, width, None, pen0, g0, out, first);
    }
    if begun {
        if !ended {
            end = end_pen;
            ext.min = ext.min.max(end - sx);
        }
        // SAFETY: as above.
        unsafe { *out.as_mut_ptr().add(op) = [end, end_pen] };
        op += 1;
        ext.max = end - pen0;
    }
    debug_assert!(op - o0 == fp - f0);
    // SAFETY: elements `o0..op` and `f0..fp` were written above, inside the reserved capacity.
    unsafe {
        out.set_len(op);
        first.set_len(fp);
    }
    ext
}

/// [`build`] by walking every glyph: any cluster mapping (`None`: glyph `i` is byte `i`), hard breaks. The
/// reference for the fast path.
#[allow(clippy::too_many_arguments)]
pub fn build_walk(text: &[u8], x: &[i32], width: i32, cluster: Option<&[u32]>, pen0: i32, g0: u32, out: &mut Vec<Brk>, first: &mut Vec<u32>) -> Extent {
    let n = x.len();
    let (mut ext, mut line0) = (Extent::default(), pen0);
    let at = |g: usize| match cluster {
        Some(c) => text[c[g] as usize],
        None => text[g],
    };
    let sep = |c: u8| (c == b' ') | (c == b'\n');
    let mut g = 0;
    // leading spaces belong to no segment
    while g < n && at(g) == b' ' {
        g += 1;
    }
    while g < n {
        let start = g;
        let sx = pen0 + x[g];
        while g < n && !sep(at(g)) {
            g += 1;
        }
        let end = pen0 + if g < n { x[g] } else { width };
        let mut hard = false;
        while g < n && sep(at(g)) {
            hard |= at(g) == b'\n';
            g += 1;
        }
        let next = pen0 + if g < n { x[g] } else { width };
        ext.min = ext.min.max(end - sx);
        ext.max = ext.max.max(end - line0);
        if hard {
            line0 = next;
            ext.hard = true;
        }
        out.push([end, if hard { next | HARD } else { next }]);
        first.push(g0 + start as u32);
    }
    ext
}

/// The lines of the table at width `w` (font units), with the interval over which they hold. `on_line` is
/// given the first segment of each line. `hard`: the table has a hard break (the entry knows).
///
/// Per line the end is found by interpolation: `end` is a prefix sum of segments of similar size, so the
/// segment where it crosses the limit is close to proportional; a few steps either way settle it.
#[inline(always)]
pub fn block_with(b: &[Brk], hard: bool, w: i32, mut on_line: impl FnMut(u32)) -> Block {
    let (mut lines, mut widest, mut lo, mut hi) = (0u32, 0i32, 0i32, i32::MAX);
    let (mut i, mut x0) = (0usize, 0i32);
    let n = b.len();
    while i < n {
        on_line(i as u32);
        let limit = x0.saturating_add(w);
        let rest = b[n - 1][0] - x0;
        let mut j = if rest <= w { n } else { i + ((n - i) as i64 * w as i64 / rest as i64) as usize };
        j = j.clamp(i + 1, n);
        // SAFETY: `j` stays in `i + 1 ..= n`, so `j` (when `j < n`) and `j - 1` index `b`.
        unsafe {
            while j < n && b.get_unchecked(j)[0] <= limit {
                j += 1;
            }
            while j > i + 1 && b.get_unchecked(j - 1)[0] > limit {
                j -= 1;
            }
        }
        if hard {
            // a hard break inside the fitted span ends the line there
            if let Some(h) = b[i..j].iter().position(|k| k[1] < 0) {
                j = i + h + 1;
            }
        }
        let last = b[j - 1];
        let width = last[0] - x0;
        widest = widest.max(width);
        // a line that fits must go on fitting; one that does not is a single segment wider than the line
        // and stands alone at every width below its own
        lo = lo.max(if width <= w { width } else { 0 });
        if j < n && last[1] >= 0 {
            // the line refused segment `j`: it must go on refusing it
            hi = hi.min(b[j][0] - x0);
        }
        x0 = last[1] & !HARD;
        i = j;
        lines += 1;
    }
    Block { lines: lines.max(1), widest, lo, hi }
}

/// The lines of `text` (one glyph per byte, pens `x`, total `width`) from glyph `s0` at pen `x0`, at width
/// `w`. `on_line` is given the first glyph of each line. No hard breaks: the caller splits at them.
#[inline(always)]
pub fn block_from(text: &[u8], x: &[i32], width: i32, w: i32, s0: usize, x0: i32, mut on_line: impl FnMut(u32)) -> Block {
    let n = text.len();
    assert!(x.len() == n);
    let (mut lines, mut widest, mut lo, mut hi) = (0u32, 0i32, 0i32, i32::MAX);
    // SAFETY (for every unchecked access below): indices are kept in `0..n`; `pen(n)` is `width`.
    let pen = |i: usize| if i < n { unsafe { *x.get_unchecked(i) } } else { width };
    let sp = |i: usize| unsafe { *text.get_unchecked(i) } == b' ';
    // the end of the word that starts at `i`: the first space after it, or `n`
    let word_end = |mut i: usize| {
        while i < n && !sp(i) {
            i += 1;
        }
        i
    };
    let mut s = s0;
    while s < n && sp(s) {
        s += 1;
    }
    let mut x0 = x0;
    let mut e1 = word_end(s);
    while s < n {
        on_line(s as u32);
        let limit = x0.saturating_add(w);
        let mut q = e1;
        if pen(e1) <= limit && e1 < n {
            // more than one word may fit: the last pen position within the limit, by interpolation
            let rest = (width - x0) as i64;
            let mut p = n;
            if rest > w as i64 {
                // proportional guess, then one correction by the pen actually found there (glyph advances
                // vary far more than word advances, so the first guess is off by several glyphs)
                let left = (n - s) as i64;
                let g = (s as i64 + left * w as i64 / rest).clamp(e1 as i64, n as i64);
                let d = (limit - pen(g as usize)) as i64 * left / rest;
                p = (g + d).clamp(e1 as i64, n as i64) as usize;
            }
            while p < n && pen(p + 1) <= limit {
                p += 1;
            }
            while p > e1 && pen(p) > limit {
                p -= 1;
            }
            // back to the end of a word: a space (or the end) that follows text
            while p > e1 && !((p == n || sp(p)) && !sp(p - 1)) {
                p -= 1;
            }
            q = p;
        }
        let wd = pen(q) - x0;
        widest = widest.max(wd);
        // a line that fits must go on fitting; one that does not is a single word wider than the line
        lo = lo.max(if wd <= w { wd } else { 0 });
        let mut t = q;
        while t < n && sp(t) {
            t += 1;
        }
        lines += 1;
        if t >= n {
            break;
        }
        // the line refused the next word: it must go on refusing it
        e1 = word_end(t);
        hi = hi.min(pen(e1) - x0);
        x0 = pen(t);
        s = t;
    }
    Block { lines: lines.max(1), widest, lo, hi }
}

/// [`block_from`] for the whole text.
#[inline]
pub fn block_bytes(text: &[u8], x: &[i32], width: i32, w: i32) -> Block {
    block_from(text, x, width, w, 0, 0, |_| {})
}

/// `block_with` when only the answer is wanted (layout's question).
#[inline]
pub fn block(b: &[Brk], hard: bool, w: i32) -> Block {
    block_with(b, hard, w, |_| {})
}

/// The same lines by walking every segment: the reference for `block`, and the façade's algorithm
/// (`Para::greedy`) restated on this table.
pub fn block_walk(b: &[Brk], w: i32) -> u32 {
    let (mut lines, mut x0, mut i) = (0u32, 0i32, 0usize);
    while i < b.len() {
        let mut j = i + 1;
        while j < b.len() && b[j - 1][1] >= 0 && b[j][0] - x0 <= w {
            j += 1;
        }
        x0 = b[j - 1][1] & !HARD;
        i = j;
        lines += 1;
    }
    lines.max(1)
}

/// An O(1) estimate of the line count at width `w` from the text's length alone: for a row that has never
/// been measured. `mean_adv` is the font's mean advance per byte of prose, in font units.
#[inline(always)]
pub fn guess(bytes: u32, mean_adv: i32, w: i32) -> u32 {
    let total = bytes as i64 * mean_adv as i64;
    // lines lose on average half a word at their end; fold it into the divisor
    ((total + w as i64 - 1) / (w.max(1) as i64)).max(1) as u32
}
