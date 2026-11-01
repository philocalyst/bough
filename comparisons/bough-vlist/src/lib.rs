//! zgx's vlist scene (`zgx_workload::vlist`) as a bough view: 100,000 (or `rows`) chat rows of
//! wrapped text in a measured virtual list, laid out by cephal. Geometry, fonts, line height, widths,
//! colours, the image placeholder and the modes come from the workload functions.
use bough::layout::prelude::*;
use bough::prelude::*;
use zgx_workload::vlist::{self, Mode};

pub struct VList {
    pub scroll: Signal<f32>,
    pub rows: usize,
}

/// A three-line row with no image: what an unmeasured row counts as (zgx's `vlist_view::ESTIMATE`).
pub const ESTIMATE: f32 = 2. * vlist::PAD.1 + vlist::LINE_HEIGHT + vlist::GAP + 3. * vlist::LINE_HEIGHT;
pub const OVERSCAN: usize = 2;

/// One chat row: padding 16/10, gap 6, background by parity; an author + time line (20 pt), the wrapped
/// text at `TEXT_WIDTH`, and for one row in four a 160 x 90 image placeholder.
pub fn chat_row(index: usize) -> impl Element<VList> {
    let f = |s: String, c: u32| label(s).font(vlist::FONT_SIZE, vlist::LINE_HEIGHT).color(rgb(c));
    column((
        row((f(vlist::author(index), vlist::AUTHOR), f(vlist::time(index), vlist::MUTED))).gap(8.).h(vlist::LINE_HEIGHT),
        para(vlist::text(index)).font(vlist::FONT_SIZE, vlist::LINE_HEIGHT).color(rgb(vlist::TEXT)).w(vlist::TEXT_WIDTH),
        each(vlist::has_image(index).then(|| block(vlist::IMAGE.0, vlist::IMAGE.1).bg(rgb(vlist::IMAGE_COLOR))).into_iter()),
    ))
    .w(vlist::WIDTH)
    .px(vlist::PAD.0)
    .py(vlist::PAD.1)
    .gap(vlist::GAP)
    .bg(rgb(vlist::ROW_COLORS[index % 2]))
}

impl VList {
    pub fn new(rows: usize) -> VList {
        VList { scroll: Signal::new(0.0), rows }
    }
}

impl View for VList {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, vlist::WIDTH, vlist::HEIGHT).bg(rgb(vlist::BACKGROUND)),
            measured_list(&self.scroll, self.rows, ESTIMATE, [0.0, 0.0, vlist::WIDTH, vlist::HEIGHT], OVERSCAN, |_, i, m: &mut Mounter<'_, VList>| m.mount(chat_row(i))),
        )
    }
}

pub fn font_bytes() -> Vec<u8> {
    zgx_workload::font_file().unwrap_or_else(|| zgx_workload::FONT_BYTES.to_vec())
}

pub fn config(scale: f32) -> Config {
    Config {
        width_pt: vlist::WIDTH,
        height_pt: vlist::HEIGHT,
        scale,
        font: font_bytes(),
        ring: std::env::var("BOUGH_RING").ok().and_then(|v| v.parse().ok()).unwrap_or(4),
    }
}

/// One tick of the scene's driver (zgx's `vlist_workload` loop body): a jump tick of `Jump` mode puts the
/// row at the top by its (estimated) offset; any other tick adds the mode's delta, back to the top past
/// the end. The list may move the offset itself (anchoring while rows are measured); that is mirrored
/// back so the next tick continues from where the list is.
pub fn step(app: &mut App<VList>, mode: Mode, tick: u64) {
    let next = match vlist::jump(tick) {
        Some(row) if mode == Mode::Jump => app.rt.mregion_row_offset_pt(0, row),
        _ => {
            let max = (app.rt.mregion_content_height_pt(0) - vlist::HEIGHT).max(0.);
            let moved = *app.owner.scroll + vlist::delta(mode, tick);
            if moved > max { 0. } else { moved }
        }
    };
    set_scroll(app, next);
}

pub fn set_scroll(app: &mut App<VList>, v: f32) {
    app.turn(|d| {
        d.scroll.set(v);
    });
    let at = app.rt.mregion_scroll_pt(0);
    app.owner.scroll.set_silent(at);
}

/// The probe (`ZGX_MODE=probe`): each probe row at the top in turn, measured after a few frames.
pub fn probe(app: &mut App<VList>) -> String {
    let mut measured = Vec::new();
    for row in probe_set() {
        for _ in 0..2 {
            let off = app.rt.mregion_row_offset_pt(0, row);
            set_scroll(app, off);
            for _ in 0..6 {
                app.frame();
            }
        }
        measured.push((row, app.rt.mregion_row_height_pt(0, row)));
    }
    vlist::probe_report(&measured, vlist::WIDTH)
}

/// The rows a probe measures: `vlist::probe_rows()`, or with `PROBE_SPREAD=N` N rows spread over the list
/// (`i * 97 % ROWS`; the same set in the zgx-side headless probe), for a wider height comparison.
pub fn probe_set() -> Vec<usize> {
    match std::env::var("PROBE_SPREAD").ok().and_then(|v| v.parse::<usize>().ok()) {
        Some(n) => (0..n).map(|i| i * 97 % vlist::ROWS).collect(),
        None => vlist::probe_rows(),
    }
}
