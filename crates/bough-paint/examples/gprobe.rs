//! Where a GPU-resident frame's time goes. `gprobe dump FILE` writes the canvas's generated Metal source;
//! `gprobe run FILE..` times a full canvas frame with each given source (variants with parts removed draw
//! wrong pixels on purpose: a measurement tool, never a result about pixels).
use bough_paint::expr::U;
use bough_paint::metal::{Gpu, source};
use bough_paint::scene::Desc;
use bough_paint::scenes::canvas;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (mut s, _) = canvas::build(20);
    let descs: Vec<&Desc> = s.cols().iter().map(|c| c.desc).collect();
    let lays: Vec<(usize, usize)> = s.cols().iter().map(|c| (c.base, c.desc.words)).collect();
    let good = source(&descs, &lays, s.tab_region().0);
    if args.get(1).map(|a| a.as_str()) == Some("dump") {
        std::fs::write(&args[2], &good).unwrap();
        return;
    }
    if args.get(1).map(|a| a.as_str()) == Some("cpustats") {
        // one CPU full frame: how many shadow pixels there are, how many are at corners, how many of those a table answers
        let pool = bough_kernel::cpu::Pool::new(5);
        let mut px = vec![0u32; (s.fw * s.fh) as usize];
        let keys = s.tab_jobs().to_vec();
        s.fill_tabs();
        for extra in [0usize, 5] {
            let mut best = (f64::MAX, 0u64);
            for _ in 0..3 {
                s.invalidate();
                let (i0, _) = bough_mem::pmu::instructions_cycles();
                let t = std::time::Instant::now();
                s.frame(&U::default(), &mut px, s.fw as usize, &pool, extra);
                let w = t.elapsed().as_secs_f64();
                let (i1, _) = bough_mem::pmu::instructions_cycles();
                if w < best.0 {
                    best = (w, i1 - i0);
                }
            }
            println!("cpu full frame, {} thread(s): best of 3 {:.1} ms, {:.1} M instructions on the frame thread", extra + 1, best.0 * 1e3, best.1 as f64 / 1e6);
        }
        s.invalidate();
        let fs = s.frame(&U::default(), &mut px, s.fw as usize, &pool, 5);
        println!("tables: {} keys, {} words; frame: {:?}", keys.len(), s.tab_region().1, fs.raster);
        let mut by: std::collections::BTreeMap<i32, (usize, u32)> = Default::default();
        for k in &keys {
            let e = by.entry(k.w / 256).or_default();
            e.0 += 1;
            e.1 = e.1.max(k.n);
        }
        println!("keys by blur width (px): (count, largest side) {by:?}");
        return;
    }
    let stride = s.fw as usize;
    let words = stride * s.fh as usize;
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
    for path in &args[2..] {
        let src = std::fs::read_to_string(path).unwrap();
        let mut g = unsafe { Gpu::with_source(&s, words, 192, &src) }.expect("Metal");
        if std::env::var("NO_TABS").is_err() {
            g.fill_tabs(&mut s);
        }
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
