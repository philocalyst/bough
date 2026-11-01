//! Round 3: a measured paragraph that is **edited line-locally** and re-wrapped only **until a line break lands
//! where it was before**.
//!
//! A paragraph is shaped once into byte slots (`slots::any_slots`, ligatures and Latin-1 included), in runs of
//! one font each (a rich paragraph: body, bold, code; no kerning across a font change, as every shaper does at a
//! font boundary). A *word table* (a word = a run of non-spaces, with its start pen and the pen at the following
//! space) is what wrapping reads, so a wrap is a loop over ~65 words, not ~400 glyphs.
//!
//! An edit (`edit`, `append`) re-shapes only the words it touches:
//!
//! * the slots before the edited word keep their pens; the edited words are shaped from the start of the first
//!   one (a word boundary is a space, which never takes part in a ligature or a multi-byte character);
//! * the slots after it **keep their tokens and shift by one constant**: their pens move by
//!   `delta = new pen at the suffix - old pen at the suffix` (one NEON add over the suffix, or none), their byte
//!   indices by the length change (one move);
//! * the word table is spliced the same way (the words after shift by the same two constants).
//!
//! Greedy wrapping is a function of the words from a line start on, and pens after the edit only shift, so a
//! re-wrap can start one line before the edited word and **stop at the first line start (after the edit) that
//! equals an old line start shifted by the edit**: from there every later line is the old one. That convergence
//! point is the value that was there all along: the old wrap, kept per width (two widths cached, zgx's
//! `text_bench` alternates two). Streaming text (`append`) re-wraps one or two lines per step whatever the
//! paragraph's length.

use crate::pool::GlyphPool;
use crate::shape::Shaper;
use crate::slots::{any_slots, Refusal};

/// One word: bytes `ws..we`, its start pen `x0` and the pen at the byte after it `x1` (font units).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Word {
    pub ws: u32,
    pub we: u32,
    pub x0: i32,
    pub x1: i32,
}

/// A wrap of the paragraph at one width: the first word of each line and each line's width.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wrap {
    pub max_units: i32,
    /// Word index of each line's first word (line 0 starts at word 0).
    pub starts: Vec<u32>,
    /// Width of each line (font units, trailing spaces excluded).
    pub widths: Vec<i32>,
    pub widest: i32,
}

/// A font run: bytes from `start` to the next run's start are shaped with `font`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub start: u32,
    pub font: u8,
}

/// Counters of what edits and wraps did (for the benchmarks and the gates).
#[derive(Clone, Copy, Debug, Default)]
pub struct ParaStats {
    pub bytes_shaped: u64,
    pub slots_shifted: u64,
    pub words_wrapped: u64,
    pub converged: u64,
}

pub struct Para {
    pub text: Vec<u8>,
    pub runs: Vec<Run>,
    /// Pool block holding the byte slots (`base`, capacity `cap`).
    pub base: u32,
    pub cap: u32,
    /// Pen after the last glyph.
    pub width: i32,
    pub words: Vec<Word>,
    /// The last two wraps (by width).
    wraps: [Wrap; 2],
    /// Words `>= dirty_from` changed since the wraps were made, shifted by `dirty_shift` words after `dirty_to`
    /// (`u32::MAX`: clean).
    pending: Option<Pending>,
    pub stats: ParaStats,
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    /// First word (in the new table) that changed, and the end of the changed words in the new table.
    from: u32,
    to_new: u32,
    /// Word index delta for the words after the change.
    dw: i32,
}

/// The fonts a paragraph may use (index = `Run::font`); all share one `upem`.
pub type Fonts<'a> = &'a [&'a Shaper];

impl Default for Para {
    fn default() -> Self {
        Para { text: Vec::new(), runs: Vec::new(), base: u32::MAX, cap: 0, width: 0, words: Vec::new(), wraps: Default::default(), pending: None, stats: ParaStats::default() }
    }
}

impl Para {
    pub fn release(&mut self, pool: &mut GlyphPool) {
        if self.base != u32::MAX {
            pool.free_block(self.base, self.cap);
            self.base = u32::MAX;
            self.cap = 0;
        }
    }

    fn ensure(&mut self, pool: &mut GlyphPool, n: usize) {
        let need = n.max(16).next_power_of_two() as u32;
        if self.base != u32::MAX && need <= self.cap {
            return;
        }
        let cap = need.max(self.cap * 2);
        let base = pool.alloc_block(cap as usize, u32::MAX);
        if self.base != u32::MAX {
            let (o, n) = (self.base as usize, self.text.len());
            pool.x.copy_within(o..o + n, base as usize);
            pool.ent.copy_within(o..o + n, base as usize);
            pool.free_block(self.base, self.cap);
        }
        self.base = base;
        self.cap = cap;
    }

    /// The font run of byte `i` (index into `runs`).
    fn run_of(&self, i: u32) -> usize {
        self.runs.partition_point(|r| r.start <= i).saturating_sub(1)
    }

    fn run_end(&self, k: usize) -> u32 {
        self.runs.get(k + 1).map_or(self.text.len() as u32, |r| r.start)
    }

    /// Shape bytes `[a, e)` (a word boundary on both sides, or the text's ends) starting at pen `x`, run by run.
    /// Returns the pen after them. A run's last glyph steps into the run's end (no kerning across fonts).
    fn shape_range(&mut self, pool: &mut GlyphPool, fonts: Fonts, a: u32, e: u32, mut x: i32) -> Result<i32, Refusal> {
        let mut i = a;
        while i < e {
            let k = self.run_of(i);
            let end = self.run_end(k).min(e);
            let sh = fonts[self.runs[k].font as usize];
            let s = &self.text[i as usize..end as usize];
            // SAFETY: the block holds `cap >= len + 16` slots (`ensure`), so a 16-lane store past `end` stays in
            // it; `s` is UTF-8 cut at ASCII bytes (spaces, run starts at word boundaries).
            let (w, _) = unsafe { any_slots(sh, s, x, pool.x.as_mut_ptr().add((self.base + i) as usize), pool.ent.as_mut_ptr().add((self.base + i) as usize))? };
            self.stats.bytes_shaped += s.len() as u64;
            x = w;
            i = end;
        }
        Ok(x)
    }

    /// Words of bytes `[a, e)` (`a` a word start or a space, `e` a word end or the text's end), from the pens in
    /// the pool, appended to `out`. Word boundaries come from a space mask built 16 bytes at a time (NEON
    /// compare + the `shrn` nibble mask): a word starts where a non-space follows a space and ends where a space
    /// follows a non-space, so the loop runs per word, not per byte.
    fn words_of(&self, pool: &GlyphPool, a: u32, e: u32, out: &mut Vec<Word>) {
        let t = &self.text;
        let xs = &pool.x[self.base as usize..];
        let n = t.len() as u32;
        let width = self.width;
        let push = |ws: u32, we: u32, out: &mut Vec<Word>| {
            let x1 = if we < n { xs[we as usize] } else { width };
            out.push(Word { ws, we, x0: xs[ws as usize], x1 });
        };
        let mut i = a;
        let mut start: Option<u32> = None;
        #[cfg(target_arch = "aarch64")]
        unsafe {
            use core::arch::aarch64::*;
            let sp = vdupq_n_u8(b' ');
            // SAFETY: each load reads 16 bytes inside `t[..e]`.
            while i + 16 <= e {
                let eq = vceqq_u8(vld1q_u8(t.as_ptr().add(i as usize)), sp);
                // bit 4k of `m` set iff byte k is a space
                let m = vget_lane_u64::<0>(vreinterpret_u64_u8(vshrn_n_u16::<4>(vreinterpretq_u16_u8(eq)))) & 0x1111_1111_1111_1111;
                // boundaries: a byte whose space-ness differs from the previous byte's
                let prev_space = match start { Some(_) => 0u64, None => 1 };
                let flips = m ^ ((m << 4) | prev_space);
                let mut f = flips & 0x1111_1111_1111_1111;
                while f != 0 {
                    let k = (f.trailing_zeros() >> 2) as u32;
                    f &= f - 1;
                    match start {
                        None => start = Some(i + k),
                        Some(ws) => {
                            push(ws, i + k, out);
                            start = None;
                        }
                    }
                }
                i += 16;
            }
        }
        while i < e {
            let space = t[i as usize] == b' ';
            match (start, space) {
                (None, false) => start = Some(i),
                (Some(ws), true) => {
                    push(ws, i, out);
                    start = None;
                }
                _ => {}
            }
            i += 1;
        }
        if let Some(ws) = start {
            push(ws, e, out);
        }
    }

    /// Set the whole text: shaped from scratch (the oracle of `edit`/`append`).
    pub fn set(&mut self, pool: &mut GlyphPool, fonts: Fonts, text: &str, runs: &[Run]) -> Result<(), Refusal> {
        self.text.clear();
        self.text.extend_from_slice(text.as_bytes());
        self.runs.clear();
        self.runs.extend_from_slice(runs);
        if self.runs.is_empty() || self.runs[0].start != 0 {
            self.runs.insert(0, Run { start: 0, font: 0 });
        }
        self.ensure(pool, self.text.len() + 16);
        let n = self.text.len() as u32;
        self.width = self.shape_range(pool, fonts, 0, n, 0)?;
        let mut words = std::mem::take(&mut self.words);
        words.clear();
        self.words_of(pool, 0, n, &mut words);
        self.words = words;
        for w in &mut self.wraps {
            w.starts.clear();
            w.max_units = i32::MIN;
        }
        self.pending = None;
        Ok(())
    }

    /// The pen of slot `a` given the token that now starts there (`t_first`, or `0` = the end): the slot before it
    /// (a space, or nothing) keeps its pen; its step is re-read because its next token may have changed. No
    /// kerning across a font change.
    fn pen_into(&self, pool: &GlyphPool, fonts: Fonts, a: u32, t_first: u16) -> i32 {
        if a == 0 {
            return 0;
        }
        let b = self.base as usize;
        let k = self.run_of(a - 1);
        let same = (a as usize) < self.text.len() && self.run_of(a) == k;
        let tt = fonts[self.runs[k].font as usize].script.tokens.as_ref().expect("token model");
        let t = pool.ent[b + a as usize - 1] as usize;
        let u = if same { t_first as usize } else { 0 };
        pool.x[b + a as usize - 1].wrapping_add(tt.step[t << 8 | u] as i32)
    }

    /// Shape `[a, e)` from pen 0, then move it to the pen it takes after slot `a - 1`. Returns the pen after it.
    fn reshape(&mut self, pool: &mut GlyphPool, fonts: Fonts, a: u32, e: u32) -> Result<i32, Refusal> {
        let b = self.base as usize;
        // the kernel stores whole 16-lane chunks: keep the (already moved) slots after `e`
        let keep = (self.cap as usize).min(e as usize + 16) - e as usize;
        let (mut kx, mut ke) = ([0i32; 16], [0u16; 16]);
        kx[..keep].copy_from_slice(&pool.x[b + e as usize..b + e as usize + keep]);
        ke[..keep].copy_from_slice(&pool.ent[b + e as usize..b + e as usize + keep]);
        let r = self.shape_range(pool, fonts, a, e, 0);
        pool.x[b + e as usize..b + e as usize + keep].copy_from_slice(&kx[..keep]);
        pool.ent[b + e as usize..b + e as usize + keep].copy_from_slice(&ke[..keep]);
        let end = r?;
        let t_first = if e > a { pool.ent[b + a as usize] } else if (a as usize) < self.text.len() { self.text[a as usize] as u16 } else { 0 };
        let xa = self.pen_into(pool, fonts, a, t_first);
        if xa != 0 {
            add_pens(&mut pool.x[b + a as usize..b + e as usize], xa);
        }
        Ok(end.wrapping_add(xa))
    }

    /// Append `more` in font `font` (a stream). Shapes only the new bytes plus the word they continue.
    pub fn append(&mut self, pool: &mut GlyphPool, fonts: Fonts, more: &str, font: u8) -> Result<(), Refusal> {
        let old_n = self.text.len() as u32;
        if self.runs.is_empty() {
            self.runs.push(Run { start: 0, font });
        } else if self.runs.last().unwrap().font != font {
            self.runs.push(Run { start: old_n, font });
        }
        self.text.extend_from_slice(more.as_bytes());
        self.ensure(pool, self.text.len() + 16);
        // restart at the start of the last word (it may continue, or form a ligature with the new bytes)
        let mut a = old_n;
        while a > 0 && self.text[a as usize - 1] != b' ' {
            a -= 1;
        }
        let n = self.text.len() as u32;
        let first_changed = self.words.partition_point(|w| w.ws < a);
        self.width = self.reshape(pool, fonts, a, n)?;
        let old_words = self.words.len() as i32;
        self.words.truncate(first_changed);
        let mut tail = std::mem::take(&mut self.words);
        let before = tail.len();
        self.words_of(pool, a, n, &mut tail);
        self.words = tail;
        let to = self.words.len() as u32;
        self.mark(before as u32, to, to as i32 - old_words);
        Ok(())
    }

    /// Replace the text by `new` (one font run: the paragraph's first) re-shaping only the changed words; the
    /// suffix's slots and words shift by constants. Equal to `set` (gated).
    pub fn edit(&mut self, pool: &mut GlyphPool, fonts: Fonts, new: &str) -> Result<(), Refusal> {
        debug_assert!(self.runs.len() <= 1, "edit is single-run");
        let nb = new.as_bytes();
        let (on, nn) = (self.text.len(), nb.len());
        let p = first_diff(&self.text, nb);
        if p == on && on == nn {
            return Ok(());
        }
        let s = common_suffix(&self.text[p..], &nb[p..]);
        // widen to word boundaries: the changed words are [a, e_old) in the old text, [a, e_new) in the new one;
        // the kept suffix starts at a space (or is empty)
        let mut a = p;
        while a > 0 && nb[a - 1] != b' ' {
            a -= 1;
        }
        let mut sfx = s;
        while sfx > 0 && nb[nn - sfx] != b' ' {
            sfx -= 1;
        }
        let (e_old, e_new) = (on - sfx, nn - sfx);
        let dbytes = nn as i64 - on as i64;
        let b = self.base as usize;
        let old_sfx_x = if sfx > 0 { pool.x[b + e_old] } else { self.width };
        let wa = self.words.partition_point(|w| (w.ws as usize) < a);
        let wb = self.words.partition_point(|w| (w.ws as usize) < e_old);
        // bytes and slots: move the suffix
        self.ensure(pool, nn + 16);
        let b = self.base as usize;
        if dbytes != 0 && sfx > 0 {
            pool.x.copy_within(b + e_old..b + on, b + e_new);
            pool.ent.copy_within(b + e_old..b + on, b + e_new);
        }
        self.text.clear();
        self.text.extend_from_slice(nb);
        // shape the changed words; the suffix keeps its tokens
        let pen_after = self.reshape(pool, fonts, a as u32, e_new as u32)?;
        let new_sfx_x = if sfx > 0 {
            // the last changed glyph steps into the suffix's first byte (a space), not into the end
            let tt = fonts[self.runs.first().map_or(0, |r| r.font) as usize].script.tokens.as_ref().expect("token model");
            if e_new > a {
                let mut g = e_new - 1;
                while g > a && pool.ent[b + g] == crate::script::TOK_PH as u16 {
                    g -= 1;
                }
                let t = pool.ent[b + g] as usize;
                let x = pool.x[b + g].wrapping_add(tt.step[t << 8 | b' ' as usize] as i32);
                for k in g + 1..e_new {
                    pool.x[b + k] = x;
                }
                x
            } else {
                pen_after
            }
        } else {
            pen_after
        };
        let dx = new_sfx_x.wrapping_sub(old_sfx_x);
        if sfx > 0 && dx != 0 {
            add_pens(&mut pool.x[b + e_new..b + nn], dx);
            self.stats.slots_shifted += sfx as u64;
        }
        self.width = if sfx > 0 { self.width.wrapping_add(dx) } else { pen_after };
        // words: splice the changed ones, shift the rest by the two constants
        let mut mid = Vec::new();
        self.words_of(pool, a as u32, e_new as u32, &mut mid);
        let n_mid = mid.len();
        for w in &mut self.words[wb..] {
            w.ws = (w.ws as i64 + dbytes) as u32;
            w.we = (w.we as i64 + dbytes) as u32;
            w.x0 = w.x0.wrapping_add(dx);
            w.x1 = w.x1.wrapping_add(dx);
        }
        // the word before `a` ends at the space `a - 1`: its end pen (that space's pen) is unchanged
        self.words.splice(wa..wb, mid);
        self.mark(wa as u32, (wa + n_mid) as u32, n_mid as i32 - (wb - wa) as i32);
        Ok(())
    }

    /// Record a word change for the cached wraps (merging with an earlier pending one conservatively).
    fn mark(&mut self, from: u32, to_new: u32, dw: i32) {
        self.pending = Some(match self.pending {
            None => Pending { from, to_new, dw },
            Some(p) => {
                // the union of both changes in the newest table
                let f = p.from.min(from);
                let pt = (p.to_new as i64 + if p.to_new >= from { dw as i64 } else { 0 }) as u32;
                Pending { from: f, to_new: pt.max(to_new), dw: p.dw + dw }
            }
        });
    }

    /// Greedy wrap at `max_units` (break before a word whose end pen is past the line; a word wider than the line
    /// stays alone). Cached per width; after an edit, re-wraps from the line before the change until a line start
    /// equals an old one shifted by the edit.
    pub fn wrap(&mut self, max_units: i32) -> &Wrap {
        if let Some(p) = self.pending.take() {
            // bring both cached wraps up to date (each converges on its own)
            for k in 0..2 {
                if self.wraps[k].max_units != i32::MIN && !self.wraps[k].starts.is_empty() {
                    let mut w = std::mem::take(&mut self.wraps[k]);
                    self.rewrap(&mut w, p);
                    self.wraps[k] = w;
                }
            }
        }
        let k = match self.wraps.iter().position(|w| w.max_units == max_units && !w.starts.is_empty()) {
            Some(k) => k,
            None => {
                // evict the older entry (slot 1 moves to 0)
                self.wraps.swap(0, 1);
                let mut w = std::mem::take(&mut self.wraps[1]);
                w.max_units = max_units;
                w.starts.clear();
                w.widths.clear();
                self.greedy(&mut w, 0, 0);
                self.wraps[1] = w;
                1
            }
        };
        &self.wraps[k]
    }

    /// Full greedy wrap from line `line` (whose first word is `w0`), truncating what follows.
    fn greedy(&mut self, w: &mut Wrap, line: usize, w0: u32) -> usize {
        w.starts.truncate(line);
        w.widths.truncate(line);
        let words = &self.words;
        if words.is_empty() {
            w.starts.push(0);
            w.widths.push(0);
            w.widest = 0;
            return 1;
        }
        w.starts.push(w0);
        let mut x0 = words[w0 as usize].x0;
        let mut last = words[w0 as usize].x1;
        for (i, wd) in words.iter().enumerate().skip(w0 as usize + 1) {
            if wd.x1 - x0 > w.max_units {
                w.widths.push(last - x0);
                w.starts.push(i as u32);
                x0 = wd.x0;
            }
            last = wd.x1;
        }
        w.widths.push(last - x0);
        self.stats.words_wrapped += (words.len() - w0 as usize) as u64;
        w.widest = w.widths.iter().copied().max().unwrap_or(0);
        w.starts.len()
    }

    /// Re-wrap `w` after the change `p`: from the line before the one holding word `p.from`, until a line start
    /// past the change equals an old start shifted by `p.dw`.
    fn rewrap(&mut self, w: &mut Wrap, p: Pending) {
        let words = &self.words;
        if words.is_empty() || w.starts.is_empty() {
            let mu = w.max_units;
            w.starts.clear();
            w.max_units = mu;
            self.greedy(w, 0, 0);
            return;
        }
        // line holding the first changed word (old numbering == new numbering before `from`)
        let l = w.starts.partition_point(|&s| s <= p.from).saturating_sub(1);
        let line = l.saturating_sub(1);
        let w0 = w.starts[line];
        let old_starts = std::mem::take(&mut w.starts);
        let old_widths = std::mem::take(&mut w.widths);
        w.starts.extend_from_slice(&old_starts[..line]);
        w.widths.extend_from_slice(&old_widths[..line]);
        w.starts.push(w0);
        let mut x0 = words[w0 as usize].x0;
        let mut last = words[w0 as usize].x1;
        let mut i = w0 as usize + 1;
        // old line index to try for convergence
        let mut k = line + 1;
        let mut scanned = 1u64;
        while i < words.len() {
            let wd = words[i];
            if wd.x1 - x0 > w.max_units {
                w.widths.push(last - x0);
                // converged? a new line start past the change that equals an old start (shifted)
                if i as u32 >= p.to_new {
                    let old = i as i64 - p.dw as i64;
                    while k < old_starts.len() && (old_starts[k] as i64) < old {
                        k += 1;
                    }
                    if k < old_starts.len() && old_starts[k] as i64 == old {
                        for j in k..old_starts.len() {
                            w.starts.push((old_starts[j] as i64 + p.dw as i64) as u32);
                        }
                        w.widths.extend_from_slice(&old_widths[k..]);
                        w.widest = w.widths.iter().copied().max().unwrap_or(0);
                        self.stats.words_wrapped += scanned;
                        self.stats.converged += 1;
                        return;
                    }
                }
                w.starts.push(i as u32);
                x0 = wd.x0;
            }
            last = wd.x1;
            i += 1;
            scanned += 1;
        }
        w.widths.push(last - x0);
        w.widest = w.widths.iter().copied().max().unwrap_or(0);
        self.stats.words_wrapped += scanned;
    }

    /// Byte offset where line `i` of `wrap` starts.
    pub fn line_start(&self, wrap: &Wrap, i: usize) -> u32 {
        if self.words.is_empty() { 0 } else if i == 0 { 0 } else { self.words[wrap.starts[i] as usize].ws }
    }
}

/// `x[k] += d` for every slot (NEON, 4 lanes per add).
fn add_pens(x: &mut [i32], d: i32) {
    for v in x {
        *v = v.wrapping_add(d);
    }
}

/// Index of the first byte where `a` and `b` differ (`min(len)` if one is a prefix of the other).
pub fn first_diff(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + 8 <= n {
        let x = u64::from_le_bytes(a[i..i + 8].try_into().unwrap()) ^ u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        if x != 0 {
            return i + (x.trailing_zeros() / 8) as usize;
        }
        i += 8;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

/// Length of the longest common suffix of `a` and `b`.
pub fn common_suffix(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut k = 0;
    while k < n && a[a.len() - 1 - k] == b[b.len() - 1 - k] {
        k += 1;
    }
    k
}
