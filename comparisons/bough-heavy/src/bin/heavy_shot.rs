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
            println!("tick {_t}: repaired {} tiles", app.repaired_tiles);
        }
    }
    if std::env::var_os("BISECT").is_some() {
        // first tick at which the retained frame differs from a fresh mount of the same state
        let mut work = Heavy::from_env();
        let mut app = App::new(Dash::new(&work), config(2.0), true);
        app.frame();
        for t in 1..=ticks {
            work.tick();
            app.turn(|d| publish(d, &work, mode));
            {
                let fresh = App::new(Dash::new(&work), config(2.0), false);
                let mut a: Vec<_> = app.rt.bough.records().iter().map(|r| (r.ink.0, r.kind, r.depth)).collect();
                let mut b: Vec<_> = fresh.rt.bough.records().iter().map(|r| (r.ink.0, r.kind, r.depth)).collect();
                a.sort();
                b.sort();
                if a != b {
                    println!("ink multiset differs at tick {t} (live {} scroll {})", work.live, work.scroll);
                    let rec = app.rt.bough.records();
                    for (i, r) in rec.iter().enumerate() {
                        if r.ink.0 == [32767; 4] {
                            println!("   empty ink record at pos {i}: kind {} depth {} handle {}", r.kind, r.depth, r.handle);
                        }
                    }
                    break;
                }
            }
            if app.frame() {
                let px = app.presented_pixels();
                let mut fresh = App::new(Dash::new(&work), config(2.0), true);
                fresh.frame();
                let fp = fresh.presented_pixels();
                let n = px.iter().zip(&fp).filter(|(a, b)| a != b).count();
                if n > 0 {
                    println!("first retained != fresh at tick {t}: {n} bytes (live {} scroll {})", work.live, work.scroll);
                    let mut cells = std::collections::BTreeMap::new();
                    for (i, (a, b)) in px.iter().zip(&fp).enumerate() {
                        if a != b {
                            let p = i / 4;
                            *cells.entry(((p % 2560) / 32 * 32, (p / 2560) / 32 * 32)).or_insert(0u32) += 1;
                        }
                    }
                    let mut a: Vec<_> = app.rt.bough.records().iter().map(|r| (r.ink.0, r.kind, r.depth)).collect();
                    let mut b: Vec<_> = fresh.rt.bough.records().iter().map(|r| (r.ink.0, r.kind, r.depth)).collect();
                    println!("   records retained {} fresh {}", a.len(), b.len());
                    a.sort();
                    b.sort();
                    let (mut i, mut j) = (0, 0);
                    let mut shown = 0;
                    while (i < a.len() || j < b.len()) && shown < 10 {
                        if j >= b.len() || (i < a.len() && a[i] < b[j]) {
                            println!("   only retained: {:?}", a[i]);
                            i += 1;
                            shown += 1;
                        } else if i >= a.len() || b[j] < a[i] {
                            println!("   only fresh: {:?}", b[j]);
                            j += 1;
                            shown += 1;
                        } else {
                            i += 1;
                            j += 1;
                        }
                    }
                    for ((x, y), n) in cells.iter().take(12) {
                        println!("   cell x={x} y={y}: {n}");
                    }
                    break;
                }
            }
        }
    }
    let px = app.presented_pixels();
    let reference = app.reference_pixels();
    let reference = if std::env::var_os("FRESH").is_some() {
        let mut fresh = App::new(Dash::new(&work), config(2.0), true);
        fresh.frame();
        fresh.presented_pixels()
    } else {
        reference
    };
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
