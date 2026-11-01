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

/// A view: a plain struct whose `Signal` fields are the state. `view` describes it once at mount.
pub trait View: Sized + 'static {
    /// `true`: `view` runs once at mount and only wires, lists and readers change the subtree afterwards;
    /// `false`: every handler (`App::turn`) re-describes the owner and patches what differs (`describe.rs`).
    const FIXED: bool = false;
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
}

impl Config {
    pub fn new(width_pt: f32, height_pt: f32, scale: f32, font: Vec<u8>) -> Config {
        Config { width_pt, height_pt, scale, font, ring: 0, caps: Caps::DEFAULT }
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
        self.ovh = bough_metal::perf::snap_overhead();
    }
    #[inline]
    pub fn reset(&mut self) {
        if self.on {
            self.last = bough_metal::perf::snap().instrs;
        }
    }
    #[inline]
    pub fn mark(&mut self, i: usize) {
        if self.on {
            let s = bough_metal::perf::snap().instrs;
            self.acc[i] += (s - self.last).saturating_sub(self.ovh);
            self.last = bough_metal::perf::snap().instrs;
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
    pub gpu: Option<bough_metal::fused::Fused>,
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
    trim_mark: u64,
    /// Throughput mode: the surface of the last released frame, published at the next sync.
    pending: Option<usize>,
    /// The root's re-description tape (`describe.rs`; empty for a `FIXED` view).
    tape: Vec<u32>,
}

impl<V: View> App<V> {
    /// Mount `view`. `gpu`: create the fused presenter (its shared buffer is the region); otherwise the
    /// region is heap memory and `frame` stops after the tile list (the RENDER=0 shape).
    pub fn new(view: V, cfg: Config, gpu: bool) -> App<V> {
        let (win_w, win_h) = ((cfg.width_pt * cfg.scale).round() as i32, (cfg.height_pt * cfg.scale).round() as i32);
        let ring = if cfg.ring == 0 { 4 } else { cfg.ring.min(4) };
        let bytes = cfg.caps.region_bytes(win_w as u32, win_h as u32);
        #[cfg(target_os = "macos")]
        let metal = gpu.then(|| bough_metal::fused::Fused::new(win_w as usize, win_h as usize, cfg.scale as f64, bytes, ring, 1024));
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
            None => bough_mem::Region::heap(bytes),
        };
        #[cfg(not(target_os = "macos"))]
        let region = bough_mem::Region::heap(bytes);
        let mut rt = Rt::new(win_w, win_h, cfg.scale, &cfg.font, region, cfg.caps);
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
        let ptr = &*owner as *const V;
        {
            let el = owner.view();
            let tp: *mut Vec<u32> = if V::FIXED { std::ptr::null_mut() } else { &mut tape };
            let mut m = Mounter { rt: &mut rt, owner: ptr, recs: &mut recs, scope: &mut scope, depth: 0, ox: 0.0, oy: 0.0, space: 0, clip: 0, pending_num: None, cls: 0, tape: tp, ti: 0, pos: 0, kid: 0, keep: false };
            el.mount(&mut m);
        }
        rt.bough.insert_run(0, &recs);
        // layout groups of the first mount, then the measured lists' first rows
        rt.lay_flush();
        // SAFETY: `ptr` is the owner just mounted (boxed: it never moves).
        unsafe { rt.mregions_init(ptr as *const u8) };
        let mut owner = owner;
        // wires queued while mounting (a producer's first request flips its reader to pending)
        // SAFETY: the owner just mounted, uniquely borrowed here.
        unsafe { rt.settle(&mut *owner as *mut V as *const u8) };
        rt.lay_flush();
        drop(scope); // the root scope lives as long as the app: nothing of it is ever released
        let ty = rt.tiles_y;
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
            pending: None,
            tape,
        }
    }

    /// Throughput mode for animation that no input drives (RESULTS 12.8): the GPU's command stream is
    /// pre-encoded `k` frames ahead, each waiting on a shared event; a frame writes its slot and signals,
    /// and its surface is published at the next turn (one refresh later). No command buffer, encoder or
    /// completion wait per frame.
    #[cfg(target_os = "macos")]
    pub fn stream(&mut self, k: usize) {
        if let Some(g) = self.gpu.as_mut() {
            g.sync();
            g.arm_stream(k);
        }
    }

    #[cfg(target_os = "macos")]
    fn sync(&mut self) {
        if let Some(g) = self.gpu.as_mut() {
            g.sync();
            if let Some(i) = self.pending.take() {
                g.publish_later(i);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    fn sync(&mut self) {}

    /// One turn: run the handler, settle the wires it queued (text patches and marks happen inside the
    /// wires), lay out what changed.
    pub fn turn(&mut self, f: impl FnOnce(&mut V)) {
        self.sync();
        self.rt.reclaim(RECLAIM_BUDGET);
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
    fn effects(&mut self) {
        for _ in 0..8 {
            if self.rt.pubs.is_empty() && self.rt.asy.committed.is_empty() {
                return;
            }
            let d = crate::changed::Changed { pubs: &self.rt.pubs, readers: &self.rt.asy.committed, _v: std::marker::PhantomData };
            self.owner.changed(d);
            self.rt.pubs.clear();
            self.rt.asy.committed.clear();
            let owner = &mut *self.owner as *mut V as *const u8;
            // SAFETY: as in `turn`.
            unsafe { self.rt.settle(owner) };
        }
        panic!("View::changed did not settle in 8 waves (a cycle between changed and its own boundaries)");
    }

    /// A `FIXED = false` view: describe again and patch the retained records (`describe.rs`).
    fn redescribe(&mut self) {
        if !V::FIXED {
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

    #[cfg(target_os = "macos")]
    fn in_use(&self, i: usize) -> bool {
        self.gpu.as_ref().is_some_and(|g| g.is_in_use(i))
    }
    #[cfg(not(target_os = "macos"))]
    fn in_use(&self, _: usize) -> bool {
        false
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
                if Some(c) == self.shown {
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

    /// Damage -> surface -> tile list -> GPU repair -> present. Returns false (and does nothing at all) if
    /// the epoch has no damage: idle is zero work.
    pub fn frame(&mut self) -> bool {
        let core = !self.rt.bough.plane(0).is_clear();
        let paint = !self.rt.paint.plane(0).is_clear();
        if !core && !paint {
            // an idle frame returns released subtrees' resources (`Rt::reclaim`), a budget at a time
            self.rt.reclaim(RECLAIM_BUDGET);
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
                    g.show_held(i);
                }
                self.held[i] = Some(epoch);
                self.shown = Some(i);
                self.last_surface = i;
            }
            Pick::Draw(i) => self.draw(i, epoch),
        }
        self.prof.mark(PH_PRESENT);
        self.rt.bough.next_epoch();
        self.rt.paint.advance();
        self.rt.paint_log[self.rt.paint.epoch as usize % RING].clear();
        self.frames += 1;
        true
    }

    fn draw(&mut self, i: usize, epoch: u32) {
        let mut purged = false;
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut() {
            purged = g.prepare_target(i);
        }
        if purged {
            self.held[i] = None;
            self.surf_stats.purged += 1;
        }
        let age = self.held[i].map(|l| epoch - l);
        let full = !matches!(age, Some(a) if a as usize <= RING);
        if !full {
            self.repair_rows(age.unwrap());
        }
        self.prof.mark(PH_DAMAGE);
        let n_dir = self.rt.bough.num_pages() as u32;
        self.surf_stats.uses[i] += 1;
        self.surf_stats.gpu += 1;
        self.last_use[i] = self.frames;
        #[allow(unused_mut)]
        let mut drawn = false;
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut() {
            self.prof.mark(PH_CULL);
            let a = &mut self.rt.text.cache.atlas;
            if g.sync_atlas(a.width, a.height, &a.pixels, a.dirty_bytes != 0) {
                a.dirty_bytes = 0;
            }
            self.repaired_tiles = g.draw(i, &self.rows, full, n_dir);
            if g.streaming() {
                self.pending = Some(i);
            }
            drawn = true;
        }
        if !drawn {
            // headless: the CPU side of the frame ends at the tile list in the region's frame slot
            // SAFETY: the runtime's region, laid out by `rt.layout`; headless, so no GPU reads it.
            let (nt, _) = unsafe { bough_metal::fused::write_slot(self.rt.region_base(), &self.rt.layout, (self.frames % 2) as usize, i as u32, &self.rows, full) };
            self.repaired_tiles = nt;
            self.prof.mark(PH_CULL);
        }
        self.held[i] = Some(epoch);
        self.shown = Some(i);
        self.last_surface = i;
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
            return (0..self.held.len()).any(|i| Some(i) != self.shown && g.surfs[i].touched.get() && !g.surfs[i].volatile.get());
        }
        false
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
    #[cfg(target_os = "macos")]
    pub fn presented_pixels(&mut self) -> Vec<u8> {
        self.sync();
        let i = self.last_surface;
        self.gpu.as_mut().unwrap().surface_pixels(i)
    }

    /// A from-scratch full redraw of the current retained state (every tile, the same kernels).
    #[cfg(target_os = "macos")]
    pub fn reference_pixels(&mut self) -> Vec<u8> {
        let n_dir = self.rt.bough.num_pages() as u32;
        let g = self.gpu.as_mut().unwrap();
        let a = &mut self.rt.text.cache.atlas;
        if g.sync_atlas(a.width, a.height, &a.pixels, a.dirty_bytes != 0) {
            a.dirty_bytes = 0;
        }
        g.reference_pixels(n_dir)
    }

    /// Did a GPU row bin overflow in the last frame? (tests)
    #[cfg(target_os = "macos")]
    pub fn bins_overflowed(&mut self) -> bool {
        self.sync();
        self.gpu.as_ref().is_some_and(|g| g.bins_overflowed())
    }

    /// Shrink the GPU row bins to `cap` entries per row (tests: forces the grow-and-redraw path).
    #[cfg(target_os = "macos")]
    pub fn reset_bins(&mut self, cap: u32) {
        if let Some(g) = self.gpu.as_mut() {
            g.sync();
            g.reset_bins(cap);
        }
    }
}
