//! Cull work counters per frame for the benchmark shapes (build with `--features stats`).
use bough_core::Plane;
use bough_core::shapes::*;

fn main() {
    for shape in all() {
        let mut b = build_bough(&shape);
        let targets = scatter(&shape, 256, 11);
        let hs: Vec<_> = targets.iter().map(|&t| t).collect();
        let mut pl = Plane::EMPTY;
        let mut out = Vec::new();
        for (name, nw) in [("write1", 1usize), ("write80", 80)] {
            let frames = 64;
            let mut tot_out = 0;
            let mut tiles = 0;
            #[cfg(feature = "stats")]
            bough_core::stats::take();
            for f in 0..frames {
                for j in 0..nw {
                    b.touch(hs[(f * nw + j * 3) & 255]);
                }
                let bb = b.repair_into(1, &mut pl);
                tiles += pl.count();
                b.cull_tiles(&pl, bb, &mut out);
                tot_out += out.len();
                b.next_epoch();
            }
            print!("{:10} {:8} out/frame {:7.1} tiles/frame {:6.1}", shape.name, name, tot_out as f64 / frames as f64, tiles as f64 / frames as f64);
            #[cfg(feature = "stats")]
            {
                let c = bough_core::stats::take();
                for (i, n) in bough_core::stats::NAMES.iter().enumerate() {
                    print!(" {n} {:.1}", c[i] as f64 / frames as f64);
                }
            }
            println!();
        }
    }
}
