//! The kernel backend (compute lane, round 7; `BOUGH_REPAIR=kernel`): frames drawn by `bough-kernel`, the one
//! integer kernel whose CPU executors and GPU executor produce the same bytes by construction. The damaged
//! tiles' records are staged into kernel commands, binned into tile programs (with the occlusion cut), and
//! run by whichever executor the measured costs favour: this thread, bands of tile rows across the pool, or
//! the GPU. All three write the target IOSurface's memory in place (the GPU through a buffer that aliases
//! it), so there is no second arithmetic model, no per-machine self-check and no seam when the choice changes
//! from one frame to the next.
//!
//! It draws every kind there is (solids, rounded boxes, borders, glyph runs, images, ramp gradients, shadows,
//! transformed spaces) and never refuses a frame: a record that indexes outside its columns (a corrupted
//! region) is skipped, and every texel index is clamped by the kernel, so a bad region draws wrong pixels
//! but reads nothing outside its buffers.
use crate::rt::Rt;
use bough_core::Plane;
use bough_kernel::cpu::{self, Atlas, Binner, Damage, Exec, Opt, Pool, Prog};
use bough_kernel::stage::{self, Scene};

const PAY_RUN: u32 = 0x8000_0000;
const PAY_IRUN: u32 = 0x4000_0000;

/// Counters.
#[derive(Clone, Copy, Debug, Default)]
pub struct KStats {
    pub frames: u64,
    pub cpu1: u64,
    pub cpun: u64,
    pub gpu: u64,
    pub refused: u64,
    pub tiles: u64,
    pub entries: u64,
    /// Commands staged.
    pub staged: u64,
}

/// `BOUGH_KERNEL_EXEC`: pin the executor (gates and measurements); `auto` (the default) lets the costs choose.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Force {
    Auto,
    Cpu1,
    CpuN,
    Gpu,
}

pub struct KernelBackend {
    scene: Scene,
    prog: Prog,
    bin: Binner,
    pool: Pool,
    pos: Vec<u32>,
    plane: Box<Plane>,
    tiles: Vec<(u16, u16)>,
    force: Force,
    /// `BOUGH_KERNEL_TRACE`: one line per frame on stderr.
    pub trace: bool,
    dry: bool,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    gpu: Option<bough_kernel::gpu::Gpu>,
    /// Per surface: the address its target buffer wraps, and the buffer.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    targets: Vec<Option<(usize, bough_kernel::gpu::Target)>>,
    /// The atlases' stamps at their last upload to the GPU executor.
    uploaded: Option<(u64, usize, u64)>,
    /// When the GPU executor last ran (its fixed cost depends on whether it has clocked down).
    last_gpu: Option<std::time::Instant>,
    pub stats: KStats,
    /// The executor of the last frame.
    pub last: Option<Exec>,
}

impl Default for KernelBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelBackend {
    pub fn new() -> KernelBackend {
        // performance cores only: an efficiency core holding a band stalls the frame
        let workers = std::env::var("BOUGH_KERNEL_WORKERS").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1)).min(5));
        let force = match std::env::var("BOUGH_KERNEL_EXEC").as_deref() {
            Ok("cpu1") => Force::Cpu1,
            Ok("cpun") => Force::CpuN,
            Ok("gpu") => Force::Gpu,
            _ => Force::Auto,
        };
        KernelBackend {
            scene: Scene::default(),
            prog: Prog::default(),
            bin: Binner::default(),
            pool: Pool::new(workers),
            pos: Vec::new(),
            plane: Box::new(Plane::EMPTY),
            tiles: Vec::new(),
            force,
            trace: std::env::var_os("BOUGH_KERNEL_TRACE").is_some(),
            dry: std::env::var_os("BOUGH_KERNEL_DRY").is_some(),
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            gpu: None,
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            targets: Vec::new(),
            uploaded: None,
            last_gpu: None,
            stats: KStats::default(),
            last: None,
        }
    }

    /// A space's origin and, if it is transformed, its affine (`f_xf` of the presenter's kernels: the stored
    /// matrix is M - I, the pivot is relative to the origin, and an identity with no translation is
    /// untransformed).
    fn space(rt: &Rt, s: u16) -> Option<([f32; 2], Option<stage::Xf>)> {
        let xf = rt.xforms.get(s as usize)?;
        let o = rt.glyphs.spaces.get(s as usize)?;
        let off = [o[0] as f32, o[1] as f32];
        if xf.iter().all(|&v| v == 0.0) || (xf[..4].iter().all(|&v| v == 0.0) && xf[6] == 0.0 && xf[7] == 0.0) {
            return Some((off, None));
        }
        Some((off, Some(stage::Xf { m: [xf[0] + 1.0, xf[1], xf[2], xf[3] + 1.0], k: [off[0] + xf[4], off[1] + xf[5]], t: [xf[6], xf[7]] })))
    }

    /// Stage every record touching a damaged tile into kernel commands, in paint order. Always `true` (the
    /// return value is kept for the callers that still test it): records that index outside their columns
    /// are skipped and counted in `stats.refused`.
    pub fn stage(&mut self, rt: &Rt, rows: &[u128], full: bool) -> bool {
        let (w, h) = (rt.win_w, rt.win_h);
        let pl = &mut *self.plane;
        let ty = (h as usize).div_ceil(32);
        let all = if w >= 128 * 32 { u128::MAX } else { (1u128 << (w as u32).div_ceil(32)) - 1 };
        // only the rows the last frame set need clearing (the plane is otherwise zero)
        if pl.y0 < pl.y1 {
            pl.rows[pl.y0 as usize..(pl.y1 as usize).min(bough_core::damage::MAX_TILES_Y)].fill(0);
        }
        pl.n = ty as u32;
        let (mut y0, mut y1) = (u32::MAX, 0u32);
        self.tiles.clear();
        for y in 0..ty {
            let r = if full { all } else { rows.get(y).copied().unwrap_or(0) & all };
            pl.rows[y] = r;
            if r != 0 {
                y0 = y0.min(y as u32);
                y1 = y as u32 + 1;
                let mut m = r;
                while m != 0 {
                    self.tiles.push((m.trailing_zeros() as u16, y as u16));
                    m &= m - 1;
                }
            }
        }
        pl.y0 = y0;
        pl.y1 = y1;
        pl.solid = false;
        self.scene.clear();
        if y0 >= y1 {
            return true;
        }
        kprof::lap(2);
        let bbox = pl.px_bbox(rt.bough.window());
        rt.bough.cull_tiles_pos(pl, bbox, &mut self.pos);
        kprof::lap(3);
        let pos = std::mem::take(&mut self.pos);
        for &p in &pos {
            if !self.stage_record(rt, p as usize) {
                self.stats.refused += 1;
            }
        }
        self.pos = pos;
        self.stats.staged += self.scene.len() as u64;
        kprof::lap(4);
        true
    }

    fn stage_record(&mut self, rt: &Rt, pos: usize) -> bool {
        let r = rt.bough.record(pos);
        if r.clip as usize >= rt.bough.gpu_extent().1 {
            return false;
        }
        let [cx0, cy0, cx1, cy1] = rt.bough.clip_rect(r.clip).to_xyxy();
        let cl = [cx0 as f32, cy0 as f32, cx1 as f32, cy1 as f32];
        let atlas = &rt.text.cache.atlas;
        let p = r.payload;
        if p & PAY_RUN != 0 {
            let (r0, rn) = (p & 0xFFFFF, ((p >> 20) & 0x7FF) + 1);
            for run in r0..r0 + rn {
                let Some(rh) = rt.glyphs.runs.get(run as usize).copied() else { return false };
                let Some((off, xf)) = Self::space(rt, rh.space) else { return false };
                let Some(toks) = rt.text.cache.tok_strike(rh.strike as usize) else { return false };
                for slot in rh.base..rh.base + rh.len {
                    let (Some(&x), Some(&e)) = (rt.glyphs.x.get(slot as usize), rt.glyphs.ent.get(slot as usize)) else { break };
                    let en = &toks.entry[(e & 255) as usize];
                    if en.uv[2] == 0 {
                        continue;
                    }
                    // bough-text's pen formula (the one float expression kept from the old kernels: it is
                    // evaluated here, once, on the CPU, for every executor)
                    let dx = x.wrapping_sub(rh.x0);
                    let px = (dx as f32).mul_add(rh.scale, rh.origin[0] + 0.5).floor() + en.geo[0];
                    let texel0 = en.uv[1] as u32 * atlas.width + en.uv[0] as u32;
                    let (gx, gy, gw, gh) = (px + off[0], rh.origin[1] + en.geo[1] + off[1], en.geo[2] as u32, en.geo[3] as u32);
                    match &xf {
                        None => self.scene.add(stage::glyph(gx, gy, gw, gh, texel0, rh.color, &cl)),
                        Some(x) => self.scene.add(stage::xf(stage::glyph(gx, gy, gw, gh, texel0, rh.color, &stage::NO_CLIP), x, &cl)),
                    }
                }
            }
            return true;
        }
        let (i0, n) = if p & PAY_IRUN != 0 { (p & 0x3FFFF, ((p >> 18) & 0xFFF) + 1) } else { (p, 1) };
        for i in i0..i0 + n {
            let Some(q) = rt.inst.get(i as usize) else { return false };
            let Some((off, xf)) = Self::space(rt, q.space) else { return false };
            let (x, y, w, h) = (q.pos[0] + off[0], q.pos[1] + off[1], q.size[0], q.size[1]);
            // a transformed record is staged in its own frame, unclipped, and then placed (`stage::xf`)
            let lc = if xf.is_some() { &stage::NO_CLIP } else { &cl };
            let iw = bough_frame::IMG_W;
            let c = match q.kind {
                0 => stage::solid(x, y, w, h, q.color, lc),
                1 => stage::rrect(x, y, w, h, q.radius, q.color, lc),
                2 => stage::border(x, y, w, h, q.radius, q.bw, q.color, q.color2, lc),
                3 => stage::glyph(x, y, w as u32, h as u32, q.uv[1] as u32 * atlas.width + q.uv[0] as u32, q.color, lc),
                // an image's `radius` word is its angle
                4 => stage::image(x, y, w, h, q.radius, q.uv[1] as u32 * iw + q.uv[0] as u32, q.color, lc),
                5 => {
                    let dir = if q.uv[2] == 0 { stage::Ramp::Linear(q.bw) } else { stage::Ramp::Radial(f32::from_bits(q.color), f32::from_bits(q.color2)) };
                    stage::ramp(x, y, w, h, q.radius, dir, q.uv[1] as u32 * iw + q.uv[0] as u32, lc)
                }
                6 => {
                    // the record's ink (what damage tracks) is its box grown by `flags` px: the shadow is
                    // clipped to it, so nothing is painted where a later move would not repair
                    let m = if q.flags != 0 { q.flags as f32 } else { 1.0 };
                    let ink = [lc[0].max(x - m), lc[1].max(y - m), lc[2].min(x + w + m), lc[3].min(y + h + m)];
                    stage::shadow(x, y, w, h, q.radius, q.bw.max(1e-3), q.color, &ink)
                }
                _ => return false,
            };
            let c = match &xf {
                Some(x) => stage::xf(c, x, &cl),
                None => c,
            };
            self.scene.add(c);
        }
        true
    }

    /// Repair the staged frame (`stage` returned true) in the surface at `dst` (`bpr` bytes per row, `alloc`
    /// bytes in all, surface index `surf`). Returns the tiles repaired.
    ///
    /// # Safety
    /// `dst` must be the base of a BGRA8 surface of the runtime's window size, valid for `alloc` bytes with
    /// `bpr >= 4 * width`, page aligned, locked for this thread's writes, and it must stay mapped while the
    /// backend exists (the GPU executor keeps a buffer over it; a new address replaces the buffer).
    pub unsafe fn run(&mut self, rt: &Rt, full: bool, dst: *mut u8, bpr: usize, alloc: usize, surf: usize) -> u32 {
        let (w, h) = (rt.win_w as u32, rt.win_h as u32);
        let a = &rt.text.cache.atlas;
        let atlas = Atlas::new(&a.pixels, a.width).with_images(rt.images.pixels(), bough_frame::IMG_W);
        let stride = bpr / 4;
        let damage = if full { Damage::All } else { Damage::Tiles(&self.tiles) };
        #[cfg(target_os = "macos")]
        let i0 = if self.trace { instrs() } else { 0 };
        kprof::lap(5);
        self.bin.bin(&self.scene, w, h, stride as u32, &atlas, 0, &damage, true, &mut self.prog);
        kprof::lap(6);
        #[cfg(target_os = "macos")]
        let i1 = if self.trace { instrs() } else { 0 };
        let (tiles, entries) = (self.prog.tiles() as u64, self.prog.entries() as u64);
        // the cost model's GPU is Apple's; on Linux the GPU executor (Vulkan) runs only when pinned
        // (`BOUGH_KERNEL_EXEC=gpu`): the device there may be a CPU rasteriser (lavapipe)
        let gpu_able = cfg!(target_os = "macos");
        let warm = self.last_gpu.is_some_and(|t| t.elapsed().as_millis() < 50);
        let mut exec = match self.force {
            Force::Auto => cpu::choose(self.bin.est_cpu_ns, tiles, entries, self.pool.workers(), gpu_able, warm, &cpu::COSTS),
            Force::Cpu1 => Exec::Cpu1,
            Force::CpuN => Exec::CpuN(self.pool.workers()),
            Force::Gpu => Exec::Gpu,
        };
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        if exec == Exec::Gpu && tiles > 0 {
            if self.gpu.is_none() {
                self.gpu = bough_kernel::gpu_executor();
            }
            let ok = match self.gpu.as_mut() {
                Some(g) => {
                    if self.targets.len() <= surf {
                        self.targets.resize_with(surf + 1, || None);
                    }
                    if self.targets[surf].as_ref().is_none_or(|t| t.0 != dst as usize) {
                        // SAFETY: the caller's contract (mapped for the backend's life, page aligned).
                        self.targets[surf] = unsafe { g.wrap(dst, alloc.next_multiple_of(PAGE)) }.map(|t| (dst as usize, t));
                    }
                    match &self.targets[surf] {
                        Some((_, t)) => {
                            // the atlases are uploaded when they change (their stamps), not per frame
                            let stamps = (a.stamp, a.pixels.len(), rt.images.stamp());
                            if self.uploaded != Some(stamps) {
                                g.upload_atlas(&a.pixels);
                                g.upload_images(rt.images.pixels());
                                self.uploaded = Some(stamps);
                            }
                            kprof::lap(7);
                            g.upload_frame(&self.scene, &self.prog);
                            g.render_to(tiles as usize, t);
                            kprof::lap(8);
                            true
                        }
                        None => false,
                    }
                }
                None => false,
            };
            if ok {
                self.last_gpu = Some(std::time::Instant::now());
                self.stats.gpu += 1;
            } else {
                // no Metal device, or it refused the surface's memory: the CPU draws the same pixels
                exec = Exec::CpuN(self.pool.workers());
            }
        }
        if exec != Exec::Gpu {
            let extra = match exec {
                Exec::CpuN(n) => n,
                _ => 0,
            };
            // SAFETY: the caller's contract: `h` rows of `stride` words, aligned (page-aligned base, 4 | bpr),
            // exclusively this thread's until it returns (bands are disjoint sub-slices of it).
            let out = unsafe { std::slice::from_raw_parts_mut(dst as *mut u32, stride * h as usize) };
            kprof::lap(7);
            // (BOUGH_KERNEL_DRY: everything but the raster, to measure it by subtraction)
            if !self.dry {
                cpu::raster(&self.scene, &self.prog, &atlas, out, stride, &self.pool, extra, Opt::default());
            }
            kprof::lap(8);
            if extra > 0 {
                self.stats.cpun += 1;
            } else {
                self.stats.cpu1 += 1;
            }
        }
        if self.trace {
            #[cfg(target_os = "macos")]
            eprintln!("kernel run: bin {} instructions, choose + execute {}", i1 - i0, instrs() - i1);
            for i in 0..self.scene.len() as u32 {
                let c = self.scene.load(i);
                eprintln!("  cmd {i}: kind {} win {:?} core {:?} r {} alpha {}", c.par.x, (c.win.x, c.win.y, c.win.z, c.win.w), (c.core.x, c.core.y, c.core.z, c.core.w), c.rad.x, c.pm.w);
            }
            eprintln!("  tiles {:?}", self.tiles);
            eprintln!("kernel frame: {} staged commands, {tiles} tiles, {entries} entries, estimate {} ns, {exec:?}", self.scene.len(), self.bin.est_cpu_ns);
        }
        self.stats.frames += 1;
        self.stats.tiles += tiles;
        self.stats.entries += entries;
        self.last = Some(exec);
        tiles as u32
    }

    /// The scroll copy on the CPU: every pixel p of `rect` (x0, y0, x1, y1) of the target takes the source
    /// surface's pixel at p - (dx, dy). The kernel is translation invariant, so copied pixels equal
    /// re-rastered ones.
    ///
    /// # Safety
    /// Both surfaces are `h` rows of their bytes-per-row, locked (the source for reading), distinct, and the
    /// rect and its source lie inside the window (the copy plan's contract).
    pub unsafe fn copy(src: *const u8, sbpr: usize, dst: *mut u8, dbpr: usize, rect: [i32; 4], dx: i32, dy: i32, w: i32, h: i32) {
        let (x0, y0, x1, y1) = (rect[0].max(0).max(dx), rect[1].max(0).max(dy), rect[2].min(w).min(w + dx), rect[3].min(h).min(h + dy));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let n = (x1 - x0) as usize * 4;
        for y in y0..y1 {
            // SAFETY: rows y and y - dy are in [0, h), columns [x0, x1) and [x0 - dx, x1 - dx) in [0, w).
            unsafe { std::ptr::copy_nonoverlapping(src.add((y - dy) as usize * sbpr + (x0 - dx) as usize * 4), dst.add(y as usize * dbpr + x0 as usize * 4), n) };
        }
    }

    /// A from-scratch redraw of the whole window by the kernel's definition (`cpu::reference`: every pixel
    /// folds its tile's whole uncut list through `shade`, with no executor shortcut): the gates' oracle in kernel mode. `None`: the
    /// scene holds something this kernel does not draw.
    pub fn reference(&mut self, rt: &Rt) -> Vec<u8> {
        self.stage(rt, &[], true);
        let (w, h) = (rt.win_w as u32, rt.win_h as u32);
        let a = &rt.text.cache.atlas;
        let atlas = Atlas::new(&a.pixels, a.width).with_images(rt.images.pixels(), bough_frame::IMG_W);
        let mut out = vec![0u32; (w * h) as usize];
        cpu::reference(&self.scene, w, h, &atlas, 0, &mut out, w as usize, &self.pool);
        out.iter().flat_map(|p| p.to_le_bytes()).collect()
    }
}

/// The import granule of a surface's memory (Apple's VM page; Linux's page, which Vulkan's host-pointer import
/// alignment equals on every driver we run).
#[cfg(target_os = "macos")]
const PAGE: usize = 16384;
#[cfg(not(target_os = "macos"))]
const PAGE: usize = 4096;

/// Retired instructions of this process so far (trace only; bough-sys's counters: proc_pid_rusage on macOS,
/// perf_event_open on Linux).
pub fn instrs() -> u64 {
    bough_sys::perf::snap().instrs
}

/// Where a frame's instructions go (`BOUGH_KPROF=1`): laps of the calling thread's retired-instruction
/// counter between named points of the frame path, summed over frames and printed when the backend is
/// dropped. A lap costs two counter reads; the cost of a read is measured at start-up and subtracted.
pub mod kprof {
    use std::cell::Cell;
    use std::sync::OnceLock;

    pub const NAMES: [&str; 14] = [
        "frame: idle test, pick a surface",
        "draw: surface state, damage rows (ring OR), copy plan",
        "stage: reset the plane, list damaged tiles",
        "stage: page cull",
        "stage: records -> commands",
        "surface: lock, scroll copy",
        "run: bin (tile programs)",
        "run: choose",
        "run: raster (decode, shade, store)",
        "run: counters",
        "surface: certificates, unlock",
        "present (show_held)",
        "frame: epoch advance",
        "(unused)",
    ];
    thread_local! {
        static ACC: Cell<[u64; 14]> = const { Cell::new([0; 14]) };
        static LAST: Cell<u64> = const { Cell::new(0) };
        static FRAMES: Cell<u64> = const { Cell::new(0) };
    }
    static ON: OnceLock<(bool, u64)> = OnceLock::new();
    /// `BOUGH_KPROF=wall`: laps are nanoseconds of wall time instead of instructions.
    static WALL: OnceLock<Option<std::time::Instant>> = OnceLock::new();

    fn read() -> u64 {
        match WALL.get_or_init(|| (std::env::var("BOUGH_KPROF").as_deref() == Ok("wall")).then(std::time::Instant::now)) {
            Some(t0) => t0.elapsed().as_nanos() as u64,
            None => read_instr(),
        }
    }

    #[cfg(target_os = "macos")]
    fn read_instr() -> u64 {
        unsafe extern "C" {
            fn thread_selfcounts(kind: i32, buf: *mut u64, nbytes: usize) -> i32;
        }
        let mut c = [0u64; 2];
        // SAFETY: kind 1 writes two u64s (instructions, cycles) into the 16-byte buffer.
        unsafe { thread_selfcounts(1, c.as_mut_ptr(), 16) };
        c[0]
    }
    #[cfg(not(target_os = "macos"))]
    fn read_instr() -> u64 {
        bough_sys::perf::snap().instrs
    }
    fn cfg() -> (bool, u64) {
        *ON.get_or_init(|| {
            if std::env::var_os("BOUGH_KPROF").is_none() {
                return (false, 0);
            }
            // the cost of one read, as the smallest gap between consecutive reads
            let c = (0..64).map(|_| { let a = read(); read() - a }).min().unwrap_or(0);
            (true, c)
        })
    }
    #[inline]
    pub fn on() -> bool {
        cfg().0
    }
    /// Begin a frame.
    #[inline]
    pub fn start() {
        if on() {
            FRAMES.with(|f| f.set(f.get() + 1));
            LAST.with(|l| l.set(read()));
        }
    }
    /// Charge everything since the last point to phase `k`.
    #[inline]
    pub fn lap(k: usize) {
        let (on, c) = cfg();
        if on {
            let t = read();
            ACC.with(|a| {
                let mut v = a.get();
                v[k] += (t - LAST.with(|l| l.get())).saturating_sub(c);
                a.set(v);
            });
            LAST.with(|l| l.set(read()));
        }
    }
    pub fn report() {
        let (on, c) = cfg();
        let n = FRAMES.with(|f| f.get());
        if !on || n == 0 {
            return;
        }
        let acc = ACC.with(|a| a.get());
        let unit = if WALL.get().is_some_and(|w| w.is_some()) { "nanoseconds" } else { "instructions" };
        eprintln!("kprof: {n} frames; {unit} per frame by phase (one counter read = {c}, subtracted)");
        for (name, v) in NAMES.iter().zip(acc) {
            if v != 0 {
                eprintln!("  {:>8.0}  {name}", v as f64 / n as f64);
            }
        }
        eprintln!("  {:>8.0}  total inside frame()", acc.iter().sum::<u64>() as f64 / n as f64);
    }
}

impl Drop for KernelBackend {
    fn drop(&mut self) {
        kprof::report();
    }
}
