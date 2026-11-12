//! `App<V>`: owns the view value (the owner), the runtime, the optional fused presenter, and drives turns:
//! handler -> settle wires (text patches, layout) -> damage -> tile list -> GPU repair -> present.
//!
//! The CPU's frame ends at the tile list: the GPU culls bough-core's pages and expands bough-text's runs
//! itself (bough-metal `fs_bin`/`fs_tile`), so there is no CPU cull and no draw list.
//!
//! # Surfaces (the ring)
//! A surface is chosen per frame, never skipped:
//! 1. **Content addressing**: a surface holds the current state iff no epoch since it was drawn had
//!    non-paint damage (geometry, text, structure: one flag per epoch) and the paint writes since then
//!    (logged per epoch as record + old/new state hash, `Rt::paint_log`) compose to the identity for every
//!    record. If the surface on screen already shows the current state nothing happens at all; if a free
//!    surface holds it, it is published with no GPU work (a caret blink after its first two frames, a hover
//!    toggle). O(1) per paint write and nothing per tile: a scene without paint-only frames pays nothing.
//! 2. Otherwise the free surface (neither on screen nor held by the compositor) that was drawn most
//!    recently is repaired (the smallest repair).
//! 3. If the compositor holds every drawn surface, a fresh one is drawn (an IOSurface owns no memory until
//!    first written, so the ring is exactly as large as contention has needed, up to 4); only if all 4 are
//!    held does the frame wait for a release. **A frame is never skipped.**
//!
//! Offscreen (tests, headless renders) the ring is round-robin over `Config::ring` surfaces, so every repair
//! age 1..=ring is exercised.
use crate::elem::{Element, Mounter};
use crate::rt::*;
use bough_core::damage::RING;
use bough_core::{Plane, RecordInit};

/// A view: a plain struct whose fields are the state (`designs/reactivity/final/00-model.md` §3). `view`
/// describes it; `Signal` fields bound into operands are direct ports, so publishing one patches its
/// destinations without rerunning `view`.
pub trait View: Sized + 'static {
    fn view(&self) -> impl Element<Self> + '_;
    /// The one effect site: what crossed the owner's boundaries this turn (`changed.rs`). It may write
    /// state; the writes settle in another wave of the same turn.
    fn changed(&mut self, _d: crate::changed::Changed<'_, Self>) {}
}

pub struct Config {
    pub width_pt: f32,
    pub height_pt: f32,
    pub scale: f32,
    pub font: Vec<u8>,
    /// Surfaces: 0 = default (4: round-robin offscreen; windowed, at most 4, drawn only as contention needs).
    pub ring: usize,
    pub caps: Caps,
    /// Further faces: (logical face `text::FACE_BOLD` / `FACE_MONO` / `FACE_OBLIQUE`, font file).
    pub faces: Vec<(u8, Vec<u8>)>,
    /// A harness option, never an application's: mount the view once and change it afterwards only through
    /// wires, lists and readers (no re-description tape, no re-description after a handler). The scene
    /// benchmarks set it to measure the wire path alone; `Application::run` never does, so every
    /// application view re-describes after its handlers (`designs/reactivity/final/01-state.md` §2).
    pub mount_once: bool,
}

impl Config {
    pub fn new(width_pt: f32, height_pt: f32, scale: f32, font: Vec<u8>) -> Config {
        Config { width_pt, height_pt, scale, font, ring: 0, caps: Caps::DEFAULT, faces: Vec::new(), mount_once: false }
    }
    /// Surfaces of the ring.
    pub fn surfaces(&self) -> usize {
        if self.ring == 0 { 4 } else { self.ring.min(4) }
    }
    /// `mount_once` set (a scene benchmark measuring the wire path alone).
    pub fn once(mut self) -> Config {
        self.mount_once = true;
        self
    }
}

/// Released nodes returned to the free lists per idle frame or turn (`Rt::reclaim`).
pub const RECLAIM_BUDGET: usize = 4096;

pub const PH_UPDATE: usize = 0;
pub const PH_TEXT: usize = 1;
pub const PH_DAMAGE: usize = 2;
pub const PH_CULL: usize = 3;
pub const PH_PRESENT: usize = 4;
pub const PHASE_NAMES: [&str; 5] = ["update", "layout", "damage", "tile list", "present"];

/// Instruction-count phase profiler (proc_pid_rusage; off unless enabled).
pub struct Prof {
    pub on: bool,
    pub acc: [u64; 5],
    last: u64,
    ovh: u64,
}
impl Prof {
    fn new() -> Prof {
        Prof { on: false, acc: [0; 5], last: 0, ovh: 0 }
    }
    pub fn enable(&mut self) {
        self.on = true;
        self.ovh = bough_sys::perf::snap_overhead();
    }
    #[inline]
    pub fn reset(&mut self) {
        if self.on {
            self.last = bough_sys::perf::snap().instrs;
        }
    }
    #[inline]
    pub fn mark(&mut self, i: usize) {
        if self.on {
            let s = bough_sys::perf::snap().instrs;
            self.acc[i] += (s - self.last).saturating_sub(self.ovh);
            self.last = bough_sys::perf::snap().instrs;
        }
    }
}

/// Surface ring counters (see the module docs).
#[derive(Default, Clone, Copy, Debug)]
pub struct SurfStats {
    /// Frames drawn into each ring slot.
    pub uses: [u64; 4],
    /// Frames presented: GPU repairs + surfaces published without GPU work + "already on screen".
    pub frames: u64,
    pub gpu: u64,
    /// Published a surface that already held the state (no GPU).
    pub held: u64,
    /// The surface on screen already showed the state (nothing at all).
    pub on_screen: u64,
    /// Frames that found every drawn surface held and drew a fresh one (the ring grew).
    pub grew: u64,
    /// Frames that waited for the compositor (all 4 surfaces held), and the longest wait (us).
    pub waits: u64,
    pub max_wait_us: u64,
    /// Frames skipped (by construction 0: kept to make it measurable).
    pub skipped: u64,
    /// Surfaces found purged (volatile memory reclaimed) when made resident again.
    pub purged: u64,
    /// Trim passes that made at least one surface volatile.
    pub trims: u64,
    /// At each growth, why the drawn surfaces other than the one on screen were unavailable, by reason
    /// (bough-metal `Fused::in_use_why` bits: committing, queued, the publisher's last, the calling thread's
    /// last or paced, the compositor).
    pub grew_why: [u64; 5],
    /// Frames repaired by the CPU backend (no GPU work).
    pub cpu: u64,
}

/// The presenter, chosen at compile time (never per frame): the vendored-wgpu-hal port (`bough_gpu::fused`,
/// feature `wgpu`, ROUND4 §8) or the direct-Metal one (`bough_metal::fused`). Same API, same region layout.
#[cfg(all(target_os = "macos", feature = "wgpu"))]
pub use bough_gpu::fused as presenter;
#[cfg(all(target_os = "macos", not(feature = "wgpu")))]
pub use bough_metal::fused as presenter;

/// Which backend may repair a frame (`BOUGH_REPAIR`: `gpu`, the default for now; `auto`, the cost model; `cpu`,
/// whenever the frame's records are all modelled, which tests use to hold the CPU path to the GPU's reference).
/// Every backend writes the same pixels (cpu_raster.rs); `Auto` also requires the device's self-check
/// (`repair::cpu_exact`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repair {
    Auto,
    Gpu,
    Cpu,
}

impl Repair {
    pub fn from_env() -> Repair {
        match std::env::var("BOUGH_REPAIR").as_deref() {
            Ok("auto") => Repair::Auto,
            Ok("cpu") => Repair::Cpu,
            _ => Repair::Gpu,
        }
    }
}

enum Pick {
    /// The surface on screen already shows the state.
    OnScreen,
    /// Publish this surface: it already holds the state.
    Held(usize),
    Draw(usize),
}

pub struct App<V: View> {
    pub owner: Box<V>,
    pub rt: Rt,
    #[cfg(target_os = "macos")]
    pub gpu: Option<presenter::Fused>,
    /// Per surface: the epoch whose state it holds (`None`: nothing valid).
    held: Vec<Option<u32>>,
    shown: Option<usize>,
    next_rr: usize,
    /// Content-addressed, contention-sized ring (windowed) instead of round-robin (offscreen).
    pub windowed: bool,
    pub prof: Prof,
    pub frames: u64,
    pub last_surface: usize,
    /// Tiles the last frame repaired.
    pub repaired_tiles: u32,
    pub surf_stats: SurfStats,
    rows: Vec<u128>,
    pa: Box<Plane>,
    pb: Box<Plane>,
    /// Per epoch (`epoch % RING`): did it have non-paint damage?
    core_epochs: [bool; RING],
    /// `frames` value when each ring slot was last drawn into.
    last_use: Vec<u64>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    trim_mark: u64,
    /// How the mount overlapped the GPU's preparation (see [`prepare`]).
    pub startup: Startup,
    /// The last drawn frame repainted the whole window (a surface with nothing reusable).
    last_full: bool,
    /// The root's re-description tape (`describe.rs`; empty when mounted once, `Config::mount_once`).
    tape: Vec<u32>,
    /// `Config::mount_once`: no tape, no re-description.
    once: bool,
    /// The repair backends' choice and the CPU backend (cpu_raster.rs).
    pub repair: Repair,
    pub cpu: crate::cpu_raster::CpuRaster,
    /// Surfaces the CPU executor draws every frame into, where there is no GPU presenter (`backend::Surfaces`: a
    /// window host's buffers, or memory offscreen). `None` with a GPU presenter or headless.
    pub target: Option<Box<dyn crate::backend::Surfaces>>,
    /// The tile rows a presented frame differs from the one on screen by (the target's damage).
    shown_rows: Vec<u128>,
}

/// Per raster worker started by [`prepare`]: (strikes, glyphs, instructions, ns), for those finished.
pub fn raster_stats() -> Vec<[u64; 4]> {
    bough_text::atlas::prerender_stats()
}

/// The main thread's share of [`prepare`]'s rasterising: strikes it rasterised itself (no worker had
/// started them), strikes it waited for, ns waited.
pub fn raster_main() -> [u64; 3] {
    bough_text::atlas::prerender_main()
}

/// Cold start accounting of [`App::new`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Startup {
    /// Main thread: runtime, mount, shaping and layout.
    pub mount_ns: u64,
    pub mount_instr: u64,
    /// The GPU preparation thread (device, queues, library, pipelines), if it ran from [`prepare`].
    pub gpu_prep_instr: u64,
    pub gpu_prep_ns: u64,
    /// Main thread: waiting for that thread, then the presenter's buffers and surfaces.
    pub gpu_wait_ns: u64,
}

/// Call at process entry: start the work of a cold start that does not depend on the mount on other
/// threads: the Metal device, queues and pipelines on one, and the glyphs of the document's static text
/// (`strikes`: pixel size and text, in the order the mount meets them) on `raster_threads` more. The next
/// [`App::new`] mounts while they run and takes their results.
pub fn prepare(cfg: &Config, strikes: &[(f32, &str, u8)], svgs: &[(&'static str, f32, f32)], raster_threads: usize) {
    #[cfg(target_os = "macos")]
    {
        let (w, h) = ((cfg.width_pt * cfg.scale).round() as usize, (cfg.height_pt * cfg.scale).round() as usize);
        bough_metal::fused::prepare_in_background(Some((w, h, cfg.surfaces(), BIN_CAP)));
    }
    crate::text::prepare_faces(&cfg.faces);
    if !strikes.is_empty() && raster_threads > 0 {
        // faces by logical id: the configured font, or the regular one when the face is absent
        let font_of = |l: u8| cfg.faces.iter().find(|f| f.0 == l).map_or(&cfg.font[..], |f| &f.1[..]);
        let fonts: Vec<&[u8]> = (0..4u8).map(font_of).collect();
        // each strike is ASCII-complete from its creation (text.rs), so its whole printable alphabet too
        const ASCII: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";
        let mut px: Vec<(f32, &str, usize)> = strikes.iter().map(|&(s, t, f)| (s * cfg.scale, t, f as usize & 3)).collect();
        let mut seen: Vec<(u32, usize)> = Vec::new();
        for &(s, _, f) in strikes {
            if !seen.contains(&(s.to_bits(), f as usize)) {
                seen.push((s.to_bits(), f as usize));
                px.push((s * cfg.scale, ASCII, f as usize & 3));
            }
        }
        bough_text::atlas::prerender_in_background(&fonts, &px, raster_threads);
    }
    if !svgs.is_empty() && raster_threads > 0 {
        let items = svgs.iter().map(|&(s, w, h)| (s, (w * cfg.scale).round().max(1.0) as u32, (h * cfg.scale).round().max(1.0) as u32)).collect();
        crate::deco::prerender_svgs(items, raster_threads);
    }
}

/// Bin capacity per tile row the presenter starts with.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const BIN_CAP: u32 = 1024;

/// Repair-backend limits (damaged tiles per frame below which the CPU backend repairs): provisional values until
/// the per-tile costs of both backends are measured (card `2026-11-11-repair-backends`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const CPU_TILES_WARM: u32 = 16;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const CPU_TILES_COLD: u32 = 4096;

impl<V: View> App<V> {
    /// Mount `view`. `gpu`: create the fused presenter (its shared buffer is the region); otherwise the
    /// region is heap memory and `frame` stops after the tile list (the RENDER=0 shape).
    pub fn new(view: V, cfg: Config, gpu: bool) -> App<V> {
        let t_mount = std::time::Instant::now();
        let i_mount = bough_sys::perf::thread_instructions();
        let (win_w, win_h) = ((cfg.width_pt * cfg.scale).round() as i32, (cfg.height_pt * cfg.scale).round() as i32);
        let ring = cfg.surfaces();
        // direct Metal: the GPU sees each column through a view of its used prefix (bough-metal
        // `Fused::new_views`), so the region reserves the record format's limits as address space; the wgpu-hal
        // presenter wraps the whole region in one buffer and keeps the configured capacities
        #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
        let caps = if gpu { cfg.caps.reserve() } else { cfg.caps };
        #[cfg(not(all(target_os = "macos", not(feature = "wgpu"))))]
        let caps = cfg.caps;
        let bytes = caps.region_bytes(win_w as u32, win_h as u32);
        // wgpu (`--features wgpu`): the device boots on another thread (the OS's driver load, ~22 ms cold) while this
        // thread mounts, lays out and shapes into region memory the presenter then adopts without a copy (bough-gpu
        // RESULTS)
        #[cfg(all(target_os = "macos", feature = "wgpu"))]
        let boot = gpu.then(bough_gpu::fused::Boot::spawn);
        #[cfg(all(target_os = "macos", feature = "wgpu"))]
        let mem = gpu.then(|| std::sync::Arc::new(bough_gpu::fused::RegionMem::new(bytes)));
        #[cfg(all(target_os = "macos", feature = "wgpu"))]
        #[allow(unused_mut)]
        let metal: Option<presenter::Fused> = None;
        #[cfg(all(target_os = "macos", feature = "wgpu"))]
        let overlapped = false;
        // direct Metal: a GPU prepared from process entry (`prepare`): the main thread mounts, shapes and lays out
        // into heap memory first; the presenter is made afterwards, its shared buffer wrapping that memory (no copy)
        #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
        let overlapped = gpu && bough_metal::fused::prep_pending();
        // the region is always this process's own lazily backed memory; the presenter is made over it below
        #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
        let metal: Option<presenter::Fused> = None;
        #[cfg(not(target_os = "macos"))]
        let _ = gpu;
        #[cfg(target_os = "macos")]
        let region = match &metal {
            Some(m) => {
                let (p, len, keep) = m.region();
                // SAFETY: the shared buffer's contents: `len` bytes, page-aligned, zero-filled, alive as long as
                // `keep` (the MTLBuffer) is; written only through the region's columns and by the presenter's
                // header/slot writers while the GPU is idle.
                unsafe { bough_mem::Region::external(p, len, Box::new(keep)) }
            }
            #[cfg(feature = "wgpu")]
            None if mem.is_some() => {
                let m = mem.clone().unwrap();
                // SAFETY: `m` is page-aligned, zero-filled and lives as long as this keep-alive (and the presenter's
                // clone); written only through the region's columns and the presenter's header/slot writers.
                unsafe { bough_mem::Region::external(m.ptr, m.len, Box::new(m)) }
            }
            None => bough_mem::Region::heap(bytes),
        };
        #[cfg(not(target_os = "macos"))]
        let region = bough_mem::Region::heap(bytes);
        #[cfg(all(target_os = "macos", feature = "wgpu"))]
        let _ = overlapped;
        let mut rt = Rt::new(win_w, win_h, cfg.scale, &cfg.font, region, caps);
        // the presenter over the region's views: made now (its device was built here), or after the mount when
        // a preparation thread is building the device meanwhile (`prepare`)
        #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
        let views = |rt: &Rt| {
            let (p, len, keep) = rt.region_mem();
            let base = rt.layout.slot[2] as usize + bough_frame::Layout::slot_bytes(rt.layout.w, rt.layout.h.max(MAX_WINDOW));
            // SAFETY: the runtime's region: page-aligned (mmap), `len` bytes, alive while `keep` (a share of it)
            // is, which the presenter holds as long as its buffers; every GPU-read column was carved on a page
            // boundary (`Rt::new`) and `region_bytes` leaves a page after each.
            unsafe { bough_metal::fused::Fused::new_views(p, len, keep, base, win_w as usize, win_h as usize, cfg.scale as f64, ring, BIN_CAP) }
        };
        #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
        let metal = match metal {
            None if gpu && !overlapped => Some(views(&rt).0),
            m => m,
        };
        for (logical, font) in &cfg.faces {
            rt.text.add_face(*logical, font);
        }
        #[cfg(target_os = "macos")]
        let mut metal = metal;
        #[cfg(target_os = "macos")]
        if let Some(m) = metal.as_mut() {
            m.set_layout(rt.layout);
        }
        rt.lay.mount_avail = (win_w as f32, win_h as f32);
        let owner = Box::new(view);
        let mut recs: Vec<RecordInit> = Vec::with_capacity(4096);
        let mut scope = Scope::default();
        let mut tape: Vec<u32> = Vec::new();
        let once = cfg.mount_once;
        let ptr = &*owner as *const V;
        rt.owner = ptr as *const u8;
        {
            let el = owner.view();
            let tp: *mut Vec<u32> = if once { std::ptr::null_mut() } else { &mut tape };
            let mut m = Mounter { rt: &mut rt, owner: ptr, recs: &mut recs, scope: &mut scope, depth: 0, ox: 0.0, oy: 0.0, space: 0, clip: 0, pending_num: None, cls: 0, tape: tp, ti: 0, pos: 0, kid: 0, keep: false };
            el.mount(&mut m);
        }
        rt.bough.insert_run(0, &recs);
        // layout groups of the first mount (list regions mount their first rows between its passes)
        rt.lay_flush();
        let mut owner = owner;
        // wires queued while mounting (a producer's first request flips its reader to pending)
        // SAFETY: the owner just mounted, uniquely borrowed here.
        unsafe { rt.settle(&mut *owner as *mut V as *const u8) };
        rt.lay_flush();
        drop(scope); // the root scope lives as long as the app: nothing of it is ever released
        #[allow(unused_mut)]
        let mut startup = Startup { mount_ns: t_mount.elapsed().as_nanos() as u64, mount_instr: bough_sys::perf::thread_instructions() - i_mount, ..Startup::default() };
        #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
        if overlapped {
            let t = std::time::Instant::now();
            let (mut f, instr, ns) = views(&rt);
            f.set_layout(rt.layout);
            startup = Startup { gpu_prep_instr: instr, gpu_prep_ns: ns, gpu_wait_ns: t.elapsed().as_nanos() as u64, ..startup };
            metal = Some(f);
        }
        #[cfg(all(target_os = "macos", feature = "wgpu"))]
        if let (Some(b), Some(m)) = (boot, mem) {
            let t = std::time::Instant::now();
            let mut f = presenter::Fused::adopt(b.join().expect("device boot"), win_w as usize, win_h as usize, cfg.scale as f64, m, ring, BIN_CAP);
            f.set_layout(rt.layout);
            startup.gpu_wait_ns = t.elapsed().as_nanos() as u64;
            metal = Some(f);
        }
        let ty = rt.tiles_y;
        // no GPU presenter on this platform: the CPU executor draws every frame, offscreen until a host gives the
        // app its window's surfaces
        #[cfg(not(target_os = "macos"))]
        let target = gpu.then(|| Box::new(crate::backend::MemSurfaces::new(win_w as u32, win_h as u32, ring)) as Box<dyn crate::backend::Surfaces>);
        #[cfg(target_os = "macos")]
        let target = None;
        App {
            owner,
            rt,
            #[cfg(target_os = "macos")]
            gpu: metal,
            held: vec![None; ring],
            shown: None,
            next_rr: 0,
            windowed: false,
            prof: Prof::new(),
            frames: 0,
            last_surface: 0,
            repaired_tiles: 0,
            surf_stats: SurfStats::default(),
            rows: vec![0; ty],
            pa: Box::new(Plane::EMPTY),
            pb: Box::new(Plane::EMPTY),
            core_epochs: [false; RING],
            last_use: vec![0; ring],
            trim_mark: 0,
            startup,
            last_full: true,
            tape,
            once,
            repair: Repair::from_env(),
            cpu: crate::cpu_raster::CpuRaster::new(),
            target,
            shown_rows: vec![0; ty],
        }
    }

    /// The cost model's limit: frames of at most this many damaged tiles are repaired on the CPU. While the
    /// GPU's queue has not run work (or was released) a GPU frame costs a queue wake (~3 ms, ~11 M instructions,
    /// drvprobe) and the driver's 69 MiB, so the CPU takes much larger frames.
    #[inline]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn repair_limit(&self, gpu_warm: bool) -> u32 {
        if gpu_warm { CPU_TILES_WARM } else { CPU_TILES_COLD }
    }

    /// Frames per pre-armed command buffer. Every frame goes through the pre-armed stream (bough-metal
    /// RESULTS Part IV: released by a shared event, published in the same turn); this only sizes its batches.
    pub fn stream(&mut self, k: usize) {
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut() {
            g.set_batch(k);
        }
        let _ = k;
    }

    #[cfg(target_os = "macos")]
    fn sync(&mut self) {
        if let Some(g) = self.gpu.as_mut() {
            g.sync();
        }
    }
    #[cfg(not(target_os = "macos"))]
    fn sync(&mut self) {}

    /// `sync` for the input module (an input turn writes the region, so the GPU must be done with it).
    pub(crate) fn sync_pub(&mut self) {
        self.sync();
    }

    /// The window is now `w` x `h` device px: the tile-row buffer follows and every surface is stale.
    pub(crate) fn resized(&mut self, w: i32, h: i32) {
        self.rows = vec![0; (h as usize).div_ceil(32)];
        self.shown_rows = vec![0; (h as usize).div_ceil(32)];
        for e in &mut self.held {
            *e = None;
        }
        if let Some(t) = self.target.as_mut() {
            t.resize(w as u32, h as u32);
        }
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut() {
            // surfaces, bins and the layer follow the window (bough-metal `Fused::resize`); the next frame
            // is a full redraw at the new size, published with the new layer frame in one transaction
            g.resize(w as usize, h as usize);
            g.set_layout(self.rt.layout);
        }
    }

    /// One turn: run the handler, settle the wires it queued (text patches and marks happen inside the
    /// wires), lay out what changed.
    pub fn turn(&mut self, f: impl FnOnce(&mut V)) {
        self.sync();
        if self.reclaiming() {
            self.rt.reclaim(RECLAIM_BUDGET);
        }
        self.prof.reset();
        self.rt.pubs.clear();
        self.rt.admit();
        f(&mut self.owner);
        let owner = &mut *self.owner as *mut V as *const u8;
        // SAFETY: the owner is the mounted view; every wire was installed from a `sig!` proof for it.
        unsafe { self.rt.settle(owner) };
        self.effects();
        self.redescribe();
        self.prof.mark(PH_UPDATE);
        self.rt.lay_flush();
        self.prof.mark(PH_TEXT);
    }

    /// The tile rows the last drawn frame repaired (bitmask of tile columns per row; inspect.rs).
    pub(crate) fn repaired_rows(&self) -> (&[u128], bool) {
        (&self.rows, self.last_full)
    }

    /// Wait until the GPU has finished every frame submitted so far (the last one is presented).
    pub fn wait_presented(&mut self) {
        self.sync();
    }

    /// Frames reach surfaces (a GPU presenter or a CPU target): false headless (the frame ends at the tile list).
    pub fn has_surfaces(&self) -> bool {
        #[cfg(target_os = "macos")]
        if self.gpu.is_some() {
            return true;
        }
        self.target.is_some()
    }

    /// The width in pixels of the surfaces frames are drawn into (the window's, headless).
    pub fn surface_width(&self) -> usize {
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_ref() {
            return g.w;
        }
        self.target.as_ref().map_or(self.rt.win_w as usize, |t| t.size().0 as usize)
    }

    /// The handler half of a turn alone: mutate the view's signals (no settling). Pair with `flush`.
    pub fn write(&mut self, f: impl FnOnce(&mut V)) {
        f(&mut self.owner);
    }

    /// The rest of a turn after `write`: settle the queued signals' wires and lay out what changed.
    pub fn flush(&mut self) {
        self.sync();
        let owner = &mut *self.owner as *mut V as *const u8;
        // SAFETY: as in `turn`.
        unsafe { self.rt.settle(owner) };
        self.effects();
        self.redescribe();
        self.rt.lay_flush();
    }

    /// A turn with no handler: admit async completions (a committed cohort's wires run in this turn).
    pub fn poll(&mut self) {
        self.turn(|_| {});
    }
    /// Run up to `n` queued async jobs (the manual executor; their completions are admitted next turn).
    pub fn run_jobs(&mut self, n: usize) -> usize {
        self.rt.run_jobs(n)
    }

    /// `View::changed` waves: while something crossed a boundary, show it to the owner and settle what it
    /// wrote (at most 8 waves; a ninth is a cycle).
    pub(crate) fn effects(&mut self) {
        for _ in 0..8 {
            if self.rt.pubs.is_empty() && self.rt.asy.committed.is_empty() {
                return;
            }
            let d = crate::changed::Changed { pubs: &self.rt.pubs, readers: &self.rt.asy.committed, _v: std::marker::PhantomData };
            self.owner.changed(d);
            // other owners (list rows, entities, row-owned readers) whose own boundaries published
            let owners = std::mem::take(&mut self.rt.pubs.owners);
            for &(o, cls) in &owners {
                if let Some(f) = self.rt.class_changed[cls as usize] {
                    // SAFETY: `o` is the live owner the publishing wire ran for (a row in its list's slab, an
                    // entity in its slab), of the class's type; settled right after with its own class.
                    unsafe {
                        f(o as *mut u8, &self.rt.pubs, &self.rt.asy.committed);
                        self.rt.settle_cls(o as *const u8, cls);
                    }
                }
            }
            self.rt.pubs.clear();
            self.rt.asy.committed.clear();
            let owner = &mut *self.owner as *mut V as *const u8;
            // SAFETY: as in `turn`.
            unsafe { self.rt.settle(owner) };
        }
        panic!("View::changed did not settle in 8 waves (a cycle between changed and its own boundaries)");
    }

    /// Describe the view again and patch the retained records (`describe.rs`); not when mounted once.
    pub(crate) fn redescribe(&mut self) {
        if !self.once {
            // SAFETY: the tape was recorded by this owner's mount at the start of the Bough.
            unsafe { crate::describe::redescribe(&mut self.rt, &*self.owner, &mut self.tape) };
        }
    }

    /// OR of the last `age` planes of both damage rings into `self.rows` (`false`: nothing damaged).
    fn repair_rows(&mut self, age: u32) -> bool {
        self.rt.bough.repair_into(age, &mut self.pa);
        self.rt.paint.repair_into(age, &mut self.pb);
        let (a0, a1) = self.pa.occ_range();
        let (b0, b1) = self.pb.occ_range();
        self.rows.fill(0);
        let mut any = 0u128;
        for y in a0..a1.min(self.rows.len()) {
            self.rows[y] = self.pa.rows[y];
            any |= self.pa.rows[y];
        }
        for y in b0..b1.min(self.rows.len()) {
            self.rows[y] |= self.pb.rows[y];
            any |= self.pb.rows[y];
        }
        any != 0
    }

    /// Scroll by copy (bough-metal RESULTS §15) for target `i` of age `age` at `epoch`, after `repair_rows`.
    /// A scroll epoch damaged only its exposed strip and static bands, so a surface whose age window holds one
    /// lacks the scrolled clip's content. It is taken from the surface showing epoch - 1 (`shown`, always exact)
    /// by ONE copy whatever the age: clip C = the latest record's, move = this epoch's (0 if the scroll was
    /// earlier). The rows then are: repair(age) on tiles not wholly inside C, this epoch's own marks on tiles
    /// meeting C (they include the exposed strip and the static bands), and every other record's clip whole.
    /// No source (ring of 1: the target IS `shown`) or a clip that changed inside the window: those clips are
    /// marked whole. Bounds conditions of the copy's one source and one rect, not a policy.
    fn copy_plan(&mut self, i: usize, epoch: u32, age: u32) -> bough_frame::SlotCopy {
        use bough_frame::SlotCopy;
        let b = &self.rt.bough;
        let Some((b0, r0)) = (0..age).find_map(|k| b.scroll_rec(k).map(|r| (k, r))) else { return SlotCopy::NONE };
        // (the CPU target moves the clip in place when it repairs the surface on screen: `copy_rect` is a memmove)
        let src = self.shown.filter(|&s| (s != i || self.target.is_some()) && self.held[s] == Some(epoch - 1));
        let rows = self.rows.len();
        let mark = |out: &mut [u128], c: bough_core::NRect| {
            if c.is_empty() {
                return;
            }
            let (tx0, ty0, tx1, ty1) = bough_core::damage::tile_span(c);
            let m = (u128::MAX >> (128 - (tx1 - tx0))) << tx0;
            for r in &mut out[ty0 as usize..(ty1 as usize).min(rows)] {
                *r |= m;
            }
        };
        let Some(s) = src else {
            for k in b0..age {
                if let Some(r) = b.scroll_rec(k) {
                    mark(&mut self.rows, r.clip);
                }
            }
            return SlotCopy::NONE;
        };
        let c = r0.clip;
        let (dx, dy) = if b0 == 0 { (r0.dx, r0.dy) } else { (0, 0) };
        // the gates' sabotage: a copy one pixel off must make every pixel gate fail
        #[cfg(feature = "sabotage_copy")]
        let dy = dy + 1;
        let [x0, y0, x1, y1] = c.to_xyxy();
        let [_, _, ww, wh] = b.window().to_xyxy();
        // tiles wholly inside C (a tile past the window's edge counts as inside if C reaches that edge)
        let (ix0, iy0) = ((x0 + 31) >> 5, (y0 + 31) >> 5);
        let ix1 = if x1 >= ww { (ww + 31) >> 5 } else { x1 >> 5 };
        let iy1 = if y1 >= wh { (wh + 31) >> 5 } else { y1 >> 5 };
        // none when C holds no whole tile across x or y: ix0 >= ix1 or iy0 >= iy1 (a nested viewport cut by its
        // parent's clip to a strip inside one tile row, e.g. [0, 364, 220, 372]: iy0 = 12, iy1 = 11)
        let (ry0, ry1) = (iy0.max(0) as usize, (iy1.max(0) as usize).min(rows));
        if ix0 < ix1 && ry0 < ry1 {
            let inner = (u128::MAX >> (128 - (ix1 - ix0))) << ix0;
            for r in &mut self.rows[ry0..ry1] {
                *r &= !inner;
            }
        }
        // this epoch's own marks on every tile meeting C
        let (tx0, ty0, tx1, ty1) = bough_core::damage::tile_span(c);
        let span = (u128::MAX >> (128 - (tx1 - tx0))) << tx0;
        let (p, q) = (b.plane(0), self.rt.paint.plane(0));
        for y in ty0 as usize..(ty1 as usize).min(rows) {
            self.rows[y] |= (p.rows[y] | q.rows[y]) & span;
        }
        for k in b0 + 1..age {
            if let Some(r) = b.scroll_rec(k) {
                if r.clip != c {
                    mark(&mut self.rows, r.clip);
                }
            }
        }
        let dst = c.intersect(c.translate(dx, dy)).canon();
        if dst.is_empty() {
            return SlotCopy::NONE;
        }
        SlotCopy { src: s as u32, rect: dst.to_xyxy(), dx, dy }
    }

    /// Does surface `s` hold every pixel of the current state (epoch `epoch`, which has only paint damage)?
    /// No epoch since it was drawn may have had non-paint damage, none may have overflowed its paint log,
    /// and the logged paint writes since then must compose to the identity for every record.
    fn holds_current(&self, s: usize, epoch: u32) -> bool {
        let Some(last) = self.held[s] else { return false };
        let age = epoch - last;
        if age as usize > RING || age == 0 {
            return age == 0;
        }
        if (1..age).any(|b| self.core_epochs[(epoch - b) as usize % RING]) {
            return false;
        }
        // (record, state when s was drawn, state now), in write order
        let mut net: [(u64, u64, u64); 4 * PAINT_LOG] = [(0, 0, 0); 4 * PAINT_LOG];
        let mut n = 0;
        for e in last + 1..=epoch {
            let l = &self.rt.paint_log[e as usize % RING];
            if l.overflow {
                return false;
            }
            for &(k, old, new) in &l.e[..l.n as usize] {
                match net[..n].iter_mut().find(|x| x.0 == k) {
                    Some(x) => x.2 = new,
                    None => {
                        net[n] = (k, old, new);
                        n += 1;
                    }
                }
            }
        }
        net[..n].iter().all(|x| x.1 == x.2)
    }

    fn in_use(&self, i: usize) -> bool {
        #[cfg(target_os = "macos")]
        if self.gpu.as_ref().is_some_and(|g| g.is_in_use(i)) {
            return true;
        }
        self.target.as_ref().is_some_and(|t| t.in_use(i))
    }

    fn pick(&mut self, epoch: u32, paint_only: bool) -> Pick {
        let n = self.held.len();
        if !self.windowed {
            let i = self.next_rr % n;
            self.next_rr = (i + 1) % n;
            return Pick::Draw(i);
        }
        if paint_only {
            if let Some(s) = self.shown {
                if self.holds_current(s, epoch) {
                    return Pick::OnScreen;
                }
            }
        }
        let t0 = std::time::Instant::now();
        let mut waited = false;
        loop {
            // free drawn surfaces, most recently drawn first (the smallest repair)
            let mut best: Option<(usize, u32)> = None;
            let mut fresh: Option<usize> = None;
            for c in 0..n {
                // the surface on screen is never drawn while the compositor may read it; a CPU target's surface the
                // compositor released (it copied it) is the smallest repair of all (age 1)
                if Some(c) == self.shown && (self.target.is_none() || self.in_use(c)) {
                    continue;
                }
                match self.held[c] {
                    Some(e) => {
                        if self.in_use(c) {
                            continue;
                        }
                        if paint_only && self.holds_current(c, epoch) {
                            return Pick::Held(c);
                        }
                        if best.is_none_or(|(_, be)| e > be) {
                            best = Some((c, e));
                        }
                    }
                    None => {
                        if fresh.is_none() && !self.in_use(c) {
                            fresh = Some(c);
                        }
                    }
                }
            }
            if let Some((c, _)) = best {
                return Pick::Draw(c);
            }
            if let Some(c) = fresh {
                if self.held.iter().any(|h| h.is_some()) {
                    self.surf_stats.grew += 1;
                    #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
                    if let Some(g) = self.gpu.as_ref() {
                        for d in (0..n).filter(|&d| Some(d) != self.shown && self.held[d].is_some()) {
                            let w = g.in_use_why(d);
                            for k in 0..5 {
                                self.surf_stats.grew_why[k] += (w >> k & 1) as u64;
                            }
                        }
                    }
                }
                return Pick::Draw(c);
            }
            // all four held by the compositor: wait for a release (never skip)
            if !waited {
                waited = true;
                self.surf_stats.waits += 1;
            }
            std::thread::sleep(std::time::Duration::from_micros(150));
            let w = t0.elapsed().as_micros() as u64;
            self.surf_stats.max_wait_us = self.surf_stats.max_wait_us.max(w);
        }
    }

    /// Does the current epoch have damage (would `frame` draw)? A host that paces frames (Wayland frame
    /// callbacks) asks this to decide whether it needs to draw when the compositor is ready.
    pub fn dirty(&self) -> bool {
        !self.rt.bough.plane(0).is_clear() || !self.rt.paint.plane(0).is_clear()
    }

    /// Damage -> surface -> tile list -> GPU repair -> present, for an input-driven turn: the frame reaches
    /// the screen in the same refresh (the publisher thread commits it once the GPU is done). Returns false
    /// (and does nothing at all) if the epoch has no damage: idle is zero work.
    pub fn frame(&mut self) -> bool {
        self.frame_as::<false>()
    }

    /// `frame` for a turn a clock drives (a timer, the display link: animations, streams): no input waits
    /// on it, so it is published at the next turn on the calling thread for free instead of by the
    /// publisher thread (latency vs throughput: +1 refresh, no thread wake, no completion wait).
    pub fn frame_paced(&mut self) -> bool {
        self.frame_as::<true>()
    }

    /// The publication policy is chosen by the turn's source at compile time (`PACED`), never per frame.
    pub fn frame_as<const PACED: bool>(&mut self) -> bool {
        let core = !self.rt.bough.plane(0).is_clear();
        let paint = !self.rt.paint.plane(0).is_clear();
        if !core && !paint {
            // an idle frame returns released subtrees' resources (`Rt::reclaim`), a budget at a time
            if self.reclaiming() {
                self.rt.reclaim(RECLAIM_BUDGET);
            }
            return false;
        }
        self.prof.reset();
        let epoch = self.rt.bough.epoch();
        self.core_epochs[epoch as usize % RING] = core;
        let pick = self.pick(epoch, !core);
        self.surf_stats.frames += 1;
        match pick {
            Pick::OnScreen => {
                self.surf_stats.on_screen += 1;
                let s = self.shown.unwrap();
                self.held[s] = Some(epoch);
            }
            Pick::Held(i) => {
                self.surf_stats.held += 1;
                #[cfg(target_os = "macos")]
                if let Some(g) = self.gpu.as_mut() {
                    g.show_held::<PACED>(i);
                }
                if self.target.is_some() {
                    let full = self.damage_since_shown(epoch);
                    let t = self.target.as_mut().unwrap();
                    t.present(i, &self.shown_rows, full);
                }
                self.held[i] = Some(epoch);
                self.shown = Some(i);
                self.last_surface = i;
            }
            Pick::Draw(i) => self.draw::<PACED>(i, epoch),
        }
        self.prof.mark(PH_PRESENT);
        self.rt.bough.next_epoch();
        self.rt.paint.advance();
        self.rt.paint_log[self.rt.paint.epoch as usize % RING].clear();
        self.frames += 1;
        true
    }

    #[cfg_attr(not(target_os = "macos"), allow(unused_variables, unused_mut))]
    fn draw<const PACED: bool>(&mut self, i: usize, epoch: u32) {
        let mut purged = false;
        // a CPU-repairable frame must not make the surface resident (a first residency drains the pre-armed
        // frames: GPU work); its volatility is handled by `write_begin` / `prepare_target`, whichever runs
        #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
        if self.repair == Repair::Auto && !crate::repair::cpu_exact(self.rt.text.font_bytes()) {
            // this device, OS build or shader draws differently from the CPU model: the GPU repairs everything
            self.repair = Repair::Gpu;
        }
        #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
        let cpu_able = self.repair != Repair::Gpu;
        #[cfg(not(all(target_os = "macos", not(feature = "wgpu"))))]
        let cpu_able = false;
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut() {
            if !cpu_able {
                purged = g.prepare_target(i);
            }
        }
        #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
        if cpu_able {
            if let Some(g) = self.gpu.as_mut() {
                purged = g.make_nonvolatile(i);
            }
        }
        if purged {
            self.held[i] = None;
            self.surf_stats.purged += 1;
        }
        let age = self.held[i].map(|l| epoch - l);
        let full = !matches!(age, Some(a) if a as usize <= RING);
        let mut copy = bough_frame::SlotCopy::NONE;
        self.last_full = full;
        if !full {
            self.repair_rows(age.unwrap());
            copy = self.copy_plan(i, epoch, age.unwrap());
        }
        self.prof.mark(PH_DAMAGE);
        let n_dir = self.rt.bough.num_pages() as u32;
        self.surf_stats.uses[i] += 1;
        self.surf_stats.gpu += 1;
        self.last_use[i] = self.frames;
        #[allow(unused_mut)]
        let mut drawn = false;
        if self.target.is_some() {
            self.draw_target(i, epoch, full, &copy);
            drawn = true;
        }
        // the CPU backend (cpu_raster.rs): the damaged tiles written into the surface directly, no GPU work
        #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
        if cpu_able && copy.is_none() {
            let tiles = if full { self.rt.layout.tiles_x() * self.rt.layout.tiles_y() } else { self.rows.iter().map(|r| r.count_ones()).sum() };
            let warm = self.gpu.as_ref().is_some_and(|g| g.gpu_used());
            let limit = self.repair_limit(warm);
            if let Some(g) = self.gpu.as_mut() {
                if self.repair == Repair::Cpu || tiles <= limit {
                    let (p, bpr, _) = g.write_begin(i);
                    // SAFETY: the surface is locked for this thread's writes, `bpr` bytes per row for the window's
                    // rows (`write_begin`); the GPU finished every frame.
                    let r = unsafe { self.cpu.repair(&self.rt, &self.rows, full, p, bpr) };
                    g.write_end(i);
                    if let Some(nt) = r {
                        self.repaired_tiles = nt;
                        g.show_held::<PACED>(i);
                        self.surf_stats.cpu += 1;
                        drawn = true;
                    }
                }
            }
        }
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut().filter(|_| !drawn) {
            if cpu_able {
                g.prepare_target(i);
            }
            self.prof.mark(PH_CULL);
            let a = &mut self.rt.text.cache.atlas;
            if g.sync_atlas(a.width, a.height, &a.pixels, a.dirty_bytes != 0) {
                a.dirty_bytes = 0;
            }
            if let Some(rows) = self.rt.images.take_dirty() {
                g.sync_images(self.rt.images.pixels(), rows);
            }
            // the GPU's views of the region's columns cover what the CPU wrote (seven compares; a growth is cold)
            #[cfg(not(feature = "wgpu"))]
            g.ensure_columns(&self.rt.column_needs());
            self.repaired_tiles = g.draw::<PACED>(i, &self.rows, full, n_dir, &copy);
            drawn = true;
        }
        if !drawn {
            // headless: the CPU side of the frame ends at the tile list in the region's frame slot
            // SAFETY: the runtime's region, laid out by `rt.layout`; headless, so no GPU reads it.
            let (nt, _) = unsafe { bough_frame::write_slot(self.rt.region_base(), &self.rt.layout, (self.frames % 2) as usize, i as u32, &self.rows, full, &copy) };
            self.repaired_tiles = nt;
            self.prof.mark(PH_CULL);
        }
        self.held[i] = Some(epoch);
        self.shown = Some(i);
        self.last_surface = i;
    }

    /// The tiles where the current state differs from the surface on screen, into `shown_rows` (the damage a
    /// presented surface carries: a host's compositor repaints only it). `true`: everything (nothing shown yet,
    /// or the surface on screen is older than the damage rings).
    fn damage_since_shown(&mut self, epoch: u32) -> bool {
        let Some(age) = self.shown.and_then(|s| self.held[s]).map(|e| epoch - e) else { return true };
        if age as usize > RING || age == 0 {
            return age != 0;
        }
        self.rt.bough.repair_into(age, &mut self.pa);
        self.rt.paint.repair_into(age, &mut self.pb);
        self.shown_rows.fill(0);
        let n = self.shown_rows.len();
        for p in [&self.pa, &self.pb] {
            let (a0, a1) = p.occ_range();
            for y in a0..a1.min(n) {
                self.shown_rows[y] |= p.rows[y];
            }
        }
        false
    }

    /// A frame through the CPU executor into the target's surface `i`: the scroll copy from the surface on screen,
    /// the repair of the damaged tiles, then `present` with the damage against what is on screen.
    fn draw_target(&mut self, i: usize, epoch: u32, full: bool, copy: &bough_frame::SlotCopy) {
        self.prof.mark(PH_CULL);
        let t = self.target.as_mut().expect("draw_target without a target");
        if !full && !copy.is_none() {
            let (src, sbpr) = t.pixels(copy.src as usize);
            let (dst, dbpr) = t.pixels(i);
            // SAFETY: the copy plan names the surface on screen as the source (the target itself when it repairs that
            // one in place: same pointer and stride); both are the target's surfaces of the window's size, and the
            // plan's rect minus the move lies in the window.
            unsafe { crate::backend::surfaces::copy_rect(src, sbpr, dst, dbpr, copy) };
        }
        let (p, bpr) = t.pixels(i);
        // SAFETY: the target's contract: `p` holds the window's rows at `bpr`, and nothing else touches surface
        // `i` while it is not in use (`pick` chose it so).
        let r = unsafe { self.cpu.repair(&self.rt, &self.rows, full, p, bpr) };
        self.repaired_tiles = r.unwrap_or_else(|| panic!("the CPU executor refused a frame (a record kind or transform it does not model: {:?})", self.cpu.stats));
        self.surf_stats.cpu += 1;
        let all = self.damage_since_shown(epoch);
        if !copy.is_none() && !full {
            let r = copy.rect;
            let (tx0, ty0, tx1, ty1) = ((r[0] >> 5) as u32, (r[1] >> 5) as u32, ((r[2] + 31) >> 5) as u32, ((r[3] + 31) >> 5) as u32);
            let m = if tx1 - tx0 >= 128 { u128::MAX } else { ((1u128 << (tx1 - tx0)) - 1) << tx0 };
            let n = self.shown_rows.len();
            for row in &mut self.shown_rows[(ty0 as usize).min(n)..(ty1 as usize).min(n)] {
                *row |= m;
            }
        }
        let t = self.target.as_mut().unwrap();
        t.present(i, &self.shown_rows, full || all);
    }

    /// Release what a calm window does not need: every drawn surface except the one on screen becomes
    /// purgeable-volatile (its pages leave the footprint; a purged surface is repainted whole when next
    /// used). Returns whether anything was trimmed.
    pub fn trim(&mut self, active: bool) -> bool {
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut() {
            if !active {
                g.sync();
            }
            let quiet = if active { 8 } else { 0 };
            let (frames, last_use, shown) = (self.frames, &self.last_use, self.shown);
            let n = g.trim_where(|i| Some(i) != shown && frames - last_use[i] >= quiet);
            if n > 0 {
                self.surf_stats.trims += 1;
            }
            self.trim_mark = self.frames;
            return n > 0;
        }
        let _ = active;
        false
    }

    /// True if some drawn surface other than the one on screen is not yet volatile.
    pub fn needs_trim(&self) -> bool {
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_ref() {
            return (0..self.held.len()).any(|i| Some(i) != self.shown && (g.surfs[i].touched.get() || g.surfs[i].cpu_written.get()) && !g.surfs[i].volatile.get());
        }
        false
    }

    /// The App's own tables (re-description tape, tile-row buffers), as memory-ledger lines (`ledger.rs`).
    pub(crate) fn ledger_app(&self, l: &mut bough_mem::Ledger) {
        use bough_mem::Scales;
        l.vec("app", "re-description tape", &self.tape, self.tape.capacity(), Scales::Content);
        let rows = self.rows.capacity() * 16 + 2 * size_of::<Plane>() + self.held.capacity() * 8 + self.last_use.capacity() * 8;
        l.many("app", "tile-row buffer and repair planes", Scales::Window, rows, rows);
    }

    /// Surfaces in the ring.
    pub fn surfaces(&self) -> usize {
        self.held.len()
    }

    /// Are released subtrees' resources still waiting to return to the free lists (`Rt::reclaim`)?
    pub fn reclaiming(&self) -> bool {
        !self.rt.reclaim_q.is_empty() || !self.rt.lay.eng.orphans.is_empty()
    }

    /// The tile rows the last repair covered (tests and diagnostics).
    pub fn debug_rows(&self) -> &[u128] {
        &self.rows
    }

    /// Frames presented so far.
    pub fn frames_presented(&self) -> u64 {
        self.frames
    }

    /// Pixels (BGRA) of the surface the last frame showed.
    pub fn presented_pixels(&mut self) -> Vec<u8> {
        self.sync();
        let i = self.last_surface;
        if let Some(t) = self.target.as_mut() {
            let (w, h) = t.size();
            let (p, bpr) = t.pixels(i);
            let mut v = vec![0u8; w as usize * 4 * h as usize];
            for y in 0..h as usize {
                // SAFETY: the target's contract: `h` rows of `bpr >= 4 w` bytes at `p`.
                let row = unsafe { std::slice::from_raw_parts(p.add(y * bpr), w as usize * 4) };
                v[y * w as usize * 4..(y + 1) * w as usize * 4].copy_from_slice(row);
            }
            return v;
        }
        #[cfg(target_os = "macos")]
        return self.gpu.as_mut().unwrap().surface_pixels(i);
        #[cfg(not(target_os = "macos"))]
        panic!("presented_pixels: no surfaces (headless App)")
    }

    /// A from-scratch full redraw of the current retained state (every tile, the same kernels).
    pub fn reference_pixels(&mut self) -> Vec<u8> {
        if self.target.is_some() {
            let (w, h) = (self.rt.win_w as usize, self.rt.win_h as usize);
            let mut v = vec![0u8; w * 4 * h];
            // SAFETY: `v` holds `h` rows of `4 w` bytes, the window's size.
            unsafe { self.cpu.repair(&self.rt, &[], true, v.as_mut_ptr(), w * 4) }.expect("the CPU executor refused the reference frame");
            return v;
        }
        #[cfg(target_os = "macos")]
        return self.reference_pixels_gpu();
        #[cfg(not(target_os = "macos"))]
        panic!("reference_pixels: no surfaces (headless App)")
    }

    #[cfg(target_os = "macos")]
    fn reference_pixels_gpu(&mut self) -> Vec<u8> {
        let n_dir = self.rt.bough.num_pages() as u32;
        let g = self.gpu.as_mut().unwrap();
        let a = &mut self.rt.text.cache.atlas;
        if g.sync_atlas(a.width, a.height, &a.pixels, a.dirty_bytes != 0) {
            a.dirty_bytes = 0;
        }
        if let Some(rows) = self.rt.images.take_dirty() {
            g.sync_images(self.rt.images.pixels(), rows);
        }
        #[cfg(not(feature = "wgpu"))]
        g.ensure_columns(&self.rt.column_needs());
        g.reference_pixels(n_dir)
    }

    /// Did a GPU row bin overflow in the last frame? (tests; the CPU executor has no bins)
    pub fn bins_overflowed(&mut self) -> bool {
        self.sync();
        #[cfg(target_os = "macos")]
        return self.gpu.as_ref().is_some_and(|g| g.bins_overflowed());
        #[cfg(not(target_os = "macos"))]
        false
    }

    /// Shrink the GPU row bins to `cap` entries per row (tests: forces the grow-and-redraw path; nothing without
    /// a GPU presenter).
    pub fn reset_bins(&mut self, cap: u32) {
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut() {
            g.sync();
            g.reset_bins(cap);
        }
        let _ = cap;
    }
}
