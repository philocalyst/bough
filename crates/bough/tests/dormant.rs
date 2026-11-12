//! Dormant runs (`dormant.rs`): `each` over an indexable source mounts what is exposed.
//!
//! Gates:
//!  1. **exposure never changes a pixel**: after every scroll, jump, resize, edit and re-description, the
//!     presented frame == a full redraw of the retained tree == a full redraw of a fresh `App` of the same
//!     state == a full redraw of a fresh `App` with `Config::eager_parts` (every item mounted: the path that
//!     existed before this change), at rings 1..4;
//!  2. the same with rows of different heights, compared at the same anchor (item, offset in the port);
//!  3. the fixed point converges (`DormStats::unconverged == 0`) and the mounted set is bounded;
//!  4. anchoring: an item on screen stays where the scroll put it while sizes above it are corrected;
//!  5. mutants (`BOUGH_DORM_MUTANT=short|norelease|noanchor`, each run alone) fail their gate.
use bough::dormant::Mutant;
use bough::prelude::*;
use bough::raw::{App, Config};

const FONT: &[u8] = zgx_workload::FONT_BYTES;

fn mutant() -> Mutant {
    match std::env::var("BOUGH_DORM_MUTANT").as_deref() {
        Ok("short") => Mutant::Short,
        Ok("norelease") => Mutant::NoRelease,
        Ok("noanchor") => Mutant::NoAnchor,
        _ => Mutant::None,
    }
}

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

/// A header, a scroll box of rows, a footer. `tall`: a row's height by its id (None: 20 pt each).
struct Doc {
    rows: Vec<(u32, String)>,
    y: Signal<f32>,
    selected: u32,
    tall: Option<fn(u32) -> f32>,
    port_h: f32,
    gap: f32,
    w: f32,
    h: f32,
}
impl Doc {
    fn new(n: u32) -> Doc {
        Doc { rows: (0..n).map(|i| (i, format!("Row {i}"))).collect(), y: Signal::new(0.0), selected: u32::MAX, tall: None, port_h: 200.0, gap: 0.0, w: 300.0, h: 280.0 }
    }
    fn snap(&self) -> Doc {
        Doc { rows: self.rows.clone(), y: Signal::new(*self.y), selected: self.selected, tall: self.tall, port_h: self.port_h, gap: self.gap, w: self.w, h: self.h }
    }
    fn row_h(&self, id: u32) -> f32 {
        self.tall.map_or(20.0, |f| f(id))
    }
}
impl Doc {
    fn line<'a>(&'a self, i: usize, r: &'a (u32, String)) -> impl Element<Doc> + 'a {
        let bg = if r.0 == self.selected { rgb(0x2f6bff) } else if i % 2 == 0 { rgb(0xf5f5f5) } else { rgb(0xdcdcdc) };
        row((text(r.0).w(48.0), text(r.1.as_str()).color(rgb(0x101010)))).w(240.0).h(self.row_h(r.0)).bg(bg)
    }
}
impl View for Doc {
    fn view(&self) -> impl Element<Self> + '_ {
        let line = move |(i, r)| self.line(i, r);
        column((
            text("head").color(rgb(0xffffff)),
            div().w(260.0).h(self.port_h).gap(self.gap).bg(rgb(0x203040)).scroll(sig!(self.y)).child(each(self.rows.iter().enumerate().map(line))).child(block(100.0, 30.0).bg(rgb(0xff8800))),
            block(60.0, 12.0).bg(rgb(0x00aa55)),
        ))
        .bg(rgb(0x101010))
    }
}

fn config(d: &Doc, ring: usize, eager: bool) -> Config {
    let mut c = Config::new(d.w, d.h, 2.0, FONT.to_vec());
    c.ring = ring;
    c.eager_parts = eager;
    c
}

fn diff(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len());
    a.chunks(4).zip(b.chunks(4)).filter(|(a, b)| a != b).count()
}

/// The part of the rows (the only dynamic part that mounts by exposure).
fn part(a: &App<Doc>) -> bough::dormant::PartView {
    a.rt.parts().into_iter().find(|p| p.dormant).expect("a dormant-capable part")
}

/// Compare `app` after a frame with fresh mounts of `state` (dormant and eager) at the same offset.
fn gate(app: &mut App<Doc>, state: &Doc, ctx: &str) -> Result<(), String> {
    app.frame();
    let retained = app.reference_pixels();
    let n = diff(&app.presented_pixels(), &retained);
    if n != 0 {
        return Err(format!("{ctx}: presented != full redraw ({n} px)"));
    }
    if app.rt.dorm.stats.unconverged != 0 {
        return Err(format!("{ctx}: the exposure fixed point did not converge"));
    }
    for eager in [false, true] {
        let mut s = state.snap();
        s.y = Signal::new(*app.owner.y);
        let mut fresh = App::new(s, config(state, 4, eager), true);
        fresh.frame();
        let n = diff(&retained, &fresh.reference_pixels());
        if n != 0 {
            return Err(format!("{ctx}: retained != fresh mount (eager parts: {eager}; {n} px; realised {:?})", part(app).ranges));
        }
    }
    Ok(())
}

type Step = (&'static str, Box<dyn Fn(&mut Doc)>);

fn steps() -> Vec<Step> {
    vec![
        ("scroll 7", Box::new(|d| { d.y.set(7.0); })),
        ("scroll 33", Box::new(|d| { d.y.set(33.0); })),
        ("scroll 260", Box::new(|d| { d.y.set(260.0); })),
        ("select 20", Box::new(|d| d.selected = 20)),
        ("jump 6000", Box::new(|d| { d.y.set(6000.0); })),
        ("select 305", Box::new(|d| d.selected = 305)),
        ("relabel 303", Box::new(|d| d.rows[303].1 = "Renamed".into())),
        ("scroll 6010.5", Box::new(|d| { d.y.set(6010.5); })),
        ("scroll 5990", Box::new(|d| { d.y.set(5990.0); })),
        ("remove above (10)", Box::new(|d| drop(d.rows.remove(10)))),
        ("remove in view (302)", Box::new(|d| drop(d.rows.remove(302)))),
        ("insert in view (301)", Box::new(|d| d.rows.insert(301, (9001, "Inserted".into())))),
        ("insert above (5)", Box::new(|d| d.rows.insert(5, (9002, "Above".into())))),
        ("remove below (500)", Box::new(|d| drop(d.rows.remove(500)))),
        ("taller port", Box::new(|d| d.port_h = 236.0)),
        ("window", Box::new(|d| d.selected = 300)),
        ("jump back 120", Box::new(|d| { d.y.set(120.0); })),
        ("gap 3", Box::new(|d| d.gap = 3.0)),
        ("jump to end", Box::new(|d| { d.y.set(600.0 * 23.0 + 30.0 - d.port_h); })),
        ("append 40", Box::new(|d| d.rows.extend((0..40).map(|i| (7000 + i, format!("More {i}")))))),
        ("truncate 590", Box::new(|d| d.rows.truncate(590))),
        ("gap 0", Box::new(|d| d.gap = 0.0)),
        ("truncate 9", Box::new(|d| d.rows.truncate(9))),
        ("scroll 0", Box::new(|d| { d.y.set(0.0); })),
        ("clear", Box::new(|d| d.rows.clear())),
        ("refill 250", Box::new(|d| d.rows = (0..250).map(|i| (i + 40, format!("Again {i}"))).collect())),
        ("scroll 1500", Box::new(|d| { d.y.set(1500.0); })),
        ("swap 76 and 78", Box::new(|d| d.rows.swap(76, 78))),
        ("deselect", Box::new(|d| d.selected = u32::MAX)),
    ]
}

fn run_sequence(rings: std::ops::RangeInclusive<usize>) -> Result<(), String> {
    for ring in rings {
        let mut state = Doc::new(600);
        let mut app = App::new(state.snap(), config(&state, ring, false), true);
        gate(&mut app, &state, &format!("ring {ring}, mount"))?;
        for (name, step) in &steps() {
            step(&mut state);
            app.turn(|d| step(d));
            gate(&mut app, &state, &format!("ring {ring}, after `{name}`"))?;
            let p = part(&app);
            if p.realised > 60 {
                return Err(format!("ring {ring}, after `{name}`: {} items realised for a port of 10", p.realised));
            }
        }
    }
    Ok(())
}

#[test]
fn exposure_never_changes_a_pixel() {
    if mutant() != Mutant::None {
        return;
    }
    run_sequence(1..=4).unwrap();
}

/// A window resize exposes rows (the port grows with nothing else changing).
#[test]
fn a_resize_exposes_rows() {
    if mutant() != Mutant::None {
        return;
    }
    let mut state = Doc::new(400);
    state.port_h = 500.0;
    state.h = 180.0;
    let mut app = App::new(state.snap(), config(&state, 2, false), true);
    gate(&mut app, &state, "mount").unwrap();
    let before = part(&app).realised;
    for (w, h) in [(300.0, 420.0), (300.0, 150.0), (340.0, 600.0), (300.0, 280.0)] {
        (state.w, state.h) = (w, h);
        app.resize(w, h);
        app.turn(|d| (d.w, d.h) = (w, h));
        gate(&mut app, &state, &format!("resize {w}x{h}")).unwrap();
    }
    assert!(part(&app).realised >= before);
}

fn wild(id: u32) -> f32 {
    [20.0, 20.0, 64.0, 22.0, 20.0, 110.0, 34.0, 20.0][(id % 8) as usize]
}

/// Rows of very different heights: a fresh mount has measured other rows than the retained tree, so the two
/// are compared at the same anchor (the first visible item and where it sits in the port), which is what the
/// user sees held still.
#[test]
fn different_sizes_agree_at_the_same_anchor() {
    if mutant() != Mutant::None {
        return;
    }
    let mut state = Doc::new(500);
    state.tall = Some(wild);
    let mut app = App::new(state.snap(), config(&state, 3, false), true);
    app.frame();
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let mut y = 0.0f32;
    for step in 0..60 {
        y = match r.below(5) {
            0 => r.below(15000) as f32,
            1 => (y - 37.0).max(0.0),
            2 => y + 61.0,
            3 => (y - 300.0).max(0.0),
            _ => y + 9.5,
        };
        app.turn(|d| { d.y.set(y); });
        app.frame();
        // anchoring may have moved the offset: read it back
        y = *app.owner.y;
        let retained = app.reference_pixels();
        assert_eq!(diff(&app.presented_pixels(), &retained), 0, "step {step}: presented != full redraw");
        assert_eq!(app.rt.dorm.stats.unconverged, 0, "step {step}: no fixed point");
        let p = part(&app);
        let s = app.rt.scale;
        // the first item whose top is inside the port
        let port_top = 20.0 * s;
        let Some(k) = p.ranges.iter().flat_map(|r| r.0..r.1).find(|&k| app.rt.dorm_item_y(p.part, k).is_some_and(|t| t >= port_top)) else { continue };
        let at = (app.rt.dorm_item_y(p.part, k).unwrap() - port_top) / s;
        // eager: every height is known
        let top: f32 = state.rows[..k as usize].iter().map(|row| state.row_h(row.0)).sum();
        let mut e = state.snap();
        e.y = Signal::new(top - at);
        let mut eager = App::new(e, config(&state, 4, true), true);
        eager.frame();
        let n = diff(&retained, &eager.reference_pixels());
        assert_eq!(n, 0, "step {step} (offset {y}, anchor item {k} at {at}): retained != every item mounted ({n} px)");
        // fresh, by exposure, scrolled to the anchor
        let mut fresh = App::new(state.snap(), config(&state, 4, false), true);
        let fp = part(&fresh).part;
        assert!(fresh.scroll_to_item(fp, k as usize, at));
        fresh.frame();
        let n = diff(&retained, &fresh.reference_pixels());
        assert_eq!(n, 0, "step {step} (offset {y}, anchor item {k} at {at}): retained != fresh mount scrolled to the anchor ({n} px)");
    }
}

/// An item on screen moves by exactly what the scroll asked while rows of other sizes than the estimate are
/// realised above it (the correction goes into the offset).
fn track() -> Result<(), String> {
    let mut state = Doc::new(4000);
    state.tall = Some(wild);
    let mut app = App::new(state.snap(), config(&state, 2, false), false);
    let s = app.rt.scale;
    // far down, where everything above is estimated
    app.turn(|d| { d.y.set(40000.0); });
    let mut moved = 0;
    for step in 0..120 {
        let p = part(&app);
        let port = (20.0 * s, 220.0 * s);
        let Some(k) = p.ranges.iter().flat_map(|r| r.0..r.1).find(|&k| app.rt.dorm_item_y(p.part, k).is_some_and(|t| t >= port.0 + 100.0 && t < port.1)) else {
            return Err(format!("step {step}: nothing realised in the port ({:?})", p.ranges));
        };
        let before = app.rt.dorm_item_y(p.part, k).unwrap();
        let y0 = *app.owner.y;
        app.turn(|d| { d.y.set(y0 - 31.0); });
        let after = app.rt.dorm_item_y(p.part, k).ok_or("the tracked item was released")?;
        if after - before != 31.0 * s {
            return Err(format!("step {step}: item {k} moved {} px on screen for a scroll of {} px", after - before, 31.0 * s));
        }
        if *app.owner.y != y0 - 31.0 {
            moved += 1;
        }
        if app.rt.dorm.stats.unconverged != 0 {
            return Err(format!("step {step}: no fixed point"));
        }
    }
    if moved == 0 && mutant() == Mutant::None {
        return Err("no step corrected a size above the port: the test does not exercise anchoring".into());
    }
    Ok(())
}

#[test]
fn a_visible_item_stays_put_while_sizes_above_are_corrected() {
    if mutant() != Mutant::None {
        return;
    }
    track().unwrap();
}

/// A long scroll in both directions leaves a screenful mounted, not a history.
fn long_scroll() -> Result<(), String> {
    let state = Doc::new(100_000);
    let mut app = App::new(state.snap(), config(&state, 2, false), false);
    let base = app.record_count();
    let mut peak = 0;
    for k in 0..3000 {
        let y = if k < 2000 { k as f32 * 37.0 } else { 2000.0 * 37.0 - (k - 2000) as f32 * 53.0 };
        app.turn(|d| { d.y.set(y); });
        app.frame();
        peak = peak.max(app.record_count());
        if peak > base * 3 {
            return Err(format!("{peak} records at the peak of a long scroll (step {k}), {base} at mount"));
        }
    }
    if app.rt.dorm.stats.unconverged != 0 {
        return Err("no fixed point".into());
    }
    if peak > base * 3 {
        return Err(format!("{peak} records at the peak of a long scroll, {base} at mount"));
    }
    // the freed resources came back: nothing waits, and the tables did not grow with the distance
    while app.reclaiming() {
        app.turn(|_| {});
    }
    if app.rt.nodes.len() > 4000 {
        return Err(format!("{} nodes allocated after the scroll", app.rt.nodes.len()));
    }
    Ok(())
}

#[test]
fn a_long_scroll_keeps_a_screenful() {
    if mutant() != Mutant::None {
        return;
    }
    long_scroll().unwrap();
}

// ------------------------------------------------------------------ what mounts how

struct Shapes {
    n: usize,
    y: Signal<f32>,
}
impl View for Shapes {
    fn view(&self) -> impl Element<Self> + '_ {
        let cell = |i: usize| block(30.0, 20.0).bg(rgb(0x202020 + i as u32));
        column((
            // 0: a row container: eager
            row(each((0..self.n).map(cell))).h(24.0),
            // 1: a source without an index (a filter): eager
            column(each((0..self.n).filter(|i| i % 2 == 0).map(cell))).h(40.0).clip_overflow(),
            // 2: a column of a definite height: eager
            column(each((0..self.n).map(cell))).h(40.0).clip_overflow(),
            // 3: sections of rows in a scroll box: dormant, and each realised section's rows are a part too
            div().h(120.0).scroll(sig!(self.y)).child(each((0..self.n).map(move |s| column((text(s), each((0..self.n).map(cell))))))),
        ))
    }
}

#[test]
fn what_cannot_be_a_run_mounts_eagerly_and_nested_parts_nest() {
    if mutant() != Mutant::None {
        return;
    }
    let mut cfg = Config::new(300.0, 260.0, 1.0, FONT.to_vec());
    cfg.caps.instances = 1 << 15;
    let mut app = App::new(Shapes { n: 300, y: Signal::new(0.0) }, cfg, false);
    let parts = app.rt.parts();
    let eager: Vec<_> = parts.iter().filter(|p| !p.dormant).collect();
    assert_eq!(eager.len(), 3, "{parts:?}");
    assert_eq!(eager[0].realised, 300);
    assert_eq!(eager[1].realised, 150);
    assert_eq!(eager[2].realised, 300);
    let outer = parts.iter().find(|p| p.dormant && p.items == 300 && p.realised < 20).expect("the sections are dormant");
    // a section is 20 + 300 * 20 pt tall: one is visible, and of its rows a screenful
    let inner: Vec<_> = parts.iter().filter(|p| p.dormant && p.part != outer.part).collect();
    assert!(!inner.is_empty() && inner.iter().any(|p| p.realised < 30 && p.runs >= 1), "{parts:?}");
    let records = app.record_count();
    assert!(records < 750 + 150, "{records} records: {parts:?}");
    assert!(parts.iter().filter(|p| p.dormant).map(|p| p.realised).sum::<u32>() < 60, "{parts:?}");
    // into the third section: the first one's rows go with it
    app.turn(|d| { d.y.set(2.0 * 6020.0 + 900.0); });
    app.frame();
    app.turn(|d| { d.y.set(2.0 * 6020.0 + 910.0); });
    app.turn(|d| { d.y.set(2.0 * 6020.0 + 920.0); });
    app.turn(|d| { d.y.set(2.0 * 6020.0 + 930.0); });
    let parts = app.rt.parts();
    let outer = parts.iter().find(|p| p.dormant && p.items == 300 && p.realised < 20).expect("the sections are dormant");
    assert!(outer.ranges.iter().any(|r| r.0 <= 2 && r.1 > 2), "{outer:?}");
    assert!(app.record_count() < 750 + 150, "{} records", app.record_count());
    assert!(parts.iter().filter(|p| p.dormant).map(|p| p.realised).sum::<u32>() < 60, "{parts:?}");
    assert_eq!(app.rt.dorm.stats.unconverged, 0);
}

// ------------------------------------------------------------------ mutants

/// `BOUGH_DORM_MUTANT=short cargo test --release -p bough --test dormant -- one_item_too_few`
#[test]
fn one_item_too_few_is_caught_by_the_pixel_gate() {
    if mutant() != Mutant::Short {
        return;
    }
    let e = run_sequence(4..=4).expect_err("the mutant passed the gate");
    assert!(e.contains("retained != fresh mount (eager parts: true"), "caught by the wrong gate: {e}");
}

/// `BOUGH_DORM_MUTANT=norelease cargo test --release -p bough --test dormant -- never_releasing`
#[test]
fn never_releasing_is_caught_by_the_record_bound() {
    if mutant() != Mutant::NoRelease {
        return;
    }
    let e = long_scroll().expect_err("the mutant passed the gate");
    assert!(e.contains("records at the peak"), "caught by the wrong gate: {e}");
}

/// `BOUGH_DORM_MUTANT=noanchor cargo test --release -p bough --test dormant -- no_anchoring`
#[test]
fn no_anchoring_is_caught_by_the_tracked_item() {
    if mutant() != Mutant::NoAnchor {
        return;
    }
    let e = track().expect_err("the mutant passed the gate");
    assert!(e.contains("moved"), "caught by the wrong gate: {e}");
}
