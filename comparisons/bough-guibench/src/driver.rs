//! Harness-only escapes, enumerated in docs/lanes/generalization/FRICTION.md.
use crate::{
    model::{Coarse, Fine, Rows, f_key},
    spec,
};
use bough::prelude::*;
use bough::raw::{App, Button, Caps, Config};
use bough::sys::perf::{snap, tinstr};
use std::time::Instant;

pub fn config(scale: f32) -> Config {
    // BOUGH_FONT, ZGX_FONT_FILE, then the platform's sans-serif (macOS Arial; Linux fontconfig's `sans-serif`)
    let font = bough::ui::default_font();
    let mut c = Config::new(spec::WINDOW.0, spec::WINDOW.1, scale, font);
    // Exactly ui.rs::APP_CAPS, not Config::new's smaller Caps::DEFAULT (G5).
    c.caps = Caps {
        instances: 1 << 16,
        pages: 1 << 13,
        glyphs: 1 << 19,
        runs: 1 << 16,
        spaces: 1 << 12,
        clips: 1 << 12,
        strikes: 16,
    };
    apply_caps(&mut c.caps);
    c
}

/// `GUIBENCH_CAPS`: `default`, `raised-spaces` (spaces = 16,384) or `name=value,...`. Never implicit.
pub fn apply_caps(caps: &mut Caps) {
    let Ok(v) = std::env::var("GUIBENCH_CAPS") else {
        return;
    };
    match v.as_str() {
        "" | "default" => {}
        "raised-spaces" => caps.spaces = 1 << 14,
        list => {
            for kv in list.split(',') {
                let (k, n) = kv.split_once('=').expect("caps name=value");
                let n: usize = n.parse().expect("caps value");
                match k {
                    "instances" => caps.instances = n,
                    "pages" => caps.pages = n,
                    "glyphs" => caps.glyphs = n,
                    "runs" => caps.runs = n,
                    "spaces" => caps.spaces = n,
                    "clips" => caps.clips = n,
                    "strikes" => caps.strikes = n,
                    _ => panic!("unknown cap {k}"),
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct Opts {
    pub repeats: usize,
    pub render: bool,
    pub gate: bool,
    pub ring: usize,
    /// > 0: two warm-up pairs, then this many timed blocks of `repeats` operation/undo pairs.
    pub blocks: usize,
    pub scale: f32,
}

pub trait Bench: View {
    fn copy_state(&self) -> Self;
    fn data(&self) -> Vec<spec::Row>;
    fn selection(&self) -> Option<u64> {
        None
    }
    fn clock(&mut self) {}
    fn period(&self) -> Option<f64> {
        None
    }
    fn ticks(&self) -> u64 {
        0
    }
    fn updates(&self) -> usize {
        0
    }
    fn query(&self) -> Option<&str> {
        None
    }
    fn header() -> bool;
    fn rows(&self) -> usize;
}
impl Bench for Rows {
    fn copy_state(&self) -> Self {
        Self {
            count: self.count,
            y: Signal::new(*self.y),
        }
    }
    fn data(&self) -> Vec<spec::Row> {
        vec![]
    }
    fn header() -> bool {
        false
    }
    fn rows(&self) -> usize {
        self.count
    }
}
impl Bench for Coarse {
    fn updates(&self) -> usize {
        self.timer.map_or(0, |t| t.updates)
    }
    fn clock(&mut self) {
        self.clock();
    }
    fn period(&self) -> Option<f64> {
        self.timer.map(|t| t.interval_ms as f64 / 1000.0)
    }
    fn ticks(&self) -> u64 {
        *self.ticks
    }

    fn copy_state(&self) -> Self {
        self.snapshot()
    }
    fn data(&self) -> Vec<spec::Row> {
        self.rows.clone()
    }
    fn selection(&self) -> Option<u64> {
        self.selected
    }
    fn header() -> bool {
        true
    }
    fn rows(&self) -> usize {
        self.rows.len()
    }
}
impl Bench for Fine {
    fn updates(&self) -> usize {
        self.timer.map_or(0, |t| t.updates)
    }
    fn clock(&mut self) {
        self.clock();
    }
    fn period(&self) -> Option<f64> {
        self.timer.map(|t| t.interval_ms as f64 / 1000.0)
    }
    fn ticks(&self) -> u64 {
        *self.ticks
    }

    fn copy_state(&self) -> Self {
        self.snapshot()
    }
    fn data(&self) -> Vec<spec::Row> {
        self.data()
    }
    fn selection(&self) -> Option<u64> {
        self.selected_id()
    }
    fn header() -> bool {
        true
    }
    fn rows(&self) -> usize {
        self.rows.len()
    }
}

impl Bench for crate::scenarios::Search {
    fn copy_state(&self) -> Self {
        self.snapshot()
    }
    fn data(&self) -> Vec<spec::Row> {
        self.rows.clone()
    }
    fn rows(&self) -> usize {
        self.rows.len()
    }
    fn header() -> bool {
        true
    }
    fn query(&self) -> Option<&str> {
        Some(&self.query)
    }
}
impl Bench for crate::scenarios::Spinner {
    fn copy_state(&self) -> Self {
        self.snapshot()
    }
    fn data(&self) -> Vec<spec::Row> {
        self.rows.clone()
    }
    fn rows(&self) -> usize {
        self.rows.len()
    }
    fn header() -> bool {
        true
    }
    fn clock(&mut self) {
        self.clock();
    }
    fn period(&self) -> Option<f64> {
        Some(0.016)
    }
    fn ticks(&self) -> u64 {
        self.ticks
    }
}
impl Bench for crate::scenarios::Virtual {
    fn copy_state(&self) -> Self {
        Self {
            count: self.count,
            y: Signal::new(*self.y),
        }
    }
    fn data(&self) -> Vec<spec::Row> {
        vec![]
    }
    fn rows(&self) -> usize {
        self.count
    }
    fn header() -> bool {
        false
    }
}

/// Same callback used by the native NSTimer and the headless clock input.
pub fn deliver_clock<V: Bench>(a: &mut App<V>) {
    a.turn(|v| v.clock());
}

pub fn focus<V: Bench>(a: &mut App<V>) {
    if V::header() {
        let Some(pos) = (0..a.record_count()).find(|&p| a.record_view(p).kind & 0x40 != 0) else {
            return;
        };
        let handle = a.rt.bough.handle_at(pos);
        a.set_focus(handle, false);
        if a.owner.query().is_some() {
            a.key_down(Key::End, Mods::default());
        }
    }
}
pub fn window<V: Bench>(view: V, title: &str) -> ! {
    let seconds = std::env::var("GUIBENCH_SECONDS")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(0.0);
    let period = view.period();
    let mut run = bough::backend::Run::default()
        .seconds(seconds)
        .app_id("gui-bench.bough")
        .start(|a: &mut App<V>| focus(a))
        .report(|a: &mut App<V>| {
            eprintln!(
                "window frames={} records={} ticks={}",
                a.frames,
                a.record_count(),
                a.owner.ticks()
            );
            if std::env::var_os("GUIBENCH_EXPECT_TICKS").is_some() {
                assert!(
                    a.owner.ticks() > 0,
                    "timer integration: callback owns live app"
                );
            }
        });
    if let Some(p) = period {
        run = run.every(p, |a: &mut App<V>| deliver_clock(a));
    }
    bough::backend::run(title, config, view, run)
}

pub fn input<V: Bench>(a: &mut App<V>, op: &str, undo: bool) {
    input_op(a, op, undo);
    // Diagnostic only (never in measured cells): re-deliver the pointer after a scroll, at the same
    // point (`same`) or one point away and back (`jitter`), to separate stale hover from stale content.
    if matches!(op, "scroll" | "jump") {
        match std::env::var("GUIBENCH_REHOVER").as_deref() {
            Ok("same") => a.pointer_move(640.0, 400.0),
            Ok("jitter") => {
                a.pointer_move(700.0, 100.0);
                a.pointer_move(640.0, 400.0);
            }
            _ => {}
        }
    }
}
fn input_op<V: Bench>(a: &mut App<V>, op: &str, undo: bool) {
    match op {
        "select" => {
            let y = spec::HEADER_HEIGHT + spec::ROW_HEIGHT / 2.0;
            a.pointer_down(640.0, y, Button::Primary);
            a.pointer_up(640.0, y, Button::Primary);
        }
        "hover" => a.pointer_move(640.0, if undo { 16.0 } else { 5.0 * 32.0 + 16.0 }),
        "scroll" => a.wheel_points(640.0, 400.0, if undo { -16.0 } else { 16.0 }),
        "jump" => {
            let d = a.owner.rows() as f32 * spec::ROW_HEIGHT / 2.0;
            a.wheel_points(640.0, 400.0, if undo { -d } else { d })
        }
        "update10" => a.key_down(f_key(1), Mods::default()),
        "swap" => a.key_down(f_key(2), Mods::default()),
        "remove" => a.key_down(f_key(if undo { 4 } else { 3 }), Mods::default()),
        "insert" => a.key_down(f_key(if undo { 3 } else { 4 }), Mods::default()),
        "replace" => a.key_down(f_key(5), Mods::default()),
        "append" => a.key_down(f_key(if undo { 7 } else { 6 }), Mods::default()),
        "truncate" => a.key_down(f_key(if undo { 6 } else { 7 }), Mods::default()),
        "clear" => a.key_down(f_key(8), Mods::default()),
        "replace-select" => {
            if !undo {
                a.key_down(f_key(5), Mods::default());
            }
            a.pointer_down(640.0, 56.0, Button::Primary);
            a.pointer_up(640.0, 56.0, Button::Primary);
        }
        "ticker" | "spinner" => deliver_clock(a),
        "typing" | "filter" | "filter-nochange" => a.key_down(
            if undo {
                Key::Backspace
            } else {
                Key::Char(if op == "filter" { 'z' } else { 'a' })
            },
            Mods::default(),
        ),
        "resize" => a.resize(
            if undo { 1280.0 } else { 640.0 },
            if undo { 800.0 } else { 480.0 },
        ),
        "scale" => panic!("UNSUPPORTED: no live scale-change host entry point"),
        "mount" => (),
        _ => panic!("unsupported operation {op}"),
    }
}
fn spec_op(op: &str, undo: bool) -> Option<spec::Op> {
    use spec::Op::*;
    Some(match op {
        "update10" => Update10,
        "swap" => Swap,
        "remove" => {
            if undo {
                Insert
            } else {
                Remove
            }
        }
        "insert" => {
            if undo {
                Remove
            } else {
                Insert
            }
        }
        "replace" => Replace,
        "replace-select" if !undo => Replace,
        "append" => {
            if undo {
                Truncate
            } else {
                Append
            }
        }
        "truncate" => {
            if undo {
                Append
            } else {
                Truncate
            }
        }
        "clear" => Clear,
        _ => return None,
    })
}
fn compare(a: &[u8], b: &[u8], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: length");
    let wrong = a.iter().zip(b).filter(|(a, b)| a != b).count();
    if wrong != 0 {
        if let Ok(dir) = std::env::var("GUIBENCH_FAILURE_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(format!("{dir}/{what}-actual.bgra"), a).unwrap();
            std::fs::write(format!("{dir}/{what}-expected.bgra"), b).unwrap();
        }
        panic!(
            "{what}: {wrong} differing bytes; first {:?}",
            a.iter().zip(b).position(|(a, b)| a != b)
        );
    }
}
fn pixels<V: Bench>(a: &mut App<V>, ring: usize) {
    let shown = a.presented_pixels();
    let retained = a.reference_pixels();
    compare(&shown, &retained, "presented-full");
    let mut c = config(a.rt.scale);
    c.width_pt = a.rt.win_w as f32 / a.rt.scale;
    c.height_pt = a.rt.win_h as f32 / a.rt.scale;
    c.ring = ring;
    let mut fresh = App::new(a.owner.copy_state(), c, true);
    focus(&mut fresh);
    fresh.frame();
    // Reproduce transient hover through the input path (row data/scroll are copied above).
    if !V::header() {
        if let Some((x, y)) = a.rt.input.pointer {
            fresh.pointer_move(x as f32 / a.rt.scale, y as f32 / a.rt.scale);
        }
    }
    compare(&retained, &fresh.reference_pixels(), "retained-fresh");
}
fn layout<V: Bench>(a: &mut App<V>) {
    let p = a.presented_pixels();
    let checks = if a.owner.rows() < 2 {
        if V::header() {
            vec![(640, 20, spec::HEADER_BG), (640, 784, spec::ROW_EVEN)]
        } else {
            vec![(640, 784, spec::ROW_EVEN)]
        }
    } else if V::header() {
        vec![
            (640, 20, spec::HEADER_BG),
            (640, 56, spec::ROW_EVEN),
            (640, 88, spec::ROW_EVEN),
            (640, 71, spec::ROW_BORDER),
        ]
    } else {
        vec![
            (640, 16, spec::ROW_EVEN),
            (640, 48, spec::ROW_ODD),
            (640, 31, spec::ROW_BORDER),
        ]
    };
    let (k, w) = (a.rt.scale as usize, a.rt.win_w as usize);
    for (x, y, color) in checks {
        // points to device pixels (the centre of the point's k x k block)
        let off = ((y * k + k / 2) * w + x * k + k / 2) * 4;
        let (r, g, b) = spec::rgb(color);
        assert_eq!(&p[off..off + 4], &[b, g, r, 255], "layout point {x},{y}");
    }
}

pub fn headless<V: Bench>(view: V, op: &str, o: Opts) {
    let Opts {
        repeats,
        render,
        gate,
        ring,
        blocks,
        scale,
    } = o;
    let before = tinstr();
    let t = Instant::now();
    let mut c = config(scale);
    c.ring = ring;
    let mut a = App::new(view, c, render || gate);
    focus(&mut a);
    a.frame();
    let mounted = snap();
    eprintln!(
        "mount instructions={} ns={} rss={} footprint={} records={} rows={}",
        tinstr() - before,
        t.elapsed().as_nanos(),
        mounted.rss,
        mounted.footprint,
        a.record_count(),
        a.owner.rows()
    );
    if gate {
        layout(&mut a);
    }
    if op == "truncate" {
        a.key_down(f_key(6), Mods::default());
        a.frame();
    }
    if op == "filter-nochange" {
        a.key_down(Key::Char('z'), Mods::default());
        a.frame();
    }
    if blocks > 0 {
        // Diagnostic only: GUIBENCH_IDLE=k delivers k further frames (idle unless something animates)
        // after each operation's frame, which is where `Rt::reclaim` runs for input-driven turns.
        let idle: usize = std::env::var("GUIBENCH_IDLE")
            .ok()
            .map_or(0, |v| v.parse().unwrap());
        let step = |a: &mut App<V>| {
            for undo in [false, true] {
                input(a, op, undo);
                if op == "ticker" || op == "spinner" {
                    a.frame_paced();
                } else {
                    a.frame();
                }
                for _ in 0..idle {
                    a.frame();
                }
            }
        };
        for _ in 0..2 {
            step(&mut a);
        }
        if render {
            a.wait_presented();
        }
        let mut per = Vec::new();
        for _ in 0..blocks {
            let s = tinstr();
            for _ in 0..repeats {
                step(&mut a);
            }
            if render {
                a.wait_presented();
            }
            per.push((tinstr() - s) / (2 * repeats as u64));
        }
        per.sort();
        println!(
            "perop op={op} render={render} med={} lo={} hi={} records={} frames={}",
            per[per.len() / 2],
            per[0],
            per[per.len() - 1],
            a.record_count(),
            a.frames
        );
        if std::env::var_os("BOUGH_STATS").is_some() {
            eprintln!("stats repaired_tiles(last)={} surfaces={:?} cpu={:?}", a.repaired_tiles, a.surf_stats, a.cpu.stats);
        }
        return;
    }
    let mut expected = a.owner.data();
    let count = if op == "truncate" {
        expected.len() - spec::APPEND
    } else {
        expected.len()
    };
    let mut ids = spec::Ids::default();
    ids.rows(count);
    if op == "truncate" {
        ids.rows(spec::APPEND);
    }
    let start = tinstr();
    for _ in 0..repeats {
        for undo in [false, true] {
            input(&mut a, op, undo);
            if op == "ticker" || op == "spinner" {
                a.frame_paced();
            } else {
                a.frame();
            }
            if gate {
                if op == "ticker" {
                    let tick = a.owner.ticks();
                    let updates = a.owner.updates();
                    for i in spec::ticker_positions(expected.len(), updates) {
                        expected[i].value = spec::ticker_value(expected[i].id, tick);
                    }
                }
                if let Some(op) = spec_op(op, undo) {
                    op.apply(&mut expected, &mut ids, count);
                }
                assert_eq!(
                    a.owner.data(),
                    expected,
                    "semantic state after {op}, undo={undo}"
                );
                if op == "select" || op == "replace-select" {
                    assert_eq!(
                        a.owner.selection(),
                        if undo {
                            None
                        } else {
                            expected.first().map(|r| r.id)
                        }
                    );
                }
                if matches!(op, "typing" | "filter" | "filter-nochange") {
                    let expected_query = if undo {
                        if op == "filter-nochange" { "z" } else { "" }
                    } else {
                        if op == "filter" {
                            "z"
                        } else if op == "filter-nochange" {
                            "za"
                        } else {
                            "a"
                        }
                    };
                    assert_eq!(
                        a.owner.query(),
                        Some(expected_query),
                        "text input semantics"
                    );
                }
                if op == "hover" {
                    let p = a.presented_pixels();
                    let (k, w) = (a.rt.scale as usize, a.rt.win_w as usize);
                    let yy = if undo { 16 } else { 176 };
                    let i = (yy * k * w + 640 * k) * 4;
                    let (r, g, b) = spec::rgb(spec::ROW_HOVER);
                    assert_eq!(&p[i..i + 4], &[b, g, r, 255], "expected hover colour");
                }
                pixels(&mut a, ring);
            }
        }
    }
    if render || gate {
        a.wait_presented();
    }
    let instruction_delta = tinstr() - start;
    let end = snap();
    println!(
        "result op={op} repeats={repeats} render={render} gate={gate} ring={ring} instructions={} rss={} footprint={} frames={} surfaces={:?}",
        instruction_delta, end.rss, end.footprint, a.frames, a.surf_stats.uses
    );
}
