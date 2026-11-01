//! Instructions per scene write (headless): what a binding pays before any frame work.
use bough_metal::gpu::ShaderSource;
use bough_metal::perf::*;
use bough_metal::present::Engine;
use bough_metal::scene::Rng;
fn main() {
    let mut e = Engine::new(ShaderSource::Metallib);
    let mut rng = Rng(5);
    let ovh = tinstr_overhead();
    for z in [false, true] {
        if z {
            e.scene.enable_zobrist();
        }
        for (name, n) in [("op_scroll(7)", 0), ("op_one_colour", 1), ("op_scattered(80)", 2), ("op_move_card", 3)] {
            let mut v = vec![];
            for _ in 0..200 {
                let t0 = tinstr();
                match n {
                    0 => e.scene.op_scroll(7.0),
                    1 => e.scene.op_one_colour(&mut rng),
                    2 => e.scene.op_scattered(&mut rng, 80),
                    _ => e.scene.op_move_card(&mut rng),
                }
                v.push((tinstr() - t0).saturating_sub(ovh) as f64);
                e.scene.acc.clear();
            }
            println!("zobrist={z} {name}: {:.0} instr (median)", pct(&mut v, 0.5));
        }
    }
}
