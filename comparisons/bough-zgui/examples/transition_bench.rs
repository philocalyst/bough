//! Per-frame CPU cost of N running colour transitions (the CPU wire form: the frame clock evaluates each
//! and applies it through the colour applier, a paint write), N = 1, 100, 10,000: main-thread instructions
//! per `tick + frame` (RENDER=0 shape: the frame ends at the tile list), median over the transition.
use bough::raw::*;
use bough::transition::Curve;
use bough_zgui::*;

struct Many {
    n: usize,
    on: Signal<bool>,
}
impl View for Many {
    fn view(&self) -> impl Element<Self> + '_ {
        let cols = 100usize;
        each((0..self.n).map(move |i| {
            let (x, y) = ((i % cols) as f32 * 16.0, (i / cols) as f32 * 8.0);
            quad(x, y, 14.0, 6.0).bg(sig!(self.on).map(move |o| if *o { Color::rgba((i * 7 % 255) as u8, 40, 200, 255) } else { Color::rgba(30, (i * 5 % 255) as u8, 60, 255) }).transition(Curve::linear(1.0)))
        }))
    }
}

fn main() {
    for n in [1usize, 100, 10_000] {
        let mut c = config(1600.0, 1300.0);
        c.caps.instances = c.caps.instances.max(1 << 15);
        let mut a = App::new(Many { n, on: Signal::new(false) }, c, false);
        a.frame();
        a.turn(|v: &mut Many| {
            v.on.set(true);
        });
        a.frame();
        let mut v = Vec::new();
        for _ in 0..30 {
            let i0 = bough::sys::perf::thread_instructions();
            a.tick(1.0 / 60.0);
            a.frame_paced();
            v.push(bough::sys::perf::thread_instructions() - i0);
        }
        v.sort();
        println!("TRANSITIONS\tn={n}\tinstr_per_frame_p50={}\tper_transition={:.1}\tactive={}", v[v.len() / 2], v[v.len() / 2] as f64 / n as f64, a.rt.transitions.active);
    }
}
