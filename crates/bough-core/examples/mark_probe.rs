//! Instructions per `damage_rect` / per lab-style mark over a fixed rect stream (diagnostic).
use bough_core::{Bough, NRect};
use bough_damage_lab::measure;
use std::hint::black_box;

fn main() {
    let ov = measure::overhead();
    let mut rng = bough_core::rng::Rng::new(3);
    let rs: Vec<NRect> = (0..100_000)
        .map(|_| {
            let (x, y) = (rng.range(0, 2500), rng.range(0, 1550));
            NRect::new(x, y, x + rng.range(4, 30), y + rng.range(10, 40))
        })
        .collect();
    let mut b = Bough::new(2560, 1600);
    for round in 0..3 {
        let (i0, _) = measure::counters();
        for r in &rs {
            b.damage_rect(*r);
        }
        let (i1, _) = measure::counters();
        b.next_epoch();
        let (i2, _) = measure::counters();
        for r in &rs {
            b.damage_inside(*r);
        }
        let (i3, _) = measure::counters();
        b.next_epoch();
        let mut hw = bough_damage_lab::forms::hw::Hw::new(1600);
        let rr: Vec<bough_damage_lab::geom::R> = rs.iter().map(|r| { let [a, b, c, d] = r.to_xyxy(); bough_damage_lab::geom::R::new(a, b, c, d) }).collect();
        let (i4, _) = measure::counters();
        for r in &rr {
            hw.mark(*r);
        }
        let (i5, _) = measure::counters();
        black_box(&hw);
        let n = rs.len() as f64;
        println!("round {round}: damage_rect {:.1} | damage_inside {:.1} | lab Hw::mark {:.1} instr/mark; advance {}", (i1 - i0 - ov) as f64 / n, (i3 - i2 - ov) as f64 / n, (i5 - i4 - ov) as f64 / n, i2 - i1);
    }
    black_box(&b);
}
