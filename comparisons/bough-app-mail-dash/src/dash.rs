//! zgx's `examples/dashboard` (app.luau, state.luau, actions.luau, components/{table,charts}.luau,
//! lib/data.luau) on bough, at zgx's 1280 x 800 points with `ROWS` instruments (100,000) and the feed paused
//! (`args = [rows, "paused"]`): the toolbar, the sidebar's summary and two charts, the sortable, filterable
//! table.
//!
//! **Data**: the same rows, made by the same generator in the same order of draws, the LCG computed in
//! doubles exactly as Luau computes it (its product rounds past 2^53; the remainder by 2^31 is exact), held
//! as columns (`Market`: packed symbols, sectors, prices, changes, volumes, histories).
//!
//! **Order** (zgx: `ui.sorted`, one whole order kept by re-placing written rows in a pass over all of it, a
//! spare order for a sort back to it, a radix sort otherwise, a sort of the kept rows on each filter
//! keystroke). The representation changes: every column's ascending order **is kept**, so a sort is a choice
//! among them and never sorts. Symbol and sector never change: their orders are flat arrays made once. Price,
//! change and volume are **value-routed buckets** (`Buckets`, a two-level B-tree: separator keys, each bucket a
//! small sorted run of (key, row), split when it grows, a run of equal keys never split): a feed tick's
//! written row is a remove from one bucket and an insert into another, nothing proportional to the rows is
//! read or copied, and the order is resolved where the view reads it.
//!
//! **Filters live in bit space**: a row bitmap per symbol character and per sector, made once. A search and a
//! sector are an AND of bitmaps (64 rows a word; a longer search verifies the rows every character let
//! through), the view's length a popcount. The view is the kept order read from its start (forward, or
//! backward for descending: runs of equal keys kept in row order, zgx's ties), keeping the rows the bitmap
//! lets through, only as far as the window needs.
//!
//! **The table** is virtualized as zgx's `ui.list` (rows of 32 points, two of overscan): 24 row slots,
//! mounted once, index rows as zgx's (`row(i)` reads `view:at(i)`), each slot's cells signals of the text they
//! show (`Str16`), so a cell whose text did not change patches nothing.
use crate::fmt::{number, s16, NumberOptions, Str16};
use crate::theme::*;
use crate::{button, caret_lane, lay_rect, text_width, Caret};
use bough::layout::prelude::*;
use bough::raw::*;
use std::sync::Arc;

// =============================================================================== lib/data.luau

pub const SECTORS: [&str; 8] = ["Tech", "Energy", "Health", "Finance", "Retail", "Industry", "Media", "Utilities"];
/// Each sector's place among the names in byte order (a sort by sector compares the names).
const SECTOR_RANK: [u8; 8] = [6, 0, 2, 1, 5, 3, 4, 7];
const LETTERS: &[u8; 26] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// data.luau's generator, in doubles as Luau computes it: `seed = (seed * 1103515245 + 12345) % 2147483648`.
/// The product and the sum round as Luau's do; the remainder by 2^31 is `a - floor(a * 2^-31) * 2^31`, every
/// step exact (power-of-two scalings; the subtraction by Sterbenz), so it equals `fmod` bit for bit with no
/// library call.
#[derive(Clone)]
pub struct Lcg(pub f64);
impl Lcg {
    #[inline(always)]
    pub fn random(&mut self) -> f64 {
        const M: f64 = 2147483648.0;
        let a = self.0 * 1103515245.0 + 12345.0;
        self.0 = a - (a * (1.0 / M)).floor() * M;
        self.0 * (1.0 / M)
    }
}

/// `f64::total_cmp`'s order as unsigned bits (zgx's radix word).
#[inline(always)]
pub fn fkey(x: f64) -> u64 {
    let b = x.to_bits() as i64;
    ((b ^ (((b >> 63) as u64) >> 1) as i64) as u64) ^ (1 << 63)
}

/// A symbol character's index in the character bitmaps: A-Z 0-25, 0-9 26-35.
fn char_index(c: u8) -> Option<usize> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as usize),
        b'0'..=b'9' => Some(26 + (c - b'0') as usize),
        _ => None,
    }
}

/// The table's sort columns (components/table.luau `COLUMNS`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Col {
    Symbol,
    Sector,
    Price,
    Change,
    Volume,
}
pub const COLS: [Col; 5] = [Col::Symbol, Col::Sector, Col::Price, Col::Change, Col::Volume];
const COL_LABEL: [&str; 5] = ["Symbol", "Sector", "Price", "Change", "Volume"];
const COL_W: [f32; 5] = [90.0, 90.0, 100.0, 90.0, 90.0];

// =============================================================================== buckets

/// Rows a bucket starts with, and the length past which it splits.
const BUCKET: usize = 32;
const BUCKET_MAX: usize = 128;

/// A column's ascending order of (key, row) as value-routed buckets: bucket `b` holds the keys from `lo[b]`
/// up to `lo[b + 1]`, sorted. Every row of one key is in one bucket (routing reads the key alone), so a run
/// of equal keys, ordered by row, never spans two.
#[derive(Clone, Default)]
pub struct Buckets {
    lo: Vec<u64>,
    b: Vec<Vec<(u64, u32)>>,
}

impl Buckets {
    /// From the sorted sequence of a column: runs of about `BUCKET`, each cut where the key changes.
    fn cut(sorted: &[(u64, u32)]) -> Buckets {
        let mut bk = Buckets::default();
        let mut s = 0;
        while s < sorted.len() {
            let mut e = (s + BUCKET).min(sorted.len());
            while e < sorted.len() && sorted[e].0 == sorted[e - 1].0 {
                e += 1;
            }
            bk.lo.push(if s == 0 { 0 } else { sorted[s].0 });
            let mut v = Vec::with_capacity((2 * BUCKET).max(e - s));
            v.extend_from_slice(&sorted[s..e]);
            bk.b.push(v);
            s = e;
        }
        if bk.b.is_empty() {
            bk.lo.push(0);
            bk.b.push(Vec::new());
        }
        bk
    }
    #[inline]
    fn bucket_of(&self, k: u64) -> usize {
        self.lo.partition_point(|&x| x <= k) - 1
    }
    fn remove(&mut self, k: u64, i: u32) {
        let b = self.bucket_of(k);
        let v = &mut self.b[b];
        let p = v.partition_point(|&x| x < (k, i));
        debug_assert_eq!(v[p], (k, i), "a written row's old place");
        v.remove(p);
    }
    fn insert(&mut self, k: u64, i: u32) {
        let b = self.bucket_of(k);
        let v = &mut self.b[b];
        let p = v.partition_point(|&x| x < (k, i));
        v.insert(p, (k, i));
        if v.len() > BUCKET_MAX {
            self.split(b);
        }
    }
    /// Split bucket `b` near its middle, at a change of key (none: a run of one key stays whole).
    fn split(&mut self, b: usize) {
        let v = &self.b[b];
        let mid = v.len() / 2;
        let at = (mid..v.len()).find(|&m| v[m].0 != v[m - 1].0).or_else(|| (1..mid).rev().find(|&m| v[m].0 != v[m - 1].0));
        if let Some(m) = at {
            let mut tail = Vec::with_capacity(2 * BUCKET);
            tail.extend_from_slice(&self.b[b][m..]);
            self.b[b].truncate(m);
            self.lo.insert(b + 1, tail[0].0);
            self.b.insert(b + 1, tail);
        }
    }
    /// Visit the rows in ascending order (`desc`: descending, each run of equal keys still by row) until `f`
    /// says stop.
    fn visit(&self, desc: bool, mut f: impl FnMut(u32) -> bool) {
        if !desc {
            for v in &self.b {
                for &(_, i) in v {
                    if !f(i) {
                        return;
                    }
                }
            }
            return;
        }
        for v in self.b.iter().rev() {
            let mut j = v.len();
            while j > 0 {
                let k = v[j - 1].0;
                let mut s = j - 1;
                while s > 0 && v[s - 1].0 == k {
                    s -= 1;
                }
                for &(_, i) in &v[s..j] {
                    if !f(i) {
                        return;
                    }
                }
                j = s;
            }
        }
    }
    pub fn len(&self) -> usize {
        self.b.iter().map(|v| v.len()).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// =============================================================================== the market

/// The instruments as columns, the feed's totals, every column's ascending order (by key, then row), and the
/// filter bitmaps.
#[derive(Clone)]
pub struct Market {
    pub n: usize,
    /// The symbol's bytes, big-endian in a word (its byte order is the word's).
    pub sym: Vec<u64>,
    pub sector: Vec<u8>,
    pub price: Vec<f64>,
    pub change: Vec<f64>,
    pub volume: Vec<f64>,
    pub hist: Vec<[f64; 16]>,
    pub volumes: [f64; 8],
    pub value: f64,
    pub series: [f64; 120],
    pub tick: u32,
    rng: Lcg,
    /// The constant orders: by symbol, and by sector name (`sector_start[r]..sector_start[r + 1]` the rows of
    /// the `r`th name, in row order).
    pub by_symbol: Vec<u32>,
    pub by_sector: Vec<u32>,
    sector_start: [usize; 9],
    /// Price, change and volume (`Col as usize - 2`).
    pub kept: [Buckets; 3],
    /// A bit a row: which rows' symbols hold each character, which rows are in each sector.
    has_char: Vec<Vec<u64>>,
    in_sector: Vec<Vec<u64>>,
    /// The tick a row was last written in.
    pub stamp: Vec<u32>,
}

/// (key, row) of every row in order of key, ties by row: a least-significant-digit radix sort with 16-bit
/// digits (four passes at most, a digit every key shares skipped: a symbol's low bytes are zero), stable
/// over the rows' order.
fn sorted_by(n: usize, key: impl Fn(usize) -> u64) -> Vec<(u64, u32)> {
    let mut w: Vec<(u64, u32)> = (0..n).map(|i| (key(i), i as u32)).collect();
    let mut counts = vec![0u32; 4 << 16];
    for &(k, _) in &w {
        for d in 0..4 {
            counts[d << 16 | (k >> (16 * d)) as usize & 0xffff] += 1;
        }
    }
    let mut scratch = vec![(0u64, 0u32); n];
    for d in 0..4 {
        let c = &mut counts[d << 16..(d + 1) << 16];
        if c.iter().any(|&x| x as usize == n) {
            continue;
        }
        let mut at = 0;
        for x in c.iter_mut() {
            (*x, at) = (at, at + *x);
        }
        for &(k, i) in &w {
            let b = &mut c[(k >> (16 * d)) as usize & 0xffff];
            scratch[*b as usize] = (k, i);
            *b += 1;
        }
        std::mem::swap(&mut w, &mut scratch);
    }
    w
}

impl Market {
    /// `data.rows(count)` then `data.series(120)` (state.luau's order), and the totals state.luau keeps.
    pub fn new(n: usize) -> Market {
        let t0 = crate::instructions();
        let mut rng = Lcg(12345.0);
        let words = n.div_ceil(64);
        let (mut sym, mut sector) = (Vec::with_capacity(n), Vec::with_capacity(n));
        let (mut price, mut change, mut volume, mut hist) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        let mut has_char = vec![vec![0u64; words]; 36];
        let mut in_sector = vec![vec![0u64; words]; 8];
        for i in 1..=n {
            let r = i - 1;
            let (a, b, c) = (i % 26, i / 26 % 26, i / 676 % 26);
            let p0 = 5.0 + rng.random() * 495.0;
            let mut h = [0f64; 16];
            let mut p = p0;
            for x in h.iter_mut() {
                p = f64::max(1.0, p * (1.0 + (rng.random() - 0.5) * 0.04));
                *x = p;
            }
            let mut bytes = [0u8; 8];
            bytes[..3].copy_from_slice(&[LETTERS[a], LETTERS[b], LETTERS[c]]);
            let d = i % 97;
            let nd = if d >= 10 {
                bytes[3] = b'0' + (d / 10) as u8;
                bytes[4] = b'0' + (d % 10) as u8;
                5
            } else {
                bytes[3] = b'0' + d as u8;
                4
            };
            for &ch in &bytes[..nd] {
                has_char[char_index(ch).unwrap()][r / 64] |= 1 << (r % 64);
            }
            sym.push(u64::from_be_bytes(bytes));
            let s = (i % 8) as u8;
            in_sector[s as usize][r / 64] |= 1 << (r % 64);
            sector.push(s);
            price.push(p0);
            change.push((rng.random() - 0.5) * 10.0);
            volume.push((rng.random() * 5_000_000.0).floor());
            hist.push(h);
        }
        let t_gen = crate::instructions();
        let mut series = [0f64; 120];
        let mut v = 100.0;
        for x in series.iter_mut() {
            v = f64::max(10.0, v + (rng.random() - 0.5) * 6.0);
            *x = v;
        }
        let mut volumes = [0f64; 8];
        let mut value = 0.0;
        for i in 0..n {
            volumes[sector[i] as usize] += volume[i];
            value += price[i] * volume[i];
        }
        let t_sym = crate::instructions();
        let by_symbol = sorted_by(n, |i| sym[i]).into_iter().map(|w| w.1).collect();
        // by sector name, ties by row: a counting sort (every row's key is one of eight)
        let mut sector_start = [0usize; 9];
        for &s in &sector {
            sector_start[SECTOR_RANK[s as usize] as usize + 1] += 1;
        }
        for k in 1..9 {
            sector_start[k] += sector_start[k - 1];
        }
        let mut at = sector_start;
        let mut by_sector = vec![0u32; n];
        for (i, &s) in sector.iter().enumerate() {
            let a = &mut at[SECTOR_RANK[s as usize] as usize];
            by_sector[*a] = i as u32;
            *a += 1;
        }
        let t_kept = crate::instructions();
        let kept = [
            Buckets::cut(&sorted_by(n, |i| fkey(price[i]))),
            Buckets::cut(&sorted_by(n, |i| fkey(change[i]))),
            Buckets::cut(&sorted_by(n, |i| fkey(volume[i]))),
        ];
        if std::env::var_os("DASH_PROBE").is_some() { eprintln!("gen {} totals {} symbol+sector {} kept {}", t_gen - t0, t_sym - t_gen, t_kept - t_sym, crate::instructions() - t_kept); }
        Market { n, sym, sector, price, change, volume, hist, volumes, value, series, tick: 0, rng, by_symbol, by_sector, sector_start, kept, has_char, in_sector, stamp: vec![0; n] }
    }

    /// Row `i`'s sort word for column `c` (ascending).
    #[inline]
    pub fn key(&self, c: Col, i: u32) -> u64 {
        let i = i as usize;
        match c {
            Col::Symbol => self.sym[i],
            Col::Sector => SECTOR_RANK[self.sector[i] as usize] as u64,
            Col::Price => fkey(self.price[i]),
            Col::Change => fkey(self.change[i]),
            Col::Volume => fkey(self.volume[i]),
        }
    }
    /// Visit column `c`'s order (ascending or descending; ties by row either way) until `f` says stop.
    pub fn visit(&self, c: Col, desc: bool, mut f: impl FnMut(u32) -> bool) {
        match c {
            Col::Symbol => {
                // no two symbols are equal: descending is the order reversed
                if desc {
                    for &i in self.by_symbol.iter().rev() {
                        if !f(i) {
                            return;
                        }
                    }
                } else {
                    for &i in &self.by_symbol {
                        if !f(i) {
                            return;
                        }
                    }
                }
            }
            Col::Sector => {
                for r in 0..8 {
                    let r = if desc { 7 - r } else { r };
                    for &i in &self.by_sector[self.sector_start[r]..self.sector_start[r + 1]] {
                        if !f(i) {
                            return;
                        }
                    }
                }
            }
            _ => self.kept[c as usize - 2].visit(desc, f),
        }
    }
    /// The symbol's text.
    pub fn symbol(&self, i: u32) -> Str16 {
        let b = self.sym[i as usize].to_be_bytes();
        let n = b.iter().position(|&x| x == 0).unwrap_or(8);
        Str16::new(std::str::from_utf8(&b[..n]).unwrap())
    }
    /// Does row `i`'s symbol hold `needle` (zgx's `contains`, case kept)?
    pub fn sym_contains(&self, i: u32, needle: &[u8]) -> bool {
        let b = self.sym[i as usize].to_be_bytes();
        let n = b.iter().position(|&x| x == 0).unwrap_or(8);
        needle.is_empty() || (needle.len() <= n && b[..n].windows(needle.len()).any(|w| w == needle))
    }

    /// The rows a sector (0: all) and a search (upper case) let through, as a bitmap into `out`, and how
    /// many. Each character's bitmap is ANDed in; a search of more than one character then verifies the rows
    /// that hold every character of it.
    pub fn filter(&self, sector: u8, needle: &[u8], out: &mut Vec<u64>) -> usize {
        let words = self.n.div_ceil(64);
        out.clear();
        if sector != 0 {
            out.extend_from_slice(&self.in_sector[sector as usize - 1]);
        } else {
            out.resize(words, !0);
            if self.n % 64 != 0 {
                out[words - 1] = (1u64 << (self.n % 64)) - 1;
            }
        }
        for &c in needle {
            match char_index(c) {
                Some(k) => {
                    for (o, &m) in out.iter_mut().zip(&self.has_char[k]) {
                        *o &= m;
                    }
                }
                None => out.fill(0),
            }
        }
        if needle.len() > 1 {
            for w in 0..words {
                let mut bits = out[w];
                while bits != 0 {
                    let b = bits.trailing_zeros();
                    bits &= bits - 1;
                    if !self.sym_contains((w * 64) as u32 + b, needle) {
                        out[w] &= !(1 << b);
                    }
                }
            }
        }
        out.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// actions.luau `feed`: 300 rows move (price, change, volume, history), each written back whole, the
    /// totals by what changed, the series a point; each write re-places its row in the kept orders of price,
    /// change and volume (out of the bucket of its old key, into the bucket of its new one).
    pub fn feed(&mut self) {
        self.tick += 1;
        let n = self.n;
        let mut volumes = self.volumes;
        let mut value = self.value;
        for _ in 0..300 {
            let i = (self.rng.random() * n as f64).floor() as usize;
            self.stamp[i] = self.tick;
            let (p, ch, v) = (self.price[i], self.change[i], self.volume[i]);
            let moved = p * (1.0 + (self.rng.random() - 0.5) * 0.01);
            let traded = (self.rng.random() * 20_000.0).floor();
            value += moved * (v + traded) - p * v;
            volumes[self.sector[i] as usize] += traded;
            let ch2 = ch + (moved - p) / p * 100.0;
            self.change[i] = ch2;
            self.price[i] = moved;
            self.volume[i] = v + traded;
            let h = &mut self.hist[i];
            h.copy_within(1.., 0);
            h[15] = moved;
            let r = i as u32;
            for (d, (old, new)) in [(p, moved), (ch, ch2), (v, v + traded)].into_iter().enumerate() {
                let (ko, kn) = (fkey(old), fkey(new));
                if ko != kn {
                    self.kept[d].remove(ko, r);
                    self.kept[d].insert(kn, r);
                }
            }
        }
        self.volumes = volumes;
        self.value = value;
        let last = self.series[119];
        self.series.copy_within(1.., 0);
        self.series[119] = f64::max(10.0, last + (self.rng.random() - 0.5) * 6.0);
    }
}

// =============================================================================== the view

/// What the table shows: column `c`'s kept order (descending or not), the rows `pass` lets through (empty:
/// every row), from row `first`, as far as `out` holds.
pub fn window(mk: &Market, c: Col, desc: bool, pass: &[u64], first: usize, out: &mut [Option<u32>]) {
    out.fill(None);
    let (mut seen, mut k) = (0usize, 0usize);
    if out.is_empty() {
        return;
    }
    mk.visit(c, desc, |i| {
        if !pass.is_empty() && pass[i as usize / 64] >> (i % 64) & 1 == 0 {
            return true;
        }
        if seen >= first {
            out[k] = Some(i);
            k += 1;
        }
        seen += 1;
        k < out.len()
    });
}

// =============================================================================== the view's values

/// A row's sparkline: its 16 prices placed in the 80 x 20 box (zgx's `trend` canvas), and its colour.
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub struct Spark {
    pub y: [f32; 16],
    pub neg: bool,
    pub on: bool,
}
impl Spark {
    fn of(h: &[f64; 16], neg: bool) -> Spark {
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for &v in h {
            low = low.min(v);
            high = high.max(v);
        }
        let span = (high - low).max(0.0001);
        let mut y = [0f32; 16];
        for (k, &v) in h.iter().enumerate() {
            y[k] = (20.0 - (v - low) / span * 20.0) as f32;
        }
        Spark { y, neg, on: true }
    }
}
/// Segment `i` of a sparkline: the 1.5-point stroke between two prices as the box spanning them (bough has
/// no path stroke; a segment's box covers its line).
fn spark_lane(s: &Spark, i: usize) -> Lane {
    if !s.on {
        return Lane { x: 0.0, y: 0.0, w: 0.0, h: 0.0, color: CLEAR };
    }
    let step = 80.0 / 15.0;
    let (a, b) = (s.y[i], s.y[i + 1]);
    Lane { x: i as f32 * step, y: a.min(b) - 0.75, w: step, h: (a - b).abs() + 1.5, color: if s.neg { RED } else { GREEN } }
}

/// The line chart's 120 points placed in its 336 x 180 box (charts.luau `line`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Series {
    pub y: [f32; 120],
}
impl Series {
    fn of(v: &[f64; 120]) -> Series {
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for &x in v {
            low = low.min(x);
            high = high.max(x);
        }
        let span = (high - low).max(1.0);
        let mut y = [0f32; 120];
        for (k, &x) in v.iter().enumerate() {
            y[k] = (CHART_H - 12.0 - (x - low) / span * (CHART_H - 24.0)) as f32;
        }
        Series { y }
    }
}
const CHART_W: f64 = 336.0;
const CHART_H: f64 = 180.0;
const BARS_H: f32 = 160.0;
fn line_lane(s: &Series, i: usize) -> Lane {
    let step = (CHART_W as f32 - 16.0) / 119.0;
    let (a, b) = (s.y[i], s.y[i + 1]);
    Lane { x: 8.0 + i as f32 * step, y: a.min(b) - 1.0, w: step, h: (a - b).abs() + 2.0, color: ACCENT }
}
/// The sectors' volumes as fractions of the largest (charts.luau `bars`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bars {
    pub f: [f32; 8],
}
impl Bars {
    fn of(v: &[f64; 8]) -> Bars {
        let most = v.iter().fold(0f64, |a, &x| a.max(x));
        Bars { f: v.map(|x| (x / most) as f32) }
    }
}
fn bar_lane(b: &Bars, i: usize) -> Lane {
    let bar = (CHART_W as f32 - 16.0) / 8.0;
    let bh = b.f[i] * (BARS_H - 24.0);
    Lane { x: 8.0 + i as f32 * bar + 2.0, y: BARS_H - 8.0 - bh, w: bar - 4.0, h: bh, color: ACCENT }
}

// =============================================================================== geometry (points)

pub const W: f32 = 1280.0;
pub const H: f32 = 800.0;
/// The toolbar: p 12 around its tallest control (a button: py 8 + 22).
const TOOLBAR_H: f32 = 62.0;
/// The table: right of the 360-point sidebar and the split's 1-point divider; its header row (py 6 + 38).
pub const TABLE_X: f32 = 361.0;
pub const TABLE_W: f32 = W - TABLE_X;
pub const LIST_TOP: f32 = TOOLBAR_H + 50.0;
pub const LIST_H: f32 = H - LIST_TOP;
pub const ROW_H: f32 = 32.0;
/// Row slots: the rows the viewport meets (22) and zgx's two of overscan.
pub const SLOTS: usize = 24;
/// The sparkline's box in a row: after px 8 and the five cells (90, 90, 100, 90, 90) with gaps of 4.
const SPARK_X: f32 = 488.0;
const SPARK_Y: f32 = 6.0;
/// The charts (sidebar p 12, gap 12, under the 24-point summary).
pub const LINE_AT: (f32, f32) = (12.0, TOOLBAR_H + 12.0 + 24.0 + 12.0);
pub const BARS_AT: (f32, f32) = (12.0, LINE_AT.1 + 180.0 + 12.0);
/// Layout paths (child indices from the window's root group) of what the harness clicks.
pub const SEARCH_PATH: [usize; 2] = [0, 1];
pub fn sort_path(c: Col) -> [usize; 4] {
    [1, 2, 0, c as usize]
}

fn thumb_len(n: usize) -> f32 {
    let total = n as f32 * ROW_H;
    if total <= LIST_H { 0.0 } else { (LIST_H * LIST_H / total).max(24.0).min(LIST_H) }
}

// =============================================================================== state

/// A row slot: the cells' texts, the sparkline, whether it is the selected instrument.
#[derive(Default)]
pub struct DashRow {
    pub sel: Signal<bool>,
    pub sym: Signal<Str16>,
    pub sector: Signal<u8>,
    pub price: Signal<Str16>,
    /// The change, in green when it is not negative, in red when it is (two labels, one empty: a label's
    /// colour is not an operand in bough).
    pub up: Signal<Str16>,
    pub down: Signal<Str16>,
    pub vol: Signal<Str16>,
    pub spark: Signal<Spark>,
}

fn sel_bg(s: &bool) -> Color {
    if *s { SELECTION } else { CLEAR }
}
fn sector_name(s: &u8) -> String {
    SECTORS.get(*s as usize).map_or(String::new(), |s| (*s).to_owned())
}

impl View for DashRow {
    /// components/table.luau `row`.
    fn view(&self) -> impl Element<Self> + '_ {
        row((
            label(sig!(self.sym).map(s16)).font(MD.0, MD.1).color(TEXT).w(90.0),
            label(sig!(self.sector).map(sector_name)).font(MD.0, MD.1).color(TEXT2).w(90.0),
            row(label(sig!(self.price).map(s16)).font(MD.0, MD.1).color(TEXT)).w(100.0).justify(Justify::End),
            row((label(sig!(self.up).map(s16)).font(MD.0, MD.1).color(GREEN), label(sig!(self.down).map(s16)).font(MD.0, MD.1).color(RED))).w(90.0).justify(Justify::End),
            row(label(sig!(self.vol).map(s16)).font(MD.0, MD.1).color(TEXT)).w(90.0).justify(Justify::End),
            block(80.0, 20.0),
            at(SPARK_X, SPARK_Y, lanes(15, sig!(self.spark), spark_lane)),
        ))
        .w_pct(1.0)
        .h(ROW_H)
        .px(8.0)
        .gap(4.0)
        .items(Align::Center)
        .bg(sig!(self.sel).map(sel_bg))
    }
}

/// The app: the market, what the user chose, and the signals the view shows.
pub struct Dash {
    pub mk: Market,
    pub sort: Col,
    pub desc: bool,
    /// The rows the sector and the search let through (a bit a row; empty: every row), and how many.
    pass: Vec<u64>,
    pub count: usize,
    pub selected: Option<u32>,
    pub first: usize,
    /// The row each slot shows (`u32::MAX`: none).
    slot_row: [u32; SLOTS],
    pressed: Option<u32>,
    shaper: Option<Arc<bough_text::Shaper>>,
    run: bough_text::GlyphRun,
    field: (f32, f32),
    // ---- what the view shows
    pub rows: List<DashRow>,
    pub query: Signal<String>,
    pub caret: Signal<Caret>,
    /// The sector chosen (0: All, else the sector's index + 1).
    pub sector: Signal<u8>,
    /// Each header's arrow: 0 none, 1 ascending, 2 descending.
    pub h0: Signal<u8>,
    pub h1: Signal<u8>,
    pub h2: Signal<u8>,
    pub h3: Signal<u8>,
    pub h4: Signal<u8>,
    pub value: Signal<Str16>,
    pub series: Signal<Series>,
    pub bars: Signal<Bars>,
    pub live: Signal<bool>,
    pub thumb: Signal<f32>,
}

fn header_text(name: &'static str, d: u8) -> String {
    match d {
        1 => format!("{name} \u{2191}"),
        2 => format!("{name} \u{2193}"),
        _ => name.to_owned(),
    }
}
const VALUE_FMT: NumberOptions = NumberOptions { decimals: None, percent: false, compact: true, except_zero: false, currency: Some("$") };
const PRICE_FMT: NumberOptions = NumberOptions { decimals: Some(2), percent: false, compact: false, except_zero: false, currency: None };
const CHANGE_FMT: NumberOptions = NumberOptions { decimals: Some(2), percent: true, compact: false, except_zero: true, currency: None };
const VOLUME_FMT: NumberOptions = NumberOptions { decimals: None, percent: false, compact: true, except_zero: false, currency: None };

impl Dash {
    /// state.luau with `ui.args = [rows, "paused"]`: sorted by symbol, nothing filtered, the feed paused.
    pub fn new(n: usize) -> Dash {
        Dash::with_market(Market::new(n))
    }
    pub fn with_market(mk: Market) -> Dash {
        let value = Signal::new(number(mk.value, &VALUE_FMT));
        let series = Signal::new(Series::of(&mk.series));
        let bars = Signal::new(Bars::of(&mk.volumes));
        let thumb = Signal::new(thumb_len(mk.n));
        Dash {
            pass: Vec::with_capacity(mk.n.div_ceil(64)),
            count: mk.n,
            mk,
            sort: Col::Symbol,
            desc: false,
            selected: None,
            first: 0,
            slot_row: [u32::MAX; SLOTS],
            pressed: None,
            shaper: None,
            run: Default::default(),
            field: (0.0, 0.0),
            rows: List::from_iter_in((0..SLOTS).map(|_| DashRow::default())),
            query: Signal::new(String::new()),
            caret: Signal::new(Caret::default()),
            sector: Signal::new(0),
            h0: Signal::new(1),
            h1: Signal::new(0),
            h2: Signal::new(0),
            h3: Signal::new(0),
            h4: Signal::new(0),
            value,
            series,
            bars,
            live: Signal::new(false),
            thumb,
        }
    }
    /// A fresh model at the state `o` reached (market, sort, filters, selection, caret): for the gate that a
    /// retained app equals a fresh mount.
    pub fn same_state(o: &Dash) -> Dash {
        let mut d = Dash::with_market(o.mk.clone());
        d.sort = o.sort;
        d.desc = o.desc;
        d.selected = o.selected;
        d.first = o.first;
        d.query = Signal::new((*o.query).clone());
        d.caret = Signal::new(*o.caret);
        d.sector = Signal::new(*o.sector);
        for (k, c) in COLS.iter().enumerate() {
            let v = if *c == o.sort { 1 + o.desc as u8 } else { 0 };
            *d.header(k) = Signal::new(v);
        }
        d.field = o.field;
        d
    }
    fn header(&mut self, k: usize) -> &mut Signal<u8> {
        match k {
            0 => &mut self.h0,
            1 => &mut self.h1,
            2 => &mut self.h2,
            3 => &mut self.h3,
            _ => &mut self.h4,
        }
    }

    /// The rows the table shows from `first` (as many as `out` holds).
    pub fn window(&self, first: usize, out: &mut [Option<u32>]) {
        window(&self.mk, self.sort, self.desc, &self.pass, first, out);
    }

    /// The filter again (a search, a sector): its bitmap and its count (the scrollbar's length).
    pub fn refilter(&mut self) {
        let q = self.query.to_uppercase();
        let sector = *self.sector;
        if sector != 0 || !q.is_empty() {
            self.count = self.mk.filter(sector, q.as_bytes(), &mut self.pass);
        } else {
            self.pass.clear();
            self.count = self.mk.n;
        }
        self.thumb.set(thumb_len(self.count));
    }

    /// Slot `k` shows row `i` (or nothing).
    fn fill(&mut self, k: usize, i: Option<u32>) {
        let sel = i.is_some() && i == self.selected;
        let mk = &self.mk;
        let r = self.rows.edit(k);
        r.sel.set(sel);
        match i {
            Some(i) => {
                let x = i as usize;
                r.sym.set(mk.symbol(i));
                r.sector.set(mk.sector[x]);
                r.price.set(number(mk.price[x], &PRICE_FMT));
                let c = number(mk.change[x] / 100.0, &CHANGE_FMT);
                let neg = mk.change[x] < 0.0;
                r.up.set(if neg { Str16::EMPTY } else { c });
                r.down.set(if neg { c } else { Str16::EMPTY });
                r.vol.set(number(mk.volume[x], &VOLUME_FMT));
                r.spark.set(Spark::of(&mk.hist[x], neg));
            }
            None => {
                r.sym.set(Str16::EMPTY);
                r.sector.set(u8::MAX);
                r.price.set(Str16::EMPTY);
                r.up.set(Str16::EMPTY);
                r.down.set(Str16::EMPTY);
                r.vol.set(Str16::EMPTY);
                r.spark.set(Spark::default());
            }
        }
        self.slot_row[k] = i.unwrap_or(u32::MAX);
    }
    /// The slots follow the view: a slot is written when its row changed, or (after a feed tick) its row was
    /// written by the tick; the cells' signals cut off what did not change.
    fn refill(&mut self, after_feed: bool) {
        let mut win = [None; SLOTS];
        self.window(self.first, &mut win);
        for (k, &i) in win.iter().enumerate() {
            let same = i.unwrap_or(u32::MAX) == self.slot_row[k];
            let written = after_feed && i.is_some_and(|i| self.mk.stamp[i as usize] == self.mk.tick);
            if !same || written {
                self.fill(k, i);
            }
        }
    }

    // ------------------------------------------------------------------ actions.luau / table.luau

    /// The `refresh` command: one feed tick.
    pub fn feed(&mut self) {
        self.mk.feed();
        self.value.set(number(self.mk.value, &VALUE_FMT));
        self.series.set(Series::of(&self.mk.series));
        self.bars.set(Bars::of(&self.mk.volumes));
        self.refill(true);
    }
    /// `actions.sort_by(key)`: the column, descending if it was already the key and ascending.
    pub fn sort_by(&mut self, c: Col) {
        let desc = self.sort == c && !self.desc;
        self.header(self.sort as usize).set(0);
        self.sort = c;
        self.desc = desc;
        self.header(c as usize).set(1 + desc as u8);
        self.refill(false);
    }
    pub fn choose_sector(&mut self, s: u8) {
        self.sector.set(s);
        self.refilter();
        self.refill(false);
    }
    pub fn press_list(&mut self, y: f32) {
        self.pressed = None;
        if y >= LIST_TOP {
            let k = ((y - LIST_TOP) / ROW_H) as usize;
            self.pressed = self.slot_row.get(k).copied().filter(|&i| i != u32::MAX);
        }
    }
    /// A row clicked: its instrument selected (table.luau `on_click`).
    pub fn click_list(&mut self) {
        if let Some(i) = self.pressed.take() {
            let old = self.selected.replace(i);
            for k in 0..SLOTS {
                let r = self.slot_row[k];
                if Some(r) == old || r == i {
                    self.rows.edit(k).sel.set(r == i);
                }
            }
        }
    }
    pub fn focus_search(&mut self) {
        self.place_caret();
    }
    fn place_caret(&mut self) {
        let x = match &self.shaper {
            Some(s) => text_width(s, &self.query, MD.0, &mut self.run),
            None => 0.0,
        };
        self.caret.set(Caret { x: self.field.0 + x, y: self.field.1, on: true });
    }
    /// A key in the search field: the query edited where it is written, the filter judged again, the slots
    /// refilled, the caret moved.
    pub fn search_key(&mut self, e: &Ev) {
        match e.key {
            Some(Key::Char(c)) => self.query.push(c),
            Some(Key::Backspace) if !self.query.is_empty() => {
                self.query.pop();
            }
            _ => return,
        }
        self.refilter();
        self.refill(false);
        self.place_caret();
    }
    pub fn attach(&mut self, shaper: Arc<bough_text::Shaper>, field: (f32, f32)) {
        self.shaper = Some(shaper);
        self.field = field;
        self.refilter();
        self.refill(false);
        if self.caret.on {
            self.place_caret();
        }
        // The sparklines are lanes (not laid out) inside list rows: their ink was taken at mount, in the
        // row's space before the region placed it, and a row's first placement does not translate it. A
        // write after placement takes it again.
        for k in 0..SLOTS {
            let r = self.rows.edit(k);
            let s = *r.spark;
            *r.spark = s;
        }
    }
}

/// Mount `v` and fill it.
pub fn mount(v: Dash, cfg: Config, gpu: bool) -> App<Dash> {
    let mut app = App::new(v, cfg, gpu);
    let shaper = app.rt.text.shaper.clone();
    let f = lay_rect(&app.rt, &SEARCH_PATH);
    app.turn(|d| d.attach(shaper, (f[0] + 9.0, f[1] + 9.0)));
    app
}

fn vline<V: 'static>() -> impl Element<V> {
    block(1.0, 0.0).h_pct(1.0).shrink(0.0).bg(BORDER)
}

impl Dash {
    /// app.luau `toolbar`.
    fn toolbar(&self) -> impl Element<Dash> + '_ {
        let field = row((
            label(sig!(self.query)).font(MD.0, MD.1).color(TEXT),
            label(sig!(self.query).map(|q: &String| if q.is_empty() { "Search symbols".to_owned() } else { String::new() })).font(MD.0, MD.1).color(TEXT3),
        ))
        .w(220.0)
        .h(36.0)
        .px(9.0)
        .items(Align::Center)
        .bg(RAISED)
        .rounded(6.0)
        .border(1.0, BORDER)
        .on_key(|d: &mut Dash, e: &Ev| d.search_key(e))
        .on_click(|d: &mut Dash| d.focus_search());
        let chip = |s: u8| {
            let name = if s == 0 { "All" } else { SECTORS[s as usize - 1] };
            button(name).bg(sig!(self.sector).select(s, ACCENT, RAISED)).hover_bg(BUTTON_HOVER).on_click(move |d: &mut Dash| d.choose_sector(s))
        };
        row((
            label("Markets").font(XL.0, XL.1).color(TEXT),
            field,
            row(each((0..9u8).map(chip))).gap(4.0),
            block(0.0, 0.0).grow(1.0),
            row((block(16.0, 16.0).bg(RAISED).rounded(4.0).border(1.0, BORDER_STRONG), label("Live").font(MD.0, MD.1).color(TEXT))).gap(6.0).px(4.0).py(4.0).items(Align::Center),
            button("Light").hover_bg(BUTTON_HOVER).on_click(|_: &mut Dash| {}),
        ))
        .gap(8.0)
        .p(12.0)
        .items(Align::Center)
    }
    /// charts.luau: the summary, the price line, the sectors' volumes (their boxes; the marks are lanes over
    /// them, see `view`).
    fn sidebar(&self) -> impl Element<Dash> + '_ {
        column((
            label(sig!(self.value).map(|v: &Str16| format!("Market value {}", v.as_str()))).font(LG.0, LG.1).color(TEXT),
            block(CHART_W as f32, CHART_H as f32).bg(RAISED).rounded(8.0),
            block(CHART_W as f32, BARS_H).bg(RAISED).rounded(8.0),
        ))
        .w(360.0)
        .shrink(0.0)
        .h_pct(1.0)
        .p(12.0)
        .gap(12.0)
    }
    /// components/table.luau: the header's sort buttons, the list.
    fn table(&self) -> impl Element<Dash> + '_ {
        let head = |k: usize| {
            let sig = match k {
                0 => sig!(self.h0),
                1 => sig!(self.h1),
                2 => sig!(self.h2),
                3 => sig!(self.h3),
                _ => sig!(self.h4),
            };
            let name = COL_LABEL[k];
            let col = COLS[k];
            button(sig.map(move |d: &u8| header_text(name, *d))).w(COL_W[k]).hover_bg(BUTTON_HOVER).on_click(move |d: &mut Dash| d.sort_by(col))
        };
        column((
            row((each((0..5).map(head)), label("Trend").font(MD.0, MD.1).color(TEXT2).w(80.0))).gap(4.0).px(8.0).py(6.0).items(Align::Center),
            clip(
                TABLE_X,
                LIST_TOP,
                TABLE_W,
                LIST_H,
                list(sig!(self.rows)).w_pct(1.0).h(LIST_H).on_press(|d: &mut Dash, e: &Ev| d.press_list(e.y)).on_click(|d: &mut Dash| d.click_list()),
            ),
        ))
        .w(0.0)
        .grow(1.0)
        .h_pct(1.0)
    }
}

impl View for Dash {
    /// app.luau: the toolbar over the split (sidebar | table).
    fn view(&self) -> impl Element<Self> + '_ {
        (
            column((self.toolbar(), row((self.sidebar(), vline(), self.table())).grow(1.0).min_h(0.0))).w(W).h(H).bg(BASE),
            // the charts' marks (charts.luau's canvases: a stroke and bars), lanes over their boxes
            at(LINE_AT.0, LINE_AT.1, lanes(119, sig!(self.series), line_lane)),
            at(BARS_AT.0, BARS_AT.1, lanes(8, sig!(self.bars), bar_lane).rounded(3.0)),
            // the table's scrollbar (always shown) and the field's caret
            quad(W - 8.0, 0.0, 8.0, 0.0).bg(TRACK).span(sig!(self.thumb).map(|t: &f32| (LIST_TOP, if *t > 0.0 { LIST_H } else { 0.0 }))),
            quad(W - 7.0, 0.0, 6.0, 0.0).bg(ACCENT).rounded(3.0).span(sig!(self.thumb).map(|t: &f32| (LIST_TOP + 2.0, (*t - 4.0).max(0.0)))),
            lanes(1, sig!(self.caret), caret_lane),
        )
    }
}
