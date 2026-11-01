//! A/B of eager `set_ink` vs batched `set_inks` on flat100k (diagnostic).
use bough_core::NRect;
use bough_core::rng::Rng;
use bough_core::shapes::*;
use bough_damage_lab::measure;

fn main() {
    let ov = measure::overhead();
    for shape in [dashboard(), flat100k()] {
        let mut b = build_bough(&shape);
        let n = shape.specs.len();
        let mut rng = Rng::new(9);
        let mut pos: Vec<usize> = (0..200).map(|_| 1 + rng.usize_below(n - 1)).collect();
        pos.sort_unstable();
        let inks: Vec<NRect> = pos.iter().map(|&p| b.ink_at(p)).collect();
        let alt: Vec<NRect> = inks.iter().map(|r| if r.is_empty() { *r } else { r.translate(1, 0) }).collect();
        for dmg in [false, true] {
            let mut tot = [0u64; 2];
            for it in 0..200 {
                let src = if it & 1 == 1 { &alt } else { &inks };
                let ws: Vec<(usize, NRect)> = pos.iter().zip(src.iter()).map(|(&p, &r)| (p, r)).collect();
                let (i0, _) = measure::counters();
                if it % 4 < 2 {
                    for &(p, r) in &ws {
                        if dmg { b.set_ink(p, r) } else { b.set_ink_nodamage(p, r) }
                    }
                } else {
                    b.set_inks(&ws, dmg);
                }
                let (i1, _) = measure::counters();
                tot[(it % 4 >= 2) as usize] += i1 - i0 - ov;
                b.next_epoch();
            }
            println!("{:10} damage={dmg}: eager {:.0} instr/write, batched {:.0} instr/write", shape.name, tot[0] as f64 / 100.0 / 200.0, tot[1] as f64 / 100.0 / 200.0);
        }
    }
}
