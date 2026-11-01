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

// ============================================================================== shared measurement

/// Wall time and process instructions of a span (one `proc_pid_rusage` call each side).
pub struct Span {
    t: std::time::Instant,
    i: u64,
}
impl Span {
    pub fn start() -> Span {
        Span { t: std::time::Instant::now(), i: instructions() }
    }
    /// (seconds, instructions) since `start`.
    pub fn stop(&self) -> (f64, u64) {
        let i = instructions();
        (self.t.elapsed().as_secs_f64(), i - self.i)
    }
}

// ================================================================== anime (zgx-script anime_bench)

pub const ANIME_N: usize = 1000;
const DOT_RGB: (u8, u8, u8) = (0x58, 0xa6, 0xff);

/// zgx's eased phase of dot `i` at `seconds`: stagger 2 ms, 1 s per leg, alternating.
#[inline(always)]
pub fn anime_phase(i: usize, seconds: f32) -> f32 {
    let local = ((seconds * 1000.0 - i as f32 * 2.0) / 1000.0).max(0.0);
    let cycle = local % 2.0;
    let p = if cycle > 1.0 { 2.0 - cycle } else { cycle };
    0.5 - (std::f32::consts::PI * p).cos() / 2.0
}
/// The dot colour #58a6ff at opacity `o` (straight alpha).
#[inline(always)]
pub fn dot_color(o: f32) -> Color {
    Color::rgba(DOT_RGB.0, DOT_RGB.1, DOT_RGB.2, (o * 255.0).round() as u8)
}
#[inline(always)]
fn anime_lane(i: usize, seconds: f32) -> Lane {
    let p = anime_phase(i, seconds);
    Lane { x: (i % 40 * 20) as f32, y: 300.0 * p, w: 6.0, h: 6.0, color: dot_color(1.0 - 0.8 * p) }
}

/// One quad per dot; y and opacity are per-element reactive operands of one clock signal (`seconds`):
/// zgx's "rust signals" (a reactive style per dot), with the clock as the only signal.
pub struct AnimeWires {
    pub t: Signal<f32>,
}
impl View for AnimeWires {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        each((0..ANIME_N as u32).map(|i| {
            quad((i % 40 * 20) as f32, 0.0, 6.0, 6.0)
                .span(sig!(self.t).map(move |t| (300.0 * anime_phase(i as usize, *t), 6.0)))
                .bg(sig!(self.t).map(move |t| dot_color(1.0 - 0.8 * anime_phase(i as usize, *t))))
        }))
    }
}

/// The same dots as one `lanes` element.
pub struct AnimeLanes {
    pub t: Signal<f32>,
}
impl View for AnimeLanes {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        lanes(ANIME_N, sig!(self.t), |t: &f32, i: usize| anime_lane(i, *t))
    }
}

// ============================================================================ style (zgx style_bench)

pub const STYLE_N: usize = 1000;
const STYLE_RGB: (u8, u8, u8) = (0x33, 0x55, 0x77);
/// Box `i` is translated by `style_dx` from its column position (x = 0 in zgx; `STYLE_X0` here keeps the
/// swing, -20..20, inside the window).
pub const STYLE_X0: f32 = 20.0;
#[inline(always)]
pub fn style_dx(i: usize, t: f32) -> f32 {
    ((t + i as f32) * 7.0).sin() * 20.0
}
#[inline(always)]
pub fn style_opacity(i: usize, t: f32) -> f32 {
    0.5 + 0.5 * (t * 3.0 + i as f32).sin()
}
#[inline(always)]
pub fn style_color(i: usize, t: f32) -> Color {
    Color::rgba(STYLE_RGB.0, STYLE_RGB.1, STYLE_RGB.2, (style_opacity(i, t) * 255.0).round() as u8)
}
#[inline(always)]
fn style_lane(i: usize, t: f32) -> Lane {
    Lane { x: STYLE_X0 + style_dx(i, t), y: 4.0 * i as f32, w: 20.0, h: 4.0, color: style_color(i, t) }
}

/// 1000 boxes (20 x 4, column positions y = 4i), each its own one-lane `lanes` element: one wire per box
/// moves it in x and fades it (bough has no translate/opacity operand on a quad).
pub struct StyleWires {
    pub t: Signal<f32>,
}
impl View for StyleWires {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        each((0..STYLE_N as u32).map(|i| lanes(1, sig!(self.t), move |t: &f32, _| style_lane(i as usize, *t))))
    }
}

/// Fade only: a quad per box with a reactive colour (a quad's x is constant), no translation.
pub struct StyleQuads {
    pub t: Signal<f32>,
}
impl View for StyleQuads {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        each((0..STYLE_N as u32).map(|i| quad(STYLE_X0, 4.0 * i as f32, 20.0, 4.0).bg(sig!(self.t).map(move |t| style_color(i as usize, *t)))))
    }
}

/// All 1000 boxes as one `lanes` element.
pub struct StyleLanes {
    pub t: Signal<f32>,
}
impl View for StyleLanes {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        lanes(STYLE_N, sig!(self.t), |t: &f32, i: usize| style_lane(i, *t))
    }
}

// ============================================================================= i18n (zgx i18n_bench)

/// A row's state: its count and the active locale (`fr`: French).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Msg {
    pub n: u32,
    pub fr: bool,
}
/// `{n, plural, one {# message} other {# messages}}` (EN: one is 1; FR: one is 0 and 1, other is
/// "# messages reçus"), the plural rule written natively: bough has no i18n layer.
pub fn plural_text(m: &Msg) -> String {
    let one = if m.fr { m.n <= 1 } else { m.n == 1 };
    match (one, m.fr) {
        (true, _) => format!("{} message", m.n),
        (false, false) => format!("{} messages", m.n),
        (false, true) => format!("{} messages reçus", m.n),
    }
}
pub fn template_text(n: &u32) -> String {
    if *n == 1 { format!("{n} message") } else { format!("{n} messages") }
}

pub struct I18nRow {
    pub m: Signal<Msg>,
}
impl View for I18nRow {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        label(sig!(self.m).map(plural_text)).font(16.0, normal(16.0))
    }
}
pub struct I18nRows {
    pub rows: List<I18nRow>,
}
impl I18nRows {
    pub fn new(n: usize) -> I18nRows {
        I18nRows { rows: List::from_iter_in((0..n).map(|i| I18nRow { m: Signal::new(Msg { n: i as u32, fr: false }) })) }
    }
}
impl View for I18nRows {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.rows))
    }
}
/// A tenth of the rows change a frame: `skip(frame % 10).step_by(10)`, each count + 1.
pub fn i18n_step(v: &mut I18nRows, frame: usize) {
    for i in (frame % 10..v.rows.len()).step_by(10) {
        let mut m = *v.rows.get(i).unwrap().m;
        m.n += 1;
        v.rows.edit(i).m.set(m);
    }
}
/// Switch every row's locale.
pub fn i18n_locale(v: &mut I18nRows, fr: bool) {
    for i in 0..v.rows.len() {
        let mut m = *v.rows.get(i).unwrap().m;
        m.fr = fr;
        v.rows.edit(i).m.set(m);
    }
}

pub struct TplRow {
    pub n: Signal<u32>,
}
impl View for TplRow {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        label(sig!(self.n).map(template_text)).font(16.0, normal(16.0))
    }
}
pub struct TplRows {
    pub rows: List<TplRow>,
}
impl TplRows {
    pub fn new(n: usize) -> TplRows {
        TplRows { rows: List::from_iter_in((0..n).map(|i| TplRow { n: Signal::new(i as u32) })) }
    }
}
impl View for TplRows {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.rows))
    }
}
pub fn tpl_step(v: &mut TplRows, frame: usize) {
    for i in (frame % 10..v.rows.len()).step_by(10) {
        let n = *v.rows.get(i).unwrap().n + 1;
        v.rows.edit(i).n.set(n);
    }
}

// ===================================================================================== test support

/// `app_sized`'s config (columns sized for `rows` rows of about `recs` records and `glyphs` glyphs each),
/// for callers that set more of it (the pixel gates set the ring).
pub fn config_sized(w: f32, h: f32, rows: usize, recs: usize, glyphs: usize) -> Config {
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
    cfg
}

// ============================================================ style labels (zgx style_bench labels)

/// A label's clock and its index (the `glyph_alpha` function sees only the signal's value).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Phase {
    pub i: u32,
    pub t: f32,
}
/// "Label {i}" at its column position (y = 22 i), faded by `0.5 + 0.5 sin(3 t + i)` through the
/// per-glyph alpha. Opacity only: a text node has no x (translate) operand.
pub struct StyleLabel {
    pub p: Signal<Phase>,
}
impl View for StyleLabel {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        let size = 16.0;
        text(format!("Label {}", self.p.i))
            .at(0.0, normal(size) * self.p.i as f32)
            .size(size)
            .line_height(normal(size))
            .glyph_alpha(sig!(self.p), |_, _, p| (style_opacity(p.i as usize, p.t) * 255.0).round() as u8)
    }
}
pub struct StyleLabels {
    pub rows: List<StyleLabel>,
}
impl StyleLabels {
    pub fn new(n: usize) -> StyleLabels {
        StyleLabels { rows: List::from_iter_in((0..n as u32).map(|i| StyleLabel { p: Signal::new(Phase { i, t: 0.0 }) })) }
    }
}
impl View for StyleLabels {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.rows))
    }
}
/// The clock write: every label's signal (zgx's one signal read by 1000 reactive styles).
pub fn style_labels_set(v: &mut StyleLabels, t: f32) {
    for i in 0..v.rows.len() {
        v.rows.edit(i).p.set(Phase { i: i as u32, t });
    }
}

// ======================================================== list-state (zgx-script list_state_bench)

/// One message (plain data: a mailbox and an unread flag; the rest is text).
pub struct LMsg {
    pub mailbox: u8,
    pub unread: bool,
    pub sender: String,
    pub subject: String,
    pub preview: String,
}
/// A row of the inbox list: its subject and its unread mark.
pub struct LRowV {
    pub subject: String,
    pub star: Signal<bool>,
}
impl View for LRowV {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        row((label(self.subject.clone()).font(16.0, normal(16.0)), label(sig!(self.star).map(|s| if *s { "*".to_string() } else { String::new() })).font(16.0, normal(16.0)))).h(28.0)
    }
}
pub const INBOX: u8 = 0;
/// zgx's mail-like list state: `n` messages, the inbox's rows shown, its unread count. The script derives
/// both with `ui.computed` over every message on each change; here the handler that toggles a message
/// updates exactly what the toggle changes (the row's mark, the count): derive, don't react.
pub struct ListState {
    pub msgs: Vec<LMsg>,
    /// Message index -> its position in `shown` (`u32::MAX`: not shown).
    pub shown_at: Vec<u32>,
    pub shown: List<LRowV>,
    pub unread: Signal<u32>,
    pub tick: usize,
}
impl ListState {
    pub fn new(n: usize) -> ListState {
        // zgx's script, 1-based: mailbox boxes[(i % 4) + 1] = inbox, archive, sent, spam; unread when i % 3 == 0
        let msgs: Vec<LMsg> = (1..=n)
            .map(|i| LMsg { mailbox: (i % 4) as u8, unread: i % 3 == 0, sender: format!("Sender {i}"), subject: format!("Subject line {i}"), preview: format!("A preview of message {i}") })
            .collect();
        let mut shown = List::with_capacity(n / 4 + 1);
        let mut shown_at = vec![u32::MAX; n];
        let mut unread = 0;
        for (k, m) in msgs.iter().enumerate() {
            if m.mailbox == INBOX {
                shown_at[k] = shown.len() as u32;
                shown.push(LRowV { subject: m.subject.clone(), star: Signal::new(m.unread) });
                unread += m.unread as u32;
            }
        }
        ListState { msgs, shown_at, shown, unread: Signal::new(unread), tick: 0 }
    }
    /// zgx's `tick` command: message `(tick * 7) % N + 1` (1-based) flips its unread flag.
    pub fn tick(&mut self) {
        self.tick += 1;
        let k = (self.tick * 7) % self.msgs.len();
        let m = &mut self.msgs[k];
        m.unread = !m.unread;
        if m.mailbox == INBOX {
            if m.unread {
                *self.unread += 1;
            } else {
                *self.unread -= 1;
            }
            let p = self.shown_at[k] as usize;
            self.shown.edit(p).star.set(m.unread);
        }
    }
}
impl View for ListState {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        // the list's 560-point viewport (zgx's `ui.list { h = 560 }`) clips its rows
        clip(
            0.0,
            0.0,
            800.0,
            normal(16.0) + 560.0,
            column((label(sig!(self.unread).num(|u| *u as f32, NumFmt::new("Unread: ", None, ""))).font(16.0, normal(16.0)), list(sig!(self.shown)).h(560.0))).w(800.0).h(600.0),
        )
    }
}

// ================================================= script-layout (zgx-script script_layout_bench)

/// One of zgx's reordered rows: `div { w = 200, h = 10 + i % 3 * 5 }`.
pub struct Bar {
    pub i: u32,
}
impl View for Bar {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        block(200.0, 10.0 + (self.i % 3) as f32 * 5.0).bg(c(0x58, 0xa6, 0xff))
    }
}
/// 1,000 keyed rows of three heights in a 400 x 900 scroll viewport (zgx's `ui.scroll { ui.column { ui.each } }`).
pub struct Reorder {
    pub rows: List<Bar>,
}
impl Reorder {
    pub fn new(n: usize) -> Reorder {
        Reorder { rows: List::from_iter_in((0..n as u32).map(|i| Bar { i })) }
    }
}
impl View for Reorder {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        clip(0.0, 0.0, 400.0, 900.0, list(sig!(self.rows)).w(400.0))
    }
}
