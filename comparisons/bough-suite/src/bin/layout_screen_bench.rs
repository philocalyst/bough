//! zgx-gpu `examples/layout_bench.rs`, native: text layout and drawing cost of a label-heavy screen
//! (`bough_suite::layout_screen`: 50 rows x 4 labels at 11 pt, 24 wrapped paragraphs at 12 pt, 24 rich
//! paragraphs at 12 pt), 1400 x 900 points at scale 2 on the GPU, as zgx:
//!
//! * first frames of text new to every cache: 100 mounts, each with its strings salted (`salt` 1..=100);
//!   mount + layout (`App::turn`: describe, mount, lay out), then the frame (`App::frame`: damage -> tile
//!   list -> GPU encode + commit). One app and one presenter throughout, as zgx keeps one renderer; the
//!   previous screen is unmounted and its resources returned outside the span (zgx drops its old `Ui`
//!   outside it too);
//! * a window resize dragged back and forth: 400 steps of width 1400 - (step % 20) * 25 through
//!   `App::resize` (relayout), then the frame.
//!
//! Means in ms, plus the process's instructions (`proc_pid_rusage`) per span. The GPU's execution is in no
//! span: before each frame the previous one is waited for (`gpu wait`, reported apart; zgx's render
//! begins with that wait, `finish_direct`). `RENDER=0` stops each frame at the tile list; `BOUGH_RING=1`
//! draws into one offscreen surface (zgx's one target) instead of the default ring of 4; `FIRST` /
//! `RESIZES` set the counts. bough's façade has one face: the rich paragraphs' bold span is shaped and drawn
//! in the regular face.
use bough::prelude::*;
use bough_suite::*;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Acc {
    layout: Duration,
    render: Duration,
    wait: Duration,
    il: u64,
    ir: u64,
}

impl Acc {
    fn line(&self, what: &str, n: usize) -> String {
        let ms = |d: Duration| d.as_secs_f64() * 1e3 / n as f64;
        let mi = |i: u64| i as f64 / 1e6 / n as f64;
        format!(
            "{what} {:.3} ms  render {:.3} ms  (mean of {n})   {:.2} M + {:.2} M instructions  (gpu wait {:.3} ms)",
            ms(self.layout),
            ms(self.render),
            mi(self.il),
            mi(self.ir),
            ms(self.wait)
        )
    }
}

fn main() {
    let first = env_usize("FIRST", 100);
    let resizes = env_usize("RESIZES", 400);
    let gpu = std::env::var("RENDER").map_or(true, |v| v != "0");
    let mut cfg = lb_config(LB_W);
    // offscreen surfaces: 0 = the default round-robin ring of 4; 1 = one target, as zgx
    cfg.ring = env_usize("BOUGH_RING", 0);
    let mut a = App::new(LayoutHost::new(), cfg, gpu);
    a.frame();
    let sync = |a: &mut App<LayoutHost>| {
        let t = Instant::now();
        if gpu {
            a.bins_overflowed();
        }
        t.elapsed()
    };
    let mut m = Acc::default();
    for salt in 1..=first {
        m.wait += sync(&mut a);
        lb_unmount(&mut a);
        let (i0, t0) = (instructions(), Instant::now());
        a.turn(|v| {
            v.salt = salt;
            v.show = true;
        });
        let (i1, t1) = (instructions(), Instant::now());
        assert!(a.frame());
        let (i2, t2) = (instructions(), Instant::now());
        m.layout += t1 - t0;
        m.render += t2 - t1;
        m.il += i1 - i0;
        m.ir += i2 - i1;
    }
    println!("{}", m.line("first frame   mount+layout", first));
    let mut m = Acc::default();
    for step in 1..=resizes {
        let width = lb_width(step);
        m.wait += sync(&mut a);
        let (i0, t0) = (instructions(), Instant::now());
        a.resize(width, LB_H);
        let (i1, t1) = (instructions(), Instant::now());
        assert!(a.frame());
        let (i2, t2) = (instructions(), Instant::now());
        m.layout += t1 - t0;
        m.render += t2 - t1;
        m.il += i1 - i0;
        m.ir += i2 - i1;
    }
    println!("{}", m.line("resize step   layout      ", resizes));
}
