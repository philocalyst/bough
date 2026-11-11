//! Harness-only escapes, enumerated in docs/lanes/generalization/FRICTION.md.
use crate::{
    model::{Coarse, Fine, Rows, f_key},
    spec,
};
use bough::prelude::*;
use bough::raw::{App, Button, Caps, Config};
use bough_metal::perf::{snap, tinstr};
use std::time::Instant;

pub fn config(scale: f32) -> Config {
    let font = [
        std::env::var("BOUGH_FONT").ok(),
        std::env::var("ZGX_FONT_FILE").ok(),
        Some("/System/Library/Fonts/Supplemental/Arial.ttf".into()),
    ]
    .into_iter()
    .flatten()
    .find_map(|p| std::fs::read(p).ok())
    .expect("font; set BOUGH_FONT");
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
    if std::env::var("GUIBENCH_CAPS").as_deref() == Ok("raised-spaces") {
        c.caps.spaces = 1 << 14;
    }
    c
}

pub trait Bench: View {
    fn copy_state(&self) -> Self;
    fn data(&self) -> Vec<spec::Row>;
    fn selection(&self) -> Option<u64> {
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

pub fn focus<V: Bench>(a: &mut App<V>) {
    if V::header() {
        let pos = (0..a.record_count())
            .find(|&p| a.record_view(p).kind & 0x40 != 0)
            .expect("root key listener is focusable");
        let handle = a.rt.bough.handle_at(pos);
        a.set_focus(handle, false);
    }
}
pub fn window<V: Bench>(view: V, title: &str) -> ! {
    let seconds = std::env::var("GUIBENCH_SECONDS")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(0.0);
    bough::backend::host::run_interactive(
        title,
        config,
        view,
        seconds,
        |_, a| {
            focus(&mut a.borrow_mut());
        },
        |a| {
            eprintln!("window frames={} records={}", a.frames, a.record_count());
        },
    );
}

pub fn input<V: Bench>(a: &mut App<V>, op: &str, undo: bool) {
    match op {
        "select" => {
            let y = spec::HEADER_HEIGHT + spec::ROW_HEIGHT / 2.0;
            a.pointer_down(640.0, y, Button::Primary);
            a.pointer_up(640.0, y, Button::Primary);
        }
        "hover" => a.pointer_move(640.0, if undo { 16.0 } else { 5.0 * 32.0 + 16.0 }),
        "scroll" => a.wheel_points(640.0, 400.0, if undo { -16.0 } else { 16.0 }),
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
    let mut c = config(1.0);
    c.ring = ring;
    let mut fresh = App::new(a.owner.copy_state(), c, true);
    focus(&mut fresh);
    fresh.frame();
    // Reproduce transient hover through the input path (row data/scroll are copied above).
    if !V::header() {
        if let Some((x, y)) = a.rt.input.pointer {
            fresh.pointer_move(x as f32, y as f32);
        }
    }
    compare(&retained, &fresh.reference_pixels(), "retained-fresh");
}
fn layout<V: Bench>(a: &mut App<V>) {
    let p = a.presented_pixels();
    let checks = if V::header() {
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
    for (x, y, color) in checks {
        let off = (y * 1280 + x) * 4;
        let (r, g, b) = spec::rgb(color);
        assert_eq!(&p[off..off + 4], &[b, g, r, 255], "layout point {x},{y}");
    }
}

pub fn headless<V: Bench>(
    view: V,
    op: &str,
    repeats: usize,
    render: bool,
    gate: bool,
    ring: usize,
) {
    let before = tinstr();
    let t = Instant::now();
    let mut c = config(1.0);
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
            a.frame();
            if gate {
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
                if op == "hover" {
                    let p = a.presented_pixels();
                    let yy = if undo { 16 } else { 176 };
                    let i = (yy * 1280 + 640) * 4;
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
