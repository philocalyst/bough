//! Measurements of the kernel's executors (journal E3-E11). Headless; prints tab-separated rows.
//!   kbench micro                         per-kind cost per tile-command (CPU; GPU where present)
//!   kbench scene <canvas|grid|gridtext|adv> <w> <h> [nodes]   whole-frame and crossover sweeps
//!   kbench hash                          the specification hash of the exactness scenes (cross-host check)
//! Times are the minimum and the median of the repetitions; instructions are process-wide retired
//! instructions (macOS `proc_pid_rusage`; 0 elsewhere), so they include every worker thread.
use bough_kernel::cpu::{self, Atlas, Binner, Damage, Opt, Pool, Prog, Stats};
use bough_kernel::stage::{self, NO_CLIP, Scene};
use bough_kernel::{k, scenes};
use std::time::Instant;

#[cfg(target_os = "macos")]
fn instrs() -> u64 {
    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u64) -> i32;
    }
    let mut b = [0u64; 64];
    // SAFETY: flavor 4 (RUSAGE_INFO_V4) writes 296 bytes; the buffer holds 512.
    unsafe { proc_pid_rusage(std::process::id() as i32, 4, b.as_mut_ptr()) };
    b[31]
}
#[cfg(not(target_os = "macos"))]
fn instrs() -> u64 {
    0
}

/// (min seconds, median seconds, instructions per repetition) of `reps` runs of `f`.
fn time(reps: usize, mut f: impl FnMut()) -> (f64, f64, f64) {
    f();
    let mut t = Vec::with_capacity(reps);
    let i0 = instrs();
    for _ in 0..reps {
        let s = Instant::now();
        f();
        t.push(s.elapsed().as_secs_f64());
    }
    let i1 = instrs();
    t.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (t[0], t[reps / 2], (i1 - i0) as f64 / reps as f64)
}

struct Bench {
    atlas: Vec<u8>,
    pool: Pool,
    bin: Binner,
    prog: Prog,
    out: Vec<u32>,
    #[cfg(target_os = "macos")]
    gpu: Option<bough_kernel::metal::Gpu>,
}

impl Bench {
    fn new() -> Bench {
        let workers = std::env::var("KB_WORKERS").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| std::thread::available_parallelism().map_or(3, |n| n.get() - 1));
        Bench {
            atlas: scenes::atlas(),
            pool: Pool::new(workers),
            bin: Binner::default(),
            prog: Prog::default(),
            out: Vec::new(),
            #[cfg(target_os = "macos")]
            gpu: if std::env::var_os("KB_NOGPU").is_some() { None } else { bough_kernel::metal::Gpu::new(bough_kernel::KERNEL_MSL) },
        }
    }
    fn cpu(&mut self, scene: &Scene, extra: usize, opt: Opt, reps: usize) -> (f64, f64, f64, Stats) {
        let (fw, fh) = (self.prog.fw() as usize, self.prog.fh() as usize);
        self.out.resize(fw * fh, 0);
        let atlas = Atlas { px: &self.atlas, w: scenes::ATLAS_W };
        let mut st = Stats::default();
        let (pool, prog, out) = (&self.pool, &self.prog, &mut self.out);
        let (a, b, c) = time(reps, || st = cpu::raster(scene, prog, &atlas, out, fw, pool, extra, opt));
        (a, b, c, st)
    }
    /// (wall min, wall median, GPU kernel seconds min, instructions) of the uploaded program on the GPU.
    #[cfg(target_os = "macos")]
    fn gpu(&mut self, scene: &Scene, reps: usize, gap_us: u64) -> Option<(f64, f64, f64, f64)> {
        let g = self.gpu.as_mut()?;
        let words = (self.prog.fw() * self.prog.fh()) as usize;
        g.upload(scene, &self.atlas, &self.prog);
        let nt = self.prog.tiles();
        let mut kern = f64::MAX;
        g.render(nt, words);
        let mut t = Vec::new();
        let i0 = instrs();
        for _ in 0..reps {
            if gap_us > 0 {
                std::thread::sleep(std::time::Duration::from_micros(gap_us));
            }
            let r = g.render(nt, words);
            kern = kern.min(r.gpu);
            t.push(r.wall);
        }
        let i1 = instrs();
        t.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Some((t[0], t[reps / 2], kern, (i1 - i0) as f64 / reps as f64))
    }
    #[cfg(not(target_os = "macos"))]
    fn gpu(&mut self, _: &Scene, _: usize, _: u64) -> Option<(f64, f64, f64, f64)> {
        None
    }
    fn bin(&mut self, scene: &Scene, fw: u32, fh: u32, d: &Damage, cut: bool) {
        let atlas = Atlas { px: &self.atlas, w: scenes::ATLAS_W };
        self.bin.bin(scene, fw, fh, fw, &atlas, 0, d, cut, &mut self.prog);
    }
}

fn micro(b: &mut Bench) {
    let (fw, fh) = (512u32, 512u32);
    let tiles = 256.0;
    println!("# micro: one command kind stacked K times over {fw}x{fh} ({tiles} tiles); cost per tile-command = slope K=2 -> K=10");
    println!("kind\tcpu_ns\tcpu_instr\tcpu_flat_ns\tcpu_flat_instr\tgpu_kernel_ns\trows_copied_pct");
    let f = fw as f32;
    type Mk = Box<dyn Fn(usize) -> Option<bough_kernel::Cmd>>;
    let kinds: Vec<(&str, Mk)> = vec![
        ("solid_opaque", Box::new(move |_| stage::solid(0.0, 0.0, f, f, 0xff80_4020, &NO_CLIP))),
        ("solid_alpha", Box::new(move |_| stage::solid(0.0, 0.0, f, f, 0x8080_4020, &NO_CLIP))),
        // boxes of one tile each would be all edge; 96 px boxes are what cards look like
        ("rrect_r8_alpha", Box::new(move |i| stage::rrect(3.25 + i as f32, 2.5, f - 8.0, f - 8.0, 8.0, 0x8080_4020, &NO_CLIP))),
        ("border_r8_alpha", Box::new(move |i| stage::border(3.25 + i as f32, 2.5, f - 8.0, f - 8.0, 8.0, 1.5, 0x80ff_ffff, 0x6080_4020, &NO_CLIP))),
        ("grad_alpha", Box::new(move |i| stage::grad(3.25 + i as f32, 2.5, f - 8.0, f - 8.0, 8.0, 1.0, 0x8080_4020, 0x80f0_1020, &NO_CLIP))),
        ("shadow_r0_s6", Box::new(move |i| stage::shadow(60.0 + i as f32, 60.0, f - 120.0, f - 120.0, 0.0, 6.0, 0x6000_0000, &NO_CLIP))),
        ("shadow_r12_s6", Box::new(move |i| stage::shadow(60.0 + i as f32, 60.0, f - 120.0, f - 120.0, 12.0, 6.0, 0x6000_0000, &NO_CLIP))),
        ("user_stripes", Box::new(move |i| stage::user(3.25 + i as f32, 2.5, f - 8.0, f - 8.0, 8.0, 0, 0x8080_4020, 0x80f0_1020, &NO_CLIP))),
    ];
    let mut row = |b: &mut Bench, name: &str, mk: &dyn Fn(usize, &mut Scene)| {
        let mut v = [[0f64; 5]; 2];
        let mut copied = 0.0;
        for (j, kk) in [2usize, 10].into_iter().enumerate() {
            let mut s = Scene::default();
            s.add(stage::solid(0.0, 0.0, f, f, 0xffff_ffff, &NO_CLIP));
            for i in 0..kk {
                mk(i, &mut s);
            }
            b.bin(&s, fw, fh, &Damage::All, false);
            let (t0, _, i0, _) = b.cpu(&s, 0, Opt::flat(false), 30);
            let (t1, _, i1, st) = b.cpu(&s, 0, Opt::flat(true), 30);
            copied = 100.0 * st.rows_copied as f64 / (st.rows_copied + st.rows_shaded) as f64;
            let g = b.gpu(&s, 30, 0).map_or(0.0, |g| g.2);
            v[j] = [t0, i0, t1, i1, g];
        }
        let d = |i: usize| (v[1][i] - v[0][i]) / 8.0 / tiles;
        println!("{name}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{:.1}\t{copied:.0}", d(0) * 1e9, d(1), d(2) * 1e9, d(3), d(4) * 1e9);
    };
    for (name, mk) in &kinds {
        row(b, name, &|i, s| s.add(mk(i)));
    }
    // text: every tile holds ~8 glyph boxes per layer
    row(b, "glyphs_8_per_tile", &|i, s| {
        for y in 0..(fh / 20) {
            for x in 0..(fw / 13) {
                s.add(stage::glyph((x * 13) as f32 + i as f32, (y * 20) as f32, scenes::CELL_W, scenes::CELL_H, scenes::cell(x + y * 7 + i as u32), 0xff10_1010, &NO_CLIP));
            }
        }
    });
}

fn scene_by_name(name: &str, fw: u32, fh: u32, nodes: usize) -> Scene {
    match name {
        "canvas" => scenes::canvas(11, fw, fh, nodes),
        "grid" => scenes::grid(fw, fh, 96, 24, false, None),
        "gridtext" => scenes::grid(fw, fh, 96, 24, true, None),
        "gridhot" => scenes::grid(fw, fh, 96, 24, true, Some(3)),
        "adv" => scenes::adversarial(3, fw, fh, nodes),
        _ => panic!("unknown scene {name}"),
    }
}

fn scene(b: &mut Bench, name: &str, fw: u32, fh: u32, nodes: usize) {
    let s = scene_by_name(name, fw, fh, nodes);
    let workers = b.pool.workers();
    println!("# scene {name} {fw}x{fh} commands {} workers {workers}", s.len());
    // the occlusion cut (E6)
    for cut in [false, true] {
        let (tb, _, ib) = {
            let (bin, prog) = (&mut b.bin, &mut b.prog);
            let atlas = Atlas { px: &b.atlas, w: scenes::ATLAS_W };
            time(10, || bin.bin(&s, fw, fh, fw, &atlas, 0, &Damage::All, cut, prog))
        };
        let (t, _, i, st) = b.cpu(&s, 0, Opt::default(), 10);
        println!("cut={cut}\ttiles {}\tentries/tile {:.2}\tbin {:.0} us {:.0} Kinstr\traster1 {:.0} us {:.0} Kinstr", st.tiles, st.entries as f64 / st.tiles as f64, tb * 1e6, ib / 1e3, t * 1e6, i / 1e3);
    }
    // flat rows (E10)
    for flat in [false, true] {
        let (t, _, i, st) = b.cpu(&s, 0, Opt::flat(flat), 10);
        println!("flat={flat}\trows shaded {} copied {} ({:.0} %)\traster1 {:.0} us {:.0} Kinstr", st.rows_shaded, st.rows_copied, 100.0 * st.rows_copied as f64 / (st.rows_copied + st.rows_shaded) as f64, t * 1e6, i / 1e3);
    }
    // the gradient (E5): compact damage of n tiles; one core, some cores, all cores, GPU
    println!("# crossover: damage is a compact block of tiles; us = min of reps (median in brackets); Ki = K instructions per frame, all threads");
    println!("tiles\tentries\tcpu1_us\tcpu1_Ki\tcpu2_us\tcpu4_us\tcpuN_us\tcpuN_Ki\tgpu_wall_us\tgpu_kernel_us\tgpu_Ki\tgpu_cadence_us\tcpu1_cadence_us\tcpuN_cadence_us");
    let (tx, ty) = (fw.div_ceil(32), fh.div_ceil(32));
    let mut n = 1u32;
    loop {
        let n_eff = n.min(tx * ty);
        // a block of aspect ~2:1 in the middle of the window
        let bw = ((n_eff as f64 * 2.0).sqrt().ceil() as u32).clamp(1, tx);
        let bh = n_eff.div_ceil(bw).min(ty);
        let (x0, y0) = ((tx - bw) / 2, (ty - bh) / 2);
        let tiles: Vec<(u16, u16)> = (0..bh).flat_map(|y| (0..bw).map(move |x| ((x0 + x) as u16, (y0 + y) as u16))).take(n_eff as usize).collect();
        b.bin(&s, fw, fh, &Damage::Tiles(&tiles), true);
        let reps = if n_eff < 64 { 200 } else if n_eff < 1024 { 60 } else { 20 };
        let c1 = b.cpu(&s, 0, Opt::default(), reps);
        let c2 = b.cpu(&s, 1, Opt::default(), reps);
        let c4 = b.cpu(&s, 3, Opt::default(), reps);
        let cn = b.cpu(&s, workers, Opt::default(), reps);
        let g = b.gpu(&s, reps, 0);
        let gc = b.gpu(&s, 40, 4000);
        // at a display's cadence the caches are cold and the workers parked: one frame every 4 ms
        let cad = |b: &mut Bench, extra: usize| {
            let (fw, _) = (b.prog.fw() as usize, 0);
            let atlas = Atlas { px: &b.atlas, w: scenes::ATLAS_W };
            let mut t = Vec::new();
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_micros(4000));
                let st = Instant::now();
                cpu::raster(&s, &b.prog, &atlas, &mut b.out, fw, &b.pool, extra, Opt::default());
                t.push(st.elapsed().as_secs_f64());
            }
            t.sort_by(|a, b| a.partial_cmp(b).unwrap());
            t[20]
        };
        let (k1, kn) = (cad(b, 0), cad(b, workers));
        let us = |a: f64, m: f64| format!("{:.1} [{:.1}]", a * 1e6, m * 1e6);
        println!(
            "{}\t{}\t{}\t{:.1}\t{}\t{}\t{}\t{:.1}\t{}\t{}\t{}\t{}\t{:.1}\t{:.1}",
            n_eff,
            c1.3.entries,
            us(c1.0, c1.1),
            c1.2 / 1e3,
            us(c2.0, c2.1),
            us(c4.0, c4.1),
            us(cn.0, cn.1),
            cn.2 / 1e3,
            g.map_or("-".into(), |g| us(g.0, g.1)),
            g.map_or("-".into(), |g| format!("{:.1}", g.2 * 1e6)),
            g.map_or("-".into(), |g| format!("{:.1}", g.3 / 1e3)),
            gc.map_or("-".into(), |g| format!("{:.1}", g.1 * 1e6)),
            k1 * 1e6,
            kn * 1e6,
        );
        if n_eff == tx * ty {
            break;
        }
        n = if n < 8 { n * 2 } else { n * 2 };
    }
}

fn hash() {
    let atlas_px = scenes::atlas();
    let atlas = Atlas { px: &atlas_px, w: scenes::ATLAS_W };
    let (fw, fh, stride) = (320u32, 250u32, 328usize);
    let mut h = 0u64;
    let pool = Pool::new(3);
    let (mut bin, mut prog) = (Binner::default(), Prog::default());
    let mut bad = 0;
    for seed in 0..125u64 {
        let scene = scenes::adversarial(seed, fw, fh, 110);
        let now = [0u32, 37, 90, 400][(seed % 4) as usize];
        let mut spec = vec![0u32; stride * fh as usize];
        cpu::naive(&scene, fw, fh, &atlas, now, &mut spec, stride);
        h ^= bough_kernel::hash(&spec).rotate_left(seed as u32);
        bin.bin(&scene, fw, fh, stride as u32, &atlas, now, &Damage::All, true, &mut prog);
        let mut out = vec![0u32; stride * fh as usize];
        cpu::raster(&scene, &prog, &atlas, &mut out, stride, &pool, 3, Opt::default());
        bad += (out != spec) as u32;
    }
    println!("spec hash {h:016x}; fast executor differs on {bad} of 125 scenes; arch {}", std::env::consts::ARCH);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let _ = k::ONE;
    match a.get(1).map(String::as_str) {
        Some("micro") => micro(&mut Bench::new()),
        Some("scene") => {
            let (w, h) = (a[3].parse().unwrap(), a[4].parse().unwrap());
            scene(&mut Bench::new(), &a[2], w, h, a.get(5).and_then(|s| s.parse().ok()).unwrap_or(600));
        }
        Some("hash") => hash(),
        _ => eprintln!("usage: kbench micro | scene <canvas|grid|gridtext|gridhot|adv> <w> <h> [nodes] | hash"),
    }
}
