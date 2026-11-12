//! A live dashboard: 200 chart cards (5 per row, 40 rows, 240 x 170 pt each) under a header with a time-range
//! selector and a 4-series legend, in a scrolled grid.
//!
//! Each card is a rounded flow box with a title, the current value (`NumFmt`), a coloured delta badge, one chart
//! and a footer (range caption and a utilisation bar). The chart kinds rotate with the card index:
//!
//! * kind 0, bars: the raw `bars` element (24 bars, one wire per chart) over three grid lines, one series;
//! * kind 1, sparkline: 40 thin quads (`quad(..).span(..)`), each wired to the card's key signal;
//! * kind 2, trend: 2 series x 18 dots, each dot wired to the *time-range* signal (the range re-shapes them);
//! * kind 3, gauge: a track and a fill block whose width is an operand;
//! * kind 4, stacked bar of the 4 series: four blocks with width operands and four share labels.
//!
//! Data is a pure function: a chart's samples are `sample(seed, key + j)`, so a data tick is one `key += 1` (the
//! window slides) and a dashboard of 200 live charts needs 200 signals, not 4,800. Everything else that moves
//! is a signal with a wire: values, deltas, gauge and share widths, the utilisation bar.
//!
//! Fan-out (`hover`): the legend item "B" sets the highlighted series in `look` (theme bit | series). Every
//! series mark reads `look` through `.map`: sparkline columns, trend dots, gauge and stacked fills, utilisation
//! fills and, for the `bars` charts (whose colour is a constant), a scrim quad that dims the chart when its
//! series is not B. 3,484 wires at scale 1 (`fanout()`). `theme` flips a second signal (chrome: card fills,
//! text, grid lines, tracks, veils; ~2,500 wires) and the theme bit of `look`.
//!
//! Animation (`animate`): cards cross-fade (a page-coloured veil over the card whose opacity is a
//! transition; the stagger is the duration per column), the utilisation bars' widths transition (a width
//! operand with `.transition`), and one card's ring (a bordered box whose opacity transitions) lights up in turn.
//! These run during the resize sweep too.
//!
//! Resize: the grid follows the window width. The root is `w_pct(1.0)`; every row is full width and its five
//! cells are `w(240).grow(1).shrink(0)`, so cards grow with the window and keep 240 pt as their minimum; below
//! 1280 pt the rows overflow to the right and are clipped (flow has no wrapping, so the grid never reflows to 4
//! per row). The chart regions inside a card are fixed 216 x 72 anchors and do not stretch with the card; the
//! flow-built charts (gauge, stacked bar) keep their nominal widths too.
//!
//! Wanted and not expressible today (results, not workarounds):
//! * `bars` has a constant colour, no theme and no highlight: a dimmed series needs a separate scrim quad over
//!   the chart (one more wire and one more instance per bars chart), and a light and a dark theme share one
//!   mid-tone colour. Bars are also all one colour, no per-bar colour, no stacked or grouped bars, no axis ticks
//!   or text on the axis;
//! * no line path or area element: a line chart is a row of dots or thin columns (one quad and two wires per
//!   point); there is no polyline, no curve, no fill-under-line, no axis ticks or labels;
//! * raw elements (`quad`, `bars`, `deco_box`) take absolute coordinates, not layout: to put a chart in a card in
//!   flow it has to be wrapped in `anchor(w, h, ..)`, a fixed-size flow leaf with a space of its own (320 spaces
//!   and views for the 200 cards, charts plus shadows). The anchor's size is a constant, so charts cannot
//!   follow a card that grows: nothing says "this fraction of the parent";
//! * `quad`'s x and width cannot both move: a horizontal stacked bar of quads needs the x of each segment as an
//!   operand (there is only `width`); the stacked bar is four flow blocks with width operands, which restyle
//!   and re-lay out the row on every change instead of a paint-only write;
//! * a proportional width operand (a gauge fill at 40 % of its track): only points, so the fill is computed
//!   against the nominal 216 pt and is wrong once the track grows;
//! * shadows on flow boxes: `deco_box(..).shadows(..)` is raw, so each card's soft shadow is a second layer,
//!   an anchor of the nominal 240 x 170 inside an absolutely placed wrapper under the card. It does not follow
//!   the card's width when the window grows, and its colour and blur are constants (no theme, no transition of
//!   a shadow or of a border colour);
//! * a border colour is a constant on a flow box: the highlighted card's "border colour transition" is a
//!   separate bordered ring box whose opacity transitions;
//! * group opacity: `.opacity(..)` multiplies only the box's own fill, not its children, so a card cannot
//!   cross-fade as a unit; the veil over the card is the stand-in (one more box per card, painted over the
//!   whole card even when invisible);
//! * a mark that depends on two signals (series colour from theme and hovered series; a sample from the key and
//!   the range): no operand over two signals, so theme and highlight are packed into one `u32` (`look`), and the
//!   range cannot re-sample a chart that is already keyed by its tick (kind 2 reads only the range, kinds 0 and
//!   1 only the key);
//! * `Vec<Signal<T>>` cannot be wired: every per-card signal is a fixed array of `MAX_CARDS` (or 4 x) in the
//!   owner, which also caps the scene at 200 cards;
//! * a text from a map is a `String` (an allocation per update); only `.num(..)` formats without one, so the
//!   range caption is a number with a prefix ("last 24h"), not the labels "1h / 6h / 24h / 7d";
//! * no wrapping row: the grid is 40 explicit rows of 5 and cannot reflow to a different column count.
use crate::{scaled, Rng, Scene};
use bough::prelude::*;
use bough::raw::{anchor, bars, deco_box, quad, App, Button, Caps, Ev, NumFmt, Shadow};

pub const MAX_CARDS: usize = 200;
const COLS: usize = 5;

const CARD_W: f32 = 240.0;
const CARD_H: f32 = 170.0;
const PAD: f32 = 12.0;
const GAP: f32 = 12.0;
const CH_W: f32 = 216.0;
const CH_H: f32 = 72.0;
const HEADER_H: f32 = 56.0;
const UTIL_W: f32 = 138.0;
const NO_HL: u32 = 0xff;

const SPARK_N: u32 = 40;
const TREND_N: u32 = 18;
const BAR_N: usize = 24;

const VAL: NumFmt = NumFmt::new("", Some(1), "");
const PCT: NumFmt = NumFmt::new("", Some(1), "%");
const SHARE: NumFmt = NumFmt::new("", Some(0), "%");
const LAST: NumFmt = NumFmt::new("last ", Some(0), "h");
const HOURS: [f32; 4] = [1.0, 6.0, 24.0, 168.0];
const RANGE_LABELS: [&str; 4] = ["1h", "6h", "24h", "7d"];
const SERIES_NAMES: [&str; 4] = ["A", "B", "C", "D"];
const TITLES: [&str; 12] = ["CPU", "Memory", "Latency", "Requests", "Errors", "Disk IO", "Network", "Queue", "Cache hit", "Sessions", "Revenue", "Uptime"];

/// Series colours (light theme, dark theme) and the mid tones the `bars` elements use in both.
const SERIES_LIGHT: [u32; 4] = [0x2f6bff, 0xe8590c, 0x12a07a, 0x8f5be8];
const SERIES_DARK: [u32; 4] = [0x6e9bff, 0xff8a4c, 0x3fd1a5, 0xb58cff];
const SERIES_MID: [u32; 4] = [0x4a7dff, 0xf06a22, 0x1fb88c, 0x9d6df0];
const UP: Color = rgb(0x12a07a);
const DOWN: Color = rgb(0xd9453d);
const RING: Color = rgb(0x2f6bff);

static CARD_SHADOW: [Shadow; 2] = [Shadow { dx: 0.0, dy: 1.0, blur: 3.0, spread: 0.0, color: 0x1f000000 }, Shadow { dx: 0.0, dy: 6.0, blur: 18.0, spread: -2.0, color: 0x26000000 }];

fn rgba(hex: u32, a: u8) -> Color {
    Color::rgba((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, a)
}
fn page(t: u32) -> Color {
    if t != 0 { rgb(0x0e1117) } else { rgb(0xf0f2f6) }
}
fn card_fill(t: u32) -> Color {
    if t != 0 { rgb(0x1a1f2a) } else { rgb(0xffffff) }
}
fn bar_fill(t: u32) -> Color {
    if t != 0 { rgb(0x151922) } else { rgb(0xffffff) }
}
fn ink(t: u32) -> Color {
    if t != 0 { rgb(0xe9ecf3) } else { rgb(0x1b2230) }
}
fn ink2(t: u32) -> Color {
    if t != 0 { rgb(0x8d96a9) } else { rgb(0x6a7386) }
}
fn grid(t: u32) -> Color {
    if t != 0 { rgb(0x2b3242) } else { rgb(0xe7eaf0) }
}
fn track(t: u32) -> Color {
    if t != 0 { rgb(0x2b3242) } else { rgb(0xe8ebf1) }
}
/// `look` = theme bit << 8 | highlighted series (`NO_HL` for none).
fn look_theme(l: u32) -> u32 {
    (l >> 8) & 1
}
/// A series mark: the series colour, dimmed (low alpha) when another series is highlighted.
fn series_col(l: u32, s: u32) -> Color {
    let hl = l & 0xff;
    let pal = if look_theme(l) != 0 { &SERIES_DARK } else { &SERIES_LIGHT };
    if hl == NO_HL || hl == s { rgb(pal[s as usize & 3]) } else { rgba(pal[s as usize & 3], 56) }
}
/// The scrim over a `bars` chart (whose own colour is a constant): the card's fill, translucent, when dimmed.
fn scrim(l: u32, s: u32) -> Color {
    let hl = l & 0xff;
    if hl == NO_HL || hl == s {
        return Color(0);
    }
    let c = if look_theme(l) != 0 { 0x1a1f2a } else { 0xffffff };
    rgba(c, 190)
}

#[inline]
fn h32(a: u32, b: u32) -> u32 {
    let mut x = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^ (x >> 15)
}
#[inline]
fn unit(a: u32, b: u32) -> f32 {
    (h32(a, b) >> 8) as f32 / 16_777_216.0
}
/// A chart's sample at position `k` (0..1): a slow wave plus noise, a pure function of (seed, k).
#[inline]
fn sample(seed: u32, k: u64) -> f32 {
    let w = 0.5 + 0.5 * (k as f32 * 0.31 + (seed & 255) as f32 * 0.11).sin();
    0.16 + 0.84 * (0.58 * w + 0.42 * unit(seed, k as u32))
}
/// (y, h) of a sparkline column at sample `k`, in points inside the chart.
#[inline]
fn col_span(seed: u32, k: u64) -> (f32, f32) {
    let h = 3.0 + (CH_H - 5.0) * sample(seed, k);
    (CH_H - h, h)
}
/// (y, 3) of a trend dot: a smooth curve whose shape depends on the range.
#[inline]
fn trend_y(seed: u32, js: u32, r: u32) -> f32 {
    let (j, q) = ((js & 255) as f32, (js >> 8) as f32);
    let freq = 0.22 + 0.17 * r as f32;
    let v = 0.5 + 0.38 * (j * freq + (seed & 255) as f32 * 0.09 + q * 1.9).sin() + 0.1 * (unit(seed, js ^ (r << 12)) - 0.5);
    (CH_H - 5.0) * (1.0 - v.clamp(0.0, 1.0))
}

#[inline]
fn kind(i: usize) -> usize {
    i % 5
}
#[inline]
fn series_of(i: usize) -> u32 {
    (i % 4) as u32
}
#[inline]
fn seed_of(i: usize) -> u32 {
    (i as u32).wrapping_mul(0x9E37_79B1).wrapping_add(0x1234_5)
}

pub struct Dash {
    cards: usize,
    /// The live charts' data windows (kinds 0 and 1): the key of the first sample.
    keys: [Signal<u64>; MAX_CARDS],
    values: [Signal<f32>; MAX_CARDS],
    deltas: [Signal<f32>; MAX_CARDS],
    /// Gauge fill (0..1, kind 3) and the 4 stacked shares per card (percent, kind 4).
    gauge: [Signal<f32>; MAX_CARDS],
    parts: [Signal<f32>; MAX_CARDS * 4],
    /// The utilisation bar (0..1, transitions), the cross-fade veil's opacity (transitions), the ring's.
    util: [Signal<f32>; MAX_CARDS],
    veil: [Signal<f32>; MAX_CARDS],
    ring: [Signal<f32>; MAX_CARDS],
    /// Theme bit.
    theme: Signal<u32>,
    /// Theme bit << 8 | highlighted series.
    look: Signal<u32>,
    /// The time range (index into `HOURS`).
    range: Signal<u32>,
    scroll: Signal<f32>,
    rng: Rng,
}

impl Dash {
    fn rows(&self) -> usize {
        self.cards.div_ceil(COLS)
    }
    fn set_range(&mut self, r: u32) {
        self.range.set(r);
    }
    fn hover_series(&mut self, s: u32, on: bool) {
        let l = *self.look;
        let hl = l & 0xff;
        let next = if on { s } else if hl == s { NO_HL } else { hl };
        self.look.set((l & !0xff) | next);
    }
    /// One data tick of card `c`: its chart advances, its value and delta change.
    fn tick_card(&mut self, c: usize) {
        match kind(c) {
            0 | 1 => {
                let k = *self.keys[c] + 1;
                self.keys[c].set(k);
            }
            3 => {
                let g = (*self.gauge[c] + (self.rng.unit() - 0.45) * 0.3).clamp(0.05, 1.0);
                self.gauge[c].set(g);
            }
            4 => self.reshare(c),
            _ => {}
        }
        let old = *self.values[c];
        let nv = (old * (0.93 + 0.14 * self.rng.unit())).clamp(1.0, 9000.0);
        self.values[c].set(nv);
        self.deltas[c].set((nv - old) / old * 100.0);
    }
    fn reshare(&mut self, c: usize) {
        let w: [f32; 4] = std::array::from_fn(|_| 0.3 + self.rng.unit());
        let t: f32 = w.iter().sum();
        for j in 0..4 {
            self.parts[c * 4 + j].set(w[j] / t * 100.0);
        }
    }

    // ------------------------------------------------------------------------------------------ charts

    fn grid_lines(&self) -> impl Element<Self> + '_ {
        each([0.0f32, CH_H * 0.5, CH_H - 1.0].into_iter().map(move |y| quad(0.0, y, CH_W, 1.0).bg(sig!(self.theme).map(|t: &u32| grid(*t)))))
    }

    fn chart_bars(&self, i: usize) -> impl Element<Self> + '_ {
        let (s, seed) = (series_of(i), seed_of(i));
        anchor(
            CH_W,
            CH_H,
            (
                self.grid_lines(),
                bars(0.0, 9.0, 7.0, BAR_N, CH_H, CH_H - 4.0, 2.0, sig!(self.keys).at(i), move |k: u64| sample(seed, k)).color(rgb(SERIES_MID[s as usize])),
                quad(0.0, 0.0, CH_W, CH_H).bg(sig!(self.look).map(move |l: &u32| scrim(*l, s))),
            ),
        )
    }

    fn chart_spark(&self, i: usize) -> impl Element<Self> + '_ {
        let (s, seed) = (series_of(i) + 1 & 3, seed_of(i));
        anchor(
            CH_W,
            CH_H,
            (
                self.grid_lines(),
                each((0..SPARK_N).map(move |j| {
                    quad(j as f32 * 5.4, CH_H - 20.0, 4.4, 20.0)
                        .span(sig!(self.keys).at(i).map(move |k: &u64| col_span(seed, *k + j as u64)))
                        .bg(sig!(self.look).map(move |l: &u32| series_col(*l, s)))
                })),
            ),
        )
    }

    fn chart_trend(&self, i: usize) -> impl Element<Self> + '_ {
        let (s, seed) = (series_of(i), seed_of(i));
        anchor(
            CH_W,
            CH_H,
            (
                self.grid_lines(),
                each((0..2u32).flat_map(move |q| {
                    let sq = (s + q) & 3;
                    (0..TREND_N).map(move |j| {
                        let js = j | q << 8;
                        quad(j as f32 * 12.0, 30.0, 5.0, 3.0)
                            .span(sig!(self.range).map(move |r: &u32| (trend_y(seed, js, *r), 3.0)))
                            .bg(sig!(self.look).map(move |l: &u32| series_col(*l, sq)))
                    })
                })),
            ),
        )
    }

    fn chart_gauge(&self, i: usize) -> impl Element<Self> + '_ {
        let s = series_of(i);
        let label = move |t: &'static str| text(t).font(10.0, 14.0).color(sig!(self.theme).map(|t: &u32| ink2(*t)));
        column((
            row((block(0.0, 34.0).bg(sig!(self.look).map(move |l: &u32| series_col(*l, s))).rounded(8.0).w_op(sig!(self.gauge).at(i).map(|v: &f32| v * CH_W)),))
                .w(CH_W)
                .h(34.0)
                .rounded(8.0)
                .bg(sig!(self.theme).map(|t: &u32| track(*t))),
            row((label("0"), label("50"), label("100"))).justify(Justify::SpaceBetween).w(CH_W),
        ))
        .gap(6.0)
        .h(CH_H)
    }

    fn chart_stack(&self, i: usize) -> impl Element<Self> + '_ {
        column((
            row(each((0..4u32).map(move |k| {
                block(0.0, 30.0)
                    .bg(sig!(self.look).map(move |l: &u32| series_col(*l, k)))
                    .w_op(sig!(self.parts).at(i * 4 + k as usize).map(|v: &f32| v * 2.1))
            })))
            .gap(2.0)
            .w(CH_W)
            .h(30.0),
            row(each((0..4u32).map(move |k| {
                text(sig!(self.parts).at(i * 4 + k as usize).num(|v: &f32| *v, SHARE)).font(10.0, 14.0).color(sig!(self.theme).map(|t: &u32| ink2(*t)))
            })))
            .justify(Justify::SpaceBetween)
            .w(CH_W),
        ))
        .gap(8.0)
        .h(CH_H)
    }

    // ------------------------------------------------------------------------------------------ card

    fn card(&self, i: usize) -> impl Element<Self> + '_ {
        let s = series_of(i);
        let dur = 0.30 + 0.06 * (i % COLS) as f32;
        let title = format!("{} {}", TITLES[i % TITLES.len()], i / TITLES.len() + 1);
        let head = row((
            text(title).font(12.0, 16.0).color(sig!(self.theme).map(|t: &u32| ink2(*t))),
            div().grow(1.0),
            row((text(sig!(self.deltas).at(i).num(|d: &f32| *d, PCT)).font(10.0, 14.0).color(rgb(0xffffff)),))
                .px(6.0)
                .bg(sig!(self.deltas).at(i).map(|d: &f32| if *d >= 0.0 { UP } else { DOWN }))
                .rounded(8.0),
        ))
        .items(Align::Center)
        .h(18.0);
        let value = text(sig!(self.values).at(i).num(|v: &f32| *v, VAL)).font(22.0, 26.0).color(sig!(self.theme).map(|t: &u32| ink(*t)));
        let footer = row((
            text(sig!(self.range).num(|r: &u32| HOURS[*r as usize & 3], LAST)).font(10.0, 14.0).color(sig!(self.theme).map(|t: &u32| ink2(*t))).w(70.0),
            row((block(0.0, 4.0)
                .bg(sig!(self.look).map(move |l: &u32| series_col(*l, s)))
                .rounded(2.0)
                .w_op(sig!(self.util).at(i).map(|u: &f32| u * UTIL_W).transition(Curve::out(0.35))),))
            .w(UTIL_W)
            .h(4.0)
            .rounded(2.0)
            .bg(sig!(self.theme).map(|t: &u32| track(*t))),
        ))
        .gap(8.0)
        .items(Align::Center)
        .h(14.0);
        let k = kind(i);
        let card = column((
            head,
            value,
            each((k == 0).then(|| self.chart_bars(i)).into_iter()),
            each((k == 1).then(|| self.chart_spark(i)).into_iter()),
            each((k == 2).then(|| self.chart_trend(i)).into_iter()),
            each((k == 3).then(|| self.chart_gauge(i)).into_iter()),
            each((k == 4).then(|| self.chart_stack(i)).into_iter()),
            footer,
        ))
        .child(
            div()
                .abs(0.0, 0.0)
                .w_pct(1.0)
                .h_pct(1.0)
                .rounded(10.0)
                .border(2.0, RING)
                .opacity(sig!(self.ring).at(i).transition(Curve::out(0.25))),
        )
        .child(
            div()
                .abs(0.0, 0.0)
                .w_pct(1.0)
                .h_pct(1.0)
                .rounded(10.0)
                .bg(sig!(self.theme).map(|t: &u32| bar_fill(*t)))
                .opacity(sig!(self.veil).at(i).transition(Curve::out(dur))),
        )
        .relative()
        .p(PAD)
        .gap(4.0)
        .w_pct(1.0)
        .h(CARD_H)
        .rounded(10.0)
        .bg(sig!(self.theme).map(|t: &u32| card_fill(*t)));
        // the shadow layer under the card: raw, so an anchor of the nominal size in an absolutely placed wrapper
        let shadow = div().abs(0.0, 0.0).w(CARD_W).h(CARD_H).child(anchor(CARD_W, CARD_H, deco_box(0.0, 0.0, CARD_W, CARD_H, 10.0).shadows(&CARD_SHADOW)));
        column((shadow, card)).w(CARD_W).grow(1.0).shrink(0.0).h(CARD_H).relative()
    }

    fn grid_row(&self, r: usize) -> impl Element<Self> + '_ {
        let n = self.cards;
        row(each((0..COLS).map(move |c| r * COLS + c).filter(move |&i| i < n).map(move |i| self.card(i)))).gap(GAP).w_pct(1.0).h(CARD_H)
    }
}

impl View for Dash {
    fn view(&self) -> impl Element<Self> + '_ {
        let range_button = move |j: u32| button(RANGE_LABELS[j as usize]).w(52.0).on_click(move |s: &mut Dash| s.set_range(j));
        let legend_item = move |k: u32| {
            row((
                block(10.0, 10.0).bg(sig!(self.look).map(move |l: &u32| series_col(*l, k))).rounded(5.0),
                text(SERIES_NAMES[k as usize]).font(13.0, 18.0).color(sig!(self.theme).map(|t: &u32| ink(*t))),
            ))
            .gap(6.0)
            .items(Align::Center)
            .w(56.0)
            .on_hover(move |s: &mut Dash, e: &Ev| s.hover_series(k, e.on))
        };
        let header = row((
            text("Fleet overview").font(18.0, 24.0).color(sig!(self.theme).map(|t: &u32| ink(*t))).w(200.0),
            row((range_button(0), range_button(1), range_button(2), range_button(3))).gap(6.0),
            div().grow(1.0),
            row((legend_item(0), legend_item(1), legend_item(2), legend_item(3))).gap(8.0),
        ))
        .gap(24.0)
        .px(16.0)
        .h(HEADER_H)
        .items(Align::Center)
        .bg(sig!(self.theme).map(|t: &u32| card_fill(*t)));
        let body = column(each((0..self.rows()).map(move |r| self.grid_row(r))))
            .gap(GAP)
            .p(16.0)
            .grow(1.0)
            .bg(sig!(self.theme).map(|t: &u32| page(*t)))
            .scroll(sig!(self.scroll));
        column((header, body)).w_pct(1.0).h_pct(1.0).bg(sig!(self.theme).map(|t: &u32| page(*t)))
    }
}

/// The pointer positions of the header's controls (points) at the scene's size: range button `j`'s centre and
/// the legend item `k`'s centre. (Fixed widths in the header make these exact.)
fn button_xy(j: u32) -> (f32, f32) {
    (16.0 + 200.0 + 24.0 + 58.0 * j as f32 + 26.0, HEADER_H / 2.0)
}
fn legend_xy(k: u32) -> (f32, f32) {
    (Dash::SIZE.0 - 16.0 - (4.0 * 56.0 + 3.0 * 8.0) + 64.0 * k as f32 + 28.0, HEADER_H / 2.0)
}

impl Scene for Dash {
    const NAME: &'static str = "dash";

    fn build(scale: f32) -> Dash {
        let cards = scaled(MAX_CARDS, scale, 10).min(MAX_CARDS);
        let mut rng = Rng::new(0xda54);
        let values: [f32; MAX_CARDS] = std::array::from_fn(|_| 20.0 + rng.unit() * 900.0);
        let gauge: [f32; MAX_CARDS] = std::array::from_fn(|_| 0.2 + rng.unit() * 0.7);
        let mut s = Dash {
            cards,
            keys: std::array::from_fn(|i| Signal::new(1000 + i as u64 * 7)),
            values: values.map(Signal::new),
            deltas: std::array::from_fn(|_| Signal::new((rng.unit() - 0.4) * 12.0)),
            gauge: gauge.map(Signal::new),
            parts: std::array::from_fn(|_| Signal::new(25.0)),
            util: std::array::from_fn(|_| Signal::new(0.2 + rng.unit() * 0.7)),
            veil: std::array::from_fn(|_| Signal::new(0.0)),
            ring: std::array::from_fn(|_| Signal::new(0.0)),
            theme: Signal::new(0),
            look: Signal::new(NO_HL),
            range: Signal::new(0),
            scroll: Signal::new(0.0),
            rng,
        };
        for c in 0..cards {
            if kind(c) == 4 {
                s.reshare(c);
            }
        }
        s
    }

    fn caps(&self) -> Caps {
        let n = self.cards;
        // per card: ~110 instances (marks, boxes, ~40 glyphs); ~60 records are boxes and texts
        let instances = (n * 160 + 4096).next_power_of_two();
        let records = n * 160 + 4096;
        Caps {
            instances: instances.min(Caps::RESERVE.instances),
            pages: (records / 32 + 1024).next_power_of_two().min(Caps::RESERVE.pages),
            glyphs: (n * 100 + 65536).next_power_of_two().min(Caps::RESERVE.glyphs),
            runs: (n * 40 + 8192).next_power_of_two().min(Caps::RESERVE.runs),
            spaces: 2048,
            clips: 2048,
            ..Caps::DEFAULT
        }
    }

    fn describe(&self) -> String {
        let n = |k: usize| (0..self.cards).filter(|&c| kind(c) == k).count();
        format!(
            "{} cards in {} rows of {COLS}: {} bars, {} sparklines ({SPARK_N} quads), {} trends (2 x {TREND_N} dots), {} gauges, {} stacked bars; {} series marks on the highlight",
            self.cards,
            self.rows(),
            n(0),
            n(1),
            n(2),
            n(3),
            n(4),
            self.fanout()
        )
    }

    fn fanout(&self) -> usize {
        // wires on `look`: the scrim (bars), sparkline columns, trend dots, gauge fill, 4 stacked fills, and each
        // card's utilisation fill; plus the 4 legend dots
        let per = |k: usize| match k {
            0 => 1,
            1 => SPARK_N as usize,
            2 => 2 * TREND_N as usize,
            3 => 1,
            _ => 4,
        };
        4 + (0..self.cards).map(|c| per(kind(c)) + 1).sum::<usize>()
    }

    fn steady(app: &mut App<Self>, k: usize) {
        // one data tick: 20 cards spread over the grid, rotating through all of them over ten steps
        app.turn(|s| {
            let n = s.cards;
            for j in 0..20 {
                s.tick_card((j * 10 + k) % n);
            }
        });
        // every 8th step a click on a time-range button
        if k % 8 == 7 {
            let (x, y) = button_xy(((k / 8 + 1) % 4) as u32);
            app.pointer_down(x, y, Button::Primary);
            app.pointer_up(x, y, Button::Primary);
        }
    }

    fn hover(app: &mut App<Self>, k: usize) {
        // onto the legend item "B", then off it into the header's blank middle
        if k % 2 == 0 {
            let (x, y) = legend_xy(1);
            app.pointer_move(x, y);
        } else {
            app.pointer_move(700.0, HEADER_H / 2.0);
        }
    }

    fn theme(app: &mut App<Self>, _k: usize) {
        app.turn(|s| {
            let t = *s.theme ^ 1;
            s.theme.set(t);
            let l = *s.look;
            s.look.set((l & 0xff) | t << 8);
        });
    }

    fn animate(app: &mut App<Self>, k: usize) {
        app.turn(|s| {
            let n = s.cards;
            let vis = n.min(20);
            // cross-fade the cards of the first screen: half fade out under the veil while the other half fade in
            for c in 0..vis {
                s.veil[c].set(if (c + k) % 2 == 0 { 0.85 } else { 0.0 });
                // new targets for the utilisation bars: widths transition
                let u = 0.1 + 0.85 * unit(c as u32, k as u32);
                s.util[c].set(u);
            }
            // and a rotating group offscreen
            for j in 0..10 {
                let c = vis + (k * 10 + j) % (n - vis).max(1);
                if c < n {
                    s.veil[c].set(if (k + j) % 2 == 0 { 0.85 } else { 0.0 });
                }
            }
            // the highlighted card's ring: this one lights up, the previous goes out
            let (cur, prev) = (k % vis, (k + vis - 1) % vis);
            s.ring[prev].set(0.0);
            s.ring[cur].set(1.0);
        });
    }
}
