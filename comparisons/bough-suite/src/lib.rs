//! zgx's `scripts/perf/suite.py` scenes and `crates/zgx/examples` benches, ported natively onto bough.
//! Each scene's view lives here (the binaries and the pixel gates in `tests/` share it); the workload, the
//! tree and the counts are zgx's, read from its source (see RESULTS: `crates/bough/API-RESULTS.md`).
use bough::layout::prelude::*;
use bough::prelude::*;

pub use zgx_workload::FONT_BYTES;

pub fn font() -> Vec<u8> {
    zgx_workload::font_file().unwrap_or_else(|| FONT_BYTES.to_vec())
}

/// zgx's "normal" line pitch.
pub fn normal(size: f32) -> f32 {
    (size * 1.4).ceil()
}

/// zgx's headless `Ui::new(w, h)` (points, no renderer): bough at scale 1, heap region; `gpu` for the
/// pixel gates.
pub fn app<V: View>(v: V, w: f32, h: f32, gpu: bool) -> App<V> {
    App::new(v, Config::new(w, h, 1.0, font()), gpu)
}

/// `app` with the region's columns sized for `rows` rows of about `recs` records and `glyphs` glyphs
/// each (the columns are fixed at mount; untouched capacity costs no memory).
pub fn app_sized<V: View>(v: V, w: f32, h: f32, gpu: bool, rows: usize, recs: usize, glyphs: usize) -> App<V> {
    let mut cfg = Config::new(w, h, 1.0, font());
    let d = Caps::DEFAULT;
    let n = rows.max(1);
    cfg.caps = Caps {
        instances: d.instances.max((n * recs * 2).next_power_of_two()),
        pages: d.pages.max(n * recs * 2 / 32 + 64),
        glyphs: d.glyphs.max((n * glyphs * 2).next_power_of_two()),
        runs: d.runs.max(n * recs * 2),
        ..d
    };
    App::new(v, cfg, gpu)
}

pub fn env_usize(name: &str, d: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

/// Instructions this process has retired (`proc_pid_rusage`, `ri_instructions`: zgx's `mount_bench`).
pub fn instructions() -> u64 {
    bough_metal::perf::snap().instrs
}

/// Instructions this thread has retired (zgx's `update_bench` / `teardown_bench`: `thread_selfcounts`).
pub fn thread_instructions() -> u64 {
    unsafe extern "C" {
        fn thread_selfcounts(kind: i32, buffer: *mut u64, size: usize) -> i32;
    }
    let mut c = [0u64; 2];
    // SAFETY: THSC_CPI (1) fills two u64s: instructions, cycles.
    unsafe { thread_selfcounts(1, c.as_mut_ptr(), 16) };
    c[0]
}

pub const fn c(r: u8, g: u8, b: u8) -> Color {
    Color::rgba(r, g, b, 255)
}

// ======================================================================= mount (zgx-script mount_bench)

/// `N` rows of two texts, padded, rounded and coloured, in a column (zgx's `rows(n)`).
pub struct MountRows {
    pub n: usize,
}

pub fn mount_row<V: 'static>(i: usize) -> impl Element<V> {
    row((label(format!("Row {i}")).font(13.0, normal(13.0)), label(format!("{} MB", i * 3)).font(12.0, normal(12.0)).color(c(0x88, 0x99, 0xaa))))
        .gap(8.0)
        .px(12.0)
        .py(6.0)
        .bg(c(0x1b, 0x22, 0x30))
        .rounded(6.0)
}

impl View for MountRows {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        column(each((1..=self.n).map(mount_row))).w(1200.0).gap(4.0)
    }
}

// ================================================================== teardown (zgx teardown_bench)

/// zgx's `teardown_bench` rows (no rounding, default text sizes), mounted under an `Option` so a
/// re-description removes them (zgx: `mounted.unmount()`).
pub struct Teardown {
    pub n: usize,
    pub show: bool,
}

pub fn teardown_row<V: 'static>(i: usize) -> impl Element<V> {
    row((label(format!("Row {i}")).font(16.0, normal(16.0)), label(format!("{} MB", i * 3)).font(16.0, normal(16.0)).color(c(0x88, 0x99, 0xaa))))
        .gap(8.0)
        .px(12.0)
        .py(6.0)
        .bg(c(0x1b, 0x22, 0x30))
}

impl View for Teardown {
    fn view(&self) -> impl Element<Self> + '_ {
        self.show.then(|| column(each((1..=self.n).map(teardown_row))).w(1200.0))
    }
}

// ============================================================ script-update (zgx-script update_bench)

pub const ROWS: usize = 200;
pub const CHANGED: usize = 10;

pub fn upd_label(row: usize, tick: usize) -> String {
    format!("Row {row} at {tick}")
}
pub fn upd_published(ticks: usize) -> Vec<Vec<String>> {
    (0..=ticks).map(|tick| (0..ROWS).map(|i| upd_label(i, if (i + tick).is_multiple_of(ROWS / CHANGED) { tick } else { 0 })).collect()).collect()
}

/// `template`: `text("Count: {count}")`.
pub struct UTemplate {
    pub count: Signal<u32>,
}
impl View for UTemplate {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        text(sig!(self.count).num(|c| *c as f32, NumFmt::new("Count: ", None, ""))).size(16.0).line_height(normal(16.0))
    }
}

/// `function-prop`: 200 boxes whose colour is `selected == i`.
pub struct USelect {
    pub selected: Signal<usize>,
}
pub const ORANGE: Color = c(0xff, 0x88, 0x00);
pub const SLATE: Color = c(0x22, 0x33, 0x44);
impl View for USelect {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        column(each((1..=ROWS).map(|i| block(20.0, 20.0).bg(sig!(self.selected).select(i, ORANGE, SLATE)))))
    }
}

/// `rows` / `rows-in-place` (the Rust variant of both): 200 keyed rows of a label each.
pub struct URow {
    pub label: Signal<String>,
}
impl View for URow {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        row(label(sig!(self.label)).font(13.0, normal(13.0))).gap(8.0).px(6.0)
    }
}
pub struct URows {
    pub rows: List<URow>,
}
impl URows {
    pub fn new() -> URows {
        URows { rows: List::from_iter_in((0..ROWS).map(|i| URow { label: Signal::new(upd_label(i, 0)) })) }
    }
}
impl Default for URows {
    fn default() -> Self {
        URows::new()
    }
}
impl View for URows {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.rows))
    }
}
/// One update of `rows`: the published labels that differ are set (zgx: `if signal != next { set }`).
pub fn urows_step(v: &mut URows, next: &[String]) {
    for (i, n) in next.iter().enumerate() {
        if *v.rows.get(i).unwrap().label != *n {
            v.rows.edit(i).label.set(n.clone());
        }
    }
}

/// `handler`: a button (80 x 30) with "Clicks", and the count as text below.
pub struct UHandler {
    pub count: Signal<u32>,
}
impl View for UHandler {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            row(label("Clicks").font(16.0, normal(16.0))).w(80.0).h(30.0).bg(c(0x2a, 0x3a, 0x50)),
            label(sig!(self.count).num(|c| *c as f32, NumFmt::new("", None, ""))).font(16.0, normal(16.0)),
        ))
    }
}
