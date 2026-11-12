//! zgx's `scripts/perf/suite.py` scenes and `crates/zgx/examples` benches, ported natively onto bough.
//! Each scene's view lives here (the binaries and the pixel gates in `tests/` share it); the workload, the
//! tree and the counts are zgx's, read from its source (see RESULTS: `crates/bough/API-RESULTS.md`).
use bough::layout::prelude::*;
use bough::raw::*;

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
    App::new(v, Config::new(w, h, 1.0, font()).once(), gpu)
}

/// `app` with the region's columns sized for `rows` rows of about `recs` records and `glyphs` glyphs
/// each (the columns are fixed at mount; untouched capacity costs no memory).
pub fn app_sized<V: View>(v: V, w: f32, h: f32, gpu: bool, rows: usize, recs: usize, glyphs: usize) -> App<V> {
    let mut cfg = Config::new(w, h, 1.0, font()).once();
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
    bough::sys::perf::snap().instrs
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
    fn view(&self) -> impl Element<Self> + '_ {
        column(each((1..=self.n).map(mount_row))).w(1200.0).gap(4.0)
    }
}

// ================================================================== teardown (zgx teardown_bench)

/// zgx's `teardown_bench` rows (no rounding, default text sizes), mounted under an `Option` so a
/// re-description removes them (zgx: `mounted.unmount()`). Its configs are not `Config::once()`: with no
/// re-description, writing `show` changes nothing.
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
    fn view(&self) -> impl Element<Self> + '_ {
        column(each((1..=ROWS).map(|i| block(20.0, 20.0).bg(sig!(self.selected).select(i, ORANGE, SLATE)))))
    }
}

/// `rows` / `rows-in-place` (the Rust variant of both): 200 keyed rows of a label each.
pub struct URow {
    pub label: Signal<String>,
}
impl View for URow {
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
    fn view(&self) -> impl Element<Self> + '_ {
        each((0..STYLE_N as u32).map(|i| lanes(1, sig!(self.t), move |t: &f32, _| style_lane(i as usize, *t))))
    }
}

/// Fade only: a quad per box with a reactive colour (a quad's x is constant), no translation.
pub struct StyleQuads {
    pub t: Signal<f32>,
}
impl View for StyleQuads {
    fn view(&self) -> impl Element<Self> + '_ {
        each((0..STYLE_N as u32).map(|i| quad(STYLE_X0, 4.0 * i as f32, 20.0, 4.0).bg(sig!(self.t).map(move |t| style_color(i as usize, *t)))))
    }
}

/// All 1000 boxes as one `lanes` element.
pub struct StyleLanes {
    pub t: Signal<f32>,
}
impl View for StyleLanes {
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
    let mut cfg = Config::new(w, h, 1.0, font()).once();
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
    fn view(&self) -> impl Element<Self> + '_ {
        // the list's 560-point viewport (zgx's `ui.list { h = 560, height = 28 }`): only the rows meeting it
        // are mounted (a row is 28 points: the estimate is the measured height)
        clip(
            0.0,
            0.0,
            800.0,
            normal(16.0) + 560.0,
            column((label(sig!(self.unread).num(|u| *u as f32, NumFmt::new("Unread: ", None, ""))).font(16.0, normal(16.0)), list(sig!(self.shown)).h(560.0).viewport().estimate(28.0))).w(800.0).h(600.0),
        )
    }
}

// ================================================= script-layout (zgx-script script_layout_bench)

/// One of zgx's reordered rows: `div { w = 200, h = 10 + i % 3 * 5 }`.
pub struct Bar {
    pub i: u32,
}
impl View for Bar {
    fn view(&self) -> impl Element<Self> + '_ {
        block(200.0, 10.0 + (self.i % 3) as f32 * 5.0).bg(c(0x58, 0xa6, 0xff))
    }
}
/// 1,000 keyed rows of three heights in a 400 x 900 scroll viewport (zgx's `ui.scroll { ui.column { ui.each } }`),
/// every row mounted (as zgx's); `animate`: rows a reorder moves slide there (zgx's `animate = true`).
pub struct Reorder {
    pub rows: List<Bar>,
    pub animate: bool,
}
impl Reorder {
    pub fn new(n: usize) -> Reorder {
        Reorder { rows: List::from_iter_in((0..n as u32).map(|i| Bar { i })), animate: false }
    }
    pub fn animated(n: usize) -> Reorder {
        Reorder { animate: true, ..Reorder::new(n) }
    }
}
impl View for Reorder {
    fn view(&self) -> impl Element<Self> + '_ {
        let l = list(sig!(self.rows)).w(400.0);
        clip(0.0, 0.0, 400.0, 900.0, if self.animate { l.animate_moves() } else { l })
    }
}

// ==================================================== motion page (zgx-gpu examples/frame_bench.rs)

/// zgx `frame_bench`'s window (points; rendered at scale 2).
pub const MOTION_W: f32 = 1180.0;
pub const MOTION_H: f32 = 780.0;
/// The sidebar's and the event log's fixed widths: the scroll port between them is `MOTION_W - 520` wide.
pub const MOTION_SIDEBAR_W: f32 = 220.0;
pub const MOTION_LOG_W: f32 = 300.0;
/// zgx's card: rounded 14, white at alpha 0x14, a 1-pt border white at alpha 0x1f.
pub const CARD_BG: Color = Color::rgba(255, 255, 255, 0x14);
pub const CARD_BORDER: Color = Color::rgba(255, 255, 255, 0x1f);
/// zgx's border takes part in layout (border box: the content is inset by border + padding); bough's border
/// is paint only, so a card's padding is zgx's plus this.
pub const CARD_BORDER_W: f32 = 1.0;
pub const MOTION_SHIMMER: &str = "Reading workspace files";
/// The text size of zgx texts that set none (its body size without a theme: `font.size.md` unset = 16).
pub const ZGX_BODY: f32 = 16.0;

/// A struct of named signals (`sig!` proves struct field paths only, so zgx's `Vec<Signal<_>>` becomes
/// one named field per signal), with all of them as an array of `&mut`.
macro_rules! signal_row {
    ($(#[$m:meta])* $name:ident, $t:ty, $n:literal, $($f:ident)+) => {
        $(#[$m])*
        pub struct $name {
            $(pub $f: Signal<$t>,)+
        }
        impl $name {
            pub fn new(v: $t) -> $name {
                $name { $($f: Signal::new(v),)+ }
            }
            pub fn all_mut(&mut self) -> [&mut Signal<$t>; $n] {
                [$(&mut self.$f),+]
            }
            pub fn values(&self) -> [$t; $n] {
                [$(*self.$f),+]
            }
        }
    };
}
/// `[sig!(self.group.f0), sig!(self.group.f1), ..]`.
macro_rules! wired_row {
    ($s:tt . $g:ident, $($f:ident)+) => {
        [$(sig!($s . $g . $f)),+]
    };
}

signal_row!(
    /// The MATRIX card's 15 dot opacities.
    MotionDots, f32, 15, d0 d1 d2 d3 d4 d5 d6 d7 d8 d9 d10 d11 d12 d13 d14
);
signal_row!(
    /// The shimmer row's 23 character alphas ("Reading workspace files").
    MotionShimmer, u8, 23, s0 s1 s2 s3 s4 s5 s6 s7 s8 s9 s10 s11 s12 s13 s14 s15 s16 s17 s18 s19 s20 s21 s22
);
signal_row!(
    /// The WAVE card's 5 cell sizes (points).
    MotionCells, f32, 5, c0 c1 c2 c3 c4
);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MotionMode {
    /// Opacity of 15 dots and the colour alpha of 23 one-character texts.
    Paint,
    /// 5 cells resized inside fixed 16 x 16 slots.
    Cells,
    /// The FOLD column's height: everything below it moves (layout up to the root).
    Fold,
}
pub const MOTION_MODES: [(&str, MotionMode); 3] = [
    ("paint-only (opacity + text colour)", MotionMode::Paint),
    ("layout inside fixed slots", MotionMode::Cells),
    ("layout up to the root (fold grows)", MotionMode::Fold),
];

/// zgx `frame_bench`'s Motion page: a sidebar of 8 rows, a header and 5 step texts, the loaders band (MATRIX:
/// 15 dots whose opacity is a signal; WAVE: 5 cells whose size is a signal), the shimmer row (one text per
/// character, its colour alpha a signal), the FOLD card (a column whose height is a signal, overflow hidden),
/// `rows` static cards, and an event log of 40 lines.
pub struct Motion {
    pub dots: MotionDots,
    pub shimmer: MotionShimmer,
    pub cells: MotionCells,
    pub fold: Signal<f32>,
    pub rows: usize,
    pub geom: MotionGeom,
}

/// What the page reads from a probe mount (`motion_geom`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MotionGeom {
    /// The FOLD column's laid-out (x, y, w) in points: its overflow clip is (x, y, w, fold). bough's paint
    /// clips are rects in a space, not layout boxes, so the rect is read from a probe mount; it is constant
    /// here (nothing above the column ever moves, and the shimmer row's height does not depend on its widths).
    pub fold_at: [f32; 3],
    /// A space's advance at 18 pt: a bough label's width excludes trailing spaces (a label of one space is
    /// 0 wide), so the shimmer row's two space labels get it as their width (zgx measures a space as its
    /// advance, cosmic-text's line width).
    pub space_w: f32,
}

impl Motion {
    pub fn new(rows: usize, geom: MotionGeom) -> Motion {
        Motion { dots: MotionDots::new(1.0), shimmer: MotionShimmer::new(90), cells: MotionCells::new(14.0), fold: Signal::new(0.0), rows, geom }
    }
}

fn motion_card<Ch>(ch: Ch) -> bough::layout::LBox<Color, Ch> {
    column(ch).bg(CARD_BG).rounded(14.0).border(CARD_BORDER_W, CARD_BORDER)
}
fn white_alpha(a: &u8) -> Color {
    Color::rgba(255, 255, 255, *a)
}
fn zgx_text<S>(s: S, size: f32) -> bough::layout::Para<S> {
    label(s).font(size, normal(size))
}

impl View for Motion {
    fn view(&self) -> impl Element<Self> + '_ {
        let dots = wired_row!(self.dots, d0 d1 d2 d3 d4 d5 d6 d7 d8 d9 d10 d11 d12 d13 d14);
        let shimmer = wired_row!(self.shimmer, s0 s1 s2 s3 s4 s5 s6 s7 s8 s9 s10 s11 s12 s13 s14 s15 s16 s17 s18 s19 s20 s21 s22);
        let cells = wired_row!(self.cells, c0 c1 c2 c3 c4);
        let b = CARD_BORDER_W;
        let loaders = row((
            motion_card((zgx_text("MATRIX", ZGX_BODY), row(each(dots.into_iter().map(|o| block(10.0, 10.0).bg(rgb(0x6ea8ff)).rounded(5.0).opacity(o)))).gap(6.0)))
                .grow(1.0)
                .h(150.0)
                .p(16.0 + b),
            motion_card((
                zgx_text("WAVE", ZGX_BODY),
                row(each(cells.into_iter().map(|s| column(block(14.0, 14.0).bg(rgb(0x9ad2ff)).rounded(4.0).w_op(s).h_op(s)).size(16.0, 16.0).items(Align::Center).justify(Justify::Center)))).gap(4.0),
            ))
            .grow(1.0)
            .h(150.0)
            .p(16.0 + b),
        ))
        .w_pct(1.0)
        .gap(12.0);
        let space_w = self.geom.space_w;
        let shimmer_row = motion_card(row(each(MOTION_SHIMMER.chars().zip(shimmer).map(move |(ch, a)| {
            let t = zgx_text(ch.to_string(), 18.0).color(a.map(white_alpha));
            if ch == ' ' { t.w(space_w) } else { t }
        }))))
        .w_pct(1.0)
        .p(20.0 + b);
        let [fx, fy, fw] = self.geom.fold_at;
        let fold = motion_card((
            zgx_text("FOLD", ZGX_BODY),
            clip_by(
                sig!(self.fold),
                move |h: &f32| [fx, fy, fw, *h],
                column(each((0..3).map(|i| zgx_text(format!("Folded line {i}"), ZGX_BODY).h(24.0)))).w_pct(1.0).clip_overflow().h_op(sig!(self.fold)),
            ),
        ))
        .w_pct(1.0)
        .p(20.0 + b);
        let content = column((
            zgx_text("Motion", 30.0),
            each((0..5).map(|i| zgx_text(format!("{}. A step describing what to check on this page.", i + 1), 13.0))),
            loaders,
            shimmer_row,
            fold,
            each((0..self.rows).map(move |i| motion_card(zgx_text(format!("Static row {i}: retained content that never changes"), 13.0)).w_pct(1.0).p(12.0 + b))),
        ))
        .grow(1.0)
        .min_w(0.0)
        .p(32.0)
        .gap(22.0);
        // zgx's `scroll(..)` at offset 0: a port clipped to its rect whose content takes its own height
        let port_w = MOTION_W - MOTION_SIDEBAR_W - MOTION_LOG_W;
        let port = clip(MOTION_SIDEBAR_W, 0.0, port_w, MOTION_H, row(content).grow(1.0).min_w(0.0).h_pct(1.0).items(Align::Start));
        let log = column(each((0..40).map(|i| zgx_text(format!("{:>5.1}s  EVENT something happened {i}", i as f32 * 0.7), 11.0)))).w(MOTION_LOG_W).p(20.0).gap(5.0);
        let sidebar = column(each((0..8).map(|i| row(zgx_text(format!("Section {i}"), 13.0)).h(34.0).px(10.0).items(Align::Center)))).w(MOTION_SIDEBAR_W).p(12.0).gap(2.0);
        row((sidebar, row((port, log)).grow(1.0).min_w(0.0).bg(Color::rgba(0x12, 0x16, 0x1e, 0xa6)).rounded(14.0)))
            .w_pct(1.0)
            .h_pct(1.0)
            .items(Align::Stretch)
            .bg(c(18, 22, 30))
    }
}

/// zgx's `step(animated, frame, mode)`: the signals of `mode` at `frame` (t = frame / 120 s).
pub fn motion_step(v: &mut Motion, frame: usize, mode: MotionMode) {
    let t = frame as f32 / 120.0;
    match mode {
        MotionMode::Fold => {
            v.fold.set(84.0 * (0.5 - 0.5 * (t * 3.0).cos()));
        }
        MotionMode::Paint => {
            for (i, dot) in v.dots.all_mut().into_iter().enumerate() {
                dot.set(0.1 + 0.9 * ((t * 1.3 + i as f32 * 0.11).fract()));
            }
            let centre = (t / 3.4).fract() * 1.8 - 0.4;
            for (i, ch) in v.shimmer.all_mut().into_iter().enumerate() {
                let glow = (1.0 - ((i as f32 / 22.0) - centre).abs() / 0.36).clamp(0.0, 1.0);
                ch.set((90.0 + 165.0 * glow) as u8);
            }
        }
        MotionMode::Cells => {
            for (i, cell) in v.cells.all_mut().into_iter().enumerate() {
                let w = 0.5 - 0.5 * (std::f32::consts::TAU * (t - i as f32 * 0.15) / 2.4).cos();
                cell.set(12.6 + 1.4 * w);
            }
        }
    }
}

/// The Motion page's configuration: 1180 x 780 points at scale 2.
pub fn motion_config() -> Config {
    Config::new(MOTION_W, MOTION_H, 2.0, font()).once()
}

/// The layout rect (x, y, w points) of the box whose first child is the label `first`.
pub fn motion_parent_of<V: View>(a: &App<V>, first: &[u8]) -> Option<[f32; 3]> {
    let s = a.rt.scale;
    let li = a.rt.lay.info.iter().find(|li| li.live && li.kind == 2 && a.rt.texts[a.rt.nodes[li.rtn as usize].text as usize].bytes() == first)?;
    let col = a.rt.lay.info[li.parent as usize];
    Some([col.ax / s, col.ay / s, col.aw / s])
}

/// The page's probe geometry: a headless mount of the page, the FOLD column's laid-out rect (the column's
/// rect does not depend on any signal: only its height does, and nothing above it moves) and the font's
/// space advance at 18 pt.
pub fn motion_geom(rows: usize) -> MotionGeom {
    let a = App::new(Motion::new(rows, MotionGeom { fold_at: [0.0, 0.0, MOTION_W], space_w: 0.0 }), motion_config(), false);
    let script = &a.rt.text.shaper.script;
    let space_w = script.adv[b' ' as usize] as f32 * 18.0 / script.upem as f32;
    let fold_at = motion_parent_of(&a, b"Folded line 0").expect("the probe mount has a FOLD column");
    MotionGeom { fold_at, space_w }
}

// ============================================== label-heavy screen (zgx-gpu examples/layout_bench.rs)

pub const LB_WORDS: &[&str] = &["process", "memory", "renderer", "streams", "tokens", "while", "layout", "measures", "paragraphs", "at", "every", "width", "and", "the", "glyphs", "stay", "retained"];
pub const LB_W: f32 = 1400.0;
pub const LB_H: f32 = 900.0;
const LB_TEXT: Color = Color::rgba(230, 230, 235, 255);

/// zgx's `sentence(seed, words)` under `salt` (salted words are new to every cache).
pub fn lb_sentence(salt: usize, seed: usize, words: usize) -> String {
    let seed = seed + salt * 101;
    (0..words)
        .map(|i| {
            let word = LB_WORDS[(seed * 7 + i * 3) % LB_WORDS.len()];
            if salt > 0 && i % 2 == 0 {
                format!("{word}{salt}")
            } else {
                word.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rich paragraph `i`: zgx's three spans (8 words, 3 bold words, 9-15 words), concatenated as its rich text
/// lays them out. bough's façade has one face: the bold span is shaped in the regular face.
pub fn lb_rich(salt: usize, i: usize) -> String {
    let mut s = lb_sentence(salt, i + 50, 8);
    s.push_str(&lb_sentence(salt, i + 90, 3));
    s.push_str(&lb_sentence(salt, i + 130, 9 + i % 7));
    s
}

/// zgx's `screen()`: 50 rows of 4 labels at 11 pt (34 %), 24 wrapped paragraphs at 12 pt (33 %), 24 rich
/// paragraphs at 12 pt (33 %), in a row 900 points tall. The texts are built as the region mounts them.
pub fn layout_screen<V: 'static>(salt: usize) -> impl Element<V> {
    let t = |s: String, size: f32| label(s).font(size, normal(size)).color(LB_TEXT);
    let labels = column(each((0..50).map(move |r| {
        row((
            t(format!("{} {r}", LB_WORDS[(r + salt) % LB_WORDS.len()]), 11.0),
            t(format!("{:.1}%", (r * 37 % 1000) as f32 / 10.0), 11.0),
            t(format!("{} MB", r * 13 % 900), 11.0),
            t(format!("pid {}", 1000 + r * 17 + salt * 7919), 11.0),
        ))
        .gap(8.0)
    })))
    .w_pct(0.34)
    .gap(2.0);
    let plain = column(each((0..24).map(move |i| para(lb_sentence(salt, i, 18 + i % 11)).font(12.0, normal(12.0)).color(LB_TEXT)))).w_pct(0.33).gap(6.0);
    let rich = column(each((0..24).map(move |i| para(lb_rich(salt, i)).w_pct(1.0).font(12.0, normal(12.0)).color(LB_TEXT)))).w_pct(0.33).gap(6.0);
    row((labels, plain, rich)).w_pct(1.0).h(LB_H).gap(8.0).bg(c(20, 22, 28))
}

/// The screen mounted once (the gates' fresh mount).
pub struct LayoutScreen {
    pub salt: usize,
}
impl View for LayoutScreen {
    fn view(&self) -> impl Element<Self> + '_ {
        layout_screen(self.salt)
    }
}

/// The screen remounted per salt in one app (zgx: a new `Ui` per salt, one renderer): a re-describing owner
/// whose description is the screen or nothing. The screen sits under `offset` at (0, 0) — an opaque element
/// — so a re-description (which `App::resize` runs for a re-describing root) steps over it instead of
/// diffing its 350 texts; its rows are lazy iterators, so the step builds no strings.
pub struct LayoutHost {
    pub salt: usize,
    pub show: bool,
    pub origin: Signal<(f32, f32)>,
}
impl LayoutHost {
    pub fn new() -> LayoutHost {
        LayoutHost { salt: 0, show: false, origin: Signal::new((0.0, 0.0)) }
    }
}
impl Default for LayoutHost {
    fn default() -> Self {
        Self::new()
    }
}
impl View for LayoutHost {
    fn view(&self) -> impl Element<Self> + '_ {
        self.show.then(|| offset(sig!(self.origin), layout_screen(self.salt)))
    }
}

/// Unmount the host's screen and return its resources (outside zgx's timed span: its old `Ui` drops there).
pub fn lb_unmount(a: &mut App<LayoutHost>) {
    a.turn(|v| v.show = false);
    while a.reclaiming() {
        a.poll();
    }
}

/// The label-heavy screen's configuration at `w` points wide, scale 2. Not mounted once: `LayoutHost`'s
/// handlers change its plain fields (`salt`, `show`), and only a re-description mounts and unmounts the screen.
pub fn lb_config(w: f32) -> Config {
    Config::new(w, LB_H, 2.0, font())
}

/// zgx's resize step `k`: 1400 - (k % 20) * 25 points.
pub fn lb_width(k: usize) -> f32 {
    LB_W - (k % 20) as f32 * 25.0
}
