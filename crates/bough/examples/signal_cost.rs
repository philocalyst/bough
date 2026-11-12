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
use bough_metal::perf::tinstr;

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

fn cells(n: usize, once: bool) -> [u64; 6] {
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
                    3 => a.turn(|_| {}),
                    // round 7: the same turns with their frame (headless: up to the tile list), so that work
                    // moved from the write to the frame is counted
                    4 => {
                        a.turn(|r| drop(r.tint[i].set(Color(0xff00_0000 | (k & 0xff_ffff)))));
                        a.frame();
                    }
                    _ => {
                        a.turn(|_| {});
                        a.frame();
                    }
                }
            }
            tinstr() - t0
        })
    };
    // the frame cells last: a frame advances the epoch, the cells above measure turns without one
    let c = [cell(&mut a, 0), cell(&mut a, 1), cell(&mut a, 2), cell(&mut a, 3)];
    a.frame();
    [c[0], c[1], c[2], c[3], cell(&mut a, 4), cell(&mut a, 5)]
}

/// Feature `attr`: where the instructions of a colour-write turn and its frame go (instrumented build).
#[cfg(feature = "attr")]
fn attribute() {
    let ovh = bough_metal::perf::tinstr_overhead();
    let names = ["handler (the write)", "settle (all)", "run_head", "patch (applier)", "epoch advance", "paint_resolve", "(empty span: calibration)", "pick", "", "repair_rows", "copy_plan", "write_slot", "draw: before repair_rows", "draw: between copy_plan and the slot", "plane(0).is_clear"];
    for n in [1usize, 256] {
        let mut a = app(n, true);
        let reps = 4000u64;
        for with_frame in [false, true] {
            bough::attr::on::take();
            for k in 0..reps as u32 {
                let i = (k as usize * 7) % n;
                a.turn(|r| drop(r.tint[i].set(Color(0xff00_0000 | (k & 0xff_ffff)))));
                if with_frame {
                    a.frame();
                }
            }
            let t = bough::attr::on::take();
            println!("rows {n}, frame {with_frame}:");
            for (i, name) in names.iter().enumerate() {
                if t[i].1 != 0 {
                    println!("  {name:24} {:6} per turn ({} spans)", (t[i].0.saturating_sub(t[i].1 * ovh)) / reps, t[i].1 / reps);
                }
            }
            a.frame();
        }
    }
}

fn main() {
    #[cfg(feature = "attr")]
    if std::env::var_os("ATTR").is_some() {
        return attribute();
    }
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
    println!("rows,mode,colour,number,string,idle,colour+frame,idle+frame");
    for n in sizes {
        assert!(n <= MAX && n > 0);
        for (name, once) in [("once", true), ("redescribe", false)] {
            let c = cells(n, once);
            println!("{n},{name},{},{},{},{},{},{}", c[0], c[1], c[2], c[3], c[4], c[5]);
        }
    }
}
