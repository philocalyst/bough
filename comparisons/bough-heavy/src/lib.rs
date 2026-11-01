//! zgx's heavy dashboard (`zgx_workload::heavy`) as a bough view: one `live` signal fanning out
//! to every value text / bar / progress / status wire, a `scroll` signal driving a keyed virtual
//! table, a `log` signal. Geometry, colours and strings come from the workload functions.
use bough::prelude::*;
use zgx_workload::heavy::*;
use zgx_workload::Mode;

pub struct Dash {
    pub live: Signal<u64>,
    pub scroll: Signal<f32>,
    pub log: Signal<String>,
}

impl Dash {
    pub fn new(work: &Heavy) -> Dash {
        Dash { live: Signal::new(work.live), scroll: Signal::new(work.scroll), log: Signal::new(work.visible_log()) }
    }
}

/// What the workload's tick publishes into the view for a mode (zgx's `runtime.batch` body).
pub fn publish(d: &mut Dash, work: &Heavy, mode: Mode) {
    if matches!(mode, Mode::Stream | Mode::Both) {
        d.live.set(work.live);
        d.log.set(work.visible_log());
    }
    if matches!(mode, Mode::Scroll | Mode::Both) {
        d.scroll.set(work.scroll);
    }
}

pub fn font_bytes() -> Vec<u8> {
    zgx_workload::font_file().unwrap_or_else(|| FONT_BYTES_FALLBACK.to_vec())
}
const FONT_BYTES_FALLBACK: &[u8] = zgx_workload::FONT_BYTES;

pub fn config(scale: f32) -> Config {
    Config { width_pt: WIDTH, height_pt: HEIGHT, scale, font: font_bytes(), ring: 4 }
}

fn panel<Ch>(pos: (f32, f32), size: (f32, f32), radius: f32, ch: Ch) -> Quad<Color, f32, (f32, f32), Ch> {
    quad(pos.0, pos.1, size.0, size.1).rounded(radius).border(1.0, rgb(BORDER)).bg(rgb(PANEL)).children(ch)
}

fn tile<'a>(i: usize, live: &'a Signal<u64>) -> impl Element<Dash> + 'a {
    panel(
        tile_position(i),
        TILE,
        8.0,
        (
            text(tile_label(i)).at(TILE_LABEL.0, TILE_LABEL.1).size(11.0).line_height(14.0).color(rgb(MUTED)),
            text(live.map(move |l| tile_value(i, *l))).at(TILE_VALUE.0, TILE_VALUE.1).size(20.0).line_height(26.0).color(rgb(TEXT)),
        ),
    )
}

fn card<'a>(i: usize, live: &'a Signal<u64>) -> impl Element<Dash> + 'a {
    panel(
        card_position(i),
        CARD,
        6.0,
        (
            text(card_title(i)).at(CARD_TITLE.0, CARD_TITLE.1).size(11.0).line_height(14.0).color(rgb(MUTED)),
            text(live.map(move |l| card_value(i, *l))).at(CARD_VALUE.0, CARD_VALUE.1).size(16.0).line_height(20.0).color(rgb(TEXT)),
            quad(PROGRESS.0, PROGRESS.1, PROGRESS.2, PROGRESS.3).rounded(2.0).bg(rgb(TRACK)),
            quad(PROGRESS.0, PROGRESS.1, 0.0, PROGRESS.3).rounded(2.0).bg(rgb(ACCENT)).width(live.map(move |l| card_progress(i, *l))),
            each((0..BARS).map(move |bar| {
                quad(bar_x(bar), 0.0, BAR_WIDTH, 0.0).bg(rgb(ACCENT)).span(live.map(move |l| {
                    let h = bar_height(i, bar, *l);
                    (SPARK_BASE - h, h)
                }))
            })),
        ),
    )
}

fn cell<'a, S: IntoProp<String> + 'a>(s: S, column: usize, color: Color) -> impl Element<Dash> + 'a {
    text(s).at(COLUMNS[column].0, CELL_TOP).size(12.0).line_height(16.0).color(color)
}

fn table_row<'a>(live: &'a Signal<u64>, row: usize) -> impl Element<Dash> + 'a {
    quad(0.0, 0.0, TABLE.0, TABLE_ROW).bg(rgb(row_color(row))).children((
        cell(row_id(row), 0, rgb(MUTED)),
        cell(row_worker(row), 1, rgb(TEXT)),
        quad(PILL.0, PILL.1, PILL.2, PILL.3).rounded(8.0).bg(live.map(move |l| rgb(status(row, *l).1))).children(
            text(live.map(move |l| status(row, *l).0.to_owned())).at(PILL_TEXT.0, PILL_TEXT.1).size(10.0).line_height(14.0).color(rgb(TEXT)),
        ),
        cell(live.map(move |l| row_latency(row, *l)), 3, rgb(TEXT)),
        cell(live.map(move |l| row_throughput(row, *l)), 4, rgb(TEXT)),
    ))
}

fn table<'a>(live: &'a Signal<u64>, scroll: &'a Signal<f32>) -> impl Element<Dash> + 'a {
    let _ = live;
    panel(
        TABLE_ORIGIN,
        TABLE,
        8.0,
        (
            // header strip: a rounded rect clipped to the header band (square bottom edge)
            clip(0.0, 0.0, TABLE.0 - 2.0, TABLE_HEADER, quad(0.0, 0.0, TABLE.0, TABLE_HEADER + 8.0).rounded(7.0).bg(rgb(HEADER))),
            each(COLUMNS.iter().map(|(x, title)| text(*title).at(*x, 7.0).size(11.0).line_height(14.0).color(rgb(MUTED)))),
            virtual_list(
                scroll,
                TABLE_ROW,
                [0.0, TABLE_HEADER, TABLE.0 - 2.0, TABLE.1 - 2.0 - TABLE_HEADER],
                row_range,
                |d: &Dash, row: usize, m: &mut Mounter<'_, Dash>| m.mount(table_row(&d.live, row)),
            ),
        ),
    )
}

impl View for Dash {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, WIDTH, HEIGHT).bg(rgb(BACKGROUND)),
            text(TITLE).at(TITLE_ORIGIN.0, TITLE_ORIGIN.1).size(18.0).line_height(24.0).color(rgb(TEXT)),
            each((0..TILES).map(|i| tile(i, &self.live))),
            each((0..CARDS).map(|i| card(i, &self.live))),
            table(&self.live, &self.scroll),
            panel(
                LOG_ORIGIN,
                LOG,
                8.0,
                text(&self.log).at(LOG_TEXT.0, LOG_TEXT.1).size(11.0).line_height(LOG_LINE_HEIGHT).color(rgb(MUTED)),
            ),
        )
    }
}
