//! One measurement method for every framework, so the numbers share a definition.
//!
//! * `instr()`: retired user-space instructions of the CALLING THREAD (Linux: perf_event_open, exclude_kernel;
//!   macOS: libsystem_kernel thread_selfcounts, the counter bough_metal::perf::tinstr uses).
//! * The signal cases and the table operations are driven through two small traits, so a framework port only
//!   says what a write and a turn are in that framework; slopes, medians and percentiles are computed here.
//! * Every result is one JSON line on stdout (`emit`), merged later into matrix.json.
#![allow(dead_code)]

pub mod spec;
mod counter;
pub use counter::{instr, instr_overhead, rss_bytes};

use std::time::Instant;

/// The machine label of this process. Numbers from different machines never share an axis unlabelled.
pub fn machine() -> String {
    if let Ok(m) = std::env::var("CMP_MACHINE") {
        return m;
    }
    if cfg!(target_os = "macos") {
        "mac-m3pro-aarch64-macos".into()
    } else {
        "ilo-prod-ryzen7-3700x-x86_64-linux".into()
    }
}

pub struct Ctx {
    pub framework: String,
    pub version: String,
    pub machine: String,
    pub build: String,
}

impl Ctx {
    pub fn new(framework: &str, version: &str, build: &str) -> Self {
        Self { framework: framework.into(), version: version.into(), machine: machine(), build: build.into() }
    }
    /// One result row as a JSON line.
    pub fn emit(&self, case: &str, variant: &str, n: usize, metric: &str, value: f64, unit: &str, runs: &str, notes: &str) {
        println!(
            "{{\"case\":{},\"framework\":{},\"variant\":{},\"n\":{},\"metric\":{},\"value\":{},\"unit\":{},\"machine\":{},\"version\":{},\"build\":{},\"kind\":\"measured\",\"basis\":\"\",\"runs\":{},\"notes\":{}}}",
            js(case), js(&self.framework), js(variant), n, js(metric), num(value), js(unit), js(&self.machine), js(&self.version), js(&self.build), js(runs), js(notes)
        );
    }
}

fn num(v: f64) -> String {
    if v.is_finite() { if v == v.trunc() && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v:.3}") } } else { "null".into() }
}
pub fn js(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

pub fn pct(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p).round() as usize]
}

/// bough's `signal_cost` slope: instructions per repetition by difference of two repeat counts, which removes
/// every constant (counter read, loop setup). `run(reps)` returns the instructions of `reps` repetitions.
pub fn slope(mut run: impl FnMut(u32) -> u64) -> f64 {
    run(8);
    let (lo, hi) = (run(64), run(192));
    (hi.saturating_sub(lo)) as f64 / 128.0
}

// ---------------------------------------------------------------------------------------------------------
// The signal case (bough's crates/bough/examples/signal_cost.rs): N rows, each with a colour, a numeric
// label and a string label bound to its own state; one write per turn; cells colour/number/string/idle.

#[derive(Clone, Copy, Debug)]
pub enum Cell {
    Colour,
    Number,
    String,
    Idle,
}
impl Cell {
    pub const ALL: [Cell; 4] = [Cell::Colour, Cell::Number, Cell::String, Cell::Idle];
    pub fn name(self) -> &'static str {
        match self {
            Cell::Colour => "colour",
            Cell::Number => "number",
            Cell::String => "string",
            Cell::Idle => "idle",
        }
    }
}

/// A framework's port of the signal case. `turn_*` performs ONE write and then everything the framework does
/// before it would hand the frame to the GPU (state propagation, rebuild/diff, style, layout, paint recording),
/// returning when the next consistent frame description exists. No rasterisation and no present.
pub trait SignalApp {
    fn mount(n: usize) -> Self;
    /// Row `i` gets colour `0xff000000 | (k & 0xffffff)`.
    fn turn_colour(&mut self, i: usize, k: u32);
    /// Row `i`'s numeric label becomes `k % 1000`, drawn as an integer.
    fn turn_number(&mut self, i: usize, k: u32);
    /// Row `i`'s string label becomes `format!("Row label {}", k % 1000)`.
    fn turn_string(&mut self, i: usize, k: u32);
    /// A turn that writes nothing: the framework's own "redraw / update with no change".
    fn turn_idle(&mut self);
}

/// Runs the four cells at each size, `reps` independent processes are the caller's business.
pub fn run_signal<A: SignalApp>(cx: &Ctx, variant: &str, sizes: &[usize], extra: &str) {
    for &n in sizes {
        let before = instr();
        let t0 = Instant::now();
        let mut a = A::mount(n);
        let mount_instr = instr() - before;
        let mount_ns = t0.elapsed().as_nanos() as f64;
        cx.emit("signal.mount", variant, n, "instructions", mount_instr as f64, "instructions", "1", "construction + first frame description");
        cx.emit("signal.mount", variant, n, "wall", mount_ns, "ns", "1", "");
        let mut k = 0u32;
        for cell in Cell::ALL {
            let mut step = |a: &mut A, k: &mut u32| {
                *k = k.wrapping_add(1);
                let i = (*k as usize * 7) % n;
                match cell {
                    Cell::Colour => a.turn_colour(i, *k),
                    Cell::Number => a.turn_number(i, *k),
                    Cell::String => a.turn_string(i, *k),
                    Cell::Idle => a.turn_idle(),
                }
            };
            // instructions: bough's slope
            let ins = slope(|reps| {
                let t = instr();
                for _ in 0..reps {
                    step(&mut a, &mut k);
                }
                instr() - t
            });
            // wall: per-turn p50 / p99 over many turns (after a warm-up), no counter reads inside the loop
            let turns = if n >= 256 { 1500 } else { 3000 };
            for _ in 0..50 {
                step(&mut a, &mut k);
            }
            let mut w: Vec<f64> = Vec::with_capacity(turns);
            for _ in 0..turns {
                let t = Instant::now();
                step(&mut a, &mut k);
                w.push(t.elapsed().as_nanos() as f64);
            }
            let case = format!("signal.{}", cell.name());
            cx.emit(&case, variant, n, "instructions_per_update", ins, "instructions/turn", "slope(64,192)/128", extra);
            cx.emit(&case, variant, n, "wall_p50", pct(&mut w, 0.5), "ns/turn", &format!("{turns} turns"), extra);
            cx.emit(&case, variant, n, "wall_p99", pct(&mut w, 0.99), "ns/turn", &format!("{turns} turns"), extra);
        }
        cx.emit("signal.rss_after_mount", variant, n, "rss", rss_bytes() as f64, "bytes", "process", "process resident set at the end of the cell, includes the harness");
    }
}

// ---------------------------------------------------------------------------------------------------------
// gui-bench table operations, driven like bough's comparisons/bough-guibench driver: an operation and its undo
// per step, instructions per operation = block / (2 * repeats), median of blocks.

pub trait TableApp {
    /// The framework's table app (coarse or fine variant), `n` rows, mounted and settled.
    fn mount(n: usize) -> Self;
    /// Apply `op` (or its undo) through the framework's normal update path and run everything up to the point
    /// where the framework would hand the frame to the GPU. `op` is one of: select, update10, swap, remove,
    /// insert, replace, append, truncate, clear (remove/insert and append/truncate undo each other;
    /// select toggles).
    fn op(&mut self, op: &str, undo: bool);
}

pub fn run_ops<A: TableApp>(cx: &Ctx, variant: &str, n: usize, ops: &[&str], repeats: usize, blocks: usize, extra: &str) {
    let before = instr();
    let t0 = Instant::now();
    let mut a = A::mount(n);
    cx.emit("table.mount", variant, n, "instructions", (instr() - before) as f64, "instructions", "1", "construction + first frame description");
    cx.emit("table.mount", variant, n, "wall", t0.elapsed().as_nanos() as f64, "ns", "1", "");
    for &op in ops {
        let mut step = |a: &mut A| {
            for undo in [false, true] {
                a.op(op, undo);
            }
        };
        for _ in 0..2 {
            step(&mut a);
        }
        let mut per = Vec::new();
        for _ in 0..blocks {
            let s = instr();
            for _ in 0..repeats {
                step(&mut a);
            }
            per.push(((instr() - s) / (2 * repeats as u64)) as f64);
        }
        let (lo, hi) = (per.iter().cloned().fold(f64::MAX, f64::min), per.iter().cloned().fold(0.0, f64::max));
        let med = pct(&mut per, 0.5);
        let case = format!("table.{op}");
        cx.emit(&case, variant, n, "instructions_per_op", med, "instructions/op", &format!("median of {blocks} blocks x {repeats} pairs; lo={lo} hi={hi}"), extra);
        let mut w: Vec<f64> = Vec::new();
        for _ in 0..(repeats * blocks).max(16) {
            for undo in [false, true] {
                let t = Instant::now();
                a.op(op, undo);
                w.push(t.elapsed().as_nanos() as f64);
            }
        }
        cx.emit(&case, variant, n, "wall_p50", pct(&mut w, 0.5), "ns/op", &format!("{} ops", w.len()), extra);
        cx.emit(&case, variant, n, "wall_p99", pct(&mut w, 0.99), "ns/op", &format!("{} ops", w.len()), extra);
    }
    cx.emit("table.rss_after_run", variant, n, "rss", rss_bytes() as f64, "bytes", "process", "process resident set after the operations");
}
