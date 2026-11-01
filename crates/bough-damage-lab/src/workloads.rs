//! The comparison scenes as deterministic change traces. Geometry, text and
//! motion are copied from zgx's `comparisons/workload/src/lib.rs` (the pure
//! functions) and the zgx adapters' views (`heavy_view.rs`,
//! `component_workload.rs`, `animation_view.rs`, `blink_workload.rs`,
//! `vlist_view.rs`). Window @2x device pixels.

use crate::geom::R;
use crate::trace::*;

fn rgb(c: u32) -> u32 {
    c
}

// ---------------------------------------------------------------- heavy
pub mod heavy {
    pub const WIDTH: f32 = 1280.;
    pub const HEIGHT: f32 = 800.;
    pub const BACKGROUND: u32 = 0x0e1218;
    pub const PANEL: u32 = 0x161c26;
    pub const HEADER: u32 = 0x1c2430;
    pub const STRIPE: u32 = 0x19202b;
    pub const BORDER: u32 = 0x263041;
    pub const TRACK: u32 = 0x243042;
    pub const TEXT: u32 = 0xe5edf7;
    pub const MUTED: u32 = 0x8b98ab;
    pub const ACCENT: u32 = 0x5eb1ff;
    pub const TITLE: &str = "Fleet overview";
    pub const TITLE_ORIGIN: (f32, f32) = (16., 14.);
    pub const TILES: usize = 6;
    pub const TILE: (f32, f32) = (200., 60.);
    pub const TILE_ORIGIN: (f32, f32) = (16., 48.);
    pub const TILE_STEP: f32 = 208.;
    pub const TILE_LABEL: (f32, f32) = (12., 8.);
    pub const TILE_VALUE: (f32, f32) = (12., 26.);
    pub const CARD_COLUMNS: usize = 6;
    pub const CARD_ROWS: usize = 5;
    pub const CARDS: usize = CARD_COLUMNS * CARD_ROWS;
    pub const CARD: (f32, f32) = (120., 104.);
    pub const CARD_ORIGIN: (f32, f32) = (16., 120.);
    pub const CARD_STEP: (f32, f32) = (128., 112.);
    pub const CARD_TITLE: (f32, f32) = (8., 6.);
    pub const CARD_VALUE: (f32, f32) = (8., 20.);
    pub const PROGRESS: (f32, f32, f32, f32) = (8., 46., 104., 4.);
    pub const BARS: usize = 26;
    pub const BAR_X: f32 = 8.;
    pub const BAR_WIDTH: f32 = 3.;
    pub const BAR_STEP: f32 = 4.;
    pub const SPARK_BASE: f32 = 96.;
    pub const SPARK_HEIGHT: f32 = 42.;
    pub const TABLE_ORIGIN: (f32, f32) = (792., 120.);
    pub const TABLE: (f32, f32) = (472., 552.);
    pub const TABLE_HEADER: f32 = 28.;
    pub const TABLE_VIEWPORT: f32 = TABLE.1 - TABLE_HEADER;
    pub const TABLE_ROW: f32 = 24.;
    pub const TABLE_ROWS: usize = 100_000;
    pub const TABLE_OVERSCAN: usize = 2;
    pub const COLUMNS: [(f32, &str); 5] =
        [(10., "ID"), (84., "Worker"), (238., "Status"), (312., "Latency"), (386., "Throughput")];
    pub const CELL_TOP: f32 = 4.;
    pub const PILL: (f32, f32, f32, f32) = (238., 4., 64., 16.);
    pub const PILL_TEXT: (f32, f32) = (8., 1.);
    pub const LOG_ORIGIN: (f32, f32) = (16., 684.);
    pub const LOG: (f32, f32) = (1248., 100.);
    pub const LOG_TEXT: (f32, f32) = (12., 8.);
    pub const LOG_LINES: usize = 5;
    pub const LOG_LINE_HEIGHT: f32 = 16.;

    pub fn row_range(scroll: f32) -> std::ops::Range<usize> {
        let first = (scroll / TABLE_ROW) as usize;
        let end = ((scroll + TABLE_VIEWPORT) / TABLE_ROW).ceil() as usize;
        first.saturating_sub(TABLE_OVERSCAN)..(end + TABLE_OVERSCAN).min(TABLE_ROWS)
    }
    pub fn sample(series: usize, t: u64) -> f32 {
        let x = t as f32 * 0.09 + series as f32 * 1.7;
        (0.5 + 0.28 * x.sin() + 0.14 * (x * 2.3 + series as f32).sin() + 0.08 * (x * 5.1).sin()).clamp(0.02, 1.)
    }
    pub fn tile_label(tile: usize) -> &'static str {
        ["Requests / s", "Error rate", "p50 latency", "p99 latency", "Active workers", "Queue depth"][tile]
    }
    pub fn tile_position(tile: usize) -> (f32, f32) {
        (TILE_ORIGIN.0 + tile as f32 * TILE_STEP, TILE_ORIGIN.1)
    }
    pub fn tile_value(tile: usize, live: u64) -> String {
        let s = sample(900 + tile, live);
        match tile {
            0 => format!("{:.0}", 12_000. + 40_000. * s),
            1 => format!("{:.2}%", 3. * s),
            2 => format!("{:.1} ms", 4. + 30. * s),
            3 => format!("{:.1} ms", 40. + 200. * s),
            4 => format!("{}", 800 + (s * 400.) as u32),
            _ => format!("{}", (s * 5_000.) as u32),
        }
    }
    pub fn card_position(card: usize) -> (f32, f32) {
        let (column, row) = (card % CARD_COLUMNS, card / CARD_COLUMNS);
        (CARD_ORIGIN.0 + column as f32 * CARD_STEP.0, CARD_ORIGIN.1 + row as f32 * CARD_STEP.1)
    }
    pub fn card_title(card: usize) -> String {
        format!("node-{card:02}")
    }
    pub fn card_value(card: usize, live: u64) -> String {
        format!("{:.1} ms", 4. + 96. * sample(card, live))
    }
    pub fn card_progress(card: usize, live: u64) -> f32 {
        PROGRESS.2 * sample(card + 1000, live)
    }
    pub fn bar_height(card: usize, bar: usize, live: u64) -> f32 {
        (SPARK_HEIGHT * sample(card, live + bar as u64)).max(2.)
    }
    pub fn bar_x(bar: usize) -> f32 {
        BAR_X + bar as f32 * BAR_STEP
    }
    pub fn row_id(row: usize) -> String {
        format!("#{row:06}")
    }
    pub fn row_worker(row: usize) -> String {
        format!("worker-{:05}.{}", row % 100_000, ["iad", "sfo", "fra", "nrt"][row % 4])
    }
    pub fn status(series: usize, live: u64) -> (&'static str, u32) {
        let s = sample(series + 5000, live / 20);
        if s > 0.8 {
            ("degraded", 0x8a5a12)
        } else if s < 0.12 {
            ("down", 0x8a2230)
        } else {
            ("healthy", 0x1d6b4a)
        }
    }
    pub fn row_latency(row: usize, live: u64) -> String {
        format!("{:.1} ms", 2. + 120. * sample(row + 2000, live))
    }
    pub fn row_throughput(row: usize, live: u64) -> String {
        format!("{:.2} MB/s", 0.2 + 48. * sample(row + 3000, live))
    }
    pub fn row_color(row: usize) -> u32 {
        if row.is_multiple_of(2) { PANEL } else { STRIPE }
    }
    pub fn log_line(live: u64) -> String {
        let node = live as usize % CARDS;
        format!(
            "{live:06}  {}  latency {}  p99 {:.1} ms  queue {:>3}  {}  checksum {:08x}",
            card_title(node),
            card_value(node, live),
            8. + 180. * sample(node + 300, live),
            (sample(node + 600, live) * 400.) as u32,
            status(node, live).0,
            (live as u32).wrapping_mul(2_654_435_761),
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Stream,
    Scroll,
    Both,
}

const RING_HEAVY: usize = 28;

pub fn heavy_trace(mode: Mode, frames: u64) -> Trace {
    use heavy::*;
    let stream = matches!(mode, Mode::Stream | Mode::Both);
    let scrolls = matches!(mode, Mode::Scroll | Mode::Both);
    let start = 10u64;
    let name = format!("heavy-{}", ["stream", "scroll", "both"][mode as usize]);
    make_trace(&name, WIDTH, HEIGHT, start..start + frames + 1, |sb, t| {
        let live = if stream { t } else { 0 };
        let scroll = if scrolls { (t as f32 * 14.) % (TABLE_ROWS as f32 * TABLE_ROW - TABLE_VIEWPORT) } else { 0. };
        let vp = sb.viewport;
        let mut i = 0usize;
        let rr = |x: f32, y: f32, w: f32, h: f32| R::new(px(x), px(y), px(x + w), px(y + h));
        sb.rect(&mut i, Some(rr(0., 0., WIDTH, HEIGHT)), vp, SOLID, rgb(BACKGROUND), 0);
        sb.text(&mut i, 16, TITLE_ORIGIN.0, TITLE_ORIGIN.1, 18., 24., Some(&[TITLE]), TEXT, vp);
        for tile in 0..TILES {
            let (x, y) = tile_position(tile);
            sb.rect(&mut i, Some(rr(x, y, TILE.0, TILE.1)), vp, ROUND, PANEL ^ BORDER, 16);
            sb.text(&mut i, 16, x + TILE_LABEL.0, y + TILE_LABEL.1, 11., 14., Some(&[tile_label(tile)]), MUTED, vp);
            let v = tile_value(tile, live);
            sb.text(&mut i, 12, x + TILE_VALUE.0, y + TILE_VALUE.1, 20., 26., Some(&[&v]), TEXT, vp);
        }
        for card in 0..CARDS {
            let (x, y) = card_position(card);
            sb.rect(&mut i, Some(rr(x, y, CARD.0, CARD.1)), vp, ROUND, PANEL ^ BORDER, 12);
            sb.text(&mut i, 8, x + CARD_TITLE.0, y + CARD_TITLE.1, 11., 14., Some(&[&card_title(card)]), MUTED, vp);
            let v = card_value(card, live);
            sb.text(&mut i, 10, x + CARD_VALUE.0, y + CARD_VALUE.1, 16., 20., Some(&[&v]), TEXT, vp);
            let (px0, py0, pw, ph) = PROGRESS;
            sb.rect(&mut i, Some(rr(x + px0, y + py0, pw, ph)), vp, ROUND, TRACK, 4);
            let fw = card_progress(card, live);
            sb.rect(&mut i, Some(rr(x + px0, y + py0, fw, ph)), vp, ROUND, ACCENT, 4);
            for bar in 0..BARS {
                let h = bar_height(card, bar, live);
                sb.rect(&mut i, Some(rr(x + bar_x(bar), y + SPARK_BASE - h, BAR_WIDTH, h)), vp, SOLID, ACCENT, 0);
            }
        }
        // Table.
        let (tx, ty) = TABLE_ORIGIN;
        let panel = rr(tx, ty, TABLE.0, TABLE.1);
        sb.rect(&mut i, Some(panel), vp, ROUND, PANEL ^ BORDER, 16);
        sb.rect(&mut i, Some(rr(tx, ty, TABLE.0, TABLE_HEADER)), panel, SOLID, HEADER, 0);
        for (cx, title) in COLUMNS {
            sb.text(&mut i, 10, tx + cx, ty + 7., 11., 14., Some(&[title]), MUTED, panel);
        }
        let list = rr(tx, ty + TABLE_HEADER, TABLE.0, TABLE_VIEWPORT).inter(panel);
        sb.begin_group(0, list, (0, -px(scroll)));
        let range = row_range(scroll);
        for k in 0..RING_HEAVY {
            let row = range.clone().find(|r| r % RING_HEAVY == k);
            let Some(row) = row else {
                sb.rect(&mut i, None, list, SOLID, 0, 0);
                for cap in [7, 16] {
                    sb.text(&mut i, cap, 0., 0., 12., 16., None, 0, list);
                }
                sb.rect(&mut i, None, list, ROUND, 0, 16);
                sb.text(&mut i, 8, 0., 0., 10., 14., None, 0, list);
                for cap in [10, 11] {
                    sb.text(&mut i, cap, 0., 0., 12., 16., None, 0, list);
                }
                continue;
            };
            let y = ty + TABLE_HEADER + row as f32 * TABLE_ROW - scroll;
            sb.rect(&mut i, Some(rr(tx, y, TABLE.0, TABLE_ROW)), list, SOLID, row_color(row), 0);
            sb.text(&mut i, 7, tx + COLUMNS[0].0, y + CELL_TOP, 12., 16., Some(&[&row_id(row)]), MUTED, list);
            sb.text(&mut i, 16, tx + COLUMNS[1].0, y + CELL_TOP, 12., 16., Some(&[&row_worker(row)]), TEXT, list);
            let (label, colour) = status(row, live);
            sb.rect(&mut i, Some(rr(tx + PILL.0, y + PILL.1, PILL.2, PILL.3)), list, ROUND, colour, 16);
            sb.text(&mut i, 8, tx + PILL.0 + PILL_TEXT.0, y + PILL.1 + PILL_TEXT.1, 10., 14., Some(&[label]), TEXT, list);
            sb.text(&mut i, 10, tx + COLUMNS[3].0, y + CELL_TOP, 12., 16., Some(&[&row_latency(row, live)]), TEXT, list);
            sb.text(&mut i, 11, tx + COLUMNS[4].0, y + CELL_TOP, 12., 16., Some(&[&row_throughput(row, live)]), TEXT, list);
        }
        sb.end_group(0);
        // Log.
        let (lx, ly) = LOG_ORIGIN;
        sb.rect(&mut i, Some(rr(lx, ly, LOG.0, LOG.1)), vp, ROUND, PANEL ^ BORDER, 16);
        let lines: Vec<String> = (0..LOG_LINES as u64)
            .map(|k| {
                let l = (live + k).saturating_sub(LOG_LINES as u64 - 1);
                log_line(if live + k < LOG_LINES as u64 - 1 { 0 } else { l })
            })
            .collect();
        for (k, line) in lines.iter().enumerate() {
            sb.text(
                &mut i,
                130,
                lx + LOG_TEXT.0,
                ly + LOG_TEXT.1 + k as f32 * LOG_LINE_HEIGHT,
                11.,
                LOG_LINE_HEIGHT,
                Some(&[line]),
                MUTED,
                vp,
            );
        }
    })
}

// ---------------------------------------------------------------- lab
const TOKEN: &str = "The quick brown fox streams a token. ";

fn lab_visible_text(frames: u64) -> Vec<String> {
    let mut text = String::new();
    for f in frames.saturating_sub(40).max(1)..=frames {
        use std::fmt::Write;
        write!(text, "{:06} {TOKEN}", f).unwrap();
    }
    let tail = &text.as_bytes()[text.len().saturating_sub(735)..];
    tail.chunks(105).take(7).map(|s| String::from_utf8(s.to_vec()).unwrap()).collect()
}

pub fn lab_trace(mode: Mode, frames: u64) -> Trace {
    let stream = matches!(mode, Mode::Stream | Mode::Both);
    let scrolls = matches!(mode, Mode::Scroll | Mode::Both);
    let start = 20u64;
    let name = format!("lab-{}", ["stream", "scroll", "both"][mode as usize]);
    const RING: usize = 20;
    make_trace(&name, 960., 720., start..start + frames + 1, |sb, t| {
        let vp = sb.viewport;
        let rr = |x: f32, y: f32, w: f32, h: f32| R::new(px(x), px(y), px(x + w), px(y + h));
        let mut i = 0usize;
        sb.rect(&mut i, Some(rr(0., 0., 960., 720.)), vp, SOLID, 0x10141c, 0);
        sb.text(&mut i, 20, 20., 20., 20., 28., Some(&["zgx performance lab"]), 0xe5edf7, vp);
        for (x, l) in [(20., "+"), (80., "="), (140., "0")] {
            sb.rect(&mut i, Some(rr(x, 55., 50., 32.)), vp, SOLID, 0x293b50, 0);
            sb.text(&mut i, 2, x + 12., 61., 14., 20., Some(&[l]), 0xe5edf7, vp);
        }
        sb.text(&mut i, 2, 220., 62., 14., 24., Some(&["4"]), 0xe5edf7, vp);
        sb.text(&mut i, 8, 280., 62., 14., 24., Some(&["stable"]), 0xe5edf7, vp);
        sb.text(&mut i, 2, 370., 62., 14., 24., Some(&["4"]), 0xe5edf7, vp);
        let pane = rr(20., 110., 920., 160.);
        sb.rect(&mut i, Some(pane), vp, SOLID, 0x192230, 0);
        let lines = if stream { lab_visible_text(t) } else { vec![] };
        let lref: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        sb.text(&mut i, 7 * 105, 28., 118., 14., 17., Some(&lref), 0xe5edf7, pane);
        let off = if scrolls { (t as f32 * 14.) % (100_000. * 28. - 400.) } else { 0. };
        let list = rr(20., 290., 920., 400.);
        sb.begin_group(0, list, (0, -px(off)));
        let first = (off / 28.) as usize;
        let end = ((off as f64 + 400.) / 28.).ceil() as usize;
        let range = first.saturating_sub(2)..(end + 2).min(100_000);
        for k in 0..RING {
            match range.clone().find(|r| r % RING == k) {
                Some(row) => {
                    let y = 290. + row as f32 * 28. - off;
                    let c = if row % 2 == 0 { 0x192230 } else { 0x1d2838 };
                    sb.rect(&mut i, Some(rr(20., y, 920., 28.)), list, SOLID, c, 0);
                    let label = format!("Row {row:06} — retained virtual item");
                    sb.text(&mut i, 36, 28., y + 4., 14., 20., Some(&[&label]), 0xe5edf7, list);
                }
                None => {
                    sb.rect(&mut i, None, list, SOLID, 0, 0);
                    sb.text(&mut i, 36, 0., 0., 14., 20., None, 0, list);
                }
            }
        }
        sb.end_group(0);
    })
}

// ---------------------------------------------------------------- animate
pub fn animate_trace(frames: u64) -> Trace {
    const COLUMNS: usize = 12;
    const ROWS: usize = 4;
    const DOT: f32 = 14.;
    const GAP: f32 = 12.;
    const SHIMMER: &str = "Reading workspace files and streaming tokens";
    fn dot_opacity(index: usize, seconds: f32) -> f32 {
        let phase = (seconds / 0.75 - index as f32 * 0.07).rem_euclid(1.);
        if phase < 0.45 {
            1. - 0.9 * (1. - (1. - phase / 0.45).powi(3))
        } else if phase < 0.92 {
            0.1
        } else {
            1.
        }
    }
    fn shimmer_alpha(index: usize, count: usize, seconds: f32) -> u8 {
        let last = (count.max(2) - 1) as f32;
        let centre = -0.4 + 1.8 * (seconds / 3.4).rem_euclid(1.);
        let x = (1. - (index as f32 / last - centre).abs() / 0.36).clamp(0., 1.);
        (90. + 165. * x * x * (3. - 2. * x)) as u8
    }
    make_trace("animate", 960., 720., 0..frames + 1, |sb, t| {
        let secs = t as f32 / 60.;
        let vp = sb.viewport;
        let mut i = 0usize;
        sb.rect(&mut i, Some(vp), vp, SOLID, 0x10141c, 0);
        for d in 0..COLUMNS * ROWS {
            let (c, r) = (d % COLUMNS, d / COLUMNS);
            let (x, y) = (40. + c as f32 * (DOT + GAP), 120. + r as f32 * (DOT + GAP));
            let a = (dot_opacity(d, secs) * 255.).round() as u32;
            sb.rect(&mut i, Some(R::xywh(px(x), px(y), px(DOT), px(DOT))), vp, ROUND, 0x9ad2ff ^ (a << 24), 14);
        }
        let n = SHIMMER.chars().count();
        for (k, ch) in SHIMMER.chars().enumerate() {
            let a = shimmer_alpha(k, n, secs) as u32;
            let s = ch.to_string();
            sb.text(&mut i, 1, 40. + k as f32 * 18. * ADV_EM, 260., 18., 24., Some(&[&s]), 0xffffff ^ (a << 24), vp);
        }
    })
}

// ---------------------------------------------------------------- blink
pub fn blink_trace(frames: u64) -> Trace {
    make_trace("blink", 960., 720., 0..frames + 1, |sb, t| {
        let vp = sb.viewport;
        let mut i = 0usize;
        sb.rect(&mut i, Some(vp), vp, SOLID, 0x10141c, 0);
        let shown = t % 2 == 0;
        sb.rect(&mut i, shown.then(|| R::xywh(px(40.), px(40.), px(2.), px(20.))), vp, SOLID, 0xe5edf7, 0);
    })
}

// ---------------------------------------------------------------- vlist
pub mod vlist {
    pub const WIDTH: f32 = 720.;
    pub const HEIGHT: f32 = 800.;
    pub const ROWS: usize = 100_000;
    pub const FONT_SIZE: f32 = 14.;
    pub const LINE_HEIGHT: f32 = 20.;
    pub const PAD: (f32, f32) = (16., 10.);
    pub const GAP: f32 = 6.;
    pub const TEXT_WIDTH: f32 = WIDTH - 2. * PAD.0;
    pub const IMAGE: (f32, f32) = (160., 90.);
    pub const SCROLL_STEP: f32 = 12.;
    pub const FLING: (f32, f32, u64) = (160., 0.94, 90);
    pub const JUMP_EVERY: u64 = 30;
    const WORDS_PER_LINE: usize = 16;
    const WORDS: [&str; 48] = [
        "the", "build", "passed", "on", "main", "after", "we", "moved", "layout", "into", "its", "own", "pass",
        "and", "a", "list", "of", "rows", "now", "measures", "each", "height", "once", "text", "wraps", "at",
        "the", "width", "so", "scrolling", "stays", "smooth", "even", "with", "images", "and", "long",
        "messages", "from", "everyone", "who", "reviewed", "it", "today", "thanks", "for", "looking", "again",
    ];
    pub fn hash(x: u64) -> u64 {
        let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    pub fn lines(row: usize) -> usize {
        1 + (hash(row as u64) % 8) as usize
    }
    pub fn has_image(row: usize) -> bool {
        (hash(row as u64) >> 8).is_multiple_of(4)
    }
    pub fn author(row: usize) -> String {
        format!("user-{:04}", hash(row as u64 ^ 0xa5a5) % 10_000)
    }
    pub fn time(row: usize) -> String {
        let minutes = (row / 3) % (24 * 60);
        format!("{:02}:{:02}", minutes / 60, minutes % 60)
    }
    pub fn words(row: usize, mut f: impl FnMut(&'static str)) {
        let words = lines(row) * WORDS_PER_LINE;
        let mut seed = hash(row as u64 + 0x5151);
        for _ in 0..words {
            seed = hash(seed);
            f(WORDS[(seed % WORDS.len() as u64) as usize]);
        }
    }
    /// Greedy wrap at TEXT_WIDTH with the fixed advance: line char counts.
    pub fn wrap(row: usize) -> Vec<String> {
        let adv = FONT_SIZE * super::ADV_EM;
        let max = (TEXT_WIDTH / adv).floor() as usize;
        let mut out = vec![String::new()];
        words(row, |w| {
            let cur = out.last_mut().unwrap();
            if cur.is_empty() {
                cur.push_str(w);
            } else if cur.len() + 1 + w.len() <= max {
                cur.push(' ');
                cur.push_str(w);
            } else {
                out.push(w.to_string());
            }
        });
        out
    }
    pub fn wrapped_lines(row: usize) -> usize {
        let adv = FONT_SIZE * super::ADV_EM;
        let max = (TEXT_WIDTH / adv).floor() as usize;
        let (mut n, mut cur) = (1, 0usize);
        words(row, |w| {
            if cur == 0 {
                cur = w.len();
            } else if cur + 1 + w.len() <= max {
                cur += 1 + w.len();
            } else {
                n += 1;
                cur = w.len();
            }
        });
        n
    }
    pub fn height(row: usize) -> f32 {
        2. * PAD.1 + LINE_HEIGHT + GAP + wrapped_lines(row) as f32 * LINE_HEIGHT
            + if has_image(row) { GAP + IMAGE.1 } else { 0. }
    }
    pub fn jump(tick: u64) -> Option<usize> {
        (tick > 0 && tick.is_multiple_of(JUMP_EVERY)).then(|| (hash(tick ^ 0x1234_5678) % ROWS as u64) as usize)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VMode {
    Scroll,
    Fling,
    Jump,
}

pub fn vlist_trace(mode: VMode, frames: u64) -> Trace {
    use vlist::*;
    const RING: usize = 20;
    // Exact row tops (every row measured; zgx estimates unmeasured ones, which
    // only changes where jumps land, not what a frame paints).
    let mut tops = Vec::with_capacity(ROWS + 1);
    let mut acc = 0f64;
    for r in 0..ROWS {
        tops.push(acc as f32);
        acc += height(r) as f64;
    }
    tops.push(acc as f32);
    let total = acc as f32;
    let maxoff = total - HEIGHT;
    let mut offs = vec![0f32; frames as usize + 1];
    for t in 1..=frames {
        let prev = offs[t as usize - 1];
        offs[t as usize] = match mode {
            VMode::Scroll => (prev + SCROLL_STEP).min(maxoff),
            VMode::Fling => (prev + FLING.0 * FLING.1.powi(((t - 1) % FLING.2) as i32)).min(maxoff),
            VMode::Jump => match jump(t) {
                Some(row) => tops[row].min(maxoff),
                None => (prev + SCROLL_STEP).min(maxoff),
            },
        };
    }
    let name = format!("vlist-{}", ["scroll", "fling", "jump"][mode as usize]);
    make_trace(&name, WIDTH, HEIGHT, 0..frames + 1, |sb, t| {
        let off = offs[t as usize];
        let vp = sb.viewport;
        let rr = |x: f32, y: f32, w: f32, h: f32| R::new(px(x), px(y), px(x + w), px(y + h));
        let mut i = 0usize;
        sb.rect(&mut i, Some(vp), vp, SOLID, 0x0e1218, 0);
        sb.begin_group(0, vp, (0, -px(off)));
        let first = tops.partition_point(|&y| y <= off).saturating_sub(1);
        let mut visible = vec![];
        let mut r = first;
        while r < ROWS && tops[r] < off + HEIGHT {
            visible.push(r);
            r += 1;
        }
        assert!(visible.len() <= RING);
        let adv = FONT_SIZE * ADV_EM;
        for k in 0..RING {
            match visible.iter().copied().find(|r| r % RING == k) {
                Some(row) => {
                    // Snap the row's top to device px once; children are offset from it
                    // (a translation moves every child by the same whole pixels).
                    let y = (px(tops[row]) - px(off)) as f32 / SCALE;
                    let h = height(row);
                    sb.rect(&mut i, Some(rr(0., y, WIDTH, h)), vp, SOLID, [0x161c26, 0x19202b][row % 2], 0);
                    let a = author(row);
                    sb.text(&mut i, 10, PAD.0, y + PAD.1, FONT_SIZE, LINE_HEIGHT, Some(&[&a]), 0x5eb1ff, vp);
                    let tx = PAD.0 + a.len() as f32 * adv + 8.;
                    sb.text(&mut i, 5, tx, y + PAD.1, FONT_SIZE, LINE_HEIGHT, Some(&[&time(row)]), 0x8b98ab, vp);
                    let lines = wrap(row);
                    let lref: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
                    let by = y + PAD.1 + LINE_HEIGHT + GAP;
                    sb.text(&mut i, 1300, PAD.0, by, FONT_SIZE, LINE_HEIGHT, Some(&lref), 0xe5edf7, vp);
                    let iy = by + lines.len() as f32 * LINE_HEIGHT + GAP;
                    let img = has_image(row).then(|| rr(PAD.0, iy, IMAGE.0, IMAGE.1));
                    sb.rect(&mut i, img, vp, SOLID, 0x2b3a52, 0);
                }
                None => {
                    sb.rect(&mut i, None, vp, SOLID, 0, 0);
                    sb.text(&mut i, 10, 0., 0., FONT_SIZE, LINE_HEIGHT, None, 0, vp);
                    sb.text(&mut i, 5, 0., 0., FONT_SIZE, LINE_HEIGHT, None, 0, vp);
                    sb.text(&mut i, 1300, 0., 0., FONT_SIZE, LINE_HEIGHT, None, 0, vp);
                    sb.rect(&mut i, None, vp, SOLID, 0, 0);
                }
            }
        }
        sb.end_group(0);
    })
}

pub fn all(frames: u64) -> Vec<Trace> {
    vec![
        heavy_trace(Mode::Stream, frames),
        heavy_trace(Mode::Scroll, frames),
        heavy_trace(Mode::Both, frames),
        lab_trace(Mode::Stream, frames),
        lab_trace(Mode::Scroll, frames),
        animate_trace(frames),
        blink_trace(frames / 5),
        vlist_trace(VMode::Scroll, frames),
        vlist_trace(VMode::Fling, frames),
        vlist_trace(VMode::Jump, frames),
    ]
}
