//! Measurements for the raster lane's journal. One command runs every executor on the same scene, the same
//! block states and the same damage, back to back within each step, so wall times share the machine's load:
//!
//! * `A1`, `AP`: the per-archetype CPU kernel, one core and with pool workers (window pass + bins + shade);
//! * `AG`: the GPU-resident frame (the CPU writes the block and encodes a fixed list of dispatches);
//! * `K1`, `KP`: the round-7 generic kernel's CPU executor (`Binner::bin` + `cpu::raster`) on the same pixels
//!   and the same damaged tiles, with its commands restaged for free (outside the timed region);
//! * `KG`: the generic kernel's Metal executor (upload of commands and program + its tile kernel).
//!
//! `pbench canvas [artboards]` | `pbench anim [elements]`; `REPS` steps per phase (default 24), `WORKERS`
//! pool workers (default 5). Instructions and cycles are the frame thread's (`bough_mem::pmu`); `proc_instr`
//! is the whole process (pool workers included). Every step's surfaces are compared and mismatches counted.
use bough_kernel::cpu::{self, Binner, Damage, Opt, Pool, Prog};
use bough_mem::pmu;
use bough_paint::expr::{U, U_PICK, U_TIME};
use bough_paint::metal::Gpu;
use bough_paint::plan;
use bough_paint::scene::{Desc, Scene};
use bough_paint::scenes::{Model, anim, canvas};
use std::time::Instant;

fn proc_instr() -> u64 {
    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u64) -> i32;
    }
    let mut b = [0u64; 64];
    // SAFETY: flavor 4 (RUSAGE_INFO_V4) fills at most 37 u64 fields; the buffer holds 64.
    unsafe { proc_pid_rusage(std::process::id() as i32, 4, b.as_mut_ptr()) };
    b[35]
}
fn load() -> String {
    let o = std::process::Command::new("uptime").output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    o.split("load averages:").nth(1).unwrap_or("?").trim().to_string()
}

#[derive(Default, Clone)]
struct Cell {
    wall: Vec<f64>,
    gpu: Vec<f64>,
    instr: Vec<u64>,
    cyc: Vec<u64>,
    pinstr: Vec<u64>,
}
impl Cell {
    fn time<R>(&mut self, f: impl FnOnce() -> R) -> R {
        let p0 = proc_instr();
        let (i0, c0) = pmu::instructions_cycles();
        let t = Instant::now();
        let r = f();
        let w = t.elapsed().as_secs_f64();
        let (i1, c1) = pmu::instructions_cycles();
        let p1 = proc_instr();
        self.wall.push(w);
        self.instr.push(i1 - i0);
        self.cyc.push(c1 - c0);
        self.pinstr.push(p1 - p0);
        r
    }
}
fn med<T: Copy + PartialOrd>(v: &[T]) -> Option<T> {
    let mut v = v.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v.get(v.len() / 2).copied()
}
fn min<T: Copy + PartialOrd>(v: &[T]) -> Option<T> {
    v.iter().copied().reduce(|a, b| if b < a { b } else { a })
}
fn row(phase: &str, exec: &str, c: &Cell, tiles: f64, note: &str) {
    let us = |v: Option<f64>| v.map(|v| format!("{:.0}", v * 1e6)).unwrap_or("-".into());
    let k = |v: Option<u64>| v.map(|v| format!("{:.1}", v as f64 / 1e3)).unwrap_or("-".into());
    println!("{phase}\t{exec}\t{tiles:.0}\t{}\t{}\t{}\t{}\t{}\t{}\t{note}", us(min(&c.wall)), us(med(&c.wall)), us(med(&c.gpu)), k(med(&c.instr)), k(med(&c.cyc)), k(med(&c.pinstr)));
}

struct Bench {
    name: &'static str,
    model: Model,
    /// One scene per archetype executor (each keeps its own retained state).
    a1: Scene,
    ap: Scene,
    ag: Scene,
    gpu: Gpu,
    kgpu: bough_kernel::gpu::Gpu,
    px1: Vec<u32>,
    pxp: Vec<u32>,
    pxk: Vec<u32>,
    pxkp: Vec<u32>,
    pool: Pool,
    workers: usize,
    u: U,
    reps: usize,
    mismatches: usize,
}

impl Bench {
    /// `step(k, &mut u)` returns the pointer position of the step, if it has one.
    fn phase(&mut self, phase: &str, full: bool, mut step: impl FnMut(usize, &mut U) -> Option<(i32, i32)>) {
        let (fw, fh) = (self.a1.fw, self.a1.fh);
        let stride = fw as usize;
        let words = stride * fh as usize;
        let mut cells: [Cell; 8] = Default::default();
        let (mut tiles, mut entries) = (0f64, 0f64);
        let atlas_px = self.a1.atlas.clone();
        let atlas = cpu::Atlas::new(&atlas_px, self.a1.aw);
        let (mut binner, mut prog) = (Binner::default(), Prog::default());
        for k in 0..self.reps + 2 {
            let ptr = step(k, &mut self.u);
            // warm-up steps are not recorded
            let rec = k >= 2;
            let mut scratch: [Cell; 8] = Default::default();
            let c = if rec { &mut cells } else { &mut scratch };
            if full {
                self.a1.invalidate();
                self.ap.invalidate();
                self.gpu.invalidate();
            }
            // A1: pick (in the frame the surface shows), window pass, bins, shade: one core
            let fs = c[0].time(|| {
                if let Some((x, y)) = ptr {
                    let shown = self.a1.prev;
                    self.u.w[U_PICK] = self.a1.pick(&shown, x, y);
                }
                self.a1.frame(&self.u, &mut self.px1, stride, &self.pool, 0)
            });
            let u = self.u;
            // the window pass and bins alone, on the pool scene (same work as inside A1's frame)
            let fsu = c[6].time(|| self.ap.update(&u));
            let mut tl = Vec::new();
            self.ap.damage.tiles(&mut tl);
            self.ap.damage.clear();
            c[1].time(|| self.ap.shade(&u, &tl, &mut self.pxp, stride, &self.pool, self.workers));
            let _ = fsu;
            // AG: the resident frame
            let (gt, pick) = c[2].time(|| self.gpu.frame(&u, ptr, stride));
            c[2].gpu.push(gt.gpu);
            let gpu_tiles = self.gpu.last().0;
            // the generic kernel: commands restaged for free, then bin + raster of the same damage
            let ks = self.a1.stage(&u);
            let dmg: Vec<(u16, u16)> = self.a1.last_tiles().iter().map(|&t| ((t as usize % self.a1.tx) as u16, (t as usize / self.a1.tx) as u16)).collect();
            let d = if full { Damage::All } else { Damage::Tiles(&dmg) };
            c[7].time(|| binner.bin(&ks, fw, fh, stride as u32, &atlas, 0, &d, true, &mut prog));
            let st = c[3].time(|| cpu::raster(&ks, &prog, &atlas, &mut self.pxk, stride, &self.pool, 0, Opt::default()));
            c[4].time(|| cpu::raster(&ks, &prog, &atlas, &mut self.pxkp, stride, &self.pool, self.workers, Opt::default()));
            let kt = c[5].time(|| {
                self.kgpu.upload_frame(&ks, &prog);
                self.kgpu.render(prog.tiles(), words)
            });
            c[5].gpu.push(kt.gpu);
            if rec {
                tiles += fs.tiles as f64;
                entries += st.entries as f64;
            }
            let ne = |a: &[u32], b: &[u32]| a.iter().zip(b).any(|(x, y)| x != y);
            let bad = ne(&self.px1, &self.pxk) as usize + ne(&self.pxp, &self.pxk) as usize + ne(self.gpu.out(words), &self.pxk) as usize + ne(&self.pxkp, &self.pxk) as usize + (fs.tiles != gpu_tiles) as usize + (ptr.is_some() && pick != u.w[U_PICK]) as usize;
            if bad != 0 {
                self.mismatches += 1;
                eprintln!("!! {} {phase} step {k}: {bad} of the cross-checks failed (tiles cpu {} gpu {gpu_tiles})", self.name, fs.tiles);
            }
        }
        let n = self.reps as f64;
        let p = format!("{}.{phase}", self.name);
        row(&p, "A1 archetype cpu 1 core", &cells[0], tiles / n, "pick + window pass + bins + shade");
        row(&p, "A  window pass + bins", &cells[6], tiles / n, "the per-record part of A1 / AP");
        row(&p, &format!("AP archetype cpu {}+1", self.workers), &cells[1], tiles / n, "shade only");
        row(&p, "AG archetype gpu resident", &cells[2], tiles / n, "block write + encode + wait");
        row(&p, "K  generic bin", &cells[7], tiles / n, &format!("{:.1} entries per tile after the cut", entries / tiles.max(1.0)));
        row(&p, "K1 generic cpu 1 core", &cells[3], tiles / n, "raster only; restage free, bin above");
        row(&p, &format!("KP generic cpu {}+1", self.workers), &cells[4], tiles / n, "raster only");
        row(&p, "KG generic gpu", &cells[5], tiles / n, "upload + tile kernel + wait");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let which = args.get(1).map(|s| s.as_str()).unwrap_or("canvas");
    let n: Option<usize> = args.get(2).and_then(|v| v.parse().ok());
    let reps = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(24);
    let workers = std::env::var("WORKERS").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    println!("# pbench {which} {n:?}; reps {reps}; workers {workers}; load at start: {}", load());
    let build = |k: usize| -> (Scene, Model) {
        match which {
            "canvas" => canvas::build(n.unwrap_or(20)),
            _ => anim::build(n.unwrap_or(600)),
        }
        .pipe(|r| {
            let _ = k;
            r
        })
    };
    let t = Instant::now();
    let (a1, model) = build(0);
    let mount_s = t.elapsed().as_secs_f64();
    let (ap, _) = build(1);
    let (ag, _) = build(2);
    // the plan: the scene's own constant, and the same function folded over the mounted columns
    match which {
        "canvas" => plan::print("canvas (const PLAN)", &canvas::PLAN.arch, &canvas::PLAN.whole, &canvas::PLAN.deps),
        _ => plan::print("anim (const PLAN)", &anim::PLAN.arch, &anim::PLAN.whole, &anim::PLAN.deps),
    }
    let descs: Vec<&'static Desc> = a1.cols.iter().map(|c| c.desc).collect();
    let t = Instant::now();
    let (_, whole, _) = plan::plan_dyn(&descs);
    let plan_s = t.elapsed().as_secs_f64();
    let lays: Vec<(usize, usize)> = a1.cols.iter().map(|c| (c.base, c.desc.words)).collect();
    let t = Instant::now();
    let src = bough_paint::metal::source(&descs.iter().map(|d| *d as &Desc).collect::<Vec<_>>(), &lays);
    let gen_s = t.elapsed().as_secs_f64();
    let (fw, fh) = (a1.fw, a1.fh);
    let words = (fw * fh) as usize;
    let gpu = Gpu::new(&ag, words, 192).expect("Metal");
    println!("# scene: {}x{} px, {} tiles, {} records in {} columns, {} column words ({} KiB); mount {:.1} ms", fw, fh, a1.tx * a1.ty, a1.records(), a1.cols.len(), a1.used_words(), a1.used_words() * 4 / 1024, mount_s * 1e3);
    println!("# start-up: whole-scene plan from the mounted columns {:.1} us (gpu {}), source assembly {:.2} ms ({} bytes, of which leaves {}), Metal compile + 5+{} pipelines {:.0} ms", plan_s * 1e6, whole.gpu, gen_s * 1e3, src.len(), bough_paint::emit::LEAVES_MSL.len(), descs.len(), gpu.compile_s * 1e3);
    let t = Instant::now();
    let kgpu = bough_kernel::gpu_executor().expect("Metal");
    println!("# generic kernel: Metal compile {:.0} ms ({} bytes of MSL)", t.elapsed().as_secs_f64() * 1e3, bough_kernel::KERNEL_MSL.len());
    let mut b = Bench { name: if which == "canvas" { "canvas" } else { "anim" }, model, a1, ap, ag, gpu, kgpu, px1: vec![0; words], pxp: vec![0; words], pxk: vec![0; words], pxkp: vec![0; words], pool: Pool::new(workers), workers, u: U::default(), reps, mismatches: 0 };
    {
        // the generic GPU executor's atlas (uploaded once)
        let ks = b.a1.stage(&b.u);
        let mut prog = Prog::default();
        let atlas_px = b.a1.atlas.clone();
        let atlas = cpu::Atlas::new(&atlas_px, b.a1.aw);
        Binner::default().bin(&ks, fw, fh, fw, &atlas, 0, &Damage::All, true, &mut prog);
        b.kgpu.upload(&ks, &atlas, &prog);
        let t = Instant::now();
        let ks2 = b.a1.stage(&b.u);
        println!("# generic kernel: restaging every record costs {:.2} ms for {} commands (not charged to K rows)", t.elapsed().as_secs_f64() * 1e3, ks2.len());
        let _ = &b.ag;
        let _ = &b.model;
    }
    println!("phase\texecutor\ttiles\twall_min_us\twall_med_us\tgpu_med_us\tthread_Kinstr\tthread_Kcycles\tproc_Kinstr\tnote");
    b.phase("full", true, |_, _| None);
    if which == "canvas" {
        let on = canvas::row_point(3);
        b.phase("hover", false, |k, _| Some(if k % 2 == 0 { on } else { canvas::NOWHERE }));
        b.phase("theme", false, |k, u| {
            u.w[2] = if k % 2 == 0 { 32768 } else { 0 };
            None
        });
        b.phase("pan", false, |k, u| {
            let d = if (k / 4) % 2 == 0 { -72 * 256 } else { 72 * 256 };
            u.w[1] = (u.w[1] as i32 + d) as u32;
            None
        });
        b.phase("drag", false, |k, u| {
            u.w[3] = canvas::DRAG_ID + 7;
            let d = if (k / 6) % 2 == 0 { 1 } else { -1 };
            u.w[4] = (u.w[4] as i32 + d * 3 * 256) as u32;
            u.w[5] = (u.w[5] as i32 + d * 2 * 256 + 64) as u32;
            None
        });
    } else {
        b.phase("theme", false, |k, u| {
            u.w[2] = if k % 2 == 0 { 32768 } else { 0 };
            None
        });
    }
    b.phase("time", false, |_, u| {
        u.w[U_TIME] += 16;
        None
    });
    println!("# cross-check failures: {}; load at end: {}", b.mismatches, load());
}

trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}
impl<T> Pipe for T {}
