//! Bough vs ArenaTree (zgx scene model): retired instructions and ns per operation.
//!
//! macOS only for instruction counts: `proc_pid_rusage(getpid(), RUSAGE_INFO_V4)` -> ri_instructions.
//! Single-threaded. Run: `cargo run --release --example bench` (or `--profile dist`).
//! Env: BENCH_FILTER=substring to select ops, BENCH_QUICK=1 for short runs.

use bough_core::arena::{ArenaTree, NodeId, Rect};
use bough_core::bough::Handle;
use bough_core::rng::Rng;
use bough_core::shapes::*;
use bough_core::shapes::Spec;
use bough_core::{Bough, NRect, Plane};
use std::hint::black_box;
use std::time::Instant;

// ------------------------------------------------------------------------------------ counters

#[repr(C)]
struct RusageInfoV4 {
    uuid: [u8; 16],
    f: [u64; 35],
}
unsafe extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV4) -> i32;
    fn getpid() -> i32;
}
fn counters() -> (u64, u64) {
    let mut r = RusageInfoV4 { uuid: [0; 16], f: [0; 35] };
    let rc = unsafe { proc_pid_rusage(getpid(), 4, &mut r) };
    assert_eq!(rc, 0, "proc_pid_rusage failed");
    // ri_instructions / ri_cycles: after 10 (v0) + 6 (v1) + 2 (v2) + 9 (v3) + ri_logical_writes + ri_lifetime_max_phys_footprint
    (r.f[29], r.f[30])
}

#[derive(Clone, Copy, Default)]
struct M {
    instr: f64,
    cycles: f64,
    ns: f64,
}

fn quick() -> bool {
    std::env::var("BENCH_QUICK").is_ok()
}

/// Per-iteration cost of `f`: calibrate a batch of >= 15 ms, then median of 7 batches.
fn measure(mut f: impl FnMut()) -> M {
    let target = if quick() { 0.004 } else { 0.015 };
    let mut n = 1u64;
    loop {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        let e = t.elapsed().as_secs_f64();
        if e >= target || n >= 1 << 26 {
            break;
        }
        n = if e < target / 16.0 { n * 8 } else { n * 2 };
    }
    let mut samples: Vec<M> = Vec::new();
    for _ in 0..if quick() { 3 } else { 7 } {
        let (i0, c0) = counters();
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        let e = t.elapsed().as_secs_f64();
        let (i1, c1) = counters();
        samples.push(M { instr: (i1 - i0) as f64 / n as f64, cycles: (c1 - c0) as f64 / n as f64, ns: e * 1e9 / n as f64 });
    }
    // instructions: median (near-deterministic); ns and cycles: best sample (other processes on the
    // machine only ever add time).
    samples.sort_by(|a, b| a.instr.partial_cmp(&b.instr).unwrap());
    let med_i = samples[samples.len() / 2].instr;
    let ns = samples.iter().map(|m| m.ns).fold(f64::MAX, f64::min);
    let cycles = samples.iter().map(|m| m.cycles).fold(f64::MAX, f64::min);
    M { instr: med_i, cycles, ns }
}

fn baseline() -> M {
    let mut x = 0u64;
    measure(|| {
        x = black_box(x.wrapping_add(1));
    })
}

// ------------------------------------------------------------------------------------ context

struct Ctx {
    shape: Shape,
    b: Bough,
    a: ArenaTree,
    ids: Vec<NodeId>,
    targets: Vec<usize>,
    handles: Vec<Handle>,
    points: Vec<(i32, i32)>,
    qpos: Vec<usize>,
}

impl Ctx {
    fn new(shape: Shape) -> Ctx {
        let mut b = build_bough(&shape);
        let (a, ids) = build_arena(&shape);
        let targets = scatter(&shape, 256, 11);
        let mut handles: Vec<Handle> = Vec::new();
        let mut seen = std::collections::HashMap::new();
        for &t in &targets {
            let h = *seen.entry(t).or_insert_with(|| b.attach_handle(t));
            handles.push(h);
        }
        let mut rng = Rng::new(5);
        let points = (0..64).map(|_| (rng.range(0, shape.window.0 - 1), rng.range(0, shape.window.1 - 1))).collect();
        let churn = shape.list.as_ref().map(|l| l.first_row..l.first_row + l.rows * l.row_len);
        let qpos = (0..64)
            .map(|_| loop {
                let p = 1 + rng.usize_below(shape.specs.len() - 1);
                if !churn.as_ref().is_some_and(|c| c.contains(&p)) {
                    break p;
                }
            })
            .collect();
        Ctx { shape, b, a, ids, targets, handles, points, qpos }
    }
}

struct Row {
    name: String,
    shape: &'static str,
    bough: M,
    arena: M,
    extra: Vec<(&'static str, M)>,
}

fn want(filter: &Option<String>, name: &str) -> bool {
    filter.as_ref().is_none_or(|f| name.contains(f.as_str()))
}

fn main() {
    let filter = std::env::var("BENCH_FILTER").ok();
    let base = baseline();
    println!("# bough-core bench ({})", if cfg!(debug_assertions) { "DEBUG BUILD - numbers meaningless" } else { "optimized" });
    println!("loop baseline: {:.1} instr, {:.2} ns per empty iteration (subtracted from all rows)", base.instr, base.ns);
    println!("page-start prefix: {}", bough_core::prefix::NAME);
    let sub = |m: M| M { instr: (m.instr - base.instr).max(0.0), cycles: (m.cycles - base.cycles).max(0.0), ns: (m.ns - base.ns).max(0.0) };

    let mut rows: Vec<Row> = Vec::new();
    let mut mem: Vec<(String, usize, f64, f64, usize)> = Vec::new();
    let mut mount: Vec<(String, M, M)> = Vec::new();

    for shape in all() {
        let sname = shape.name;
        // ---------------------------------------------------------------- mount + memory
        {
            let mb = measure(|| {
                black_box(build_bough(&shape));
            });
            let ma = measure(|| {
                black_box(build_arena(&shape));
            });
            mount.push((sname.to_string(), mb, ma));
        }
        let mut c = Ctx::new(shape);
        let n = c.shape.specs.len();
        mem.push((sname.to_string(), n, c.b.heap_bytes() as f64 / n as f64, c.a.heap_bytes() as f64 / n as f64, c.b.num_pages()));
        let window = c.b.window();
        let mut plane = Plane::EMPTY;
        let mut out: Vec<u32> = Vec::with_capacity(1 << 17);
        let mut out_a: Vec<u32> = Vec::with_capacity(1 << 17);

        macro_rules! op {
            ($name:expr, $bough:expr, $arena:expr) => {{
                let name = $name;
                if want(&filter, name) {
                    let mb = sub(measure($bough));
                    let ma = sub(measure($arena));
                    eprintln!("  {sname:10} {name:28} bough {:8.0} instr {:9.0} ns | arena {:9.0} instr {:9.0} ns", mb.instr, mb.ns, ma.instr, ma.ns);
                    rows.push(Row { name: name.to_string(), shape: sname, bough: mb, arena: ma, extra: vec![] });
                }
            }};
        }

        // ---------------------------------------------------------------- single paint write
        {
            let (mut k, mut k2) = (0usize, 0usize);
            let (b, a) = (&mut c.b, &mut c.a);
            let (handles, ids, targets) = (&c.handles, &c.ids, &c.targets);
            let (pl, o, oa) = (&mut plane, &mut out, &mut out_a);
            op!(
                "write1+cull(repair1)",
                || {
                    let h = handles[k & 255];
                    k += 1;
                    b.touch_handle(h);
                    b.repair_into(1, pl);
                    b.cull(pl.bbox, None, o);
                    b.next_epoch();
                    black_box(o.len());
                },
                || {
                    let id = ids[targets[k2 & 255]];
                    k2 += 1;
                    a.mark_paint(id);
                    a.cull_repair(1, oa);
                    a.end_frame();
                    black_box(oa.len());
                }
            );
        }
        // Bough variants: position-addressed write, tile-plane cull
        if want(&filter, "write1") {
            let mut k = 0usize;
            let (b, pl, o) = (&mut c.b, &mut plane, &mut out);
            let pos: Vec<usize> = c.targets.clone();
            let m_pos = sub(measure(|| {
                b.touch(pos[k & 255]);
                k += 1;
                b.repair_into(1, pl);
                b.cull(pl.bbox, None, o);
                b.next_epoch();
                black_box(o.len());
            }));
            let handles = &c.handles;
            let m_tiles = sub(measure(|| {
                b.touch_handle(handles[k & 255]);
                k += 1;
                b.repair_into(1, pl);
                let pc = pl.clone();
                b.cull(pc.bbox, Some(&pc), o);
                b.next_epoch();
                black_box(o.len());
            }));
            let m_write_only = sub(measure(|| {
                b.touch_handle(handles[k & 255]);
                k += 1;
                b.next_epoch();
            }));
            rows.last_mut().unwrap().extra.push(("touch(pos) instead of handle", m_pos));
            rows.last_mut().unwrap().extra.push(("tile-plane cull", m_tiles));
            rows.last_mut().unwrap().extra.push(("write only (touch_handle+next_epoch)", m_write_only));
        }
        if want(&filter, "write1") {
            let mut k2 = 0usize;
            let (a, ids, targets) = (&mut c.a, &c.ids, &c.targets);
            let m = sub(measure(|| {
                a.mark_paint(ids[targets[k2 & 255]]);
                k2 += 1;
                a.end_frame();
            }));
            rows.last_mut().unwrap().extra.push(("ARENA write only (mark_paint+flush)", m));
        }

        // ---------------------------------------------------------------- 80 scattered writes
        {
            let (mut k, mut k2) = (0usize, 0usize);
            let (b, a) = (&mut c.b, &mut c.a);
            let (handles, ids, targets) = (&c.handles, &c.ids, &c.targets);
            let (pl, o, oa) = (&mut plane, &mut out, &mut out_a);
            op!(
                "write80+cull(repair1)",
                || {
                    for j in 0..80 {
                        b.touch_handle(handles[(k + j * 3) & 255]);
                    }
                    k += 80;
                    b.repair_into(1, pl);
                    b.cull(pl.bbox, None, o);
                    b.next_epoch();
                    black_box(o.len());
                },
                || {
                    for j in 0..80 {
                        a.mark_paint(ids[targets[(k2 + j * 3) & 255]]);
                    }
                    k2 += 80;
                    a.cull_repair(1, oa);
                    a.end_frame();
                    black_box(oa.len());
                }
            );
            if want(&filter, "write80") {
                let (b, pl, o) = (&mut c.b, &mut plane, &mut out);
                let handles = &c.handles;
                let mut k = 0;
                let m_tiles = sub(measure(|| {
                    for j in 0..80 {
                        b.touch_handle(handles[(k + j * 3) & 255]);
                    }
                    k += 80;
                    b.repair_into(1, pl);
                    let pc = pl.clone();
                    b.cull(pc.bbox, Some(&pc), o);
                    b.next_epoch();
                    black_box(o.len());
                }));
                rows.last_mut().unwrap().extra.push(("tile-plane cull", m_tiles));
            }
        }

        // ---------------------------------------------------------------- translate ~1k subtree
        if let Some((start, len)) = c.shape.sub1k {
            let (mut k, mut k2) = (0i32, 0i32);
            let (b, a) = (&mut c.b, &mut c.a);
            let root = c.ids[start];
            let (pl, o, oa) = (&mut plane, &mut out, &mut out_a);
            let name = format!("translate{}", len);
            op!(
                &name,
                || {
                    k ^= 1;
                    b.translate_run(start, len, if k == 1 { 4 } else { -4 }, 0);
                    b.repair_into(1, pl);
                    b.cull(pl.bbox, None, o);
                    b.next_epoch();
                    black_box(o.len());
                },
                || {
                    k2 ^= 1;
                    a.translate_subtree(root, if k2 == 1 { 4.0 } else { -4.0 }, 0.0);
                    a.cull_repair(1, oa);
                    a.end_frame();
                    black_box(oa.len());
                }
            );
            if want(&filter, "translate") {
                let (b, pl) = (&mut c.b, &mut plane);
                let mut k = 0;
                let m = sub(measure(|| {
                    k ^= 1;
                    b.translate_run(start, len, if k == 1 { 4 } else { -4 }, 0);
                    b.next_epoch();
                    black_box(&pl);
                }));
                rows.last_mut().unwrap().extra.push(("translate only (no cull)", m));
                let a = &mut c.a;
                let mut k = 0;
                let m = sub(measure(|| {
                    k ^= 1;
                    a.translate_subtree(root, if k == 1 { 4.0 } else { -4.0 }, 0.0);
                    a.end_frame();
                }));
                rows.last_mut().unwrap().extra.push(("ARENA translate only (no cull)", m));
            }
        }

        // ---------------------------------------------------------------- list scroll step
        if let Some(info) = &c.shape.list {
            let (first_row, rows_n, row_len, row_h) = (info.first_row, info.rows, info.row_len, info.row_h);
            let container_id = c.ids[info.container];
            // Row payloads/specs are built once, outside the measured closures.
            let rows_b: Vec<[bough_core::bough::RecordInit; 8]> =
                (0..20).map(|k| { let sp = list_row(48 + k * 32); std::array::from_fn(|i| sp[i].init(7_000_000 + i as u32)) }).collect();
            let rows_a: Vec<Vec<Spec>> = (0..20).map(|k| list_row(48 + k * 32)).collect();
            let add_row = |a: &mut ArenaTree, spec: &[Spec], stack: &mut Vec<NodeId>| {
                stack.clear();
                for (i, s) in spec.iter().enumerate() {
                    let (parent, index) = if i == 0 { (container_id, Some(0)) } else { (stack[(s.depth - 3) as usize], None) };
                    let id = a.add(Some(parent), index, s.rect(), s.paints, s.clips, 7_000_000 + i as u32);
                    stack.truncate((s.depth - 2) as usize);
                    stack.push(id);
                }
            };
            for translate in [false, true] {
                let name = if translate { "scroll-step+translate+cull" } else { "scroll-step(ins8+rm8)+cull" };
                let (mut k, mut k2) = (0usize, 0usize);
                let (b, a) = (&mut c.b, &mut c.a);
                let (pl, o, oa) = (&mut plane, &mut out, &mut out_a);
                let mut stack: Vec<NodeId> = Vec::with_capacity(8);
                let mut kids: Vec<NodeId> = Vec::with_capacity(32);
                op!(
                    name,
                    || {
                        if translate {
                            b.translate_run(first_row, rows_n * row_len, 0, row_h);
                        }
                        let row = if translate { &rows_b[0] } else { &rows_b[k % 20] };
                        b.insert_run(first_row, row);
                        b.remove_run(first_row + rows_n * row_len, row_len);
                        k += 1;
                        b.repair_into(1, pl);
                        b.cull(pl.bbox, None, o);
                        b.next_epoch();
                        black_box(o.len());
                    },
                    || {
                        if translate {
                            kids.clear();
                            kids.extend_from_slice(a.children(container_id));
                            for &kid in &kids {
                                a.translate_subtree(kid, 0.0, row_h as f32);
                            }
                        }
                        let spec = if translate { &rows_a[0] } else { &rows_a[k2 % 20] };
                        add_row(a, spec, &mut stack);
                        let last = *a.children(container_id).last().unwrap();
                        a.remove_subtree(last);
                        k2 += 1;
                        a.cull_repair(1, oa);
                        a.end_frame();
                        black_box(oa.len());
                    }
                );
            }
            // the same step without the cull, to isolate structure + damage cost
            if want(&filter, "ins8") {
                let (b, a) = (&mut c.b, &mut c.a);
                let row = &rows_b[0];
                let m = sub(measure(|| {
                    b.insert_run(first_row, row);
                    b.remove_run(first_row + rows_n * row_len, row_len);
                    b.next_epoch();
                }));
                let mut stack: Vec<NodeId> = Vec::with_capacity(8);
                let ma = sub(measure(|| {
                    add_row(a, &rows_a[0], &mut stack);
                    let last = *a.children(container_id).last().unwrap();
                    a.remove_subtree(last);
                    a.end_frame();
                }));
                rows.push(Row { name: "insert row at top + remove last (structure+damage only, no cull)".into(), shape: sname, bough: m, arena: ma, extra: vec![] });
            }
        }

        // ---------------------------------------------------------------- front splice (prefix pressure)
        if want(&filter, "splice8") {
            let (b, a) = (&mut c.b, &mut c.a);
            let root = c.ids[0];
            let init: [bough_core::bough::RecordInit; 8] = std::array::from_fn(|i| bough_core::bough::RecordInit::new(1, NRect::new(10, 10, 20, 20), 4, 9_000_000 + i as u32));
            let m = sub(measure(|| {
                b.insert_run(1, &init);
                b.remove_run(1, 8);
                b.next_epoch();
            }));
            let ma = sub(measure(|| {
                let mut ids8 = [root; 8];
                for (i, slot) in ids8.iter_mut().enumerate() {
                    *slot = a.add(Some(root), Some(0), Rect::new(10.0, 10.0, 10.0, 10.0), true, false, 9_000_000 + i as u32);
                }
                for id in ids8 {
                    a.remove_subtree(id);
                }
                a.end_frame();
            }));
            eprintln!("  {sname:10} splice8 front                 bough {:8.0} instr {:9.0} ns | arena {:9.0} instr {:9.0} ns", m.instr, m.ns, ma.instr, ma.ns);
            rows.push(Row { name: "splice8-front (insert 8 + remove 8 at the front, no cull)".into(), shape: sname, bough: m, arena: ma, extra: vec![] });
        }

        // ---------------------------------------------------------------- hit test
        {
            let (mut k, mut k2) = (0usize, 0usize);
            let (b, a) = (&c.b, &c.a);
            let pts = &c.points;
            op!(
                "hit",
                || {
                    let (x, y) = pts[k & 63];
                    k += 1;
                    black_box(b.hit(x, y));
                },
                || {
                    let (x, y) = pts[k2 & 63];
                    k2 += 1;
                    black_box(a.hit(x as f32, y as f32));
                }
            );
            if want(&filter, "hit") {
                let mut k = 0;
                let m = sub(measure(|| {
                    let (x, y) = pts[k & 63];
                    k += 1;
                    black_box(a.hit_rev(x as f32, y as f32));
                }));
                rows.last_mut().unwrap().extra.push(("ARENA reverse early-exit hit", m));
            }
        }

        // ---------------------------------------------------------------- repair(3)
        {
            // three frames of 10 scattered writes in the history
            for f in 0..3 {
                for j in 0..10 {
                    c.b.touch_handle(c.handles[(f * 10 + j) * 7 & 255]);
                    c.a.mark_paint(c.ids[c.targets[(f * 10 + j) * 7 & 255]]);
                }
                c.a.end_frame();
                if f < 2 {
                    c.b.next_epoch();
                }
            }
            let (b, a) = (&c.b, &mut c.a);
            let (pl, o, oa) = (&mut plane, &mut out, &mut out_a);
            op!(
                "repair3",
                || {
                    b.repair_into(3, pl);
                    black_box(pl.bbox);
                },
                || {
                    black_box(a.repair(3).len());
                }
            );
            let (b, a) = (&c.b, &mut c.a);
            op!(
                "repair3+cull",
                || {
                    b.repair_into(3, pl);
                    b.cull(pl.bbox, None, o);
                    black_box(o.len());
                },
                || {
                    a.cull_repair(3, oa);
                    black_box(oa.len());
                }
            );
        }

        // ---------------------------------------------------------------- full-window cull
        {
            let (b, a) = (&c.b, &mut c.a);
            let (o, oa) = (&mut out, &mut out_a);
            let full_b = window;
            let full_a = [Rect::new(0.0, 0.0, c.shape.window.0 as f32, c.shape.window.1 as f32)];
            op!(
                "cull(full window)",
                || {
                    b.cull(full_b, None, o);
                    black_box(o.len());
                },
                || {
                    a.cull(&full_a, oa);
                    black_box(oa.len());
                }
            );
        }

        // ---------------------------------------------------------------- structure queries
        {
            let (mut k, mut k2) = (0usize, 0usize);
            let (b, a) = (&c.b, &c.a);
            let (qpos, ids) = (&c.qpos, &c.ids);
            op!(
                "subtree_end",
                || {
                    black_box(b.subtree_end(qpos[k & 63]));
                    k += 1;
                },
                || {
                    black_box(a.after_subtree(ids[qpos[k2 & 63]]));
                    k2 += 1;
                }
            );
            op!(
                "parent",
                || {
                    black_box(b.parent(qpos[k & 63]));
                    k += 1;
                },
                || {
                    black_box(a.parent(ids[qpos[k2 & 63]]));
                    k2 += 1;
                }
            );
            op!(
                "children(count) of a deep-ish node",
                || {
                    black_box(b.children(qpos[k & 63]).count());
                    k += 1;
                },
                || {
                    black_box(a.children(ids[qpos[k2 & 63]]).len());
                    k2 += 1;
                }
            );
        }
        let _ = (&c.points, &c.qpos);
        let _ = NRect::EMPTY;
    }

    // ------------------------------------------------------------------------------ report
    println!("\n## Per-operation cost (Bough vs ArenaTree), loop baseline subtracted\n");
    let mut by_op: Vec<String> = Vec::new();
    for r in &rows {
        if !by_op.contains(&r.name) {
            by_op.push(r.name.clone());
        }
    }
    for opn in &by_op {
        println!("### {opn}\n");
        println!("| shape | Bough instr | Bough ns | Bough cyc | Arena instr | Arena ns | Arena cyc | instr ratio (Arena/Bough) | ns ratio |");
        println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
        for r in rows.iter().filter(|r| &r.name == opn) {
            println!(
                "| {} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.1}x | {:.1}x |",
                r.shape,
                r.bough.instr,
                r.bough.ns,
                r.bough.cycles,
                r.arena.instr,
                r.arena.ns,
                r.arena.cycles,
                r.arena.instr / r.bough.instr.max(1.0),
                r.arena.ns / r.bough.ns.max(0.01)
            );
        }
        let extras: Vec<_> = rows.iter().filter(|r| &r.name == opn && !r.extra.is_empty()).collect();
        if !extras.is_empty() {
            println!("\nvariants:\n");
            println!("| shape | variant | instr | ns | cyc |");
            println!("|---|---|---:|---:|---:|");
            for r in extras {
                for (n, m) in &r.extra {
                    println!("| {} | {} | {:.0} | {:.0} | {:.0} |", r.shape, n, m.instr, m.ns, m.cycles);
                }
            }
        }
        println!();
    }
    println!("### Mount (build whole tree from specs)\n");
    println!("| shape | nodes | Bough instr/node | Bough ns/node | Arena instr/node | Arena ns/node |");
    println!("|---|---:|---:|---:|---:|---:|");
    for ((name, mb, ma), (_, n, _, _, _)) in mount.iter().zip(mem.iter()) {
        let n = *n as f64;
        println!("| {} | {} | {:.0} | {:.0} | {:.0} | {:.0} |", name, n, mb.instr / n, mb.ns / n, ma.instr / n, ma.ns / n);
    }
    println!("\n### Retained memory\n");
    println!("| shape | nodes | Bough pages | page fill | Bough bytes/node | Arena bytes/node (lean model) | arena size_of Slot |");
    println!("|---|---:|---:|---:|---:|---:|---:|");
    for (name, n, bb, ab, pages) in &mem {
        println!("| {} | {} | {} | {:.0}% | {:.1} | {:.1} | {} B |", name, n, pages, *n as f64 / (*pages as f64 * 64.0) * 100.0, bb, ab, ArenaTree::node_bytes());
    }
}
