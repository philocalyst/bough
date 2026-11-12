//! zgx `widget_bench` onto bough: 500 rows (argv) of two buttons and a text input, window 1200 x 800: mount
//! (+ first frame), retained heap and allocations, 1000 idle turns, 400 hover steps.
//! Run: widget_bench [rows]   (`NOPROBE=1`: skip the width probe, inputs at a fixed x)
use bough::raw::App;
use bough::sys::perf::snap;
use bough_widgets::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
    time::Instant,
};

/// Counts live heap bytes (blocks under 1 MiB) and the large blocks (the region's columns: virtual until
/// touched) apart, and allocations.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static BIG: AtomicUsize = AtomicUsize::new(0);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
const BIG_AT: usize = 1 << 20;
fn add(n: usize) {
    if n >= BIG_AT {
        BIG.fetch_add(n, Relaxed);
    } else {
        LIVE.fetch_add(n, Relaxed);
    }
    ALLOCATIONS.fetch_add(1, Relaxed);
}
fn sub(n: usize) {
    if n >= BIG_AT {
        BIG.fetch_sub(n, Relaxed);
    } else {
        LIVE.fetch_sub(n, Relaxed);
    }
}
// SAFETY: every call goes to `System` unchanged (its contract the caller's); the counters are atomics.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        add(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        add(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        sub(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        sub(layout.size());
        add(new);
        unsafe { System.realloc(ptr, layout, new) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

fn main() {
    let rows: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(500);
    let m = |label: &str, i0: u64, t0: Instant| println!("PHASE {label:<28} {:10.3} M instr {:9.3} ms", (snap().instrs - i0) as f64 / 1e6, t0.elapsed().as_secs_f64() * 1e3);
    // the input slots' x (a throwaway layout of the label texts; also warms the font tables)
    let (i0, t0) = (snap().instrs, Instant::now());
    let xs = if std::env::var_os("NOPROBE").is_some() { vec![200.0; 8] } else { field_xs(rows) };
    m("probe (button widths)", i0, t0);
    let (i0, t0) = (snap().instrs, Instant::now());
    let widgets = Widgets::new(rows, &xs);
    m("signals (500 Signal<String>)", i0, t0);
    let (live0, big0, allocs0) = (LIVE.load(Relaxed), BIG.load(Relaxed), ALLOCATIONS.load(Relaxed));
    let rss0 = snap().footprint;
    let cfg = config(1200.0, 800.0, rows, 12, 40);
    let (i0, t0) = (snap().instrs, Instant::now());
    let mut app = App::new(widgets, cfg, false);
    let (mount_i, mount_t) = (snap().instrs - i0, t0.elapsed());
    m("mount + layout (App::new)", i0, t0);
    let (i1, t1) = (snap().instrs, Instant::now());
    let drew = app.frame();
    m("first frame (tile list)", i1, t1);
    println!("first frame drew {drew}, {} tiles", app.repaired_tiles);
    m("mount + first frame", i0, t0);
    let mount_ms = t0.elapsed().as_secs_f64() * 1e3;
    let retained = LIVE.load(Relaxed) - live0;
    let big = BIG.load(Relaxed) - big0;
    let allocations = ALLOCATIONS.load(Relaxed) - allocs0;
    let footprint = snap().footprint - rss0;
    // idle: a turn with nothing to settle, and the frame that finds nothing to draw
    let (i0, t0) = (snap().instrs, Instant::now());
    for _ in 0..1000 {
        app.flush();
        assert!(!app.frame());
    }
    let idle = t0.elapsed();
    m("1000 idle (flush + frame)", i0, t0);
    // hover across the first buttons: each enter and leave restyles one
    let (i0, t0) = (snap().instrs, Instant::now());
    let (mut drawn, mut tiles, mut mv) = (0, 0u64, 0u64);
    for step in 0..400 {
        let j0 = snap().instrs;
        app.pointer_move(20.0, hover_y(step));
        mv += snap().instrs - j0;
        if app.frame() {
            drawn += 1;
            tiles += app.repaired_tiles as u64;
        }
    }
    let hover = t0.elapsed();
    let hover_i = snap().instrs - i0;
    m("400 hover steps", i0, t0);
    println!("pointer_move share {:.0} instr/step (includes the snap overhead {})", mv as f64 / 400.0, bough::sys::perf::snap_overhead());
    let st = &app.rt.input.stats;
    println!(
        "{rows} rows ({} widgets): mount {mount_ms:.2} ms (App::new {:.2} ms, {:.1} M instr), {:.1} KiB retained heap (+ {:.1} MiB large columns, virtual; footprint +{:.1} KiB), {allocations} allocations, {} records",
        rows * 3,
        mount_t.as_secs_f64() * 1e3,
        mount_i as f64 / 1e6,
        retained as f64 / 1024.0,
        big as f64 / 1048576.0,
        footprint as f64 / 1024.0,
        app.rt.bough.len(),
    );
    println!(
        "idle turn {:.1} ns; hover step {:.2} us, {:.0} instr ({drawn} frames, {tiles} tiles repaired; {} crossings of {} hit tests)",
        idle.as_nanos() as f64 / 1000.0,
        hover.as_secs_f64() * 1e6 / 400.0,
        hover_i as f64 / 400.0,
        st.crossings,
        st.hits,
    );
}
