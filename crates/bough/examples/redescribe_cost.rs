//! Round 6 (lead): what one re-description costs. A plain column of N rows (a tinted box holding two labels),
//! one plain field (`selected`) changed per turn, no frame: instructions per `App::turn` on the calling
//! thread, by slope over two repeat counts. Three row shapes: `owned` (the row's id and its label, borrowed:
//! what application code writes), `static` (two `&'static str`), `bare` (the box alone). `once` is the owned
//! shape mounted with `Config::once()` (its output goes stale: a cost control only).
//!
//!     cargo run --release -p bough --example redescribe_cost -- [rows...]
use bough::prelude::*;
use bough::raw::{App, Caps, Config};
use bough_metal::perf::tinstr;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// Live heap bytes and allocation calls (`HEAP=1`). The counters are a load and a store each (not an atomic
/// add: the measuring thread is the only one allocating while a cell runs), so the other modes pay six
/// instructions per allocator call for it.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
// SAFETY: forwards to `System` unchanged; the counters are not part of the allocation contract.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.store(LIVE.load(Relaxed) + l.size(), Relaxed);
        CALLS.store(CALLS.load(Relaxed) + 1, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        LIVE.store(LIVE.load(Relaxed) + l.size(), Relaxed);
        CALLS.store(CALLS.load(Relaxed) + 1, Relaxed);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.store(LIVE.load(Relaxed).wrapping_sub(l.size()), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        LIVE.store(LIVE.load(Relaxed).wrapping_sub(l.size()) + n, Relaxed);
        CALLS.store(CALLS.load(Relaxed) + 1, Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;

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
        let t1 = tinstr();
        a.turn(|_| {});
        let d = tinstr() - t0;
        if std::env::var_os("PHASES").is_some() && n > 1 {
            // `BOUGH_LPROF=1` adds the layout engine's and the write-back's shares of `App::new`
            let p = a.rt.lay.prof.acc;
            let k = n as u64;
            eprintln!("  shape {K}, {n} rows, per row: App::new {} (of which layout compute {}, write-back {}), first turn {}", (t1 - t0) / k, p[bough::layout::LP_COMPUTE] / k, p[bough::layout::LP_WRITE] / k, (d - (t1 - t0)) / k);
        }
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

/// Live heap bytes, allocator calls and the façade's per-table bytes after mounting `n` application rows
/// (`App::new` + the first turn), against the same for 0 rows. The GPU region (instances, pages, glyph slots)
/// is one allocation of fixed size and cancels in the difference.
fn heap(n: usize) {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let one = |n: usize| {
        let mut cfg = Config::new(1280.0, 800.0, 1.0, font.clone());
        cfg.caps = Caps { instances: 1 << 17, pages: 1 << 13, glyphs: 1 << 20, runs: 1 << 17, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
        let rows: Vec<(u32, String)> = (0..n as u32).map(|i| (i, format!("Row label {i}"))).collect();
        let (l0, c0) = (LIVE.load(Relaxed), CALLS.load(Relaxed));
        let mut a = App::new(Table::<0> { rows, selected: u32::MAX }, cfg, false);
        let (l1, c1) = (LIVE.load(Relaxed), CALLS.load(Relaxed));
        a.turn(|_| {});
        let (l2, c2) = (LIVE.load(Relaxed), CALLS.load(Relaxed));
        let ledger = a.rt.store_ledger();
        std::mem::forget(a);
        ([l1 - l0, l2 - l0], [c1 - c0, c2 - c0], ledger)
    };
    one(1);
    let (z, f) = (one(0), one(n));
    let k = n as f64;
    println!("rows {n}: live heap per row {:.0} B after App::new, {:.0} B after the first turn; allocator calls per row {:.2} / {:.2}", (f.0[0] - z.0[0]) as f64 / k, (f.0[1] - z.0[1]) as f64 / k, (f.1[0] - z.1[0]) as f64 / k, (f.1[1] - z.1[1]) as f64 / k);
    println!("  {:<52} {:>9} {:>12} {:>9}", "table", "entries", "bytes", "B/row");
    let mut sum = 0.0;
    for (i, (name, entries, bytes)) in f.2.iter().enumerate() {
        let per = (*bytes as f64 - z.2[i].2 as f64) / k;
        sum += per;
        println!("  {name:<52} {entries:>9} {bytes:>12} {per:>9.1}");
    }
    println!("  {:<52} {:>9} {:>12} {sum:>9.1}", "sum of the tables", "", "");
    // columns carved from the reserved block are not the allocator's: their touched bytes are added
    let cols: f64 = f.2.iter().enumerate().filter(|(_, e)| e.0.starts_with("col ")).map(|(i, e)| (e.2 as f64 - z.2[i].2 as f64) / k).sum();
    println!("  bytes per row, allocator live + carved columns touched: {:.0}", (f.0[1] - z.0[1]) as f64 / k + cols);
}

/// The appliers a wire calls, alone (`APPLIERS=1`): instructions per call of a colour write on a painted
/// box and of a digit tick on a laid-out label, with nothing of the turn around them (no settle, no
/// re-description, no layout flush). The node is found by its kind; the two values alternate so every call
/// writes.
fn appliers() {
    use bough::elem::{Apply, ApplyBg};
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let cfg = Config::new(1280.0, 800.0, 1.0, font);
    let rows = (0..8u32).map(|i| (i, format!("Row label {i}"))).collect();
    let mut a = App::new(Table::<0> { rows, selected: u32::MAX }, cfg, false);
    a.turn(|_| {});
    let rt = &mut a.rt;
    let n = 4096u32;
    let quad = (1..n).rev().find(|&i| rt.node_live(i) && rt.hot[i as usize].kind == bough::rt::NK_QUAD).expect("a painted box");
    let label = (1..n).rev().find(|&i| rt.node_live(i) && rt.hot[i as usize].kind == bough::rt::NK_LTEXT).expect("a label");
    let slope = |run: &mut dyn FnMut(u32) -> u64| {
        run(64);
        let (lo, hi) = (run(1000), run(3000));
        (hi - lo) / 2000
    };
    let colour = slope(&mut |reps| {
        let t0 = tinstr();
        for k in 0..reps {
            ApplyBg::apply(rt, quad, &Color(0xff00_0000 | (k & 1) * 0x0101_01));
        }
        tinstr() - t0
    });
    let number = slope(&mut |reps| {
        let t0 = tinstr();
        for k in 0..reps {
            rt.lay_text_changed(label, if k & 1 == 0 { "Row label 3" } else { "Row label 8" });
        }
        tinstr() - t0
    });
    println!("applier,instructions_per_call");
    println!("colour (ApplyBg on a painted box),{colour}");
    println!("digit tick (lay_text_changed on a label),{number}");
}

/// Round 7: what building the row descriptions alone costs (no walk, no seal, nothing retained): the floor of
/// any re-description that calls the row closure, whatever it then compares. Instructions per row.
fn build_floor(n: usize) -> [u64; 2] {
    let rows: Vec<(u32, String)> = (0..n as u32).map(|i| (i, format!("Row label {i}"))).collect();
    let t = Table::<0> { rows, selected: u32::MAX };
    let slope = |f: &dyn Fn()| {
        let run = |reps: u32| {
            let t0 = tinstr();
            for _ in 0..reps {
                f();
            }
            tinstr() - t0
        };
        run(4);
        (run(48) - run(16)) / 32 / n as u64
    };
    // the application row: a box with a tint, an integer label and a borrowed string label
    let full = slope(&|| {
        for i in 0..n {
            let (id, label) = &t.rows[i];
            let e = row((text(*id).w(64.0), text(label.as_str()))).w(960.0).h(32.0).bg(tint(&t, i));
            std::hint::black_box(&e);
        }
    });
    // what varies between rows and turns: the operands alone
    let ops = slope(&|| {
        for i in 0..n {
            let (id, label) = &t.rows[i];
            let e = (*id, label.as_str(), tint(&t, i));
            std::hint::black_box(&e);
        }
    });
    [full, ops]
}

fn main() {
    if std::env::var_os("APPLIERS").is_some() {
        return appliers();
    }
    if std::env::var_os("HEAP").is_some() {
        for n in std::env::args().skip(1).filter_map(|s| s.parse::<usize>().ok()) {
            heap(n);
        }
        return;
    }
    if std::env::var_os("BUILD").is_some() {
        println!("rows,description_build_per_row,operands_only_per_row");
        for n in std::env::args().skip(1).filter_map(|s| s.parse::<usize>().ok()) {
            let b = build_floor(n);
            println!("{n},{},{}", b[0], b[1]);
        }
        return;
    }
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
