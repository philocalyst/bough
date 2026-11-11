//! Proof that the GPU run expansion (bough-text format: 6-byte glyph slots + 32-byte window headers, expanded in the
//! bin pass) renders byte-identically to the same text as 48-byte glyph instances, on the heavy text load.
//! Same scene state, mirror switched between instance mode and runs mode, full redraw through the binned kernels.
//!   BOUGH_HEAVYTEXT=1 runs_check [steps=60]
use bough_metal::gpu::ShaderSource;
use bough_metal::present::Engine;
use bough_metal::scene::Rng;
fn main() {
    assert!(std::env::var_os("BOUGH_HEAVYTEXT").is_some(), "run with BOUGH_HEAVYTEXT=1");
    let steps: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(60);
    let mut e = Engine::new(ShaderSource::Metallib);
    let mut rng = Rng(99);
    e.scene.gpu_runs = true;
    e.scene.rebuild_mirror();
    let nrec = e.scene.recs.len();
    e.scene.gpu_runs = false;
    e.scene.rebuild_mirror();
    let glyphs: usize = e.scene.runs.iter().map(|r| r.1).sum();
    println!("{} instances ({} glyphs in {} text runs: {} numeric labels, {} log lines), {} records in runs mode",
        e.scene.n, glyphs, e.scene.runs.len(), e.scene.numeric.len(), e.scene.log_runs.len(), nrec);
    let mut bad = 0;
    for st in 0..steps {
        match st % 6 {
            0 => e.scene.op_numeric(&mut rng),
            1 => e.scene.op_log(&mut rng),
            2 => e.scene.op_one_colour(&mut rng),
            3 => e.scene.op_scroll(31.0),
            4 => e.scene.op_move_card(&mut rng),
            _ => e.scene.op_scattered(&mut rng, 20),
        }
        e.scene.acc.clear();
        e.scene.gpu_runs = false;
        e.scene.rebuild_mirror();
        let a = e.gpu.reference_binned();
        e.scene.gpu_runs = true;
        e.scene.rebuild_mirror();
        let b = e.gpu.reference_binned();
        if a != b {
            bad += 1;
            let n = a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x != y).count();
            println!("  step {st}: {n} px differ");
        }
    }
    println!("{steps} steps: {bad} differ (instances vs GPU-expanded runs)");
    if bad > 0 { std::process::exit(1) }
}
