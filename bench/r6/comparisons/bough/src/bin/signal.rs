//! bough's side of the signal case, on the same cmpkit driver as every other framework: N rows, each with a colour
//! signal, a numeric label (`.num`, no String) and a string label (`.map(format!)`), bound by `sig!`; one write per
//! turn; the turn is bough's `App::turn` (no frame: R0, everything up to the GPU's input).
//! Variants: `once` (the wire path alone: `Config::once`) and `redescribe` (an ordinary re-describing owner).
use bough::NumFmt;
use bough::prelude::*;
use bough::raw::{App, Caps, Config};
use cmpkit::{Ctx, SignalApp, run_signal};

const MAX: usize = 256;

struct Rows {
    n: usize,
    tint: [Signal<Color>; MAX],
    count: [Signal<u32>; MAX],
    name: [Signal<u32>; MAX],
}

impl View for Rows {
    fn view(&self) -> impl Element<Self> + '_ {
        let line = move |i: usize| {
            row((
                text(sig!(self.count).at(i).num(|n| *n as f32, NumFmt::new("", None, ""))).w(64.0),
                text(sig!(self.name).at(i).map(|n| format!("Row label {n}"))),
            ))
            .w(960.0)
            .h(32.0)
            .bg(sig!(self.tint).at(i))
        };
        div().bg(rgb(0xffffff)).child(each((0..self.n).map(line)))
    }
}

fn app(n: usize, once: bool) -> App<Rows> {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut cfg = Config::new(1280.0, 800.0, 1.0, font);
    cfg.caps = Caps { instances: 1 << 16, pages: 1 << 12, glyphs: 1 << 18, runs: 1 << 16, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
    if once {
        cfg = cfg.once();
    }
    let rows = Rows {
        n,
        tint: std::array::from_fn(|i| Signal::new(if i % 2 == 0 { rgb(0xf5f5f5) } else { rgb(0xe6e6e6) })),
        count: std::array::from_fn(|i| Signal::new(i as u32)),
        name: std::array::from_fn(|i| Signal::new(i as u32)),
    };
    let mut a = App::new(rows, cfg, false);
    a.turn(|_| {});
    a
}

struct Once(App<Rows>);
struct Redescribe(App<Rows>);

macro_rules! port {
    ($t:ident, $once:expr) => {
        impl SignalApp for $t {
            fn mount(n: usize) -> Self {
                $t(app(n, $once))
            }
            fn turn_colour(&mut self, i: usize, k: u32) {
                self.0.turn(|r| drop(r.tint[i].set(Color(0xff00_0000 | (k & 0xff_ffff)))))
            }
            fn turn_number(&mut self, i: usize, k: u32) {
                self.0.turn(|r| drop(r.count[i].set(k % 1000)))
            }
            fn turn_string(&mut self, i: usize, k: u32) {
                self.0.turn(|r| drop(r.name[i].set(k % 1000)))
            }
            fn turn_idle(&mut self) {
                self.0.turn(|_| {})
            }
        }
    };
}
port!(Once, true);
port!(Redescribe, false);

fn main() {
    let sizes: Vec<usize> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let sizes = if sizes.is_empty() { vec![16, 64, 256] } else { sizes };
    let cx = Ctx::new("bough", "lead 4f4c469ef5 (round6)", "release: debug=1, lto=thin (bough workspace default)");
    run_signal::<Once>(&cx, "once", &sizes, "App::turn, Config::once (wire path), no frame");
    run_signal::<Redescribe>(&cx, "redescribe", &sizes, "App::turn, re-describing owner, no frame");
}
