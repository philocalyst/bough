//! Tile redundancy of heavy frames (Opus collaborator probe; see bough-scenes/examples/opus/stats.rs).
use bough::raw::*;
use bough_heavy::*;
use zgx_workload::heavy::*;
#[path = "../../bough-scenes/examples/opus/stats.rs"]
mod stats;

unsafe extern "C" {
    fn thread_selfcounts(kind: i32, buffer: *mut u64, size: usize) -> i32;
}
fn instrs() -> u64 {
    let mut c = [0u64; 2];
    // SAFETY: kind 1 fills two u64s.
    unsafe { thread_selfcounts(1, c.as_mut_ptr(), 16) };
    c[0]
}

/// CPU cull of one 32x32 tile through bough-core's existing `Bough::cull` (what a CPU repair backend would ask).
fn cull_cost() {
    unsafe { std::env::set_var("ZGX_MODE", "both") };
    let mut work = Heavy::from_env();
    let mode = work.mode;
    let mut app = App::new(Dash::new(&work), config(2.0), false);
    app.frame();
    for _ in 0..30 {
        work.tick();
        app.turn(|d| publish(d, &work, mode));
        app.frame();
    }
    let (w, h) = (app.rt.win_w, app.rt.win_h);
    let mut q = app.rt.bough.records()[0].ink;
    let mut out: Vec<u32> = Vec::with_capacity(4096);
    let (mut n, mut ins, mut recs, mut text, mut maxr) = (0u64, 0u64, 0u64, 0u64, 0usize);
    for rep in 0..3 {
        for ty in 0..(h / 32) {
            for tx in 0..(w / 32) {
                q.0 = [(tx * 32) as i16, (ty * 32) as i16, (-(tx * 32 + 32)) as i16, (-(ty * 32 + 32)) as i16];
                let i0 = instrs();
                app.rt.bough.cull(q, None, &mut out);
                let di = instrs() - i0;
                if rep == 2 {
                    n += 1;
                    ins += di;
                    recs += out.len() as u64;
                    text += out.iter().filter(|p| *p & 0x8000_0000 != 0).count() as u64;
                    maxr = maxr.max(out.len());
                }
            }
        }
    }
    println!("cpu cull of one 32x32 tile (heavy both, {} pages): {:.0} instr per tile, {:.1} records per tile (max {maxr}), {:.2} text records per tile; {n} tiles", app.rt.bough.num_pages(), ins as f64 / n as f64, recs as f64 / n as f64, text as f64 / n as f64);
}

fn main() {
    if std::env::var_os("CULL").is_some() {
        return cull_cost();
    }
    let ticks: usize = std::env::var("TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
    for m in ["stream", "scroll", "both"] {
        // SAFETY: single-threaded, before any reader of the environment
        unsafe { std::env::set_var("ZGX_MODE", m) };
        let mut work = Heavy::from_env();
        let mode = work.mode;
        let mut cfg = config(2.0);
        cfg.ring = std::env::var("RING").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let mut app = App::new(Dash::new(&work), cfg, true);
        app.frame();
        let px = app.presented_pixels();
        let (w, h) = (app.rt.win_w as usize, app.rt.win_h as usize);
        let mut st = stats::Stats::new(w, h);
        st.frame(&px, &[]);
        for _ in 0..ticks {
            work.tick();
            app.turn(|d| publish(d, &work, mode));
            if app.frame() {
                let rows = app.debug_rows().to_vec();
                let px = app.presented_pixels();
                st.frame(&px, &rows);
            }
        }
        st.report(&format!("heavy {m}"));
    }
}
