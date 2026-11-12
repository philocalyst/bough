//! The adversarial matrix (lane brief): one case, one operation and one size per process, so a panic is
//! that cell's result. Views are prelude-only; the harness uses the same `bough::raw` escapes as the driver.
//!
//!   adversarial --case NAME --op OP --n N [--w W --h H --scale S --reps R --k K --flag]
//!
//! Stages (each caught separately): mount, R0 (instructions per operation through the GPU's input), R1 (with
//! the Metal frame), gates (presented == full redraw, retained == fresh mount, case probes) after each of
//! four transitions (or once after a sequence).
#![recursion_limit = "4096"]
#![type_length_limit = "100000000"]
use bough::prelude::*;
use bough::raw::{App, Config};
use bough_guibench::{
    driver,
    model::{Rows, plain_row},
    scenarios::Virtual,
    spec,
};
use bough::sys::perf::{snap, tinstr};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Clone)]
struct Args {
    case: String,
    op: String,
    n: usize,
    w: f32,
    h: f32,
    scale: f32,
    reps: usize,
    k: usize,
    flag: bool,
    once: bool,
}

trait Snap: View {
    fn snap(&self) -> Self;
}

fn cfg(a: &Args) -> Config {
    let mut c = driver::config(a.scale);
    c.width_pt = a.w;
    c.height_pt = a.h;
    if a.once {
        c = c.once();
    }
    c
}
fn focus<V: View>(a: &mut App<V>) {
    if let Some(pos) = (0..a.record_count()).find(|&p| a.record_view(p).kind & 0x40 != 0) {
        let h = a.rt.bough.handle_at(pos);
        a.set_focus(h, false);
    }
}
fn key<V: View>(a: &mut App<V>, c: char) {
    a.key_down(Key::Char(c), Mods::default());
}
fn diff(a: &[u8], b: &[u8]) -> usize {
    if a.len() != b.len() {
        return usize::MAX;
    }
    a.iter().zip(b).filter(|(a, b)| a != b).count()
}
fn px(p: &[u8], w: usize, x: usize, y: usize) -> u32 {
    let i = (y * w + x) * 4;
    (p[i + 2] as u32) << 16 | (p[i + 1] as u32) << 8 | p[i] as u32
}
fn want(out: &mut Vec<String>, p: &[u8], w: usize, x: usize, y: usize, c: u32, what: &str) {
    let got = px(p, w, x, y);
    if got != c {
        out.push(format!("WRONG {what} at {x},{y}: got {got:06x} want {c:06x}"));
    }
}

type Input<V> = fn(&mut App<V>, &Args, bool);
type Probe<V> = fn(&mut App<V>, &Args, bool, &[u8]) -> Vec<String>;

fn mount<V: Snap>(a: &Args, v: V, gpu: bool) -> App<V> {
    let mut app = App::new(v, cfg(a), gpu);
    focus(&mut app);
    app.frame();
    app
}
fn stage(name: &str, f: impl FnOnce() -> String) {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(s) => println!("stage {name} {s}"),
        Err(e) => {
            let m = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            println!("stage {name} PANIC {}", m.replace('\n', " "));
        }
    }
}
fn gate<V: Snap>(a: &Args, app: &mut App<V>, undo: bool, probe: Probe<V>) -> String {
    let shown = app.presented_pixels();
    let full = app.reference_pixels();
    let mut fresh = mount(a, app.owner.snap(), true);
    if let Some((x, y)) = app.rt.input.pointer {
        fresh.pointer_move(x as f32 / a.scale, y as f32 / a.scale);
        fresh.frame();
    }
    let fp = fresh.reference_pixels();
    if let (Ok(dir), true) = (std::env::var("ADV_DUMP"), full != fp) {
        let k = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) / 2;
        std::fs::write(format!("{dir}/{k}-retained.bgra"), &full).unwrap();
        std::fs::write(format!("{dir}/{k}-fresh.bgra"), &fp).unwrap();
    }
    let pr = probe(app, a, undo, &shown);
    let fr = probe(&mut fresh, a, undo, &fp);
    let show = |v: Vec<String>| if v.is_empty() { "ok".to_string() } else { v.join("; ") };
    format!("pf={} rf={} records={}/{} probes={} fresh-probes={}", diff(&shown, &full), diff(&full, &fp), app.record_count(), fresh.record_count(), show(pr), show(fr))
}

/// Operation/undo pairs.
fn pairs<V: Snap>(a: &Args, mk: &dyn Fn() -> V, input: Input<V>, probe: Probe<V>) {
    stage("mount", || {
        let (i0, m0) = (tinstr(), snap());
        let app = mount(a, mk(), false);
        let m1 = snap();
        format!("instructions={} records={} rss={} footprint_delta={}", tinstr() - i0, app.record_count(), m1.rss, m1.footprint as i64 - m0.footprint as i64)
    });
    for (name, gpu) in [("R0", false), ("R1", true)] {
        stage(name, || {
            let mut app = mount(a, mk(), gpu);
            let step = |app: &mut App<V>| {
                for undo in [false, true] {
                    input(app, a, undo);
                    app.frame();
                }
            };
            for _ in 0..2 {
                step(&mut app);
            }
            let mut per = Vec::new();
            for _ in 0..3 {
                let s = tinstr();
                for _ in 0..a.reps {
                    step(&mut app);
                }
                if gpu {
                    app.wait_presented();
                }
                per.push((tinstr() - s) / (2 * a.reps as u64));
            }
            per.sort();
            format!("med={} lo={} hi={} records={}", per[1], per[0], per[2], app.record_count())
        });
    }
    stage("gates", || {
        let mut app = mount(a, mk(), true);
        let mut out = Vec::new();
        for t in 0..4 {
            let undo = t % 2 == 1;
            input(&mut app, a, undo);
            app.frame();
            out.push(gate(a, &mut app, undo, probe));
        }
        out.join(" | ")
    });
}

/// A sequence of `a.k` operations (no undo): instructions per operation over the first and the last tenth.
fn seq<V: Snap>(a: &Args, mk: &dyn Fn() -> V, input: Input<V>, probe: Probe<V>) {
    for (name, gpu) in [("R0", false), ("R1", true)] {
        stage(name, || {
            let mut app = mount(a, mk(), gpu);
            let tenth = (a.k / 10).max(1);
            let (mut first, mut last) = (0, 0);
            let t0 = tinstr();
            for i in 0..a.k {
                let s = tinstr();
                input(&mut app, a, false);
                app.frame();
                let d = tinstr() - s;
                if i < tenth {
                    first += d;
                }
                if i >= a.k - tenth {
                    last += d;
                }
            }
            format!("first={} last={} mean={} records={}", first / tenth as u64, last / tenth as u64, (tinstr() - t0) / a.k as u64, app.record_count())
        });
    }
    stage("gates", || {
        let mut app = mount(a, mk(), true);
        for _ in 0..a.k {
            input(&mut app, a, false);
            app.frame();
        }
        gate(a, &mut app, false, probe)
    });
}
fn no_probe<V: View>(_: &mut App<V>, _: &Args, _: bool, _: &[u8]) -> Vec<String> {
    Vec::new()
}

// ------------------------------------------------------------------ wide / tall windows
const WA: u32 = 0x2255aa;
const WB: u32 = 0xaa5522;
const WH: u32 = 0x00cc66;
const WF: u32 = 0xcc00cc;
struct Wide {
    n: usize,
    flip: bool,
    vertical: bool,
}
impl Snap for Wide {
    fn snap(&self) -> Self {
        Wide { n: self.n, flip: self.flip, vertical: self.vertical }
    }
}
impl View for Wide {
    fn view(&self) -> impl Element<Self> + '_ {
        let cell = |i: usize| {
            let c = if i == self.n - 1 && self.flip { WF } else if i % 2 == 0 { WA } else { WB };
            let b = if self.vertical { block(0.0, 32.0).w_pct(1.0) } else { block(32.0, 0.0).h_pct(1.0) };
            b.shrink(0.0).bg(rgb(c)).hover_bg(rgb(WH))
        };
        let on_key = |v: &mut Self, _: &_| v.flip = !v.flip;
        (
            (!self.vertical).then(|| row((each((0..self.n).map(cell)),)).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).on_key(on_key)),
            self.vertical.then(|| column((each((0..self.n).map(cell)),)).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).on_key(on_key)),
        )
    }
}
fn wide_far(a: &Args, v: bool) -> (f32, f32) {
    if v { (a.w / 2.0, a.h - 16.0) } else { (a.w - 16.0, a.h / 2.0) }
}
fn wide_input(app: &mut App<Wide>, a: &Args, undo: bool) {
    let (fx, fy) = wide_far(a, app.owner.vertical);
    match a.op.as_str() {
        "hover-far" => {
            if undo { app.pointer_move(16.0, 16.0) } else { app.pointer_move(fx, fy) }
        }
        "flip-far" => key(app, 'f'),
        _ => panic!("op"),
    }
}
fn wide_probe(app: &mut App<Wide>, a: &Args, undo: bool, p: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let w = app.rt.win_w as usize;
    let (fx, fy) = wide_far(a, app.owner.vertical);
    let n = app.owner.n;
    let base = if (n - 1) % 2 == 0 { WA } else { WB };
    let far = match (a.op.as_str(), undo) {
        ("hover-far", false) => WH,
        ("flip-far", false) => WF,
        _ => base,
    };
    want(&mut out, p, w, fx as usize, fy as usize, far, "far cell");
    want(&mut out, p, w, 16, 16, if a.op == "hover-far" && undo { WH } else { WA }, "near cell");
    out
}

// ------------------------------------------------------------------ tall content in a scroll box
const TA: u32 = 0xdddddd;
const TB: u32 = 0x3366cc;
const TC: u32 = 0xcc6633;
struct Tall {
    px: f32,
    y: Signal<f32>,
}
impl Snap for Tall {
    fn snap(&self) -> Self {
        Tall { px: self.px, y: Signal::new(*self.y) }
    }
}
impl View for Tall {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).scroll(sig!(self.y)).child(column((
            block(0.0, self.px).w_pct(1.0).shrink(0.0).bg(rgb(TA)),
            block(0.0, 32.0).w_pct(1.0).shrink(0.0).bg(rgb(TB)),
            block(0.0, 32.0).w_pct(1.0).shrink(0.0).bg(rgb(TC)),
        )))
    }
}
fn end_input<V: View + Scrolled>(app: &mut App<V>, a: &Args, undo: bool) {
    match a.op.as_str() {
        // to the end and back to the start
        "end" => app.wheel_points(640.0, 400.0, if undo { -1.0e9 } else { 1.0e9 }),
        // one point up from the end, and back
        "end-1" => {
            if app.owner.off() == 0.0 {
                app.wheel_points(640.0, 400.0, 1.0e9);
                app.frame();
            }
            app.wheel_points(640.0, 400.0, if undo { 1.0 } else { -1.0 })
        }
        _ => panic!("op"),
    }
    // the pointer is re-delivered so stale hover after a scroll (a separate finding) is not what is gated here
    app.pointer_move(640.0, 400.0);
}
trait Scrolled {
    fn off(&self) -> f32;
}
impl Scrolled for Tall {
    fn off(&self) -> f32 {
        *self.y
    }
}
impl Scrolled for Rows {
    fn off(&self) -> f32 {
        *self.y
    }
}
impl Scrolled for Virtual {
    fn off(&self) -> f32 {
        *self.y
    }
}
fn tall_probe(app: &mut App<Tall>, a: &Args, undo: bool, p: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let (w, h) = (app.rt.win_w as usize, app.rt.win_h as usize);
    let y = *app.owner.y;
    let total = app.owner.px + 64.0;
    if !undo {
        let exp = total - a.h - if a.op == "end-1" { 1.0 } else { 0.0 };
        if y != exp {
            out.push(format!("WRONG offset {y} want {exp}"));
        }
        let d = if a.op == "end-1" { 1 } else { 0 };
        want(&mut out, p, w, 640, h - 1, TC, "last block");
        want(&mut out, p, w, 640, h - 32 + d, TC, "last block top");
        want(&mut out, p, w, 640, h - 33 + d, TB, "second block bottom");
        want(&mut out, p, w, 640, h - 64 + d, TB, "second block top");
        want(&mut out, p, w, 640, h - 65 + d, TA, "first block bottom");
    } else if a.op == "end" {
        want(&mut out, p, w, 640, 0, TA, "first block");
    } else {
        if y != total - a.h {
            out.push(format!("WRONG offset {y} want {}", total - a.h));
        }
        want(&mut out, p, w, 640, h - 1, TC, "last block");
        want(&mut out, p, w, 640, h - 33, TB, "second block bottom");
        want(&mut out, p, w, 640, h - 65, TA, "first block bottom");
    }
    out
}

fn stripes(p: &[u8], w: usize, h: usize, off: i64, n: usize) -> Vec<String> {
    let (mut bad, mut first) = (0, None);
    for y in 0..h {
        let cy = y as i64 + off;
        let i = (cy / 32) as usize;
        let exp = if cy < 0 || i >= n {
            spec::BACKGROUND
        } else if cy % 32 == 31 {
            spec::ROW_BORDER
        } else {
            spec::row_background(i)
        };
        let got = px(p, w, 640, y);
        // the row under the pointer may be hovered
        if got != exp && !(got == spec::ROW_HOVER && cy % 32 != 31) {
            bad += 1;
            first.get_or_insert((y, got, exp));
        }
    }
    match first {
        Some((y, got, exp)) => vec![format!("WRONG stripes: {bad} of {h} rows of column 640; first y={y} got {got:06x} want {exp:06x} (offset {off})")],
        None => Vec::new(),
    }
}
impl Snap for Rows {
    fn snap(&self) -> Self {
        Rows { count: self.count, y: Signal::new(*self.y) }
    }
}
impl Snap for Virtual {
    fn snap(&self) -> Self {
        Virtual { count: self.count, y: Signal::new(*self.y) }
    }
}
fn rows_probe(app: &mut App<Rows>, a: &Args, undo: bool, p: &[u8]) -> Vec<String> {
    let end = app.owner.count as f32 * 32.0 - a.h;
    let exp = if a.op == "end-1" { if undo { end } else { end - 1.0 } } else if undo { 0.0 } else { end };
    let mut out = stripes(p, app.rt.win_w as usize, app.rt.win_h as usize, *app.owner.y as i64, app.owner.count);
    if *app.owner.y != exp {
        out.push(format!("WRONG offset {} want {exp}", *app.owner.y));
    }
    out
}
fn virtual_probe(app: &mut App<Virtual>, a: &Args, undo: bool, p: &[u8]) -> Vec<String> {
    let end = app.owner.count as f32 * 32.0 - a.h;
    let exp = if a.op == "end-1" { if undo { end } else { end - 1.0 } } else if undo { 0.0 } else { end };
    let mut out = stripes(p, app.rt.win_w as usize, app.rt.win_h as usize, *app.owner.y as i64, app.owner.count);
    if *app.owner.y != exp {
        out.push(format!("WRONG offset {} want {exp}", *app.owner.y));
    }
    out
}

// ------------------------------------------------------------------ unequal-height wrapping rows
fn words(i: usize, extra: usize) -> String {
    let n = (i % 5 + 1) * 14 + extra;
    (0..n).map(|k| ["lorem", "ipsum", "dolor", "sit", "amet", "consectetur"][(i + k) % 6]).collect::<Vec<_>>().join(" ")
}
struct WRow {
    i: usize,
    text: Signal<String>,
}
impl View for WRow {
    fn view(&self) -> impl Element<Self> + '_ {
        column((para(sig!(self.text)).font(14.0, 20.0).color(rgb(spec::LABEL_COLOR)).w_pct(1.0),))
            .w_pct(1.0)
            .px(12.0)
            .py(4.0)
            .bg(rgb(spec::row_background(self.i)))
    }
}
struct Wrap {
    rows: List<WRow>,
    y: Signal<f32>,
    n: usize,
    mounted: bool,
}
impl Wrap {
    fn new(n: usize, mounted: bool) -> Self {
        Wrap { rows: List::from_iter_in((0..n).map(|i| WRow { i, text: Signal::new(words(i, 0)) })), y: Signal::new(0.0), n, mounted }
    }
}
impl Snap for Wrap {
    fn snap(&self) -> Self {
        Wrap { rows: List::from_iter_in(self.rows.iter().map(|r| WRow { i: r.i, text: Signal::new((*r.text).clone()) })), y: Signal::new(*self.y), n: self.n, mounted: self.mounted }
    }
}
impl View for Wrap {
    fn view(&self) -> impl Element<Self> + '_ {
        let over = if self.mounted { self.n } else { 2 };
        // the 8 pt block keeps the scroll port off the window origin (a list in a port at the origin mounts no rows)
        column((block(0.0, 8.0).w_pct(1.0).shrink(0.0).bg(rgb(0xeeeeee)), div().w_pct(1.0).grow(1.0).min_h(0.0).scroll(sig!(self.y)).child(list(sig!(self.rows)).w_pct(1.0).overscan(over))))
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(rgb(0xffffff))
            .on_key(|v: &mut Self, e| {
                let extra = if e.key == Some(Key::Char('g')) { 60 } else { 0 };
                if v.rows.len() > 1 {
                    let r = v.rows.edit(1);
                    r.text.set(words(r.i, extra));
                }
            })
    }
}
struct WrapC {
    rows: Vec<String>,
}
impl Snap for WrapC {
    fn snap(&self) -> Self {
        WrapC { rows: self.rows.clone() }
    }
}
impl View for WrapC {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(rgb(0xffffff))
            .child(each(self.rows.iter().enumerate().map(|(i, s)| {
                column((para(s.clone()).font(14.0, 20.0).color(rgb(spec::LABEL_COLOR)).w_pct(1.0),)).w_pct(1.0).shrink(0.0).px(12.0).py(4.0).bg(rgb(spec::row_background(i)))
            })))
            .on_key(|v: &mut Self, e| {
                let extra = if e.key == Some(Key::Char('g')) { 60 } else { 0 };
                if v.rows.len() > 1 {
                    v.rows[1] = words(1, extra);
                }
            })
    }
}
fn grow_input<V: View>(app: &mut App<V>, _: &Args, undo: bool) {
    key(app, if undo { 's' } else { 'g' });
}

// ------------------------------------------------------------------ nested scroll boxes
struct Nested {
    rows: Vec<spec::Row>,
    outer: Signal<f32>,
    a: Signal<f32>,
    b: Signal<f32>,
}
impl Nested {
    fn new() -> Self {
        Nested { rows: spec::Ids::default().rows(60), outer: Signal::new(0.0), a: Signal::new(0.0), b: Signal::new(0.0) }
    }
}
impl Snap for Nested {
    fn snap(&self) -> Self {
        Nested { rows: self.rows.clone(), outer: Signal::new(*self.outer), a: Signal::new(*self.a), b: Signal::new(*self.b) }
    }
}
impl View for Nested {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).scroll(sig!(self.outer)).child(column((
            block(0.0, 100.0).w_pct(1.0).shrink(0.0).bg(rgb(TB)),
            div().w_pct(1.0).h(300.0).shrink(0.0).bg(rgb(0xffffff)).scroll(sig!(self.a)).child(each(self.rows.iter().map(plain_row))),
            block(0.0, 100.0).w_pct(1.0).shrink(0.0).bg(rgb(TC)),
            div().w_pct(1.0).h(300.0).shrink(0.0).bg(rgb(0xffffff)).scroll(sig!(self.b)).child(each(self.rows.iter().map(plain_row))),
            each(self.rows.iter().map(plain_row)),
        )))
    }
}
fn nested_input(app: &mut App<Nested>, a: &Args, undo: bool) {
    let d = if undo { -16.0 } else { 16.0 };
    match a.op.as_str() {
        "inner" => app.wheel_points(640.0, 250.0, d),
        "outer" => app.wheel_points(640.0, 50.0, d),
        // both wheels before one frame
        "both" => {
            app.wheel_points(640.0, 250.0, d);
            app.wheel_points(640.0, 50.0, d);
        }
        // the inner box is at its end: does the wheel reach the outer box?
        "chain" => {
            if !undo {
                app.wheel_points(640.0, 250.0, 1.0e6);
                app.wheel_points(640.0, 250.0, 16.0);
            } else {
                app.wheel_points(640.0, 250.0, -1.0e6);
                app.wheel_points(640.0, 250.0, -16.0);
            }
        }
        _ => panic!("op"),
    }
}
fn nested_probe(app: &mut App<Nested>, a: &Args, undo: bool, p: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let w = app.rt.win_w as usize;
    let (o, ia) = (*app.owner.outer, *app.owner.a);
    let exp = match (a.op.as_str(), undo) {
        (_, true) => (0.0, 0.0),
        ("inner", _) => (0.0, 16.0),
        ("outer", _) => (16.0, 0.0),
        ("both", _) => (16.0, 16.0),
        // 60 rows of 32 in a 300 box
        _ => (o, 60.0 * 32.0 - 300.0),
    };
    if (o, ia) != exp {
        out.push(format!("WRONG offsets outer={o} inner={ia} want {exp:?}"));
    }
    if a.op == "chain" {
        out.push(format!("note: wheel at the inner box's end left outer={o}"));
    }
    // column 640: the first header block, then inner box rows (a border line every 32 at inner offset)
    let top = 100 - o as i64;
    for y in 0..app.rt.win_h as i64 {
        let exp = if y < top {
            TB
        } else if y < top + 300 {
            if (y - top + ia as i64) % 32 == 31 { spec::ROW_BORDER } else { spec::ROW_EVEN }
        } else if y < top + 400 {
            TC
        } else {
            continue;
        };
        let got = px(p, w, 640, y as usize);
        if got != exp {
            out.push(format!("WRONG pixel at 640,{y}: got {got:06x} want {exp:06x}"));
            break;
        }
    }
    out
}

// ------------------------------------------------------------------ a deep tree
const DL: u32 = 0x118833;
#[cfg(feature = "deep")]
macro_rules! nest {
    ($leaf:expr;) => { $leaf };
    ($leaf:expr; $h:tt $($t:tt)*) => { div().p(1.0).bg(rgb(0x335577)).child(nest!($leaf; $($t)*)) };
}
#[cfg(feature = "deep")]
macro_rules! deep {
    ($name:ident, $($t:tt)*) => {
        struct $name {
            flip: bool,
        }
        impl Snap for $name {
            fn snap(&self) -> Self {
                $name { flip: self.flip }
            }
        }
        impl View for $name {
            fn view(&self) -> impl Element<Self> + '_ {
                column((nest!(block(100.0, 40.0).bg(rgb(if self.flip { WF } else { DL })).hover_bg(rgb(WH)); $($t)*),))
                    .w_pct(1.0)
                    .h_pct(1.0)
                    .bg(rgb(0xffffff))
                    .on_key(|v: &mut Self, _| v.flip = !v.flip)
            }
        }
    };
}
#[cfg(feature = "deep")]
deep!(Deep10, x x x x x x x x x x);
#[cfg(feature = "deep")]
deep!(Deep50, x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x);
#[cfg(feature = "deep")]
deep!(Deep100, x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x);
#[cfg(feature = "deep")]
deep!(Deep200, x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x);
#[cfg(feature = "deep")]
fn deep_input<V: View>(app: &mut App<V>, a: &Args, undo: bool) {
    let d = a.n as f32;
    match a.op.as_str() {
        "hover" => {
            if undo { app.pointer_move(1000.0, 700.0) } else { app.pointer_move(d + 20.0, d + 20.0) }
        }
        "flip" => key(app, 'f'),
        _ => panic!("op"),
    }
}
#[cfg(feature = "deep")]
fn deep_probe<V: View>(app: &mut App<V>, a: &Args, undo: bool, p: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let w = app.rt.win_w as usize;
    let d = a.n;
    let leaf = match (a.op.as_str(), undo) {
        ("hover", false) => WH,
        ("flip", false) => WF,
        _ => DL,
    };
    want(&mut out, p, w, d + 20, d + 20, leaf, "leaf");
    want(&mut out, p, w, d - 1, d + 20, 0x335577, "innermost box edge");
    want(&mut out, p, w, 0, 5, 0x335577, "outermost box edge");
    out
}
/// The same depth through owners: every level is a `List` row that owns the next level's list.
struct Node {
    kids: List<Node>,
    on: Signal<bool>,
}
fn node(depth: usize) -> Node {
    let mut n = Node { kids: List::new(), on: Signal::new(false) };
    if depth > 0 {
        n.kids.push(node(depth - 1));
    }
    n
}
fn node_snap(n: &Node) -> Node {
    Node { kids: List::from_iter_in(n.kids.iter().map(node_snap)), on: Signal::new(*n.on) }
}
impl View for Node {
    fn view(&self) -> impl Element<Self> + '_ {
        div().p(1.0).min_h(4.0).bg(sig!(self.on).map(|o| rgb(if *o { WF } else { 0x335577 }))).child(list(sig!(self.kids)).w_pct(1.0))
    }
}
struct DeepList {
    root: List<Node>,
    depth: usize,
}
impl Snap for DeepList {
    fn snap(&self) -> Self {
        DeepList { root: List::from_iter_in(self.root.iter().map(node_snap)), depth: self.depth }
    }
}
impl View for DeepList {
    fn view(&self) -> impl Element<Self> + '_ {
        column((list(sig!(self.root)).w_pct(1.0),)).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).on_key(|v: &mut Self, _| {
            let mut n = v.root.edit(0);
            for _ in 1..v.depth {
                n = n.kids.edit(0);
            }
            let on = !*n.on;
            n.on.set(on);
        })
    }
}
fn deeplist_probe(app: &mut App<DeepList>, a: &Args, undo: bool, p: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let w = app.rt.win_w as usize;
    let d = a.n;
    // level k's box starts at x = k (padding 1 per level); the deepest (level d-1) is 4 px tall inside
    want(&mut out, p, w, 0, 0, 0x335577, "outermost box");
    want(&mut out, p, w, d - 1, d - 1 + 1, if undo { 0x335577 } else { WF }, "deepest box");
    out
}

// ------------------------------------------------------------------ signals: many in one turn, one with many dependents
struct Cell {
    v: Signal<u32>,
}
impl View for Cell {
    fn view(&self) -> impl Element<Self> + '_ {
        row((text(sig!(self.v).map(|n| n.to_string())).font(10.0, 12.0).color(rgb(0x000000)),)).w_pct(1.0).h(14.0).shrink(0.0).px(4.0)
    }
}
struct Cells {
    rows: List<Cell>,
    y: Signal<f32>,
    n: usize,
    k: usize,
    mounted: bool,
}
impl Snap for Cells {
    fn snap(&self) -> Self {
        Cells { rows: List::from_iter_in(self.rows.iter().map(|c| Cell { v: Signal::new(*c.v) })), y: Signal::new(*self.y), n: self.n, k: self.k, mounted: self.mounted }
    }
}
impl View for Cells {
    fn view(&self) -> impl Element<Self> + '_ {
        let over = if self.mounted { self.n } else { 2 };
        column((block(0.0, 8.0).w_pct(1.0).shrink(0.0).bg(rgb(0xeeeeee)), div().w_pct(1.0).grow(1.0).min_h(0.0).scroll(sig!(self.y)).child(list(sig!(self.rows)).w_pct(1.0).overscan(over)))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).on_key(|v: &mut Self, _| {
            let step = (v.n / v.k).max(1);
            for i in (0..v.n).step_by(step).take(v.k) {
                let c = v.rows.edit(i);
                let x = *c.v + 1;
                c.v.set(x);
            }
        })
    }
}
struct Fan {
    v: Signal<u32>,
    on: Signal<bool>,
    plain: u32,
    n: usize,
}
impl Snap for Fan {
    fn snap(&self) -> Self {
        Fan { v: Signal::new(*self.v), on: Signal::new(*self.on), plain: self.plain, n: self.n }
    }
}
impl View for Fan {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            row((text(format!("plain {}", self.plain)).font(12.0, 16.0).color(rgb(0x000000)),)).w_pct(1.0).h(20.0).shrink(0.0),
            div().w_pct(1.0).grow(1.0).min_h(0.0).child(each((0..self.n).map(|_| {
                row((
                    block(10.0, 10.0).shrink(0.0).bg(sig!(self.on).map(|o| rgb(if *o { WF } else { DL }))),
                    text(sig!(self.v).map(|n| n.to_string())).font(10.0, 12.0).color(rgb(0x000000)),
                ))
                .w_pct(1.0)
                .h(14.0)
                .shrink(0.0)
                .gap(4.0)
            }))),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(0xffffff))
        .on_key(|v: &mut Self, e| match e.key {
            Some(Key::Char('v')) => {
                let x = *v.v + 1;
                v.v.set(x);
            }
            Some(Key::Char('o')) => {
                let x = !*v.on;
                v.on.set(x);
            }
            Some(Key::Char('p')) => v.plain += 1,
            _ => {}
        })
    }
}
fn fan_input(app: &mut App<Fan>, a: &Args, _: bool) {
    key(app, match a.op.as_str() {
        "sig-text" => 'v',
        "sig-paint" => 'o',
        "plain" => 'p',
        "noop" => 'n',
        _ => panic!("op"),
    });
}
fn fan_probe(app: &mut App<Fan>, _: &Args, _: bool, p: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let w = app.rt.win_w as usize;
    want(&mut out, p, w, 5, 20 + 7, if *app.owner.on { WF } else { DL }, "first dependent block");
    if app.owner.n > 40 {
        want(&mut out, p, w, 5, 20 + 14 * 40 + 7, if *app.owner.on { WF } else { DL }, "41st dependent block");
    }
    out
}

// ------------------------------------------------------------------ where does a list mount its rows?
struct ListBox {
    rows: List<Cell>,
    y: Signal<f32>,
    shape: usize,
}
impl Snap for ListBox {
    fn snap(&self) -> Self {
        ListBox { rows: List::from_iter_in(self.rows.iter().map(|c| Cell { v: Signal::new(*c.v) })), y: Signal::new(*self.y), shape: self.shape }
    }
}
impl View for ListBox {
    fn view(&self) -> impl Element<Self> + '_ {
        let s = self.shape;
        (
            // 0: the scroll box is the root element (as `Rows` is, with `each`)
            (s == 0).then(|| div().w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).scroll(sig!(self.y)).child(list(sig!(self.rows)).w_pct(1.0))),
            // 1: the same inside a column (as the table port)
            (s == 1).then(|| column((div().w_pct(1.0).grow(1.0).min_h(0.0).scroll(sig!(self.y)).child(list(sig!(self.rows)).w_pct(1.0)),)).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))),
            // 2: a plain root box, no scroll
            (s == 2).then(|| div().w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).child(list(sig!(self.rows)).w_pct(1.0))),
            // 5: shape 1 below a 40 pt header block; 6: below a 1 pt block; 7: above a footer block; 8: rows with a background
            (s == 5).then(|| column((block(0.0, 40.0).w_pct(1.0).shrink(0.0).bg(rgb(0xeeeeee)), div().w_pct(1.0).grow(1.0).min_h(0.0).scroll(sig!(self.y)).child(list(sig!(self.rows)).w_pct(1.0)))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))),
            (s == 6).then(|| column((block(0.0, 1.0).w_pct(1.0).shrink(0.0).bg(rgb(0xeeeeee)), div().w_pct(1.0).grow(1.0).min_h(0.0).scroll(sig!(self.y)).child(list(sig!(self.rows)).w_pct(1.0)))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))),
            (s == 7).then(|| column((div().w_pct(1.0).grow(1.0).min_h(0.0).scroll(sig!(self.y)).child(list(sig!(self.rows)).w_pct(1.0)), block(0.0, 40.0).w_pct(1.0).shrink(0.0).bg(rgb(0xeeeeee)))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))),
            (s == 8).then(|| column((text("header").color(rgb(0)), div().w_pct(1.0).grow(1.0).min_h(0.0).scroll(sig!(self.y)).child(list(sig!(self.rows)).w_pct(1.0)))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))),
        )
    }
}

macro_rules! vrow {
    ($name:ident, $s:ident, $body:expr) => {
        struct $name {
            v: Signal<u32>,
        }
        impl View for $name {
            fn view(&self) -> impl Element<Self> + '_ {
                let $s = self;
                $body
            }
        }
        impl From<u32> for $name {
            fn from(v: u32) -> Self {
                $name { v: Signal::new(v) }
            }
        }
    };
}
// A: the Cell row with a background; B: 32 pt tall; C: constant text; D: no height (auto); E: a bare block with bg; F: bare block no bg
vrow!(RowA, s, row((text(sig!(s.v).map(|n| n.to_string())).font(10.0, 12.0).color(rgb(0x000000)),)).w_pct(1.0).h(14.0).shrink(0.0).px(4.0).bg(rgb(0xffffff)));
vrow!(RowB, s, row((text(sig!(s.v).map(|n| n.to_string())).font(10.0, 12.0).color(rgb(0x000000)),)).w_pct(1.0).h(32.0).shrink(0.0).px(4.0));
vrow!(RowC, _s, row((text("x").font(10.0, 12.0).color(rgb(0x000000)),)).w_pct(1.0).h(14.0).shrink(0.0).px(4.0));
vrow!(RowD, s, row((text(sig!(s.v).map(|n| n.to_string())).font(10.0, 12.0).color(rgb(0x000000)),)).w_pct(1.0).px(4.0).bg(rgb(0xffffff)));
vrow!(RowE, _s, block(0.0, 14.0).w_pct(1.0).shrink(0.0).bg(rgb(0xeeeeee)));
vrow!(RowF, _s, block(0.0, 14.0).w_pct(1.0).shrink(0.0));
struct ListBoxOf<R: View> {
    rows: List<R>,
    y: Signal<f32>,
}
impl<R: View> View for ListBoxOf<R> {
    fn view(&self) -> impl Element<Self> + '_ {
        column((div().w_pct(1.0).grow(1.0).min_h(0.0).scroll(sig!(self.y)).child(list(sig!(self.rows)).w_pct(1.0)),)).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))
    }
}
fn listbox_of<R: View + From<u32>>(a: &Args, name: &str) {
    stage("mount", || {
        let mut c = cfg(a);
        c.mount_once = a.once;
        let mut app = App::new(ListBoxOf::<R> { rows: List::from_iter_in((0..a.n as u32).map(R::from)), y: Signal::new(0.0) }, c, false);
        app.frame();
        format!("row={name} rows={} records={}", a.n, app.record_count())
    });
}

// ------------------------------------------------------------------ interaction inside list rows
struct RRow {
    on: Signal<bool>,
    route: bool,
}
impl View for RRow {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            self.route.then(|| block(0.0, 20.0).w_pct(1.0).shrink(0.0).bg(sig!(self.on).map(|o| rgb(if *o { WF } else { DL }))).on_click(|v: &mut Self| {
                let x = !*v.on;
                v.on.set(x);
            })),
            (!self.route).then(|| block(0.0, 20.0).w_pct(1.0).shrink(0.0).bg(rgb(DL)).hover_bg(rgb(WH))),
        )
    }
}
struct RList {
    rows: List<RRow>,
}
impl View for RList {
    fn view(&self) -> impl Element<Self> + '_ {
        column((block(0.0, 8.0).w_pct(1.0).shrink(0.0).bg(rgb(0xeeeeee)), list(sig!(self.rows)).w_pct(1.0))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))
    }
}
fn row_interact(a: &Args, route: bool) {
    stage(if route { "row-route" } else { "row-hover" }, || {
        let mut app = App::new(RList { rows: List::from_iter_in((0..10).map(|_| RRow { on: Signal::new(false), route })) }, cfg(a), true);
        app.frame();
        let w = app.rt.win_w as usize;
        let before = px(&app.presented_pixels(), w, 640, 8 + 30);
        if route {
            app.pointer_down(640.0, 38.0, bough::raw::Button::Primary);
            app.pointer_up(640.0, 38.0, bough::raw::Button::Primary);
        } else {
            app.pointer_move(640.0, 38.0);
        }
        app.frame();
        let after = px(&app.presented_pixels(), w, 640, 8 + 30);
        format!("records={} row 1 pixel before={before:06x} after={after:06x} want {:06x}", app.record_count(), if route { WF } else { WH })
    });
}

// ------------------------------------------------------------------ structure churn
struct CRow {
    label: Signal<String>,
}
impl View for CRow {
    fn view(&self) -> impl Element<Self> + '_ {
        row((text(sig!(self.label)).font(12.0, 16.0).color(rgb(0x000000)),)).w_pct(1.0).h(20.0).shrink(0.0).px(8.0)
    }
}
fn lcg(s: &mut u64) -> usize {
    *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (*s >> 33) as usize
}
struct Churn {
    rows: List<CRow>,
    next: usize,
    rng: u64,
    y: Signal<f32>,
    cap: usize,
    mounted: bool,
}
impl Churn {
    fn new(n: usize, cap: usize, mounted: bool) -> Self {
        Churn { rows: List::from_iter_in((0..n).map(|i| CRow { label: Signal::new(format!("item {i}")) })), next: n, rng: 1, y: Signal::new(0.0), cap, mounted }
    }
}
impl Snap for Churn {
    fn snap(&self) -> Self {
        Churn { rows: List::from_iter_in(self.rows.iter().map(|r| CRow { label: Signal::new((*r.label).clone()) })), next: self.next, rng: self.rng, y: Signal::new(*self.y), cap: self.cap, mounted: self.mounted }
    }
}
impl View for Churn {
    fn view(&self) -> impl Element<Self> + '_ {
        let over = if self.mounted { self.cap } else { 2 };
        column((block(0.0, 8.0).w_pct(1.0).shrink(0.0).bg(rgb(0xeeeeee)), div().w_pct(1.0).grow(1.0).min_h(0.0).scroll(sig!(self.y)).child(list(sig!(self.rows)).w_pct(1.0).overscan(over)))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).on_key(|v: &mut Self, e| match e.key {
            Some(Key::Char('f')) => {
                v.rows.insert(0, CRow { label: Signal::new(format!("item {}", v.next)) });
                v.next += 1;
            }
            Some(Key::Char('m')) => {
                let n = v.rows.len();
                let (a, b) = (lcg(&mut v.rng) % n, lcg(&mut v.rng) % n);
                v.rows.move_to(a, b);
            }
            _ => {}
        })
    }
}
struct ChurnC {
    rows: Vec<String>,
    next: usize,
    rng: u64,
}
impl Snap for ChurnC {
    fn snap(&self) -> Self {
        ChurnC { rows: self.rows.clone(), next: self.next, rng: self.rng }
    }
}
impl View for ChurnC {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(rgb(0xffffff))
            .child(each(self.rows.iter().map(|s| row((text(s.clone()).font(12.0, 16.0).color(rgb(0x000000)),)).w_pct(1.0).h(20.0).shrink(0.0).px(8.0))))
            .on_key(|v: &mut Self, e| match e.key {
                Some(Key::Char('f')) => {
                    v.rows.insert(0, format!("item {}", v.next));
                    v.next += 1;
                }
                Some(Key::Char('m')) => {
                    let n = v.rows.len();
                    let (a, b) = (lcg(&mut v.rng) % n, lcg(&mut v.rng) % n);
                    let s = v.rows.remove(a);
                    v.rows.insert(b.min(v.rows.len()), s);
                }
                _ => {}
            })
    }
}
fn churn_input<V: View>(app: &mut App<V>, a: &Args, _: bool) {
    key(app, if a.op == "front" { 'f' } else { 'm' });
}

// ------------------------------------------------------------------ non-Latin text
const SCRIPTS: [(&str, &str); 10] = [
    ("latin", "Hello, world"),
    ("missing", "\u{10FFFD}\u{10FFFD}\u{10FFFD}\u{10FFFD}"),
    ("cjk", "\u{6F22}\u{5B57}\u{304B}\u{306A}"),
    ("arabic", "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}"),
    ("arabic-reversed", "\u{0627}\u{0628}\u{062D}\u{0631}\u{0645}"),
    ("emoji", "\u{1F600}\u{1F389}\u{1F44D}\u{1F680}"),
    ("plain-eao", "eao"),
    ("combining-eao", "e\u{0301}a\u{0308}o\u{0302}"),
    ("precomposed-eao", "\u{00E9}\u{00E4}\u{00F4}"),
    ("mixed", "abc \u{05D0}\u{05D1}\u{05D2} def"),
];
struct Scripts;
impl Snap for Scripts {
    fn snap(&self) -> Self {
        Scripts
    }
}
impl View for Scripts {
    fn view(&self) -> impl Element<Self> + '_ {
        column((each(SCRIPTS.iter().map(|(_, s)| row((text(*s).font(24.0, 32.0).color(rgb(0x000000)),)).w_pct(1.0).h(40.0).shrink(0.0).px(8.0).items(Align::Center))),)).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))
    }
}
fn scripts(a: &Args) {
    stage("ink", || {
        let mut app = mount(a, Scripts, true);
        let p = app.presented_pixels();
        let w = app.rt.win_w as usize;
        let mut out = Vec::new();
        let mut sig = Vec::new();
        for (i, (name, s)) in SCRIPTS.iter().enumerate() {
            let (mut ink, mut col, mut x0, mut x1) = (0, 0, usize::MAX, 0);
            let mut hash = 0u64;
            for y in i * 40..i * 40 + 40 {
                for x in 0..w {
                    let c = px(&p, w, x, y);
                    if c != 0xffffff {
                        ink += 1;
                        x0 = x0.min(x);
                        x1 = x1.max(x);
                        let (r, g, b) = (c >> 16 & 255, c >> 8 & 255, c & 255);
                        if r.abs_diff(g) > 24 || g.abs_diff(b) > 24 {
                            col += 1;
                        }
                        hash = hash.wrapping_mul(1099511628211).wrapping_add((c as u64) << 20 ^ ((x - x0.min(x)) as u64) ^ ((y - i * 40) as u64) << 10);
                    }
                }
            }
            sig.push((ink, x1.saturating_sub(x0)));
            out.push(format!("{name}[{} chars]: ink={ink} coloured={col} x={}..{}", s.chars().count(), if ink > 0 { x0 } else { 0 }, x1));
        }
        let _ = sig;
        let full = app.reference_pixels();
        out.push(format!("pf={}", diff(&p, &full)));
        if let Ok(dir) = std::env::var("ADV_DUMP") {
            std::fs::write(format!("{dir}/scripts-{}x{}.bgra", w, app.rt.win_h), &p).unwrap();
        }
        out.join(" | ")
    });
}

fn main() {
    let mut a = Args { case: String::new(), op: String::new(), n: 0, w: 1280.0, h: 800.0, scale: 1.0, reps: 8, k: 1, flag: false, once: false };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        let mut v = || it.next().expect("value");
        match x.as_str() {
            "--case" => a.case = v(),
            "--op" => a.op = v(),
            "--n" => a.n = v().parse().unwrap(),
            "--w" => a.w = v().parse().unwrap(),
            "--h" => a.h = v().parse().unwrap(),
            "--scale" => a.scale = v().parse().unwrap(),
            "--reps" => a.reps = v().parse().unwrap(),
            "--k" => a.k = v().parse().unwrap(),
            "--flag" => a.flag = true,
            "--once" => a.once = true,
            _ => panic!("unknown argument {x}"),
        }
    }
    std::panic::set_hook(Box::new(|i| eprintln!("panic: {i}")));
    let n = a.n;
    match a.case.as_str() {
        "wide" => {
            let cells = (a.w / 32.0).ceil() as usize;
            pairs(&a, &|| Wide { n: cells, flip: false, vertical: false }, wide_input, wide_probe)
        }
        "high" => {
            let cells = (a.h / 32.0).ceil() as usize;
            pairs(&a, &|| Wide { n: cells, flip: false, vertical: true }, wide_input, wide_probe)
        }
        "tall-block" => pairs(&a, &|| Tall { px: n as f32, y: Signal::new(0.0) }, end_input, tall_probe),
        "tall-rows" => pairs(&a, &|| Rows::new(n), end_input, rows_probe),
        "tall-virtual" => pairs(&a, &|| Virtual::new(n), end_input, virtual_probe),
        "wrap" => pairs(&a, &|| Wrap::new(n, a.flag), grow_input, no_probe),
        "wrap-coarse" => pairs(&a, &|| WrapC { rows: (0..n).map(|i| words(i, 0)).collect() }, grow_input, no_probe),
        "nested" => pairs(&a, &Nested::new, nested_input, nested_probe),
        #[cfg(feature = "deep")]
        "deep" => match n {
            10 => pairs(&a, &|| Deep10 { flip: false }, deep_input, deep_probe),
            50 => pairs(&a, &|| Deep50 { flip: false }, deep_input, deep_probe),
            100 => pairs(&a, &|| Deep100 { flip: false }, deep_input, deep_probe),
            200 => pairs(&a, &|| Deep200 { flip: false }, deep_input, deep_probe),
            _ => panic!("deep: 10, 50, 100 or 200"),
        },
        "deep-list" => pairs(&a, &|| DeepList { root: List::from_iter_in([node(n - 1)]), depth: n }, |app, _, _| key(app, 'f'), deeplist_probe),
        "signals" => pairs(&a, &|| Cells { rows: List::from_iter_in((0..n).map(|i| Cell { v: Signal::new(i as u32) })), y: Signal::new(0.0), n, k: a.k, mounted: a.flag }, |app, _, _| key(app, 'a'), no_probe),
        "fan" => pairs(&a, &|| Fan { v: Signal::new(0), on: Signal::new(false), plain: 0, n }, fan_input, fan_probe),
        "churn" => seq(&a, &|| Churn::new(n, n + a.k, a.flag), churn_input, no_probe),
        "churn-coarse" => seq(&a, &|| ChurnC { rows: (0..n).map(|i| format!("item {i}")).collect(), next: n, rng: 1 }, churn_input, no_probe),
        "listbox" => stage("mount", || {
            let app = mount(&a, ListBox { rows: List::from_iter_in((0..n).map(|i| Cell { v: Signal::new(i as u32) })), y: Signal::new(0.0), shape: a.k }, false);
            format!("shape={} rows={} records={}", a.k, n, app.record_count())
        }),
        "listbox-rows" => {
            listbox_of::<RowA>(&a, "A cell+bg");
            listbox_of::<RowB>(&a, "B cell h32");
            listbox_of::<RowC>(&a, "C const text");
            listbox_of::<RowD>(&a, "D auto height +bg");
            listbox_of::<RowE>(&a, "E block+bg");
            listbox_of::<RowF>(&a, "F block");
        }
        "row-interact" => {
            row_interact(&a, false);
            row_interact(&a, true);
        }
        "scripts" => scripts(&a),
        _ => panic!("unknown case"),
    }
}
