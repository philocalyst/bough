//! zgx's heavy dashboard (`zgx_workload::heavy`) as a bough view: one `live` signal fanning out
//! to every value text / bar / progress / status wire, a `scroll` signal driving a keyed virtual
//! table, a `log` signal. Geometry, colours and strings come from the workload functions.
use bough::raw::*;
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
    let mut c = Config::new(WIDTH, HEIGHT, scale, font_bytes()).once();
    c.ring = std::env::var("BOUGH_RING").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    c
}

/// The numbers behind zgx's value strings, and their formats: `NumFmt` renders them straight into the
/// labels (no `String`), equal to the workload's `format!` output (`tests/correctness.rs` checks it).
pub fn tile_v(tile: usize, live: u64) -> f32 {
    let s = sample(900 + tile, live);
    match tile {
        0 => 12_000. + 40_000. * s,
        1 => 3. * s,
        2 => 4. + 30. * s,
        3 => 40. + 200. * s,
        4 => (800 + (s * 400.) as u32) as f32,
        _ => ((s * 5_000.) as u32) as f32,
    }
}
pub fn tile_fmt(tile: usize) -> NumFmt {
    match tile {
        0 => NumFmt::new("", Some(0), ""),
        1 => NumFmt::new("", Some(2), "%"),
        2 | 3 => NumFmt::new("", Some(1), " ms"),
        _ => NumFmt::new("", None, ""),
    }
}
pub fn card_v(card: usize, live: u64) -> f32 {
    4. + 96. * sample(card, live)
}
pub fn latency_v(row: usize, live: u64) -> f32 {
    2. + 120. * sample(row + 2000, live)
}
pub fn throughput_v(row: usize, live: u64) -> f32 {
    0.2 + 48. * sample(row + 3000, live)
}
pub const MS: NumFmt = NumFmt::new("", Some(1), " ms");
pub const MBS: NumFmt = NumFmt::new("", Some(2), " MB/s");

fn panel<Ch>(pos: (f32, f32), size: (f32, f32), radius: f32, ch: Ch) -> Quad<Color, f32, (f32, f32), Ch> {
    quad(pos.0, pos.1, size.0, size.1).rounded(radius).border(1.0, rgb(BORDER)).bg(rgb(PANEL)).children(ch)
}

fn tile<'a>(i: usize, live: Wired<'a, u64, Dash>) -> impl Element<Dash> + 'a {
    panel(
        tile_position(i),
        TILE,
        8.0,
        (
            text(tile_label(i)).at(TILE_LABEL.0, TILE_LABEL.1).size(11.0).line_height(14.0).color(rgb(MUTED)),
            text(live.num(move |l| tile_v(i, *l), tile_fmt(i))).at(TILE_VALUE.0, TILE_VALUE.1).size(20.0).line_height(26.0).color(rgb(TEXT)),
        ),
    )
}

fn card<'a>(i: usize, live: Wired<'a, u64, Dash>) -> impl Element<Dash> + 'a {
    panel(
        card_position(i),
        CARD,
        6.0,
        (
            text(card_title(i)).at(CARD_TITLE.0, CARD_TITLE.1).size(11.0).line_height(14.0).color(rgb(MUTED)),
            text(live.num(move |l| card_v(i, *l), MS)).at(CARD_VALUE.0, CARD_VALUE.1).size(16.0).line_height(20.0).color(rgb(TEXT)),
            quad(PROGRESS.0, PROGRESS.1, PROGRESS.2, PROGRESS.3).rounded(2.0).bg(rgb(TRACK)),
            quad(PROGRESS.0, PROGRESS.1, 0.0, PROGRESS.3).rounded(2.0).bg(rgb(ACCENT)).width(live.map(move |l| card_progress(i, *l))),
            // the 26-bar sparkline: one run record, bar `i` = `bar_height(card, i, live)`
            bars(BAR_X, BAR_STEP, BAR_WIDTH, BARS, SPARK_BASE, SPARK_HEIGHT, 2.0, live, move |t| sample(i, t)).color(rgb(ACCENT)),
        ),
    )
}

fn cell<'a, S: TextSrc<Dash> + 'a>(s: S, column: usize, color: Color) -> impl Element<Dash> + 'a {
    text(s).at(COLUMNS[column].0, CELL_TOP).size(12.0).line_height(16.0).color(color)
}

fn table_row<'a>(live: Wired<'a, u64, Dash>, row: usize) -> impl Element<Dash> + 'a {
    quad(0.0, 0.0, TABLE.0, TABLE_ROW).bg(rgb(row_color(row))).children((
        cell(row_id(row), 0, rgb(MUTED)),
        cell(row_worker(row), 1, rgb(TEXT)),
        quad(PILL.0, PILL.1, PILL.2, PILL.3).rounded(8.0).bg(live.map(move |l| rgb(status(row, *l).1))).children(
            text(live.map(move |l| status(row, *l).0.to_owned())).at(PILL_TEXT.0, PILL_TEXT.1).size(10.0).line_height(14.0).color(rgb(TEXT)),
        ),
        cell(live.num(move |l| latency_v(row, *l), MS), 3, rgb(TEXT)),
        cell(live.num(move |l| throughput_v(row, *l), MBS), 4, rgb(TEXT)),
    ))
}

fn table<'a>(live: Wired<'a, u64, Dash>, scroll: Wired<'a, f32, Dash>) -> impl Element<Dash> + 'a {
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
                |d: &Dash, row: usize, m: &mut Mounter<'_, Dash>| m.mount(table_row(sig!(d: Dash => live), row)),
            ),
        ),
    )
}

impl View for Dash {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, WIDTH, HEIGHT).bg(rgb(BACKGROUND)),
            text(TITLE).at(TITLE_ORIGIN.0, TITLE_ORIGIN.1).size(18.0).line_height(24.0).color(rgb(TEXT)),
            each((0..TILES).map(|i| tile(i, sig!(self.live)))),
            each((0..CARDS).map(|i| card(i, sig!(self.live)))),
            table(sig!(self.live), sig!(self.scroll)),
            panel(
                LOG_ORIGIN,
                LOG,
                8.0,
                text(sig!(self.log)).at(LOG_TEXT.0, LOG_TEXT.1).size(11.0).line_height(LOG_LINE_HEIGHT).color(rgb(MUTED)),
            ),
        )
    }
}
