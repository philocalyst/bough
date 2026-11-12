//! Round 6 (lead): what the signal path costs. The fine-grained path is the one an application takes for
//! anything that changes often, so it is the one to measure first: N rows, each with a colour signal, a text
//! signal and a numeric signal bound by `sig!`; one signal is written per turn. Instructions per turn on the
//! calling thread, by slope over two repeat counts, with no frame (R0: everything up to the GPU's input).
//!
//! Cells (each for `once` = the wire path alone, and for an ordinary re-describing owner):
//! * `colour`: a background colour signal of one row;
//! * `number`: a numeric label (`.num`, no `String`);
//! * `string`: a label through `.map(|n| format!(..))`;
//! * `idle`: a turn that writes nothing.
//!
//!     cargo run --release -p bough --example signal_cost -- [rows...]   (rows <= 256)
use bough::prelude::*;
use bough::raw::{App, Caps, Config};
use bough::NumFmt;
use bough_sys::perf::tinstr;

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

fn slope(mut run: impl FnMut(u32) -> u64) -> u64 {
    run(8);
    let (lo, hi) = (run(64), run(192));
    (hi - lo) / 128
}

fn cells(n: usize, once: bool) -> [u64; 4] {
    let mut a = app(n, once);
    let mut k = 0u32;
    let mut cell = |a: &mut App<Rows>, which: u8| {
        slope(|reps| {
            let t0 = tinstr();
            for _ in 0..reps {
                k = k.wrapping_add(1);
                let i = (k as usize * 7) % n;
                match which {
                    0 => a.turn(|r| drop(r.tint[i].set(Color(0xff00_0000 | (k & 0xff_ffff))))),
                    1 => a.turn(|r| drop(r.count[i].set(k % 1000))),
                    2 => a.turn(|r| drop(r.name[i].set(k % 1000))),
                    _ => a.turn(|_| {}),
                }
            }
            tinstr() - t0
        })
    };
    [cell(&mut a, 0), cell(&mut a, 1), cell(&mut a, 2), cell(&mut a, 3)]
}

fn main() {
    if let Some(reps) = std::env::var("SPIN").ok().and_then(|v| v.parse::<u32>().ok()) {
        // profiling: idle turns of a re-describing owner, nothing else
        let mut a = app(MAX, false);
        for _ in 0..reps {
            a.turn(|_| {});
        }
        return;
    }
    let sizes: Vec<usize> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let sizes = if sizes.is_empty() { vec![16, 64, 256] } else { sizes };
    println!("rows,mode,colour,number,string,idle");
    for n in sizes {
        assert!(n <= MAX && n > 0);
        for (name, once) in [("once", true), ("redescribe", false)] {
            let c = cells(n, once);
            println!("{n},{name},{},{},{},{}", c[0], c[1], c[2], c[3]);
        }
    }
}
