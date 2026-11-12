//! Round 7, lane async: what async costs a turn. Instructions per turn on the calling thread, by slope over two
//! repeat counts (as `signal_cost`), with no frame.
//!
//! * `idle/N`: an idle turn of a re-describing owner with N questions in flight (futures that never finish).
//!   Prediction (written before the run): equal to the same owner's idle turn with N = 0 (admission is
//!   wake-driven: a turn with nothing rung pays one atomic load), within 3 instructions, for N = 1, 100, 1000.
//!   Kill: growth with N.
//! * `ask/S`: a turn that writes a question's source and starts a flight, the source shown by S plain sinks (S =
//!   1, 1000). Prediction: within 5 % of each other (the hold is one bit; the sinks are skipped, not walked
//!   twice). Kill: more than 5 % growth from 1 to 1000.
//! * `land`: a turn that lands one answer (publication commit, release, the sinks' wires), and the allocations it
//!   makes. Reported, no prediction for instructions; prediction for allocations: at most 2 (the boxed answer is
//!   made by the future's poll, outside the turn; the turn's own bookkeeping allocates nothing).
//!
//!     cargo run --release -p bough --example async_cost
use bough::prelude::*;
use bough::raw::{App, Caps, Config};
use bough_metal::perf::tinstr;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

struct Count;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Count = Count;

const MAX: usize = 1000;

/// N questions that never answer (each its own source and destination).
struct Pending {
    n: usize,
    ins: [Signal<u32>; MAX],
    outs: [Signal<u32>; MAX],
    tick: Signal<u32>,
}

impl View for Pending {
    fn view(&self) -> impl Element<Self> + '_ {
        div().child((
            text(sig!(self.tick).map(|t| format!("tick {t}"))),
            each((0..self.n).map(|i| ask(sig!(self.ins).at(i), |_x| std::future::pending::<u32>()).into(sig!(self.outs).at(i)))),
        ))
    }
}

/// One question whose source is shown by S sinks; its answers come from a channel the harness fills.
struct Shown {
    s: usize,
    src: Signal<u32>,
    dst: Signal<u32>,
}

/// The control: the same sinks and destination label, no question (the landing's own writes, by hand).
struct Plain {
    s: usize,
    src: Signal<u32>,
    dst: Signal<u32>,
}

impl View for Plain {
    fn view(&self) -> impl Element<Self> + '_ {
        div().child((each((0..self.s).map(|_| text(sig!(self.src).map(|v| format!("v {v}"))))), text(sig!(self.dst).map(|v| format!("answer {v}")))))
    }
}

/// The turn a landing replaces: the source's sinks and the destination change together.
fn plain_cost(once: bool) -> (u64, f64) {
    let mut a = App::new(Plain { s: 16, src: Signal::new(0), dst: Signal::new(0) }, if once { cfg_once() } else { cfg() }, false);
    a.turn(|_| {});
    let mut k = 1u32;
    let (mut instr, mut allocs, mut n) = (0u64, 0u64, 0u64);
    for _ in 0..200 {
        k += 1;
        let (i0, a0) = (tinstr(), ALLOCS.load(Ordering::Relaxed));
        a.turn(|v| {
            *v.src = k;
            *v.dst = k * 10;
        });
        instr += tinstr() - i0;
        allocs += ALLOCS.load(Ordering::Relaxed) - a0;
        n += 1;
    }
    (instr / n, allocs as f64 / n as f64)
}

thread_local! {
    static GATE: std::cell::RefCell<Option<(u32, std::task::Waker)>> = const { std::cell::RefCell::new(None) };
    static OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Pending until the harness opens the gate.
struct Gate(u32);
impl std::future::Future for Gate {
    type Output = u32;
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<u32> {
        if OPEN.with(|o| o.get()) {
            return std::task::Poll::Ready(self.0 * 10);
        }
        GATE.with(|g| *g.borrow_mut() = Some((self.0, cx.waker().clone())));
        std::task::Poll::Pending
    }
}

impl View for Shown {
    fn view(&self) -> impl Element<Self> + '_ {
        div().child((
            each((0..self.s).map(|_| text(sig!(self.src).map(|v| format!("v {v}"))))),
            text(sig!(self.dst).map(|v| format!("answer {v}"))),
            ask(sig!(self.src), |x| Gate(x)).into(sig!(self.dst)),
        ))
    }
}

fn cfg_once() -> Config {
    cfg().once()
}

fn cfg() -> Config {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut cfg = Config::new(1280.0, 800.0, 1.0, font);
    cfg.caps = Caps { instances: 1 << 16, pages: 1 << 12, glyphs: 1 << 18, runs: 1 << 16, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
    cfg
}

fn slope(mut run: impl FnMut(u32) -> u64) -> u64 {
    run(8);
    let (lo, hi) = (run(64), run(192));
    (hi - lo) / 128
}

fn idle(n: usize, once: bool) -> u64 {
    let mut a = App::new(Pending { n, ins: std::array::from_fn(|i| Signal::new(i as u32)), outs: std::array::from_fn(|_| Signal::new(0)), tick: Signal::new(0) }, if once { cfg_once() } else { cfg() }, false);
    a.turn(|_| {});
    assert_eq!(a.rt.asks.engine.flights(), n);
    slope(|reps| {
        let t0 = tinstr();
        for _ in 0..reps {
            a.turn(|_| {});
        }
        tinstr() - t0
    })
}

fn ask_cost(s: usize, once: bool) -> u64 {
    let mut a = App::new(Shown { s, src: Signal::new(0), dst: Signal::new(0) }, if once { cfg_once() } else { cfg() }, false);
    // first answer, so later asks hold
    OPEN.with(|o| o.set(true));
    a.turn(|v| *v.src = 1);
    a.run_until_stalled();
    OPEN.with(|o| o.set(false));
    let mut k = 2u32;
    slope(|reps| {
        let t0 = tinstr();
        for _ in 0..reps {
            k += 1;
            a.turn(|v| *v.src = k);
        }
        tinstr() - t0
    })
}

fn land_cost(once: bool) -> (u64, f64) {
    let mut a = App::new(Shown { s: 16, src: Signal::new(0), dst: Signal::new(0) }, if once { cfg_once() } else { cfg() }, false);
    OPEN.with(|o| o.set(true));
    a.turn(|v| *v.src = 1);
    a.run_until_stalled();
    let mut k = 1u32;
    let (mut instr, mut allocs, mut n) = (0u64, 0u64, 0u64);
    for _ in 0..200 {
        k += 1;
        OPEN.with(|o| o.set(false));
        a.turn(|v| *v.src = k);
        // the answer is ready: wake it (the poll that makes the boxed answer happens in the landing turn too)
        OPEN.with(|o| o.set(true));
        if let Some((_, w)) = GATE.with(|g| g.borrow_mut().take()) {
            w.wake();
        }
        let (i0, a0) = (tinstr(), ALLOCS.load(Ordering::Relaxed));
        a.turn(|_| {});
        instr += tinstr() - i0;
        allocs += ALLOCS.load(Ordering::Relaxed) - a0;
        n += 1;
        assert_eq!(*a.owner.dst, k * 10);
    }
    (instr / n, allocs as f64 / n as f64)
}

fn main() {
    println!("cell,mode,instructions per turn");
    for (mode, once) in [("once", true), ("redescribe", false)] {
        for n in [0usize, 1, 100, 1000] {
            println!("idle/{n},{mode},{}", idle(n, once));
        }
        for s in [1usize, 1000] {
            println!("ask/{s},{mode},{}", ask_cost(s, once));
        }
        let (i, al) = land_cost(once);
        let (pi, pal) = plain_cost(once);
        println!("land (16 sinks),{mode},{i}");
        println!("control: the same writes by hand,{mode},{pi}");
        println!("land allocations per turn,{mode},{al:.2}");
        println!("control allocations per turn,{mode},{pal:.2}");
    }
}
