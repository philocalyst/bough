//! Where a GPU-resident frame's time goes. `gprobe dump FILE` writes the canvas's generated Metal source;
//! `gprobe run FILE..` times a full canvas frame with each given source (variants with parts removed draw
//! wrong pixels on purpose: a measurement tool, never a result about pixels).
use bough_paint::expr::U;
use bough_paint::metal::{Gpu, source};
use bough_paint::scene::Desc;
use bough_paint::scenes::canvas;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (s, _) = canvas::build(20);
    let descs: Vec<&Desc> = s.cols.iter().map(|c| c.desc).collect();
    let lays: Vec<(usize, usize)> = s.cols.iter().map(|c| (c.base, c.desc.words)).collect();
    let good = source(&descs, &lays);
    if args.get(1).map(|a| a.as_str()) == Some("dump") {
        std::fs::write(&args[2], &good).unwrap();
        return;
    }
    let stride = s.fw as usize;
    let words = stride * s.fh as usize;
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
    for path in &args[2..] {
        let src = std::fs::read_to_string(path).unwrap();
        let mut g = Gpu::with_source(&s, words, 192, &src).expect("Metal");
        let u = U::default();
        let mut t = Vec::new();
        for _ in 0..reps {
            g.invalidate();
            t.push(g.frame(&u, None, stride).0.gpu);
        }
        t.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let same = src == good;
        println!("{path}\t{}\tcompile {:.0} ms\tgpu min {:.2} ms\tmedian {:.2} ms", if same { "as generated" } else { "variant" }, g.compile_s * 1e3, t[0] * 1e3, t[t.len() / 2] * 1e3);
    }
}
