//! F8: prelude descriptions. App/Config and the instruction counter are harness-only escapes.
use bough::prelude::*;
use bough::app::{App, Config};
use bough::rt::Caps;
use bough_metal::perf::{snap, snap_overhead};
use std::hint::black_box;
#[allow(dead_code)]
#[path = "propagation_support/dashboard.rs"]
mod dashboard;
#[allow(dead_code)]
#[path = "propagation_support/scroll.rs"]
mod scroll;

struct Counter { n: u32 }
impl View for Counter {
    fn view(&self) -> impl Element<Self> + '_ {
        row((text(format!("Count: {}", self.n)), button("Increment").on_click(|v: &mut Self| v.n ^= 1)))
    }
}
struct Plain { n: usize, on: bool }
impl View for Plain {
    fn view(&self) -> impl Element<Self> + '_ {
        column(each((0..self.n).map(|i| block(16.0, 1.0).bg(rgb(if i == 0 && self.on { 0xff0000 } else { 0x112233 }))))).w(20.0)
    }
}
struct Leaf;
impl View for Leaf {
    fn view(&self) -> impl Element<Self> + '_ { block(16.0, 1.0).bg(rgb(0x112233)) }
}
struct Rows { rows: List<Leaf>, n: usize }
impl View for Rows {
    fn view(&self) -> impl Element<Self> + '_ { list(sig!(self.rows)).w(20.0).estimate(1.0).overscan(self.n) }
}
struct Wires { color: Signal<Color> }
macro_rules! tree {
    ($v:ident;) => { block(16.0, 1.0).bg(sig!($v: Wires => color)) };
    ($v:ident; x $($xs:tt)*) => { column((tree!($v; $($xs)*), tree!($v; $($xs)*), tree!($v; $($xs)*), tree!($v; $($xs)*))) };
}
impl View for Wires {
    fn view(&self) -> impl Element<Self> + '_ { tree!(self; x x x) }
}
struct Deep { on: bool }
macro_rules! deep {
    ($v:ident;) => { block(16.0, 1.0).bg(rgb(if $v.on { 0xff0000 } else { 0x112233 })) };
    ($v:ident; x $($xs:tt)*) => { column((deep!($v; $($xs)*),)) };
}
impl View for Deep {
    fn view(&self) -> impl Element<Self> + '_ {
        deep!(self; x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x x)
    }
}
fn config(once: bool) -> Config {
    let font = std::fs::read(std::env::var("BOUGH_FONT").expect("set BOUGH_FONT")).unwrap();
    let mut c = Config::new(1000.0, 700.0, 1.0, font);
    c.caps = Caps { instances: 1<<16, pages: 1<<13, glyphs: 1<<19, runs: 1<<16, spaces: 1<<14, clips: 1<<12, strikes: 16 };
    c.mount_once = once;
    c
}
fn measure<V: View>(name: &str, make: impl Fn() -> V, step: impl Fn(&mut V)) {
    let reps: usize = std::env::var("F8_REPS").ok().map(|s| s.parse().unwrap()).unwrap_or(1000);
    let ovh = snap_overhead();
    for once in [true, false] {
        let mut a = App::new(make(), config(once), false);
        for _ in 0..20 { a.turn(&step); a.frame(); }
        let start = snap().instrs;
        for _ in 0..reps { a.turn(|v| { step(v); black_box(v); }); }
        let instructions = (snap().instrs - start).saturating_sub(ovh);
        println!("{name},{once},{reps},{instructions},{:.3},{}", instructions as f64/reps as f64, a.record_count());
        black_box(a);
    }
}
fn main() {
    println!("cell,once,repeats,instructions,instructions_per_turn,records");
    measure("counter", || Counter { n: 0 }, |v| v.n ^= 1);
    measure("dashboard", dashboard::make, dashboard::step);
    measure("scroll-1000", scroll::make, scroll::step);
    for n in [10, 100, 1000, 10000] {
        measure(&format!("plain-{n}"), || Plain { n, on: false }, |v| v.on = !v.on);
        measure(&format!("list-{n}"), || Rows { rows: List::from_iter_in((0..n).map(|_| Leaf)), n }, |_| {});
    }
    measure("deep-32", || Deep { on: false }, |v| v.on = !v.on);
    measure("wires-64", || Wires { color: Signal::new(rgb(0x112233)) }, |v| { let c = *v.color; v.color.set(if c == rgb(0x112233) { rgb(0xff0000) } else { rgb(0x112233) }); });
}
