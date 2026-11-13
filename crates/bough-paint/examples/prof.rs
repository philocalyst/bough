//! Where a full canvas frame's CPU time goes, by archetype (`BOUGH_PAINT_PROF=1 prof [artboards]`).
use bough_kernel::cpu::Pool;
use bough_paint::expr::U;
use bough_paint::plan::short;
use bough_paint::scenes::canvas;
fn main() {
    let n = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(20);
    let (mut s, _) = canvas::build(n);
    let mut px = vec![0u32; (s.fw * s.fh) as usize];
    let pool = Pool::new(0);
    let t = std::time::Instant::now();
    let fs = s.frame(&U::default(), &mut px, s.fw as usize, &pool, 0);
    println!("full frame {:.1} ms, {} tiles, {} entries, rows shaded {} copied {}", t.elapsed().as_secs_f64() * 1e3, fs.tiles, fs.raster.entries, fs.raster.rows_shaded, fs.raster.rows_copied);
    println!("shadow pixels {} of which corner integrals {}", fs.raster.px_shadow, fs.raster.px_corner);
    let p = bough_paint::scene::PROF.lock().unwrap();
    for (c, col) in s.cols().iter().enumerate() {
        let n = short(col.desc.name);
        println!("{c:>2} {:>9.1} ms {:>8} entries {:>7.0} ns/entry  {}", p[c].0 as f64 / 1e6, p[c].1, p[c].0 as f64 / p[c].1.max(1) as f64, &n[..n.len().min(70)]);
    }
}
