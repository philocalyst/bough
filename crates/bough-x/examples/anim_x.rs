//! The hot uniforms, counted: what a frame costs while records animate, and what a pointer event costs when
//! records read the pointer words. Instructions and cycles of the calling thread, bracket subtracted, median.
//!
//!   H=208 cargo run --profile dist -p bough-x --example anim_x      (4 rows exposed: 32 + 32 records)
//!   H=10288 ...                                                     (256 rows: 2,048 + 2,048 records)
#[path = "../scenes/anim.rs"]
mod anim;
use anim::*;
use bough_mem::pmu;
use bough_x::prelude::*;
use bough_x::soft::Soft;

fn main() {
    let h: i32 = std::env::var("H").ok().and_then(|v| v.parse().ok()).unwrap_or(208);
    let reps = 64;
    let mut a = Anim::new(100_000, h);
    let mut world = World::<Soft>::new(W, h, NWORDS);
    world.tick(1000);
    world.mount(&a);
    world.tiles.clear();
    let (records, pages, _) = world.root().stats();
    println!("# H={h}: {records} records in {pages} pages; sweeps {}", world.tiles.sweeps.len());
    println!("scene\tphase\tsteps\tinstr_p50\tcycles_p50\ttiles_p50");
    let mut b: Vec<[u64; pmu::MAX]> = (0..64)
        .map(|_| {
            let s = pmu::sample();
            pmu::sample().since(&s).v
        })
        .collect();
    b.sort_by_key(|v| v[0]);
    let bracket = b[32][0];
    b.sort_by_key(|v| v[1]);
    let bracket_c = b[32][1];
    let mut t = 1000u32;
    let mut phase = |name: &str, world: &mut World<Soft>, a: &mut Anim, step: &mut dyn FnMut(&mut World<Soft>, &mut Anim, usize)| {
        let mut v: Vec<(u64, u64, u32)> = (0..reps)
            .map(|k| {
                if name == "fade_start_turn" {
                    // the frame begins (not measured): the turn is what is counted
                    let now = world.now() + 400;
                    world.tick(now);
                    world.tiles.clear();
                }
                let s = pmu::sample();
                step(world, a, k);
                let d = pmu::sample().since(&s);
                let n = world.tiles.count();
                world.tiles.clear();
                (d.v[0].saturating_sub(bracket), d.v[1].saturating_sub(bracket_c), n)
            })
            .collect();
        v.sort();
        let m = v[v.len() / 2];
        println!("anim_x\t{name}\t{reps}\t{}\t{}\t{}", m.0, m.1, m.2);
    };
    phase("frame_endless", &mut world, &mut a, &mut |w, _, _| {
        t += 16;
        w.tick(t);
    });
    phase("frame_endless_and_fade", &mut world, &mut a, &mut |w, a, k| {
        if k % 8 == 0 {
            a.toggle(w.now());
            w.settle(a);
            w.tiles.clear();
        }
        t += 16;
        w.tick(t);
    });
    phase("fade_start_turn", &mut world, &mut a, &mut |w, a, _| {
        a.toggle(w.now());
        w.settle(a);
    });
    t = world.now();
    phase("pointer_words", &mut world, &mut a, &mut |w, _, k| {
        w.pointer_words(700 + (k as i32 % 16) * 9, 100 + (k as i32 % 5) * 7);
    });
    phase("scroll_3_rows", &mut world, &mut a, &mut |w, a, k| {
        a.scroll_by(if (k / 4) % 2 == 0 { 3 * ROW_H } else { -3 * ROW_H });
        w.settle(a);
    });
}
