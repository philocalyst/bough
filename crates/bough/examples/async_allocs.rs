//! Round 7, lane async: where the allocations of an ask turn and of a landing turn come from (mount-once, one
//! question, 16 sinks; the same shape as `async_cost`'s `land` cell). Every allocation inside the measured turn
//! records its backtrace; the report groups them by the first frame inside bough (or bough-async).
//!
//!     cargo run --release -p bough --example async_allocs
use bough::prelude::*;
use bough::raw::{App, Caps, Config};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

struct Rec;
thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static INSIDE: Cell<bool> = const { Cell::new(false) };
    static SITES: RefCell<Vec<(usize, String)>> = const { RefCell::new(Vec::new()) };
}
unsafe impl GlobalAlloc for Rec {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        note(l.size());
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        note(n);
        unsafe { System.realloc(p, l, n) }
    }
}
fn note(size: usize) {
    if !ON.with(|o| o.get()) || INSIDE.with(|i| i.replace(true)) {
        return;
    }
    let bt = std::backtrace::Backtrace::force_capture().to_string();
    // the first frames inside bough, below the allocator
    let site: Vec<String> = bt
        .lines()
        .filter(|l| l.contains("bough") && !l.contains("async_allocs") && !l.trim_start().starts_with("at "))
        .take(3)
        .map(|l| l.trim().split_once(": ").map_or(l.trim(), |x| x.1).to_string())
        .collect();
    SITES.with(|s| s.borrow_mut().push((size, site.join(" <- "))));
    INSIDE.with(|i| i.set(false));
}
#[global_allocator]
static A: Rec = Rec;

thread_local! {
    static GATE: RefCell<Option<std::task::Waker>> = const { RefCell::new(None) };
    static OPEN: Cell<bool> = const { Cell::new(false) };
}
struct Gate(u32);
impl std::future::Future for Gate {
    type Output = u32;
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<u32> {
        if OPEN.with(|o| o.get()) {
            return std::task::Poll::Ready(self.0 * 10);
        }
        GATE.with(|g| *g.borrow_mut() = Some(cx.waker().clone()));
        std::task::Poll::Pending
    }
}

struct Shown {
    src: Signal<u32>,
    dst: Signal<u32>,
}
impl View for Shown {
    fn view(&self) -> impl Element<Self> + '_ {
        div().child((
            each((0..16).map(|_| text(sig!(self.src).map(|v| format!("v {v}"))))),
            text(sig!(self.dst).map(|v| format!("answer {v}"))),
            ask(sig!(self.src), |x| Gate(x)).into(sig!(self.dst)),
        ))
    }
}

fn report(name: &str) {
    let sites = SITES.with(|s| std::mem::take(&mut *s.borrow_mut()));
    let mut by: HashMap<String, (usize, usize)> = HashMap::new();
    for (size, site) in &sites {
        let e = by.entry(site.clone()).or_default();
        e.0 += 1;
        e.1 += size;
    }
    let mut v: Vec<_> = by.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    println!("== {name}: {} allocations", sites.len());
    for (site, (n, bytes)) in v {
        println!("{n:4} x ({bytes:6} B)  {site}");
    }
}

fn main() {
    let font = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf").unwrap();
    let mut cfg = Config::new(1280.0, 800.0, 1.0, font).once();
    cfg.caps = Caps { instances: 1 << 16, pages: 1 << 12, glyphs: 1 << 18, runs: 1 << 16, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
    let mut a = App::new(Shown { src: Signal::new(0), dst: Signal::new(0) }, cfg, false);
    OPEN.with(|o| o.set(true));
    a.turn(|v| *v.src = 1);
    a.run_until_stalled();
    // warm: several rounds so every buffer has its capacity
    for k in 2..40u32 {
        OPEN.with(|o| o.set(false));
        a.turn(|v| *v.src = k);
        OPEN.with(|o| o.set(true));
        if let Some(w) = GATE.with(|g| g.borrow_mut().take()) {
            w.wake();
        }
        a.turn(|_| {});
    }
    OPEN.with(|o| o.set(false));
    ON.with(|o| o.set(true));
    a.turn(|v| *v.src = 100);
    ON.with(|o| o.set(false));
    report("ask turn (write the source, start the question)");
    OPEN.with(|o| o.set(true));
    if let Some(w) = GATE.with(|g| g.borrow_mut().take()) {
        w.wake();
    }
    ON.with(|o| o.set(true));
    a.turn(|_| {});
    ON.with(|o| o.set(false));
    report("landing turn (poll, commit, release, 16 sinks)");
    assert_eq!(*a.owner.dst, 1000);
}
