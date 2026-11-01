//! Render the heavy scene offscreen for `TICKS` ticks of `ZGX_MODE` (default both) and write the
//! last presented frame as raw BGRA (`OUT`, default heavy.bgra); also compares it with a full
//! redraw of the same state and prints the number of differing bytes.
use bough::prelude::*;
use bough_heavy::*;
use zgx_workload::heavy::*;

fn main() {
    let ticks: usize = std::env::var("TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(121);
    let out = std::env::var("OUT").unwrap_or("heavy.bgra".into());
    let mut work = Heavy::from_env();
    let mode = work.mode;
    let mut app = App::new(Dash::new(&work), config(2.0), true);
    app.frame();
    for _t in 0..ticks {
        work.tick();
        app.turn(|d| publish(d, &work, mode));
        app.frame();
        if std::env::var_os("DBG").is_some() && _t < 8 || _t + 4 > ticks {
            println!("tick {_t}: rects {:?} repaired {} px", app.debug_rects(), app.repaired_px);
        }
    }
    let px = app.presented_pixels();
    let reference = app.reference_pixels();
    let diff = px.iter().zip(&reference).filter(|(a, b)| a != b).count();
    println!("ticks {ticks} mode {mode:?}: frames {} differing bytes vs full redraw: {diff}", app.frames);
    if diff > 0 {
        let mut cells = std::collections::BTreeMap::new();
        for (i, (a, b)) in px.iter().zip(&reference).enumerate() {
            if a != b {
                let p = i / 4;
                *cells.entry(((p % 2560) / 64, (p / 2560) / 64)).or_insert(0u32) += 1;
            }
        }
        let mut shown = 0;
        for (i, (a, b)) in px.iter().zip(&reference).enumerate() {
            if a != b && shown < 12 {
                let p = i / 4;
                println!("  px ({}, {}) ch {}: presented {a} reference {b}", p % 2560, p / 2560, i % 4);
                shown += 1;
            }
        }
        for ((cx, cy), n) in cells.iter().take(40) {
            println!("  diff cell x={} y={} (64px): {n} bytes", cx * 64, cy * 64);
        }
    }
    std::fs::write(&out, &px).unwrap();
}
