//! Scratch (surface-api lane, not a cell of the sweep): instructions of the parts of one gui-bench operation.
//!
//!     probe <rows> <variant: fine|coarse|rows> <op: select|hover|noop>
use bough::prelude::*;
use bough::raw::{App, Button};
use bough_guibench::{driver, model::{Coarse, Fine, Rows}, spec};
use bough_metal::perf::tinstr;

fn parts<V: driver::Bench>(a: &mut App<V>, op: &str) {
    let y = spec::HEADER_HEIGHT + spec::ROW_HEIGHT / 2.0;
    let mut acc = [0u64; 6];
    let reps: usize = std::env::var("SPIN").ok().map_or(64, |v| v.parse().unwrap());
    for r in 0..reps + 4 {
        let mut t = [0u64; 6];
        match op {
            "select" => {
                let t0 = tinstr();
                a.pointer_down(640.0, y, Button::Primary);
                let t1 = tinstr();
                a.pointer_up(640.0, y, Button::Primary);
                let t2 = tinstr();
                a.frame();
                let t3 = tinstr();
                t[0] = t1 - t0;
                t[1] = t2 - t1;
                t[2] = t3 - t2;
            }
            "hover" => {
                let t0 = tinstr();
                a.pointer_move(640.0, if r % 2 == 0 { 5.0 * 32.0 + 16.0 } else { 16.0 });
                let t1 = tinstr();
                a.frame();
                let t2 = tinstr();
                t[0] = t1 - t0;
                t[2] = t2 - t1;
            }
            "noop" => {
                let t0 = tinstr();
                a.key_down(Key::Char('q'), Mods::default());
                let t1 = tinstr();
                a.frame();
                let t2 = tinstr();
                t[0] = t1 - t0;
                t[2] = t2 - t1;
            }
            "turn" => {
                let t0 = tinstr();
                a.turn(|_| {});
                let t1 = tinstr();
                a.frame();
                let t2 = tinstr();
                t[0] = t1 - t0;
                t[2] = t2 - t1;
            }
            "frame" => {
                let t0 = tinstr();
                let drew = a.frame();
                let t1 = tinstr();
                t[2] = t1 - t0;
                if r == 0 {
                    println!("frame drew={drew} reclaiming={}", a.reclaiming());
                }
            }
            _ => panic!("op"),
        }
        if r >= 4 {
            for i in 0..6 {
                acc[i] += t[i];
            }
        }
    }
    let n = reps as u64;
    let o = bough_metal::perf::tinstr_overhead();
    let k = |x: u64| (x / n).saturating_sub(o);
    println!("{op}: down/move {} up {} frame {} (tinstr overhead {o} removed)", k(acc[0]), k(acc[1]), k(acc[2]));
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: usize = args.next().unwrap().parse().unwrap();
    let variant = args.next().unwrap();
    let op = args.next().unwrap();
    let c = driver::config(1.0);
    match variant.as_str() {
        "fine" => {
            let mut a = App::new(Fine::new(n, false), c, false);
            driver::focus(&mut a);
            a.frame();
            parts(&mut a, &op);
        }
        "coarse" => {
            let mut a = App::new(Coarse::new(n, false), c, false);
            driver::focus(&mut a);
            a.frame();
            parts(&mut a, &op);
        }
        _ => {
            let mut a = App::new(Rows::new(n), c, false);
            a.frame();
            parts(&mut a, &op);
        }
    }
}
