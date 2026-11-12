//! Virtualization is exposure: every list region mounts only the rows meeting what is visible of it (its own
//! viewport if it has one, else its clip ∩ the window), whatever moves it (its own scroll, an enclosing
//! list's scroll through nested spaces, a window resize, a sibling growing above it). For every ring age
//! 1..=4 and the windowed ring, after every step the presented surface equals a full redraw, and at the end
//! the retained app equals a fresh mount at the same offsets:
//!  * a plain list (no viewport) longer than the window, under random edits, splices, resizes and reveals;
//!  * a grid (index rows of cells) under random scrolls, jumps and resizes;
//!  * nested scrolls: a virtual list of groups, each with a virtual list of its own and a plain list, under
//!    random outer / inner scrolls, jumps, edits, splices and resizes;
//!  * materialize → dematerialize → materialize returns the same pixels.
mod common;
use bough::layout::prelude::*;
use bough::raw::*;
use common::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn color(k: u64) -> Color {
    let x = k.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    rgb((x >> 40) as u32 & 0xffffff)
}

struct Leaf {
    label: Signal<String>,
    c: u64,
}
impl Leaf {
    fn new(k: u64) -> Leaf {
        Leaf { label: Signal::new(format!("leaf {k}")), c: k }
    }
    fn copy(&self) -> Leaf {
        Leaf { label: Signal::new((*self.label).clone()), c: self.c }
    }
}
impl View for Leaf {
    fn view(&self) -> impl Element<Self> + '_ {
        row((block(10.0, 12.0).bg(color(self.c)), label(sig!(self.label)).font(12.0, 16.0))).gap(4.0)
    }
}

// ------------------------------------------------------------------ a plain list longer than the window

struct Plain {
    top: Signal<f32>,
    items: List<Leaf>,
}
impl View for Plain {
    fn view(&self) -> impl Element<Self> + '_ {
        column((block(40.0, 4.0).bg(rgb(0x404040)).h_op(sig!(self.top)), list(sig!(self.items)).gap(2.0))).w(220.0)
    }
}

/// The window's part of a surface `stride` px wide (a resized app keeps its first surfaces: outside the
/// window they hold whatever was there).
fn crop(px: &[u8], stride: usize, w: usize, h: usize) -> Vec<u8> {
    (0..h).flat_map(|y| px[y * stride * 4..(y * stride + w) * 4].iter().copied()).collect()
}

/// `common::check` with a fresh mount built from the retained app (its state and its window size), the
/// surfaces compared over the window.
fn gate<V: View>(name: &str, mut app: App<V>, steps: usize, mut step: impl FnMut(&mut App<V>, usize), fresh: impl Fn(&App<V>, Config) -> App<V>) -> Vec<u8> {
    app.frame();
    let stride = app.rt.win_w as usize;
    let name = format!("{name} ({})", if app.windowed { "windowed" } else { "rr" });
    let dbg = std::env::var_os("VDEBUG").is_some();
    for t in 1..=steps {
        step(&mut app, t);
        if dbg {
            let w: Vec<_> = (0..app.rt.lists.regs().len()).map(|r| app.rt.list_window(r)).collect();
            eprintln!("{name} step {t}: windows {w:?}, stubs {:?}", app.reflect_stubs().iter().map(|s| (s.region, s.rows.clone())).collect::<Vec<_>>());
        }
        let drew = app.frame();
        assert!(!app.bins_overflowed(), "{name}: row bin overflow at step {t}");
        if drew {
            let (w, h) = (app.rt.win_w as usize, app.rt.win_h as usize);
            let px = crop(&app.presented_pixels(), stride, w, h);
            let reference = crop(&app.reference_pixels(), stride, w, h);
            if px != reference && dbg {
                let (x, y, _, _) = first_diff(&px, &reference, w).unwrap();
                for (k, r) in app.rt.bough.records().iter().enumerate() {
                    let [x0, y0, x1, y1] = r.ink.to_xyxy();
                    let c = app.rt.bough.clip_rect(r.clip).to_xyxy();
                    if (x as i32) >= x0 - 4 && (x as i32) < x1 + 4 && (y as i32) >= y0 - 4 && (y as i32) < y1 + 4 {
                        eprintln!("  rec {k} kind {} depth {} ink {:?} clip {} {:?}", r.kind, r.depth, [x0, y0, x1, y1], r.clip, c);
                    }
                }
            }
            assert!(px == reference, "{name} step {t}: presented != full redraw at {:?}", first_diff(&px, &reference, w));
        }
    }
    let (w, h) = (app.rt.win_w as usize, app.rt.win_h as usize);
    let px = crop(&app.presented_pixels(), stride, w, h);
    let s = app.rt.scale;
    let mut c = Config::new(w as f32 / s, h as f32 / s, s, FONT.to_vec()).once();
    c.ring = 4;
    let mut f = fresh(&app, c);
    f.frame();
    let fp = crop(&f.presented_pixels(), f.rt.win_w as usize, w, h);
    assert!(px == fp, "{name}: retained after {steps} steps != fresh mount, first diff {:?}", first_diff(&px, &fp, w));
    px
}

type Item7 = (u8, [f32; 4], u32, u32, f32, f32, [u16; 2]);
fn dump(app: &App<impl View>) -> Vec<(u8, u16, [i16; 4], Vec<Item7>)> {
    app.rt.bough.records().iter().map(|r| (r.kind, r.depth, r.ink.0, if r.ink.is_empty() { Vec::new() } else { app.rt.expand_payload(r.payload) })).collect()
}

/// Headless: the steps (resizes included), and after every one the retained records equal a fresh mount's.
fn gate_records<V: View>(name: &str, mut app: App<V>, steps: usize, mut step: impl FnMut(&mut App<V>, usize), fresh: impl Fn(&App<V>, Config) -> App<V>) {
    app.frame();
    for t in 1..=steps {
        step(&mut app, t);
        app.frame();
        let s = app.rt.scale;
        let c = Config::new(app.rt.win_w as f32 / s, app.rt.win_h as f32 / s, s, FONT.to_vec()).once();
        let mut f = fresh(&app, c);
        f.frame();
        let (a, b) = (dump(&app), dump(&f));
        assert_eq!(a.len(), b.len(), "{name} step {t}: record count (retained {}, fresh {})", a.len(), b.len());
        for (k, (x, y)) in a.iter().zip(&b).enumerate() {
            assert!(x == y, "{name} step {t}: record {k} {:?} != fresh {:?}", (x.0, x.1, x.2), (y.0, y.1, y.2));
        }
    }
}

fn plain_step(a: &mut App<Plain>, r: &mut Rng, next: &mut u64) {
    match r.below(6) {
        0 => {
            let n = a.owner.items.len();
            let i = r.below(n);
            let v = format!("edit {}", r.next() % 1000);
            a.turn(|o| *o.items.edit(i).label = v);
        }
        1 => {
            let n = a.owner.items.len();
            let at = r.below(n + 1);
            *next += 1;
            let k = *next;
            a.turn(|o| {
                o.items.insert(at, Leaf::new(k));
            });
        }
        2 => {
            let n = a.owner.items.len();
            let at = r.below(n);
            a.turn(|o| {
                o.items.remove(at);
            });
        }
        3 => {
            // a reveal / hide: the list moves under the window
            let h = [4.0, 60.0, 150.0, 0.0][r.below(4)];
            a.turn(|o| *o.top = h);
        }
        4 => {
            let size = ([160.0, 200.0, 240.0][r.below(3)], [120.0, 160.0, 200.0][r.below(3)]);
            if !a.has_surfaces() { a.resize(size.0, size.1); }
        }
        _ => {
            let n = a.owner.items.len();
            let (x, y) = (r.below(n), r.below(n));
            a.turn(|o| o.items.move_to(x, y));
        }
    }
}

#[test]
fn a_plain_list_mounts_what_is_visible_under_edits_splices_resizes_and_reveals() {
    let cfg = || Config::new(240.0, 200.0, 2.0, FONT.to_vec()).once();
    for ring in RINGS {
        let mk = || Plain { top: Signal::new(4.0), items: List::from_iter_in((0..400).map(Leaf::new)) };
        let mut app = app_ring(mk(), cfg(), ring);
        app.frame();
        // a 400-row list in a 200 pt window: only the visible rows (± overscan) are records
        let recs = app.rt.bough.records().len();
        assert!(recs < 60, "ring {ring}: {recs} records for 400 rows");
        let stubs = app.reflect_stubs();
        assert!(stubs.iter().any(|s| s.rows.end == 400), "the rows below the window are a placeholder: {stubs:?}");
        let mut r = Rng(0x5eed + ring as u64);
        let mut next = 1000u64;
        gate(
            "plain",
            app,
            60,
            |a, _| plain_step(a, &mut r, &mut next),
            |a, c| App::new(Plain { top: Signal::new(*a.owner.top), items: List::from_iter_in(a.owner.items.iter().map(Leaf::copy)) }, c, true),
        );
    }
}

// ------------------------------------------------------------------ a grid

const COLS: usize = 12;
struct Grid {
    rows: usize,
    scroll: Signal<f32>,
}
fn grid_row(_: &Grid, i: usize, m: &mut Mounter<'_, Grid>) {
    m.mount(row((
        label(format!("{i}")).font(10.0, 14.0).w(34.0),
        row([0usize; COLS].iter().enumerate().map(|(j, _)| block(14.0, 14.0).bg(color((i * COLS + j) as u64))).collect::<Vec<_>>()).gap(2.0),
    )));
}
impl View for Grid {
    fn view(&self) -> impl Element<Self> + '_ {
        index_list(self.rows, grid_row).w(260.0).h(180.0).scroll(sig!(self.scroll)).estimate(14.0).gap(2.0)
    }
}

fn grid_step(a: &mut App<Grid>, r: &mut Rng, rows: usize) {
    match r.below(4) {
        0 => {
            let d = (r.below(200) as f32) - 60.0;
            a.turn(|o| *o.scroll = (*o.scroll + d).max(0.0));
        }
        1 => {
            let y = (r.below(rows) as f32) * 16.0;
            a.turn(|o| *o.scroll = y);
        }
        2 => {
            let size = ([200.0, 240.0, 280.0][r.below(3)], [120.0, 160.0, 200.0][r.below(3)]);
            if !a.has_surfaces() { a.resize(size.0, size.1); }
        }
        _ => {
            let d = r.below(5) as f32 * 0.5;
            a.turn(|o| *o.scroll += d);
        }
    }
}

#[test]
fn a_grid_is_exact_under_scrolls_jumps_and_resizes() {
    let cfg = || Config::new(280.0, 200.0, 2.0, FONT.to_vec()).once();
    for ring in RINGS {
        let rows = 100_000;
        let mut app = app_ring(Grid { rows, scroll: Signal::new(0.0) }, cfg(), ring);
        app.frame();
        let mounted = app.rt.list_window(0).len();
        assert!(mounted < 24, "ring {ring}: {mounted} grid rows mounted");
        let mut r = Rng(0x6d1d + ring as u64);
        gate(
            "grid",
            app,
            50,
            |a, _| grid_step(a, &mut r, rows),
            |a, c| {
                let s = a.rt.list_scroll_pt(0);
                App::new(Grid { rows, scroll: Signal::new(s) }, c, true)
            },
        );
    }
}

// ------------------------------------------------------------------ nested scrolls

struct Group {
    title: Signal<String>,
    items: List<Leaf>,
    tags: List<Leaf>,
    scroll: Signal<f32>,
}
impl Group {
    fn new(k: u64) -> Group {
        Group {
            title: Signal::new(format!("group {k}")),
            items: List::from_iter_in((0..30).map(|j| Leaf::new(k * 100 + j))),
            tags: List::from_iter_in((0..3).map(|j| Leaf::new(k * 1000 + j))),
            scroll: Signal::new(0.0),
        }
    }
    fn copy(&self) -> Group {
        Group {
            title: Signal::new((*self.title).clone()),
            items: List::from_iter_in(self.items.iter().map(Leaf::copy)),
            tags: List::from_iter_in(self.tags.iter().map(Leaf::copy)),
            scroll: Signal::new(*self.scroll),
        }
    }
}
impl View for Group {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            label(sig!(self.title)).font(12.0, 16.0),
            row((list(sig!(self.items)).viewport().scroll(sig!(self.scroll)).estimate(16.0).gap(2.0).w(110.0).h(52.0), list(sig!(self.tags)).gap(2.0).w(90.0))).gap(6.0),
        ))
        .gap(2.0)
        .w(230.0)
    }
}
/// A group's height: title 16 + gap 2 + max(inner viewport 52, three tags 3·16 + 2·2 = 52).
const GROUP_H: f32 = 70.0;

struct Nested {
    groups: List<Group>,
    scroll: Signal<f32>,
}
impl View for Nested {
    fn view(&self) -> impl Element<Self> + '_ {
        column((label("nested").font(12.0, 16.0), list(sig!(self.groups)).viewport().scroll(sig!(self.scroll)).estimate(GROUP_H).gap(4.0).w(240.0).h(170.0)))
    }
}
fn nested_copy(a: &App<Nested>) -> Nested {
    Nested { groups: List::from_iter_in(a.owner.groups.iter().map(Group::copy)), scroll: Signal::new(a.rt.list_scroll_pt(0)) }
}

fn nested_step(a: &mut App<Nested>, r: &mut Rng, next: &mut u64) {
    let n = a.owner.groups.len();
    let op = r.below(8);
    if std::env::var_os("VDEBUG").is_some() {
        eprintln!("op {op} (n {n}, scroll {}, win {}x{})", *a.owner.scroll, a.rt.win_w, a.rt.win_h);
    }
    match op {
        0 => {
            let d = r.below(160) as f32 - 50.0;
            a.turn(|o| *o.scroll = (*o.scroll + d).max(0.0));
        }
        1 => {
            let y = r.below(n) as f32 * (GROUP_H + 4.0);
            a.turn(|o| *o.scroll = y);
        }
        2 | 3 if n > 0 => {
            let i = r.below(n);
            let d = r.below(120) as f32 - 40.0;
            a.turn(|o| {
                let g = o.groups.edit(i);
                *g.scroll = (*g.scroll + d).clamp(0.0, 30.0 * 18.0 - 2.0 - 52.0);
            });
        }
        4 if n > 0 => {
            let (i, j) = (r.below(n), r.below(30));
            let v = format!("e{}", r.next() % 1000);
            a.turn(|o| *o.groups.edit(i).items.edit(j).label = v);
        }
        5 => {
            *next += 1;
            let (k, at) = (*next, r.below(n + 1));
            a.turn(|o| {
                o.groups.insert(at, Group::new(k));
            });
        }
        6 if n > 1 => {
            let at = r.below(n);
            a.turn(|o| {
                o.groups.remove(at);
            });
        }
        _ => {
            let size = ([200.0, 260.0][r.below(2)], [150.0, 190.0, 220.0][r.below(3)]);
            if !a.has_surfaces() { a.resize(size.0, size.1); }
        }
    }
}

#[test]
fn nested_scrolls_are_exact_under_scrolls_jumps_edits_splices_and_resizes() {
    let cfg = || Config::new(260.0, 220.0, 2.0, FONT.to_vec()).once();
    for ring in RINGS {
        let app = app_ring(Nested { groups: List::from_iter_in((0..40).map(Group::new)), scroll: Signal::new(0.0) }, cfg(), ring);
        let mut r = Rng(0xa11ce + ring as u64);
        let mut next = 500u64;
        gate("nested", app, 70, |a, _| nested_step(a, &mut r, &mut next), |a, c| App::new(nested_copy(a), c, true));
    }
}

#[test]
fn materialize_dematerialize_materialize_round_trips() {
    let cfg = Config::new(260.0, 220.0, 2.0, FONT.to_vec()).once();
    let mut a = App::new(Nested { groups: List::from_iter_in((0..40).map(Group::new)), scroll: Signal::new(0.0) }, cfg, true);
    a.frame();
    a.turn(|o| *o.scroll = 5.0 * (GROUP_H + 4.0));
    a.turn(|o| *o.groups.edit(6).scroll = 77.0);
    a.frame();
    let before = a.presented_pixels();
    let recs = a.rt.bough.records().len();
    // far away (the groups shown before are released) and back
    a.turn(|o| *o.scroll = 35.0 * (GROUP_H + 4.0));
    a.frame();
    assert!(a.rt.list_window(0).start > 20, "the first groups are no longer mounted");
    a.turn(|o| *o.scroll = 5.0 * (GROUP_H + 4.0));
    a.frame();
    assert_eq!(a.rt.bough.records().len(), recs);
    assert!(a.presented_pixels() == before, "materialized again != as first shown");
    assert_eq!(*a.owner.groups.get(6).unwrap().scroll, 77.0, "row state lives in the list, not in its records");
}


#[test]
fn resizes_are_exposures_too_records_equal_a_fresh_mount() {
    let plain = || Plain { top: Signal::new(4.0), items: List::from_iter_in((0..400).map(Leaf::new)) };
    let (mut r, mut next) = (Rng(0x51e), 1000u64);
    gate_records("plain", App::new(plain(), Config::new(240.0, 200.0, 2.0, FONT.to_vec()).once(), false), 80, |a, _| plain_step(a, &mut r, &mut next), |a, c| {
        App::new(Plain { top: Signal::new(*a.owner.top), items: List::from_iter_in(a.owner.items.iter().map(Leaf::copy)) }, c, false)
    });
    let rows = 100_000;
    let mut r = Rng(0x6e1d);
    gate_records("grid", App::new(Grid { rows, scroll: Signal::new(0.0) }, Config::new(280.0, 200.0, 2.0, FONT.to_vec()).once(), false), 60, |a, _| grid_step(a, &mut r, rows), |a, c| {
        App::new(Grid { rows, scroll: Signal::new(a.rt.list_scroll_pt(0)) }, c, false)
    });
    let (mut r, mut next) = (Rng(0xa11cf), 500u64);
    let nested = Nested { groups: List::from_iter_in((0..40).map(Group::new)), scroll: Signal::new(0.0) };
    gate_records("nested", App::new(nested, Config::new(260.0, 220.0, 2.0, FONT.to_vec()).once(), false), 80, |a, _| nested_step(a, &mut r, &mut next), |a, c| App::new(nested_copy(a), c, false));
}
