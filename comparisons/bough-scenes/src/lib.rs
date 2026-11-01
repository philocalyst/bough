//! zgx's comparison scenes that need no layout engine, as bough views: the lab (`component_workload`),
//! animate, blink, and the two fixed-geometry micro_bench scenes (state, vscroll-fixed). Geometry,
//! colours, strings and timing come from `zgx_workload`; zgx's "normal" line height is
//! `ceil(1.4 * size)` and every text box here is top aligned, so the line boxes are written out.
use bough::prelude::*;
use std::ops::Range;
use zgx_workload as wl;

pub const FG: Color = Color::rgba(229, 237, 247, 255);
pub const SURFACE: Color = Color::rgba(25, 34, 48, 255);
pub const SURFACE_ALT: Color = Color::rgba(29, 40, 56, 255);
pub const BUTTON: Color = Color::rgba(41, 59, 80, 255);

pub fn font_bytes() -> Vec<u8> {
    wl::font_file().unwrap_or_else(|| wl::FONT_BYTES.to_vec())
}

pub fn ring_from_env() -> usize {
    std::env::var("BOUGH_RING").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn cfg(w: f32, h: f32, scale: f32) -> Config {
    let mut c = Config::new(w, h, scale, font_bytes());
    c.ring = ring_from_env();
    c
}

/// zgx's "normal" line pitch.
pub fn normal(size: f32) -> f32 {
    (size * 1.4).ceil()
}

// ------------------------------------------------------------------------------------------ lab

pub fn lab_config(scale: f32) -> Config {
    cfg(wl::WIDTH, wl::HEIGHT, scale)
}

pub struct Lab {
    pub stream: Signal<String>,
    pub scroll: Signal<f32>,
}

impl Lab {
    pub fn new(work: &wl::Workload) -> Lab {
        Lab { stream: Signal::new(work.visible_text()), scroll: Signal::new(work.scroll_offset) }
    }
}

/// `Workload::row_range` as a function of the scroll offset (two rows of overscan each side).
pub fn lab_rows(scroll: f32) -> Range<usize> {
    let first = (scroll / wl::ROW_HEIGHT) as usize;
    let end = ((scroll as f64 + wl::VIEWPORT_HEIGHT as f64) / wl::ROW_HEIGHT as f64).ceil() as usize;
    first.saturating_sub(2)..end.saturating_add(2).min(wl::ROW_COUNT)
}

/// What the workload's tick publishes into the view (zgx's `runtime.batch` body).
pub fn lab_publish(l: &mut Lab, work: &wl::Workload, mode: wl::Mode) {
    if matches!(mode, wl::Mode::Stream | wl::Mode::Both) {
        l.stream.set(work.visible_text());
    }
    if matches!(mode, wl::Mode::Scroll | wl::Mode::Both) {
        l.scroll.set(work.scroll_offset);
    }
}

fn label(s: impl TextSrc<Lab>, x: f32, y: f32, size: f32) -> impl Element<Lab> {
    text(s).at(x, y).size(size).line_height(normal(size)).color(FG)
}

fn control(s: &'static str, x: f32) -> impl Element<Lab> {
    quad(x, 55.0, 50.0, 32.0).bg(BUTTON).children(label(s, 12.0, 6.0, 14.0))
}

impl View for Lab {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, wl::WIDTH, wl::HEIGHT).bg(rgb(0x10141c)),
            label("zgx performance lab", 20.0, 20.0, 20.0),
            (control("+", 20.0), control("=", 80.0), control("0", 140.0)),
            (label("0", 220.0, 62.0, 14.0), label("stable", 280.0, 62.0, 14.0), label("0", 370.0, 62.0, 14.0)),
            quad(20.0, 110.0, 920.0, 160.0).bg(SURFACE).children(clip(
                0.0,
                0.0,
                920.0,
                160.0,
                text(sig!(self.stream)).at(8.0, 8.0).size(14.0).line_height(normal(14.0)).color(FG),
            )),
            virtual_list(
                sig!(self.scroll),
                wl::ROW_HEIGHT,
                [20.0, 290.0, 920.0, wl::VIEWPORT_HEIGHT],
                lab_rows,
                |_: &Lab, i: usize, m: &mut Mounter<'_, Lab>| {
                    m.mount(
                        quad(0.0, 0.0, 920.0, wl::ROW_HEIGHT)
                            .bg(if i % 2 == 0 { SURFACE } else { SURFACE_ALT })
                            .children(label(wl::row_label(i), 8.0, 4.0, 14.0)),
                    )
                },
            ),
        )
    }
}

// -------------------------------------------------------------------------------------- animate

pub fn anim_config(scale: f32) -> Config {
    cfg(960.0, 720.0, scale)
}

pub struct Anim {
    pub clock: Signal<f32>,
}

fn dot_color(i: usize, t: f32) -> Color {
    let a = (wl::animation::dot_opacity(i, t) * 255.0).round() as u8;
    let c = wl::animation::DOT_COLOR;
    Color::rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, a)
}

impl View for Anim {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        use wl::animation::*;
        (
            quad(0.0, 0.0, 960.0, 720.0).bg(rgb(BACKGROUND)),
            each((0..COLUMNS * ROWS).map(|i| {
                let (x, y) = dot_position(i);
                quad(x, y, DOT, DOT).rounded(DOT / 2.0).bg(sig!(self.clock).map(move |t| dot_color(i, *t)))
            })),
            text(SHIMMER)
                .at(SHIMMER_ORIGIN.0, SHIMMER_ORIGIN.1)
                .size(18.0)
                .line_height(normal(18.0))
                .color(Color::rgba(255, 255, 255, 255))
                .glyph_alpha(sig!(self.clock), |i, n, t| shimmer_alpha(i, n, *t)),
        )
    }
}

// ---------------------------------------------------------------------------------------- blink

pub struct Blink {
    pub shown: Signal<bool>,
}

impl View for Blink {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        use wl::blink::*;
        let c = CARET_COLOR;
        let on = Color::rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 255);
        let off = Color::rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 0);
        (
            quad(0.0, 0.0, 960.0, 720.0).bg(rgb(BACKGROUND)),
            quad(CARET.0, CARET.1, CARET.2, CARET.3).bg(sig!(self.shown).map(move |s| if *s { on } else { off })),
        )
    }
}

// ------------------------------------------------------------------------------------ micro: state

pub struct MicroState {
    pub counter: Signal<u64>,
}

impl View for MicroState {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        use wl::micro::*;
        let ink = rgb(wl::vlist::TEXT);
        let cell = move |i: usize| {
            let (r, c) = (i / STATIC_COLUMNS, i % STATIC_COLUMNS);
            text(static_label(i))
                .at(8.0 + c as f32 * (CELL.0 + 2.0), 34.0 + r as f32 * (CELL.1 + 2.0))
                .size(FONT_SIZE)
                .line_height(normal(FONT_SIZE))
                .color(ink)
        };
        (
            quad(0.0, 0.0, wl::WIDTH, wl::HEIGHT).bg(rgb(wl::vlist::BACKGROUND)),
            text(sig!(self.counter).map(|c| counter_label(*c))).at(8.0, 8.0).size(FONT_SIZE).line_height(normal(FONT_SIZE)).color(ink),
            each((0..STATIC_ROWS * STATIC_COLUMNS).map(cell)),
        )
    }
}

// --------------------------------------------------------------------------------- micro: vscroll

pub struct MicroScroll {
    pub scroll: Signal<f32>,
}

pub fn micro_rows(scroll: f32) -> Range<usize> {
    use wl::micro::*;
    let first = (scroll / FIXED_ROW) as usize;
    let end = ((scroll as f64 + VIEWPORT as f64) / FIXED_ROW as f64).ceil() as usize;
    first.saturating_sub(2)..end.saturating_add(2).min(ROWS)
}

impl View for MicroScroll {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        use wl::micro::*;
        (
            quad(0.0, 0.0, wl::vlist::WIDTH, VIEWPORT).bg(rgb(wl::vlist::BACKGROUND)),
            virtual_list(
                sig!(self.scroll),
                FIXED_ROW,
                [0.0, 0.0, wl::vlist::WIDTH, VIEWPORT],
                micro_rows,
                |_: &MicroScroll, i: usize, m: &mut Mounter<'_, MicroScroll>| {
                    m.mount(quad(0.0, 0.0, wl::vlist::WIDTH, FIXED_ROW).bg(rgb(wl::vlist::ROW_COLORS[i % 2])).children(
                        text(fixed_label(i)).at(12.0, (FIXED_ROW - normal(wl::vlist::FONT_SIZE)) / 2.0).size(wl::vlist::FONT_SIZE).line_height(normal(wl::vlist::FONT_SIZE)).color(rgb(wl::vlist::TEXT)),
                    ))
                },
            ),
        )
    }
}

pub fn micro_config(scene: &str, scale: f32) -> Config {
    if scene == "state" { cfg(wl::WIDTH, wl::HEIGHT, scale) } else { cfg(wl::vlist::WIDTH, wl::micro::VIEWPORT, scale) }
}

pub fn blink_config(scale: f32) -> Config {
    cfg(960.0, 720.0, scale)
}
