//! The kernel backend (compute lane, round 7; `BOUGH_REPAIR=kernel`): frames drawn by `bough-kernel`, the one
//! integer kernel whose CPU executors and GPU executor produce the same bytes by construction. The damaged
//! tiles' records are staged into kernel commands, binned into tile programs (with the occlusion cut), and
//! run by whichever executor the measured costs favour: this thread, bands of tile rows across the pool, or
//! the GPU. All three write the target IOSurface's memory in place (the GPU through a buffer that aliases
//! it), so there is no second arithmetic model, no per-machine self-check and no seam when the choice changes
//! from one frame to the next.
//!
//! What it does not draw yet: images, ramp gradients, transformed spaces. A frame holding one is refused
//! before a byte is written and the App falls back to the presenter's kernels for good (their pixels differ
//! from this kernel's, so the two never share a surface ring).
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
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    gpu: Option<bough_kernel::gpu::Gpu>,
    /// Per surface: the address its target buffer wraps, and the buffer.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    targets: Vec<Option<(usize, bough_kernel::gpu::Target)>>,
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
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            gpu: None,
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            targets: Vec::new(),
            last_gpu: None,
            stats: KStats::default(),
            last: None,
        }
    }

    fn space(rt: &Rt, s: u16) -> Option<[f32; 2]> {
        let xf = rt.xforms.get(s as usize)?;
        if xf.iter().any(|&v| v != 0.0) {
            return None;
        }
        let o = rt.glyphs.spaces.get(s as usize)?;
        Some([o[0] as f32, o[1] as f32])
    }

    /// Stage every record touching a damaged tile into kernel commands, in paint order. `false`: something
    /// this kernel does not draw; nothing was written anywhere.
    pub fn stage(&mut self, rt: &Rt, rows: &[u128], full: bool) -> bool {
        let (w, h) = (rt.win_w, rt.win_h);
        let pl = &mut *self.plane;
        let ty = (h as usize).div_ceil(32);
        let all = if w >= 128 * 32 { u128::MAX } else { (1u128 << (w as u32).div_ceil(32)) - 1 };
        pl.rows = [0; bough_core::damage::MAX_TILES_Y];
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
        let bbox = pl.px_bbox(rt.bough.window());
        rt.bough.cull_tiles_pos(pl, bbox, &mut self.pos);
        let pos = std::mem::take(&mut self.pos);
        let ok = pos.iter().all(|&p| self.stage_record(rt, p as usize));
        self.pos = pos;
        self.stats.staged += self.scene.len() as u64;
        ok
    }

    fn stage_record(&mut self, rt: &Rt, pos: usize) -> bool {
        if rt.bough.xform_at(pos).is_some() {
            return false;
        }
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
                let Some(off) = Self::space(rt, rh.space) else { return false };
                let Some(toks) = rt.text.cache.tok_strike(rh.strike as usize) else { return false };
                for slot in rh.base..rh.base + rh.len {
                    let (Some(&x), Some(&e)) = (rt.glyphs.x.get(slot as usize), rt.glyphs.ent.get(slot as usize)) else { return false };
                    let en = &toks.entry[(e & 255) as usize];
                    if en.uv[2] == 0 {
                        continue;
                    }
                    // bough-text's pen formula (the one float expression kept from the old kernels: it is
                    // evaluated here, once, on the CPU, for every executor)
                    let dx = x.wrapping_sub(rh.x0);
                    let px = (dx as f32).mul_add(rh.scale, rh.origin[0] + 0.5).floor() + en.geo[0];
                    let texel0 = en.uv[1] as u32 * atlas.width + en.uv[0] as u32;
                    self.scene.add(stage::glyph(px + off[0], rh.origin[1] + en.geo[1] + off[1], en.geo[2] as u32, en.geo[3] as u32, texel0, rh.color, &cl));
                }
            }
            return true;
        }
        let (i0, n) = if p & PAY_IRUN != 0 { (p & 0x3FFFF, ((p >> 18) & 0xFFF) + 1) } else { (p, 1) };
        for i in i0..i0 + n {
            let Some(q) = rt.inst.get(i as usize) else { return false };
            let Some(off) = Self::space(rt, q.space) else { return false };
            let (x, y, w, h) = (q.pos[0] + off[0], q.pos[1] + off[1], q.size[0], q.size[1]);
            let c = match q.kind {
                0 => stage::solid(x, y, w, h, q.color, &cl),
                1 => stage::rrect(x, y, w, h, q.radius, q.color, &cl),
                2 => stage::border(x, y, w, h, q.radius, q.bw, q.color, q.color2, &cl),
                3 => stage::glyph(x, y, w as u32, h as u32, q.uv[1] as u32 * atlas.width + q.uv[0] as u32, q.color, &cl),
                6 => {
                    // the record's ink (what damage tracks) is its box grown by `flags` px: the shadow is
                    // clipped to it, so nothing is painted where a later move would not repair
                    let m = if q.flags != 0 { q.flags as f32 } else { 1.0 };
                    let ink = [cl[0].max(x - m), cl[1].max(y - m), cl[2].min(x + w + m), cl[3].min(y + h + m)];
                    stage::shadow(x, y, w, h, q.radius, q.bw.max(1e-3), q.color, &ink)
                }
                // images and ramp gradients: not in this kernel yet
                _ => return false,
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
        self.bin.bin(&self.scene, w, h, stride as u32, &atlas, 0, &damage, true, &mut self.prog);
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
                            // the atlas is uploaded whole per GPU frame for now (a generation counter in
                            // bough-text would make it per change)
                            g.upload_atlas(&a.pixels);
                            g.upload_frame(&self.scene, &self.prog);
                            g.render_to(tiles as usize, t);
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
            cpu::raster(&self.scene, &self.prog, &atlas, out, stride, &self.pool, extra, Opt::default());
            if extra > 0 {
                self.stats.cpun += 1;
            } else {
                self.stats.cpu1 += 1;
            }
        }
        if self.trace {
            #[cfg(target_os = "macos")]
            eprintln!("kernel run: bin {} instructions, choose + execute {}", i1 - i0, instrs() - i1);
            eprintln!("kernel frame: {} staged commands, {tiles} tiles, {entries} entries, estimate {} ns, {exec:?}", self.scene.len(), self.bin.est_cpu_ns);
        }
        self.stats.frames += 1;
        self.stats.tiles += tiles;
        self.stats.entries += entries;
        self.last = Some(exec);
        tiles as u32
    }

    /// The surfaces' memory is about to be released (a resize): drop the GPU's imports of it first (an import
    /// must not outlive the pages it names, and a new allocation may reuse an address).
    pub fn forget_targets(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        self.targets.clear();
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
    pub fn reference(&mut self, rt: &Rt) -> Option<Vec<u8>> {
        if !self.stage(rt, &[], true) {
            return None;
        }
        let (w, h) = (rt.win_w as u32, rt.win_h as u32);
        let a = &rt.text.cache.atlas;
        let atlas = Atlas::new(&a.pixels, a.width).with_images(rt.images.pixels(), bough_frame::IMG_W);
        let mut out = vec![0u32; (w * h) as usize];
        cpu::reference(&self.scene, w, h, &atlas, 0, &mut out, w as usize, &self.pool);
        Some(out.iter().flat_map(|p| p.to_le_bytes()).collect())
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
