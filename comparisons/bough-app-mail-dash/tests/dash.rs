//! Gates of the dashboard: pixels (presented == full redraw at ring ages 1-4 and the windowed ring after each
//! interaction; retained == a fresh mount at the end), the view against a dumb oracle (every row filtered and
//! sorted by zgx's comparison, after random feed ticks, sorts and filters), and the layout the harness clicks.
mod common;
use bough::prelude::*;
use bough_app_mail_dash::dash::{self, window, Col, Dash, Market, COLS, SECTORS};
use bough_app_mail_dash::{config, lay_rect};
use common::*;
use std::cmp::Ordering;

fn click(app: &mut App<Dash>, r: [f32; 4]) {
    let (x, y) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
}
fn sort(app: &mut App<Dash>, c: Col) {
    click(app, lay_rect(&app.rt, &dash::sort_path(c)));
}
fn key(app: &mut App<Dash>, k: Key) {
    app.key_down(k, Mods::default());
}
/// The toolbar's chip `k` (0: All).
fn chip(app: &App<Dash>, k: usize) -> [f32; 4] {
    lay_rect(&app.rt, &[0, 2, k])
}

/// apps.rs's interactions (sort by price, feed ticks, sorts, filter keystrokes), then descending sorts, a feed
/// tick while filtered, a two-letter search, a sector, a selected row, the pointer leaving.
fn step(app: &mut App<Dash>, t: usize) {
    match t {
        1 => sort(app, Col::Price),
        2..=4 => app.turn(|d| d.feed()),
        5..=8 => sort(app, if t % 2 == 1 { Col::Volume } else { Col::Symbol }),
        9..=11 => sort(app, [Col::Price, Col::Change, Col::Sector][t % 3]),
        12 => sort(app, Col::Sector),
        13 => {
            click(app, lay_rect(&app.rt, &dash::SEARCH_PATH));
            key(app, Key::Char('B'));
        }
        14 => app.turn(|d| d.feed()),
        15 => key(app, Key::Backspace),
        16 => {
            key(app, Key::Char('B'));
            key(app, Key::Char('C'));
        }
        17 => {
            key(app, Key::Backspace);
            key(app, Key::Backspace);
        }
        18 => click(app, chip(app, 1)),
        19 => click(app, [dash::TABLE_X, dash::LIST_TOP + 3.0 * dash::ROW_H, dash::TABLE_W, dash::ROW_H]),
        20 => click(app, chip(app, 0)),
        21 | 22 => sort(app, Col::Volume),
        23 => app.turn(|d| d.feed()),
        24 => sort(app, Col::Change),
        _ => app.pointer_leave(),
    }
}
const STEPS: usize = 25;
const ROWS: usize = 100_000;

#[cfg(target_os = "macos")]
#[test]
fn dashboard_is_pixel_exact() {
    for ring in RINGS {
        let cfg = || with_ring(config(dash::W, dash::H, 1.0), ring);
        let mut app = dash::mount(Dash::new(ROWS), cfg(), true);
        app.windowed = ring == 0;
        check("dashboard", app, STEPS, 1, step, |a| dash::mount(Dash::same_state(&a.owner), cfg(), true));
    }
}

/// The benchmark's own scale (2) on the windowed ring.
#[cfg(target_os = "macos")]
#[test]
fn dashboard_is_pixel_exact_at_scale_2() {
    let cfg = || with_ring(config(dash::W, dash::H, 2.0), 0);
    let mut app = dash::mount(Dash::new(ROWS), cfg(), true);
    app.windowed = true;
    check("dashboard x2", app, STEPS, 1, step, |a| dash::mount(Dash::same_state(&a.owner), cfg(), true));
}

/// zgx's comparison of two rows by column `c` (`Key::cmp`: numbers by `total_cmp`, strings by bytes), then by
/// row, the column's way reversed when descending but never the rows'.
fn zgx_cmp(mk: &Market, c: Col, desc: bool, a: u32, b: u32) -> Ordering {
    let (x, y) = (a as usize, b as usize);
    let o = match c {
        Col::Symbol => mk.symbol(a).as_str().cmp(mk.symbol(b).as_str()),
        Col::Sector => SECTORS[mk.sector[x] as usize].cmp(SECTORS[mk.sector[y] as usize]),
        Col::Price => mk.price[x].total_cmp(&mk.price[y]),
        Col::Change => mk.change[x].total_cmp(&mk.change[y]),
        Col::Volume => mk.volume[x].total_cmp(&mk.volume[y]),
    };
    (if desc { o.reverse() } else { o }).then(a.cmp(&b))
}

/// The view the dumb way: every row filtered (`sector`, `contains`), all of them sorted.
fn oracle(mk: &Market, c: Col, desc: bool, sector: u8, q: &str) -> Vec<u32> {
    let mut rows: Vec<u32> = (0..mk.n as u32).filter(|&i| (sector == 0 || mk.sector[i as usize] == sector - 1) && mk.symbol(i).as_str().contains(q)).collect();
    rows.sort_by(|&a, &b| zgx_cmp(mk, c, desc, a, b));
    rows
}

#[test]
fn the_view_follows_the_oracle() {
    // few rows, so a tick writes some rows twice and the buckets split
    let n = 3000;
    let mut mk = Market::new(n);
    let mut r = 0x2545_f491_4f6c_dd1du64;
    let queries = ["", "B", "BC", "A1", "Z", "9", "AB12", "QQQ", "-"];
    for round in 0..120 {
        r ^= r << 13;
        r ^= r >> 7;
        r ^= r << 17;
        for _ in 0..(r % 3) {
            mk.feed();
        }
        let c = COLS[(r >> 8) as usize % 5];
        let desc = (r >> 12) & 1 == 1;
        let sector = ((r >> 16) % 9) as u8;
        let q = queries[(r >> 24) as usize % queries.len()];
        let first = [0, 0, 7, 500][(r >> 32) as usize % 4];
        let want = oracle(&mk, c, desc, sector, q);
        let mut pass = Vec::new();
        let count = if sector != 0 || !q.is_empty() { mk.filter(sector, q.as_bytes(), &mut pass) } else { n };
        assert_eq!(count, want.len(), "round {round}: the count ({c:?} {sector} {q:?})");
        let mut win = [None; dash::SLOTS];
        window(&mk, c, desc, &pass, first, &mut win);
        let expect: Vec<Option<u32>> = (0..dash::SLOTS).map(|k| want.get(first + k).copied()).collect();
        assert_eq!(win.to_vec(), expect, "round {round}: the window ({c:?} desc {desc} sector {sector} {q:?} from {first})");
    }
    // the whole kept orders, both ways
    for c in COLS {
        for desc in [false, true] {
            let mut all = Vec::new();
            mk.visit(c, desc, |i| {
                all.push(i);
                true
            });
            assert_eq!(all, oracle(&mk, c, desc, 0, ""), "{c:?} desc {desc}: the whole order");
        }
    }
}

#[test]
fn the_dashboard_s_actions_follow_the_oracle() {
    let mut app = dash::mount(Dash::new(4000), config(dash::W, dash::H, 1.0), false);
    let cols = [Col::Price, Col::Volume, Col::Symbol, Col::Change, Col::Sector, Col::Price, Col::Price];
    for (t, &c) in cols.iter().enumerate() {
        app.turn(|d| d.sort_by(c));
        app.turn(|d| d.feed());
        if t == 3 {
            app.turn(|d| {
                d.search_key(&Ev { x: 0.0, y: 0.0, dx: 0.0, dy: 0.0, key: Some(Key::Char('B')), mods: Mods::default(), button: Button::Primary, on: true });
                d.choose_sector(3);
            });
        }
        app.frame();
        let d = &app.owner;
        let want = oracle(&d.mk, d.sort, d.desc, *d.sector, &d.query.to_uppercase());
        let shown: Vec<String> = d.rows.iter().map(|r| r.sym.as_str().to_owned()).filter(|s| !s.is_empty()).collect();
        let expect: Vec<String> = want.iter().take(dash::SLOTS).map(|&i| d.mk.symbol(i).as_str().to_owned()).collect();
        assert_eq!(shown, expect, "step {t}: the slots' symbols ({:?} desc {})", d.sort, d.desc);
        assert_eq!(d.count, want.len());
    }
}

#[test]
fn the_layout_is_where_the_harness_clicks() {
    let app = dash::mount(Dash::new(2000), config(dash::W, dash::H, 2.0), false);
    assert_eq!(lay_rect(&app.rt, &[1, 2, 1]), [dash::TABLE_X, dash::LIST_TOP, dash::TABLE_W, dash::LIST_H], "the table's viewport (its clip)");
    assert_eq!(lay_rect(&app.rt, &[1, 0, 1])[..2], [dash::LINE_AT.0, dash::LINE_AT.1], "the line chart's box (its lanes' origin)");
    assert_eq!(lay_rect(&app.rt, &[1, 0, 2])[..2], [dash::BARS_AT.0, dash::BARS_AT.1], "the bars' box");
    for c in COLS {
        let r = lay_rect(&app.rt, &dash::sort_path(c));
        assert!(r[1] == dash::LIST_TOP - 44.0 && r[3] == 38.0, "the {c:?} header button: {r:?}");
    }
    let f = lay_rect(&app.rt, &dash::SEARCH_PATH);
    assert_eq!((f[2], f[3]), (220.0, 36.0), "the search field");
}
