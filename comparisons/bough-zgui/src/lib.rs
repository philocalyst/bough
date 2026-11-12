//! zgui's interaction benchmarks on bough (`cargo xtask perf` in $BOUGH_REF/zgui: the scenarios of
//! `crates/zgui-bench/src/scenario` and the `label-churn` / `resize-slope` probes). Each document here is
//! zgui's fixture rebuilt with bough builders (same window, rows, cells, strings, colours, row heights),
//! and each driver in `bin/` delivers zgui's event sequence and times zgui's span: one event delivered and
//! the loop run until quiet (bough: the input turn plus `frame()`, which ends at the frame slot's tile
//! list as zgui's `NullRenderer` ends at its display list), plus retired instructions for the same span.
//!
//! zgui runs its headless harness at one device pixel per CSS pixel; so do these (`scale = 1`).
use bough::raw::*;
use std::ops::Range;

pub mod gallery;
pub mod inspector;
pub mod shell;
pub mod thread;

pub fn font_bytes() -> Vec<u8> {
    zgx_workload::font_file().unwrap_or_else(|| zgx_workload::FONT_BYTES.to_vec())
}

pub fn ring_from_env() -> usize {
    std::env::var("BOUGH_RING").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// DejaVu's bold and mono faces (the documents' `font-weight >= 600` and monospace text), from
/// `BOUGH_FONT_BOLD` / `BOUGH_FONT_MONO` or the system's DejaVu 2.37 install; absent files fall back to the
/// regular face.
fn face_files() -> Vec<(u8, Vec<u8>)> {
    const DIR: &str = "/Library/Fonts/Nix Fonts/vl9mbzk97wmh1s1yj3766b4zai8msjq6-dejavu-fonts-2.37/share/fonts/truetype";
    let mut v = Vec::new();
    for (logical, var, file) in [(bough::text::FACE_BOLD, "BOUGH_FONT_BOLD", "DejaVuSans-Bold.ttf"), (bough::text::FACE_MONO, "BOUGH_FONT_MONO", "DejaVuSansMono.ttf")] {
        let path = std::env::var(var).unwrap_or_else(|_| format!("{DIR}/{file}"));
        if let Ok(b) = std::fs::read(&path) {
            v.push((logical, b));
        }
    }
    v
}

pub fn config(w: f32, h: f32) -> Config {
    let mut c = Config::new(w, h, 1.0, font_bytes()).once();
    c.ring = ring_from_env();
    c.faces = face_files();
    c
}

/// zgui's gallery window (`gallery::WIDTH`, `HEIGHT`).
pub const WIDTH: f32 = 1600.0;
pub const HEIGHT: f32 = 1000.0;

/// zgx / CSS "normal" line height for a font size (the documents' text boxes are top aligned in it).
pub fn normal(size: f32) -> f32 {
    (size * 1.4).ceil()
}

// ================================================================================ measuring

/// One sampled span: wall time (µs) and retired instructions (process-wide, `proc_pid_rusage`, the
/// snapshot's own cost subtracted).
#[derive(Default)]
pub struct Samples {
    pub us: Vec<f64>,
    pub instr: Vec<f64>,
    ovh: u64,
}

impl Samples {
    pub fn new() -> Samples {
        Samples { us: Vec::new(), instr: Vec::new(), ovh: bough::sys::perf::snap_overhead() }
    }
    #[inline]
    pub fn time<R>(&mut self, f: impl FnOnce() -> R) -> R {
        let i0 = bough::sys::perf::snap().instrs;
        let t0 = std::time::Instant::now();
        let r = f();
        let dt = t0.elapsed();
        let i1 = bough::sys::perf::snap().instrs;
        self.us.push(dt.as_secs_f64() * 1e6);
        self.instr.push(i1.saturating_sub(i0).saturating_sub(self.ovh) as f64);
        r
    }
    /// Measure `f` without recording it (the caller decides the bucket): (µs, instructions, result).
    #[inline]
    pub fn measure<R>(&self, f: impl FnOnce() -> R) -> (f64, f64, R) {
        let i0 = bough::sys::perf::snap().instrs;
        let t0 = std::time::Instant::now();
        let r = f();
        let dt = t0.elapsed();
        let i1 = bough::sys::perf::snap().instrs;
        (dt.as_secs_f64() * 1e6, i1.saturating_sub(i0).saturating_sub(self.ovh) as f64, r)
    }
    pub fn push(&mut self, us: f64, instr: f64) {
        self.us.push(us);
        self.instr.push(instr);
    }
    pub fn overhead(&self) -> u64 {
        self.ovh
    }
    pub fn len(&self) -> usize {
        self.us.len()
    }
    pub fn is_empty(&self) -> bool {
        self.us.is_empty()
    }
}

/// zgui's `Spread`: p50 / p95 / p99 / max of a sorted copy (index `len * q`, as zgui's `stats.rs`).
pub fn spread(v: &[f64]) -> (f64, f64, f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let at = |q: f64| s[((s.len() as f64 * q) as usize).min(s.len() - 1)];
    (at(0.5), at(0.95), at(0.99), *s.last().unwrap())
}

/// Print one measurement the way zgui's sweep prints its `MEASURE` lines, plus an `INSTR` line.
pub fn report(scenario: &str, name: &str, unit: &str, s: &Samples, scale: f64) {
    let us: Vec<f64> = s.us.iter().map(|v| v * scale).collect();
    let (p50, p95, p99, max) = spread(&us);
    println!("MEASURE\t{scenario}\t{name}\t{unit}\t{p50:.4}\tp50={p50:.4};p95={p95:.4};p99={p99:.4};max={max:.4};n={}", s.len());
    let (i50, i95, _, imax) = spread(&s.instr);
    let mut sorted = s.instr.clone();
    sorted.sort_by(f64::total_cmp);
    let p10 = sorted.get(sorted.len() / 10).copied().unwrap_or(0.0);
    println!("INSTR\t{scenario}\t{name}\tp50={i50:.0}\tp10={p10:.0}\tp95={i95:.0}\tmax={imax:.0}\tn={}", s.len());
}

pub fn count(scenario: &str, name: &str, unit: &str, v: f64) {
    println!("MEASURE\t{scenario}\t{name}\t{unit}\t{v:.4}\t-");
}

// ================================================================================ the table (hover storm, idle)

/// zgui `fixture.rs`: `TABLE_ROWS` (hover storm) and `STILL_ROWS` (idle), 24 px rows.
pub const TABLE_ROWS: usize = 1_000;
pub const STILL_ROWS: usize = 833;
pub const ROW_H: f32 = 24.0;

pub const BG: Color = rgb(0x14161a);
pub const FG: Color = rgb(0xe7ecf5);
pub const LINE: Color = rgb(0x232833);
pub const HOVER: Color = rgb(0x1e2532);
pub const FONT: f32 = 13.0;

/// One cell: `.bench-cell { width: 140px }` in a row with `padding: 0 12px; gap: 16px`, text centred
/// vertically in the 23 px content box above the 1 px bottom border.
fn cell<V: 'static>(s: String, k: usize) -> impl Element<V> {
    let lh = normal(FONT);
    text(s).at(12.0 + k as f32 * 156.0, ((ROW_H - 1.0) - lh) * 0.5).size(FONT).line_height(lh).color(FG)
}

/// `.bench-row`: background, bottom border, five cells; `:hover` changes the background.
fn table_row(i: usize) -> impl Element<Table> {
    let y = i as f32 * ROW_H;
    quad(0.0, y, WIDTH, ROW_H - 1.0)
        .bg(BG)
        .children((
            quad(0.0, ROW_H - 1.0, WIDTH, 1.0).bg(LINE),
            cell(format!("row {i}"), 0),
            cell(format!("{}", i * 7 % 977), 1),
            cell("pending".to_string(), 2),
            cell(format!("{}.{:02}", i / 10, i % 100), 3),
            cell("\u{2014}".to_string(), 4),
        ))
        .hover_bg(HOVER)
}

/// zgui's `Table(rows)`: a column of `TableRow`s over the `:root` background.
pub struct Table {
    pub rows: usize,
}

impl View for Table {
    fn view(&self) -> impl Element<Self> + '_ {
        (quad(0.0, 0.0, WIDTH, HEIGHT).bg(BG), each((0..self.rows).map(table_row)))
    }
}

/// zgui `fixture::table_row_centre`: the pointer aim for the `index`th visible row.
pub fn table_row_centre(index: usize) -> (f32, f32) {
    (WIDTH / 2.0, index as f32 * ROW_H + ROW_H / 2.0)
}

/// zgui `fixture::visible_table_rows`.
pub fn visible_table_rows() -> usize {
    ((HEIGHT / ROW_H) as usize).saturating_sub(1).min(TABLE_ROWS)
}

// ================================================================================ the long list (scroll)

pub const LIST_ROWS: usize = 10_000;
/// `.bench-list { height: 1000px }`.
pub const LIST_H: f32 = 1000.0;

/// Points one wheel line moves the list: zgui reads it from the scrolled container's used line height
/// (13 px sans-serif); `LINE_PX` overrides.
pub fn line_px() -> f32 {
    std::env::var("LINE_PX").ok().and_then(|v| v.parse().ok()).unwrap_or(58.5)
}

pub struct LongList {
    pub scroll: Signal<f32>,
}

/// Rows `visible(scroll)` with two rows of overscan each side.
pub fn list_rows(scroll: f32) -> Range<usize> {
    let first = (scroll / ROW_H) as usize;
    let end = ((scroll as f64 + LIST_H as f64) / ROW_H as f64).ceil() as usize;
    first.saturating_sub(2)..end.saturating_add(2).min(LIST_ROWS)
}

fn list_row(_: &LongList, i: usize, m: &mut Mounter<'_, LongList>) {
    m.mount(quad(0.0, 0.0, WIDTH, ROW_H - 1.0).bg(BG).children((
        quad(0.0, ROW_H - 1.0, WIDTH, 1.0).bg(LINE),
        cell(format!("row {i}"), 0),
        cell(format!("{}", i * 7 % 977), 1),
    )))
}

impl View for LongList {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, WIDTH, HEIGHT).bg(BG),
            // the list's port: the wheel target (`VirtualList` is a scroll container)
            quad(0.0, 0.0, WIDTH, LIST_H)
                .bg(BG)
                .children(virtual_list(sig!(self.scroll), ROW_H, [0.0, 0.0, WIDTH, LIST_H], list_rows, list_row))
                .scrolls(sig!(self.scroll), line_px(), LIST_ROWS as f32 * ROW_H - LIST_H),
        )
    }
}

// ================================================================================ label churn

/// zgui `bin/label-churn.rs`: 1000 x 700, thirty 11 px labels around a 900 x 600 plot.
pub const CHURN_W: f32 = 1000.0;
pub const CHURN_H: f32 = 700.0;
pub const LABELS: usize = 30;

pub fn churn_label(zoom: usize, index: usize) -> String {
    let value = ((zoom * 7_919 + index * 104_729) % 200_000) as f64 / 1_000.0 - 100.0;
    format!("{value:.2}")
}

pub fn churn_place(index: usize) -> (f32, f32) {
    if index < LABELS / 2 { ((40 + index * 56 % 840) as f32, 580.0) } else { (4.0, ((index - LABELS / 2) * 38 % 570) as f32) }
}

pub struct Churn {
    pub zoom: Signal<usize>,
}

impl View for Churn {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, CHURN_W, CHURN_H).bg(BG),
            each((0..LABELS).map(|i| {
                let (x, y) = churn_place(i);
                text(sig!(self.zoom).map(move |z| churn_label(*z, i))).at(x, y).size(11.0).line_height(normal(11.0)).color(FG)
            })),
        )
    }
}

// ================================================================================ resize slope

/// zgui `bin/resize-slope`: 1000 x 800; `rows` rows of `{ width 100%; height 18px; padding 2px }`, each
/// holding a cell `{ width 40%; height 14px; background }` whose colour a class on the root switches.
pub const SLOPE_W: f32 = 1000.0;
pub const SLOPE_H: f32 = 800.0;
pub const COOL: Color = Color::rgba(40, 44, 56, 255);
pub const WARM: Color = Color::rgba(90, 60, 40, 255);

pub struct Slope {
    pub rows: usize,
    pub warm: Signal<bool>,
}

impl View for Slope {
    fn view(&self) -> impl Element<Self> + '_ {
        use bough::layout::prelude::*;
        column(each((0..self.rows).map(|_| {
            column(block(0.0, 14.0).w_pct(0.4).bg(sig!(self.warm).map(|w| if *w { WARM } else { COOL }))).w_pct(1.0).h(18.0).p(2.0)
        })))
        .w_pct(1.0)
        .h_pct(1.0)
        .clip_overflow()
    }
}
