//! zgx's example applications `mail` and `dashboard` (`crates/zgx-script/examples/{mail,dashboard}`, Luau) ported
//! natively onto bough, as benchmarks: the same window sizes, data (the same seeded generators), tree (the
//! same boxes, paddings, gaps, font sizes and theme), and the interactions zgx's `apps_bench` measures
//! (`support/apps.rs`), driven through bough's input path (hit test, focus, routes) and measured over the same
//! span (`harness::Bench::measure`: two warm-ups, then `REPS` interactions each followed by its frame).
//!
//! * [`mail`]: a sidebar of mailboxes, a virtualized message list (4,000 messages), the open message, search.
//! * [`dash`]: a toolbar, two charts, and a sortable, filterable table of 100,000 instruments.
//!
//! Deviations from zgx's apps are stated where they are made and collected in `RESULTS.md`.
use bough::layout::prelude::*;
use bough::raw::*;

pub mod dash;
pub mod fmt;
pub mod harness;
pub mod mail;

/// The font zgx's harness uses (`ZGX_FONT_FILE`, else the workload's bundled DejaVuSans).
pub fn font() -> Vec<u8> {
    zgx_workload::font_file().unwrap_or_else(|| zgx_workload::FONT_BYTES.to_vec())
}

/// This process's instructions retired (`proc_pid_rusage`, `ri_instructions`: zgx's `instructions()`).
pub fn instructions() -> u64 {
    bough::sys::perf::snap().instrs
}

pub fn env_usize(name: &str, d: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

// =============================================================================== theme (zgx-dark)

/// zgx's built-in dark theme (`crates/zgx/themes/zgx-dark.json`): the dashboard sets it, the mail app follows
/// the system's appearance (dark here).
pub mod theme {
    use bough::raw::*;
    pub const SUNKEN: Color = rgb(0x0b0e14);
    pub const BASE: Color = rgb(0x1a2230);
    pub const RAISED: Color = rgb(0x232e40);
    pub const TEXT: Color = rgb(0xe5edf7);
    pub const TEXT2: Color = rgb(0x8a9ab0);
    pub const TEXT3: Color = rgb(0x66758a);
    pub const BORDER: Color = rgb(0x2f3d52);
    pub const BORDER_SUBTLE: Color = rgb(0x222c3c);
    pub const BORDER_STRONG: Color = rgb(0x46576f);
    pub const ACCENT: Color = rgb(0x5ea5ff);
    pub const SELECTION: Color = rgb(0x2b4468);
    /// `hover`: a translucent wash (`#3b527080`).
    pub const HOVER: Color = Color::rgba(0x3b, 0x52, 0x70, 0x80);
    /// `button.hover` = `over({button.bg}, {hover})`.
    pub const BUTTON_HOVER: Color = rgb(0x2f4058);
    /// `scrollbar.track`; the thumb is `{accent}`.
    pub const TRACK: Color = rgb(0x2f4058);
    pub const CLEAR: Color = Color(0);
    pub const RED: Color = rgb(0xe5534b);
    pub const GREEN: Color = rgb(0x3fb950);
    /// `font.size` / `font.line`: md, sm, lg, xl (points).
    pub const MD: (f32, f32) = (16.0, 22.0);
    pub const SM: (f32, f32) = (13.0, 18.0);
    pub const LG: (f32, f32) = (18.0, 24.0);
    pub const XL: (f32, f32) = (22.0, 28.0);
}
use theme::*;

/// zgx's `ui.button` (`WidgetStyles::button`): `px 12, py 8, gap 6, items center, bg button.bg, rounded radius.md`,
/// its label in the body font. Add `.hover_bg(BUTTON_HOVER)` and a route.
pub fn button<S>(s: S) -> bough::layout::LBox<Color, bough::layout::Para<S>> {
    row(label(s).font(MD.0, MD.1).color(TEXT)).px(12.0).py(8.0).gap(6.0).items(Align::Center).bg(RAISED).rounded(6.0)
}

/// A one-line field's caret (zgx's editor caret, solid: blinking is a timer, not part of a measured frame):
/// where it is (points, absolute: the field's text origin, read from the layout after mounting, plus the
/// text's width), shown once the field has been focused.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Caret {
    pub x: f32,
    pub y: f32,
    pub on: bool,
}

/// The caret's lane (one 1 x 18 point quad).
pub fn caret_lane(c: &Caret, _: usize) -> Lane {
    Lane { x: c.x, y: c.y, w: 1.0, h: 18.0, color: if c.on { TEXT } else { CLEAR } }
}

/// A field's text and its placeholder, as a **layout island**: the one row of a `List`, so a keystroke lays
/// out this row alone. The field's box is sized by its parent whatever it holds, so nothing outside it can
/// move when its text does; a list row is bough's only way to say so (its own layout group), and without it
/// every keystroke re-lays out the window's whole group (~0.2 M instructions in the mail window).
pub struct FieldText {
    pub q: Signal<String>,
    ph: &'static str,
}
impl FieldText {
    pub fn list(ph: &'static str, q: String) -> List<FieldText> {
        List::from_iter_in([FieldText { q: Signal::new(q), ph }])
    }
}
impl View for FieldText {
    fn view(&self) -> impl Element<Self> + '_ {
        let ph = self.ph;
        row((
            label(sig!(self.q)).font(MD.0, MD.1).color(TEXT),
            label(sig!(self.q).map(move |q: &String| if q.is_empty() { ph.to_owned() } else { String::new() })).font(MD.0, MD.1).color(TEXT3),
        ))
    }
}

/// zgx's `ui.input` box (`editor` style: `input.bg`, rounded, a 1-point border drawn over): its text an island
/// (`FieldText`). Add its width, its key and click routes.
pub fn field_box<'a, V: 'static>(text: bough::list::LWired<'a, FieldText, V>) -> bough::layout::LBox<Color, bough::listview::LList<'a, bough::listview::ListRows<'a, FieldText, V>, V>> {
    row(list(text).grow(1.0).h(MD.1)).h(36.0).px(9.0).items(Align::Center).bg(RAISED).rounded(6.0).border(1.0, BORDER)
}

/// The width (points) of `text` at `size` points, shaped by the app's own shaper (the one its labels use).
pub fn text_width(shaper: &bough_text::Shaper, text: &str, size: f32, run: &mut bough_text::GlyphRun) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    shaper.shape_into(text, size, run);
    run.width as f32 * size / shaper.script.upem as f32
}

/// `text` cut to fit `max` points with an ellipsis, as zgx's `truncate = true` text ends (bough has no text
/// overflow policy: the cut is made here, on the string, with the labels' own shaper).
pub fn truncate(shaper: &bough_text::Shaper, text: &str, size: f32, max: f32, run: &mut bough_text::GlyphRun) -> String {
    if text_width(shaper, text, size, run) <= max {
        return text.to_owned();
    }
    let k = size / shaper.script.upem as f32;
    let ell = text_width(shaper, "\u{2026}", size, run);
    shaper.shape_into(text, size, run);
    // the longest prefix (cut at a glyph's cluster) whose pen plus the ellipsis fits
    let mut cut = 0;
    for i in 0..run.len() {
        let end = (run.x[i] + run.advance(i)) as f32 * k;
        if end + ell > max {
            break;
        }
        cut = run.cluster_of(i) as usize + 1;
    }
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut s = text[..cut].trim_end().to_owned();
    s.push('\u{2026}');
    s
}

/// The absolute rect (points: x, y, w, h) of the layout box reached from the window's root group by `path`
/// (child indices). The harness finds the buttons and fields it clicks this way, as zgx's finds them by id
/// or semantics.
pub fn lay_rect(rt: &bough::rt::Rt, path: &[usize]) -> [f32; 4] {
    let mut n = rt.lay.first_root();
    let (mut x, mut y, _, _) = rt.lay.eng.rect(n);
    for &i in path {
        n = rt.lay.eng.kids(n)[i];
        let (rx, ry, _, _) = rt.lay.eng.rect(n);
        x += rx;
        y += ry;
    }
    let (_, _, w, h) = rt.lay.eng.rect(n);
    let s = rt.scale;
    [x / s, y / s, w / s, h / s]
}

/// A config of `w x h` points at `scale` device px per point.
pub fn config(w: f32, h: f32, scale: f32) -> Config {
    Config::new(w, h, scale, font()).once()
}
