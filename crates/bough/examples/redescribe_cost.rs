//! Round 6 (lead): what one re-description costs. A plain column of N rows (a tinted box holding two labels),
//! one plain field (`selected`) changed per turn, no frame: instructions per `App::turn` on the calling
//! thread, by slope over two repeat counts. Three row shapes: `owned` (the row's id and its label, borrowed:
//! what application code writes), `static` (two `&'static str`), `bare` (the box alone). `once` is the owned
//! shape mounted with `Config::once()` (its output goes stale: a cost control only).
//!
//!     cargo run --release -p bough --example redescribe_cost -- [rows...]
use bough::prelude::*;
use bough::raw::{App, Caps, Config};
use bough_sys::perf::tinstr;

struct Table<const K: u32> {
    rows: Vec<(u32, String)>,
    selected: u32,
}

fn tint<const K: u32>(t: &Table<K>, i: usize) -> Color {
    if t.rows[i].0 == t.selected { rgb(0x2f6bff) } else if i % 2 == 0 { rgb(0xf5f5f5) } else { rgb(0xe6e6e6) }
}

impl View for Table<0> {
    fn view(&self) -> impl Element<Self> + '_ {
        let line = move |i: usize| {
            let (id, label) = &self.rows[i];
            row((text(*id).w(64.0), text(label.as_str()))).w(960.0).h(32.0).bg(tint(self, i))
        };
        div().bg(rgb(0xffffff)).child(each((0..self.rows.len()).map(line)))
    }
}
impl View for Table<1> {
    fn view(&self) -> impl Element<Self> + '_ {
        let line = move |i: usize| row((text("12345").w(64.0), text("Row label 12345"))).w(960.0).h(32.0).bg(tint(self, i));
        div().bg(rgb(0xffffff)).child(each((0..self.rows.len()).map(line)))
    }
}
impl View for Table<2> {
    fn view(&self) -> impl Element<Self> + '_ {
        let line = move |i: usize| row(()).w(960.0).h(32.0).bg(tint(self, i));
        div().bg(rgb(0xffffff)).child(each((0..self.rows.len()).map(line)))
    }
}

fn measure<const K: u32>(n: usize, once: bool) -> u64
where
    Table<K>: View,
{
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut cfg = Config::new(1280.0, 800.0, 1.0, font);
    cfg.caps = Caps { instances: 1 << 17, pages: 1 << 13, glyphs: 1 << 20, runs: 1 << 17, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
    if once {
        cfg = cfg.once();
    }
    let rows = (0..n as u32).map(|i| (i, format!("Row label {i}"))).collect();
    let mut a = App::new(Table::<K> { rows, selected: u32::MAX }, cfg, false);
    a.turn(|_| {});
    let nochange = std::env::var_os("NOCHANGE").is_some();
    let mut run = |reps: u32| {
        let t0 = tinstr();
        for k in 0..reps {
            if nochange {
                a.turn(|t| t.selected = std::hint::black_box(t.selected));
            } else {
                a.turn(|t| t.selected = k % n as u32);
            }
        }
        tinstr() - t0
    };
    run(8);
    if let Some(n) = std::env::var("SPIN").ok().and_then(|v| v.parse::<u32>().ok()) {
        // profiling: nothing but steady-state turns
        run(n);
    }
    let (lo, hi) = (run(16), run(48));
    (hi - lo) / 32
}

/// Instructions per (remove the row at index 2, insert it back) pair, the application row shape.
fn churn(n: usize) -> u64 {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut cfg = Config::new(1280.0, 800.0, 1.0, font);
    cfg.caps = Caps { instances: 1 << 17, pages: 1 << 13, glyphs: 1 << 20, runs: 1 << 17, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
    let rows = (0..n as u32).map(|i| (i, format!("Row label {i}"))).collect();
    let mut a = App::new(Table::<0> { rows, selected: u32::MAX }, cfg, false);
    a.turn(|_| {});
    let mut run = |reps: u32| {
        let t0 = tinstr();
        for _ in 0..reps {
            let mut gone = None;
            a.turn(|t| gone = Some(t.rows.remove(2)));
            a.turn(|t| t.rows.insert(2, gone.take().unwrap()));
        }
        tinstr() - t0
    };
    run(2);
    if let Some(n) = std::env::var("SPIN").ok().and_then(|v| v.parse::<u32>().ok()) {
        run(n);
    }
    let (lo, hi) = (run(4), run(12));
    (hi - lo) / 8
}

/// Instructions to construct an `App` over `n` rows of shape `K` (headless: no GPU), minus the same for 0 rows.
fn mount<const K: u32>(n: usize) -> u64
where
    Table<K>: View,
{
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let one = |n: usize| {
        let mut cfg = Config::new(1280.0, 800.0, 1.0, font.clone());
        cfg.caps = Caps { instances: 1 << 17, pages: 1 << 13, glyphs: 1 << 20, runs: 1 << 17, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
        let rows: Vec<(u32, String)> = (0..n as u32).map(|i| (i, format!("Row label {i}"))).collect();
        let t0 = tinstr();
        let mut a = App::new(Table::<K> { rows, selected: u32::MAX }, cfg, false);
        a.turn(|_| {});
        let d = tinstr() - t0;
        if std::env::var_os("SPIN").is_none() {
            std::mem::forget(a); // teardown is measured apart
        }
        d
    };
    one(1);
    let (z, f) = (one(0), one(n));
    if K == 0 {
        eprintln!("  an empty App (new + first turn, headless): {z} instructions");
    }
    f.saturating_sub(z)
}

fn main() {
    if std::env::var_os("MOUNT").is_some() {
        println!("rows,owned_mount,per_row,static_mount,per_row,bare_mount,per_row");
        for n in std::env::args().skip(1).filter_map(|s| s.parse::<usize>().ok()) {
            if let Some(reps) = std::env::var("SPIN").ok().and_then(|v| v.parse::<u32>().ok()) {
                for _ in 0..reps {
                    mount::<0>(n);
                }
            }
            let (o, s, b) = (mount::<0>(n), mount::<1>(n), mount::<2>(n));
            let k = n as u64;
            println!("{n},{o},{},{s},{},{b},{}", o / k, s / k, b / k);
        }
        return;
    }
    if std::env::var_os("CHURN").is_some() {
        println!("rows,remove_insert_pair,per_row");
        for n in std::env::args().skip(1).filter_map(|s| s.parse::<usize>().ok()) {
            let c = churn(n);
            println!("{n},{c},{}", c / n as u64);
        }
        return;
    }
    let sizes: Vec<usize> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let sizes = if sizes.is_empty() { vec![10, 100, 1000] } else { sizes };
    println!("rows,owned,owned_per_row,static,static_per_row,bare,bare_per_row,once");
    for n in sizes {
        let (o, s, b, c) = (measure::<0>(n, false), measure::<1>(n, false), measure::<2>(n, false), measure::<0>(n, true));
        let k = n as u64;
        println!("{n},{o},{},{s},{},{b},{},{c}", o / k, s / k, b / k);
    }
}
