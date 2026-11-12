//! Replay the damage lab's zgx-derived traces (crates/bough-damage-lab) through the Bough and
//! compare per-frame instructions with the lab's own `hw32` form and its zgx model, in-process.
//!
//! Protocol = the lab's (`bench::run`): warm up `age - 1` frames, then for every presented frame
//! A = tree update (record ink), B = A + damage marks (the lab's exact symmetric-difference rules,
//! via `Bough::damage_rect`), C = B + `repair_into(age)` + `cull_tiles`. track = B - A, cull = C - B.
//! Every frame's draw list is first checked against the lab's `hw32` cull (identical slot lists).
//!
//! Run: cargo run -p bough-core --release --example trace_bench [-- frames] (env TRACE=name filter)

use bough_core::bough::RecordInit;
use bough_core::{Bough, NRect, Plane};
use bough_damage_lab::forms::{self, Out, registry};
use bough_damage_lab::geom::R;
use bough_damage_lab::trace::Trace;
use bough_damage_lab::{bench, measure, scene::Scene, workloads};
use std::hint::black_box;

#[path = "support/cost_r6.rs"]
mod cost_r6;

/// The lab's `N::from_r` (its rects are already inside the viewport, so no clamping).
#[inline(always)]
fn nr(r: R) -> NRect {
    if r.is_empty() { NRect::EMPTY } else { NRect([r.x0 as i16, r.y0 as i16, (-r.x1) as i16, (-r.y1) as i16]) }
}

fn build(tr: &Trace) -> Bough {
    let mut b = Bough::new(tr.w, tr.h);
    let mut recs = Vec::with_capacity(tr.slots + 1);
    recs.push(RecordInit::new(0, NRect::EMPTY, 0, u32::MAX));
    for (s, st) in tr.init.iter().enumerate() {
        recs.push(RecordInit::new(1, nr(st.cov), tr.kinds[s], s as u32));
    }
    b.insert_run(0, &recs);
    b.next_epoch();
    b
}

/// Tree update for one frame: every changed slot's new coverage, as one batched `set_inks`
/// (positions ascending within each event; `BATCH=0` replays them one `set_ink_nodamage` at a time).
#[inline(always)]
fn apply(b: &mut Bough, f: &bough_damage_lab::trace::Frame, ws: &mut Vec<(usize, NRect)>) {
    ws.clear();
    for g in &f.groups {
        for j in g.at as usize..(g.at + g.n) as usize {
            ws.push((1 + f.gslots[j] as usize, nr(f.gnew[j].cov)));
        }
    }
    for e in &f.nodes {
        for k in 0..e.n as usize {
            ws.push((1 + e.slot0 as usize + k, nr(f.news[e.at as usize + k].cov)));
        }
    }
    if batch() {
        b.set_inks(ws, false);
    } else {
        for &(p, ink) in ws.iter() {
            b.set_ink_nodamage(p, ink);
        }
    }
}

fn batch() -> bool {
    static B: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *B.get_or_init(|| std::env::var("BATCH").map(|v| v != "0").unwrap_or(true))
}

/// The lab's exact marks; its rects are clipped coverage (inside the viewport), the binding path.
#[inline(always)]
fn mark(b: &mut Bough, f: &bough_damage_lab::trace::Frame) {
    forms::exact_nodes_group_bounds(f, &mut |r| b.damage_inside(nr(r)));
}

/// One replay; returns (instructions, ns) over the presented frames.
fn run(tr: &Trace, age: usize, stage: u8, ov: u64) -> (u64, f64) {
    let mut b = build(tr);
    let mut pl = Plane::EMPTY;
    let mut out: Vec<u32> = Vec::with_capacity(tr.slots + 64);
    let mut ws: Vec<(usize, NRect)> = Vec::with_capacity(tr.slots);
    let nf = tr.frames.len();
    for n in 1..age.min(nf + 1) {
        let f = &tr.frames[n - 1];
        apply(&mut b, f, &mut ws);
        mark(&mut b, f);
        b.next_epoch();
    }
    let mut sink = 0usize;
    let (i0, _) = measure::counters();
    let t0 = measure::ticks();
    for n in age..=nf {
        let f = &tr.frames[n - 1];
        apply(&mut b, f, &mut ws);
        if stage == 9 {
            // the lab's exact-difference rules with a counting sink: their cost without any mark
            let mut c = 0usize;
            forms::exact_nodes_group_bounds(f, &mut |r| c += black_box(r).x0 as usize);
            sink = sink.wrapping_add(c);
        } else if stage >= 1 {
            mark(&mut b, f);
        }
        if stage >= 2 && stage != 9 {
            let bb = b.repair_into(age as u32, &mut pl);
            b.cull_tiles(&pl, bb, &mut out);
            sink = sink.wrapping_add(out.len());
        }
        b.next_epoch();
    }
    let t1 = measure::ticks();
    let (i1, _) = measure::counters();
    black_box(sink);
    ((i1 - i0).saturating_sub(ov), measure::ticks_to_ns(t1 - t0))
}

fn best(tr: &Trace, age: usize, stage: u8, ov: u64, reps: usize) -> (u64, f64) {
    let mut b = (u64::MAX, f64::MAX);
    for _ in 0..reps {
        let (i, t) = run(tr, age, stage, ov);
        b = (b.0.min(i), b.1.min(t));
    }
    b
}

/// Frame-by-frame differential check against the lab's hw32 cull.
fn verify(tr: &Trace, age: usize) -> usize {
    let mut b = build(tr);
    let mut sc = Scene::new(tr);
    let mut form = registry::make("hw32");
    form.reset(tr, &sc, age);
    form.full(0, &sc);
    let mut lab = Out::default();
    let mut pl = Plane::EMPTY;
    let mut out = Vec::new();
    let mut ws: Vec<(usize, NRect)> = Vec::new();
    let nf = tr.frames.len();
    let mut checked = 0;
    for n in 1..=nf {
        let f = &tr.frames[n - 1];
        sc.apply(f);
        form.record(f, &sc);
        apply(&mut b, f, &mut ws);
        mark(&mut b, f);
        if n >= age {
            lab.clear();
            form.repair(n % age, age as u32, &sc, &mut lab);
            let bb = b.repair_into(age as u32, &mut pl);
            b.cull_tiles(&pl, bb, &mut out);
            assert_eq!(out, lab.list, "{} age {age} frame {n}: draw lists differ", tr.name);
            checked += 1;
        } else {
            form.full(n % age, &sc);
        }
        b.next_epoch();
    }
    checked
}

/// Instructions per mark in isolation: all rects of the trace collected first, then marked in a loop.
fn mark_cost(tr: &Trace, ov: u64) -> (f64, f64) {
    let mut rs: Vec<R> = Vec::new();
    for f in &tr.frames {
        forms::exact_nodes_group_bounds(f, &mut |r| rs.push(r));
    }
    let mut b = build(tr);
    let mut lab = forms::hw::Hw::new(tr.h);
    let (i0, _) = measure::counters();
    for r in &rs {
        b.damage_inside(nr(*r));
    }
    let (i1, _) = measure::counters();
    for r in &rs {
        lab.mark(*r);
    }
    let (i2, _) = measure::counters();
    black_box(&b);
    black_box(&lab);
    let n = rs.len() as f64;
    let (mut rows, mut two) = (0usize, 0usize);
    for r in &rs {
        let (ty0, ty1) = (r.y0 >> 5, ((r.y1 - 1) >> 5) + 1);
        let (tx0, tx1) = (r.x0 >> 5, ((r.x1 - 1) >> 5) + 1);
        rows += (ty1 - ty0) as usize;
        two += ((tx0 >> 6) != ((tx1 - 1) >> 6)) as usize;
    }
    eprintln!("{}: {} marks, {:.2} rows/mark, {:.1}% straddle tile column 64", tr.name, rs.len(), rows as f64 / n, 100.0 * two as f64 / n);
    (((i1 - i0).saturating_sub(ov)) as f64 / n, ((i2 - i1).saturating_sub(ov)) as f64 / n)
}

fn main() {
    if std::env::args().any(|a| a == "--cost-r6") {
        cost_r6::run(true);
        return;
    }
    let frames: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(600);
    let filter = std::env::var("TRACE").ok();
    measure::prefer_p_cores();
    let ov = measure::overhead();
    let reps = 3;
    println!("| trace | age | frames | marks/frame | diff rules only | core tree | core track | core cull | core track+cull | lab hw32 track | lab hw32 cull | lab hw32 track+cull | lab zgx track+cull | track+cull vs hw32 | vs zgx |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for tr in workloads::all(frames) {
        if filter.as_ref().is_some_and(|f| !tr.name.contains(f.as_str())) {
            continue;
        }
        let (mc, ml) = mark_cost(&tr, ov);
        eprintln!("{}: instr per mark in isolation: core {mc:.1}, lab hw32 {ml:.1}", tr.name);
        for age in [1usize, 3] {
            let checked = verify(&tr, age);
            let pres = bench::presented(&tr, age) as f64;
            let a = best(&tr, age, 0, ov, reps);
            let bm = best(&tr, age, 1, ov, reps);
            let c = best(&tr, age, 2, ov, reps);
            let dfo = best(&tr, age, 9, ov, reps);
            let mut nmarks = 0usize;
            for f in &tr.frames[age - 1..] {
                forms::exact_nodes_group_bounds(f, &mut |_| nmarks += 1);
            }
            let (tree, track, cull) = (a.0 as f64 / pres, (bm.0 as f64 - a.0 as f64) / pres, (c.0 as f64 - bm.0 as f64) / pres);
            let mut hw = registry::make("hw32");
            let la = bench::best(&tr, age, None, 0, reps, ov);
            let lb = bench::best(&tr, age, Some(hw.as_mut()), 1, reps, ov);
            let lc = bench::best(&tr, age, Some(hw.as_mut()), 2, reps, ov);
            let mut zg = registry::make("zgx");
            let zb = bench::best(&tr, age, Some(zg.as_mut()), 1, reps, ov);
            let zc = bench::best(&tr, age, Some(zg.as_mut()), 2, reps, ov);
            let (ltrack, lcull) = ((lb.instr as f64 - la.instr as f64) / pres, (lc.instr as f64 - lb.instr as f64) / pres);
            let ztot = (zc.instr as f64 - la.instr as f64) / pres;
            let _ = zb;
            println!(
                "| {} | {} | {} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.2}x | {:.2}x |",
                tr.name,
                age,
                checked,
                nmarks as f64 / pres,
                (dfo.0 as f64 - a.0 as f64) / pres,
                tree,
                track,
                cull,
                track + cull,
                ltrack,
                lcull,
                ltrack + lcull,
                ztot,
                (ltrack + lcull) / (track + cull),
                ztot / (track + cull)
            );
        }
    }
}
