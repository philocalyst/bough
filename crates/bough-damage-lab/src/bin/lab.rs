//! `lab verify|bench|all [--frames N] [--forms a,b,c] [--traces x,y] [--reps R]`
//! Writes results/verify.csv and results/bench.csv under the crate directory.

use bough_damage_lab::{bench, forms::registry, measure, trace::Trace, verify, workloads};
use std::fmt::Write as _;
use std::sync::Mutex;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).cloned().unwrap_or_else(|| "all".into());
    let frames: u64 = arg(&args, "--frames").map(|s| s.parse().unwrap()).unwrap_or(600);
    let reps: usize = arg(&args, "--reps").map(|s| s.parse().unwrap()).unwrap_or(3);
    let forms: Vec<String> = arg(&args, "--forms")
        .map(|s| s.split(',').map(String::from).collect())
        .unwrap_or_else(|| registry::V1.iter().map(|s| s.to_string()).collect());
    let tfilter: Option<Vec<String>> = arg(&args, "--traces").map(|s| s.split(',').map(String::from).collect());
    let rings: Vec<usize> =
        arg(&args, "--rings").map(|s| s.split(',').map(|x| x.parse().unwrap()).collect()).unwrap_or(vec![1, 2, 3]);
    let tag = arg(&args, "--tag").unwrap_or_default();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("results");
    std::fs::create_dir_all(&dir).unwrap();

    let t0 = std::time::Instant::now();
    let mut traces = workloads::all(frames);
    if let Some(f) = &tfilter {
        traces.retain(|t| f.iter().any(|x| t.name == *x));
    }
    eprintln!("traces built in {:.1}s", t0.elapsed().as_secs_f64());
    for t in &traces {
        eprintln!("  {}", t.stats());
    }

    if cmd == "asm-keep" {
        use bough_damage_lab::asm::*;
        #[allow(unused_mut)]
        let mut f = vec![
            lab_plane_mark as *const () as usize,
            lab_plane_or as *const () as usize,
            lab_exact_slot_mark as *const () as usize,
            lab_cull_plane as *const () as usize,
            lab_cull_plane_runs as *const () as usize,
            lab_hw_mark as *const () as usize,
        ];
        #[cfg(target_arch = "aarch64")]
        f.push(lab_cull_hw_neon as *const () as usize);
        println!("{:x?}", std::hint::black_box(f));
        return;
    }
    if cmd == "dump" {
        for tr in &traces {
            dump(tr, &dir);
        }
        return;
    }
    if cmd == "verify" || cmd == "all" {
        run_verify(&traces, &forms, &rings, &dir, &tag);
    }
    if cmd == "bench" || cmd == "all" {
        run_bench(&traces, &forms, &rings, reps, &dir, &tag);
    }
}

fn run_verify(traces: &[Trace], forms: &[String], rings: &[usize], dir: &std::path::Path, tag: &str) {
    let t0 = std::time::Instant::now();
    // Jobs: (trace, ring, chunk of formulations); each renders its own reference.
    let chunks: Vec<Vec<String>> = forms.chunks(3).map(|c| c.to_vec()).collect();
    let mut jl = vec![];
    for (i, _) in traces.iter().enumerate() {
        for &r in rings {
            for c in 0..chunks.len() {
                jl.push((i, r, c));
            }
        }
    }
    jl.reverse();
    let jobs = Mutex::new(jl);
    let results = Mutex::new(vec![]);
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8).min(10);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let Some((ti, ring, c)) = jobs.lock().unwrap().pop() else { break };
                    let tr = &traces[ti];
                    let fs = chunks[c].iter().map(|n| registry::make_any(n)).collect();
                    let (res, oracle) = verify::verify(tr, ring, fs, usize::MAX, c == 0);
                    results.lock().unwrap().push((ti, ring, c, res, oracle));
                }
            });
        }
    });
    let mut raw = results.into_inner().unwrap();
    raw.sort_by_key(|r| (r.0, r.1, r.2));
    let mut results: Vec<(usize, usize, Vec<verify::FormResult>, Vec<verify::OracleFrame>)> = vec![];
    for (ti, ring, c, res, oracle) in raw {
        if c == 0 {
            results.push((ti, ring, res, oracle));
        } else {
            results.last_mut().unwrap().2.extend(res);
        }
    }
    let mut csv = String::from(
        "trace,ring,form,frames,region_px,frags,prims,batches,copy_px,max_region_px,max_prims,fails,mem,oracle_px,oracle_t16,oracle_t8,oracle_t4,t32_s,t32_r,inst_c,oracle_t32\n",
    );
    let mut fails = 0;
    for (ti, ring, res, oracle) in &results {
        let tr = &traces[*ti];
        let nf = oracle.len().max(1) as f64;
        let o = |f: fn(&verify::OracleFrame) -> u64| oracle.iter().map(f).sum::<u64>() as f64 / nf;
        for r in res {
            let n = r.frames.len().max(1) as f64;
            let s = |f: fn(&verify::FrameMetrics) -> u64| r.frames.iter().map(f).sum::<u64>() as f64 / n;
            let mx = |f: fn(&verify::FrameMetrics) -> u64| r.frames.iter().map(f).max().unwrap_or(0);
            writeln!(
                csv,
                "{},{},{},{},{:.1},{:.1},{:.2},{:.2},{:.1},{},{},{},{},{:.1},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}",
                tr.name,
                ring,
                r.name,
                r.frames.len(),
                s(|m| m.region_px),
                s(|m| m.frags),
                s(|m| m.prims),
                s(|m| m.batches),
                s(|m| m.copy_px),
                mx(|m| m.region_px),
                mx(|m| m.prims),
                r.fails,
                r.mem,
                o(|f| f.px),
                o(|f| f.tiles16),
                o(|f| f.tiles8),
                o(|f| f.tiles4),
                s(|m| m.t32_s),
                s(|m| m.t32_r),
                s(|m| m.inst_c),
                o(|f| f.tiles32)
            )
            .unwrap();
            if r.fails > 0 {
                fails += r.fails;
                eprintln!("FAIL {} {}", r.name, r.first_fail.as_deref().unwrap_or(""));
            }
        }
    }
    let path = dir.join(format!("verify{tag}.csv"));
    std::fs::write(&path, &csv).unwrap();
    eprintln!("verify: {} failures, {:.1}s -> {}", fails, t0.elapsed().as_secs_f64(), path.display());
}

fn run_bench(traces: &[Trace], forms: &[String], rings: &[usize], reps: usize, dir: &std::path::Path, tag: &str) {
    measure::prefer_p_cores();
    let ov = measure::overhead();
    eprintln!("counter overhead: {ov} instructions");
    let mut csv = String::from("trace,ring,form,frames,base_instr,base_ns,track_instr,track_ns,cull_instr,cull_ns\n");
    for tr in traces {
        for &ring in rings {
            let nf = bench::presented(tr, ring).max(1) as f64;
            let a = bench::best(tr, ring, None, 0, reps, ov);
            for name in forms {
                let mut f = registry::make(name);
                let b = bench::best(tr, ring, Some(&mut *f), 1, reps, ov);
                let c = bench::best(tr, ring, Some(&mut *f), 2, reps, ov);
                let track_i = b.instr as f64 - a.instr as f64;
                let cull_i = c.instr as f64 - b.instr as f64;
                writeln!(
                    csv,
                    "{},{},{},{},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0}",
                    tr.name,
                    ring,
                    name,
                    nf,
                    a.instr as f64 / nf,
                    a.ns / nf,
                    track_i / nf,
                    (b.ns - a.ns) / nf,
                    cull_i / nf,
                    (c.ns - b.ns) / nf
                )
                .unwrap();
            }
            eprintln!("bench {} ring {} done", tr.name, ring);
        }
    }
    let path = dir.join(format!("bench{tag}.csv"));
    std::fs::write(&path, &csv).unwrap();
    eprintln!("bench -> {}", path.display());
}

/// Binary dump for the GPU experiment (calib/gpu_damage.swift):
/// header [w, h, slots, frames] u32; per frame, per slot 24 bytes:
/// ink x0,y0,x1,y1 (i16) cov x0,y0,x1,y1 (i16) key u32 kind u8 rad u8 pad u16.
/// Then per frame (from frame `ring`), the CPU sig32 repaired tile bitmap for
/// ring 1 and ring 2 (80x50 bits padded to rows of 2 u64).
fn dump(tr: &Trace, dir: &std::path::Path) {
    use bough_damage_lab::{forms::Out, scene::Scene, verify::apply_state};
    let nf = tr.frames.len();
    let mut b: Vec<u8> = vec![];
    for v in [tr.w as u32, tr.h as u32, tr.slots as u32, (nf + 1) as u32] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    let mut st = tr.init.clone();
    let put = |st: &[bough_damage_lab::trace::PState], b: &mut Vec<u8>| {
        for p in st {
            for v in [p.ink.x0, p.ink.y0, p.ink.x1, p.ink.y1, p.cov.x0, p.cov.y0, p.cov.x1, p.cov.y1] {
                b.extend_from_slice(&(v as i16).to_le_bytes());
            }
            b.extend_from_slice(&p.key.to_le_bytes());
            b.extend_from_slice(&[p.kind, p.rad, 0, 0]);
        }
    };
    put(&st, &mut b);
    for f in &tr.frames {
        apply_state(&mut st, f);
        put(&st, &mut b);
    }
    for ring in [1usize, 2] {
        let mut sc = Scene::new(tr);
        let mut form = registry::make("sig32");
        form.reset(tr, &sc, ring);
        form.full(0, &sc);
        let mut out = Out::default();
        for n in 1..=nf {
            sc.apply(&tr.frames[n - 1]);
            form.record(&tr.frames[n - 1], &sc);
            if n < ring {
                form.full(n % ring, &sc);
                continue;
            }
            out.clear();
            form.repair(n % ring, ring as u32, &sc, &mut out);
            let mut bits = vec![0u64; 2 * ((tr.h + 31) / 32) as usize];
            form.region(&mut |r| {
                for ty in r.y0 >> 5..((r.y1 - 1) >> 5) + 1 {
                    for tx in r.x0 >> 5..((r.x1 - 1) >> 5) + 1 {
                        bits[ty as usize * 2 + (tx >> 6) as usize] |= 1 << (tx & 63);
                    }
                }
            });
            for w in bits {
                b.extend_from_slice(&w.to_le_bytes());
            }
        }
    }
    let p = dir.join(format!("dump_{}.bin", tr.name));
    std::fs::write(&p, &b).unwrap();
    eprintln!("dumped {} ({} bytes)", p.display(), b.len());
}
