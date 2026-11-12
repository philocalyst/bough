//! Lane async-entities, round 6: what one signal write costs when nothing is async. A view of 16 quads, each
//! with a background wired to its own signal; one signal is written per turn (no frame). Instructions per
//! `App::turn` on the calling thread, by slope over two repeat counts, mounted once (the wire path alone) and
//! re-describing (the application mode). The async machinery must add nothing here.
//!
//!     cargo run --release -p bough --example signal_write
use bough::raw::*;
use bough_metal::perf::tinstr;

struct V {
    hue: [Signal<u32>; 16],
}
impl View for V {
    fn view(&self) -> impl Element<Self> + '_ {
        let q = |i: usize| quad(10.0 + 20.0 * i as f32, 10.0, 16.0, 16.0).bg(sig!(self.hue).at(i).map(|h| rgb(*h)));
        ((q(0), q(1), q(2), q(3), q(4), q(5), q(6), q(7)), (q(8), q(9), q(10), q(11), q(12), q(13), q(14), q(15)))
    }
}

fn measure(once: bool) -> u64 {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut cfg = Config::new(400.0, 100.0, 1.0, font);
    if once {
        cfg = cfg.once();
    }
    let mut a = App::new(V { hue: std::array::from_fn(|_| Signal::new(0)) }, cfg, false);
    a.turn(|_| {});
    let mut run = |reps: u32| {
        let t0 = tinstr();
        for k in 0..reps {
            a.turn(|v| *v.hue[(k % 16) as usize] = 0x10_0000 + k);
        }
        tinstr() - t0
    };
    run(64);
    let (lo, hi) = (run(256), run(1280));
    (hi - lo) / 1024
}

fn main() {
    println!("mode,instructions_per_turn");
    println!("once,{}", measure(true));
    println!("redescribe,{}", measure(false));
}
