//! Second-generation presenter (Opus iteration). One struct, independently switchable mechanisms, so every
//! attempt in RESULTS.md section 12+ is the same code with one representation changed:
//!
//! * raster  `rp`   render pass + stencil (the prototype's path, for A/B)
//!           `cs`   compute tile repair: one threadgroup per damaged 16x16 tile writes the IOSurface texture
//!           `armK` the same kernel, but the command stream is PRE-ENCODED K frames ahead and each frame is
//!                  released by one `MTLSharedEvent.signaledValue = 2n-1` store (tile list + target live in the pool)
//! * publish `h`    CATransaction on the Metal completion thread (prototype b-h)
//!           `hm`   completion thread hops to the main queue, which commits
//!           `nt`   main thread commits at the next display-link tick (no thread hop, +1 frame of latency)
//! * pick    `rr`   round robin (prototype)
//!           `free` first surface that is neither the one on screen nor `IOSurfaceIsInUse`
//!           `z`    content-addressed: per-tile Zobrist hashes say which free surface already holds the most of
//!                  the current pixels; if one holds all of them, the GPU is skipped (and if the surface on screen
//!                  does, everything is skipped)
//! * n       ring size (3 or 4)
//!
//! Spec string: `raster:publish:pick:n`, e.g. `cs:nt:z:4`.
use crate::damage::{Plane, TX, TY};
use crate::gpu::*;
use crate::instance::*;
use crate::perf::tinstr;
use crate::present::{make_iosurface, Engine, Presenter};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_core_foundation::CFRetained;
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::*;
use objc2_quartz_core::{CACurrentMediaTime, CALayer, CATransaction};
use std::collections::VecDeque;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raster {
    Render,
    /// vertex-clipped instance stream (format C expanded by the vertex shader from the damage plane)
    VClip,
    Compute,
    Armed(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pub {
    Handler,
    HandlerMain,
    NextTick,
    /// main-queue one-shot ~0.3 ms after submit polls completion and commits on the main thread (re-arms itself
    /// every 0.25 ms until done): no completion handler / listener per frame, low latency
    TimerPoll,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    RoundRobin,
    Free,
    Content,
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub raster: Raster,
    pub publish: Pub,
    pub pick: Pick,
    pub n: usize,
}

impl Spec {
    pub fn parse(s: &str) -> Option<Spec> {
        let p: Vec<&str> = s.split(':').collect();
        if p.len() != 4 {
            return None;
        }
        let raster = match p[0] {
            "rp" => Raster::Render,
            "vc" => Raster::VClip,
            "cs" => Raster::Compute,
            a if a.starts_with("arm") => Raster::Armed(a[3..].parse().ok()?),
            _ => return None,
        };
        let publish = match p[1] {
            "h" => Pub::Handler,
            "hm" => Pub::HandlerMain,
            "nt" => Pub::NextTick,
            "tp" => Pub::TimerPoll,
            _ => return None,
        };
        let pick = match p[2] {
            "rr" => Pick::RoundRobin,
            "free" => Pick::Free,
            "z" => Pick::Content,
            _ => return None,
        };
        let n: usize = p[3].parse().ok()?;
        if !(2..=4).contains(&n) {
            return None;
        }
        Some(Spec { raster, publish, pick, n })
    }
}

/// Counters shared with completion/listener threads.
#[derive(Default)]
pub struct Shared {
    pub pub_instr: AtomicU64,
    pub pub_n: AtomicU64,
    /// CACurrentMediaTime (f64 bits) right after the most recent publish commit, and its frame number
    pub pub_time: AtomicU64,
    pub pub_frame: AtomicU64,
    /// publishes of a surface the compositor already held (must stay 0)
    pub in_use_at_publish: AtomicU64,
    /// CACurrentMediaTime (f64 bits) at present() of GPU frame f, slot f % 8
    pub produced_at: [AtomicU64; 8],
    /// present() -> publish-commit latencies, ms
    pub lat_ms: std::sync::Mutex<Vec<f64>>,
}

pub struct Surf {
    pub io: CFRetained<IOSurfaceRef>,
    pub tex: Tex,
    /// epoch whose scene state this surface holds
    pub last: Option<u64>,
    /// Zobrist tile hashes of what this surface holds (pick = z)
    pub zh: Vec<u64>,
    pub salt: u64,
}

struct SendPtr<T>(T);
unsafe impl<T> Send for SendPtr<T> {}

fn publish_txn(layer: &CALayer, io: &IOSurfaceRef, sh: &Shared, frame: u64) {
    let t0 = tinstr();
    if io.is_in_use() {
        sh.in_use_at_publish.fetch_add(1, Relaxed);
    }
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    unsafe { layer.setContents(Some(&*(io as *const IOSurfaceRef as *const AnyObject))) };
    CATransaction::commit();
    let now = unsafe { CACurrentMediaTime() };
    let t0p = f64::from_bits(sh.produced_at[(frame % 8) as usize].swap(0, Relaxed));
    if t0p > 0.0 {
        sh.lat_ms.lock().unwrap().push((now - t0p) * 1e3);
    }
    sh.pub_time.store(now.to_bits(), Relaxed);
    sh.pub_frame.store(frame, Relaxed);
    sh.pub_instr.fetch_add(tinstr() - t0, Relaxed);
    sh.pub_n.fetch_add(1, Relaxed);
}

pub struct V2 {
    pub spec: Spec,
    pub layer: Retained<CALayer>,
    pub surfs: Vec<Surf>,
    next: usize,
    /// surface most recently handed to the compositor (or about to be): never a draw target
    shown: Option<usize>,
    last_cmd: Option<Cmd>,
    /// NextTick: (surface, gpu frame number) drawn but not yet published
    pending: Option<(usize, u64)>,
    /// a frame was deferred because no surface was free: re-present on the next poll
    need_frame: bool,
    // armed command stream
    ev: Option<Retained<ProtocolObject<dyn MTLSharedEvent>>>,
    listener: Option<Retained<MTLSharedEventListener>>,
    armed_upto: u64,
    batches: VecDeque<Cmd>,
    /// GPU frames issued (1-based); frame n is released by value 2n-1 and signals 2n when done
    pub gpu_frames: u64,
    pub sh: Arc<Shared>,
    // proof / diagnostics
    pub drawn_in_use: u64,
    pub deferred: u64,
    pub skipped_gpu: u64,
    pub skipped_all: u64,
    pub free_hist: [u64; 5],
    pub tiles_last: usize,
    pub tiles_total: u64,
    pub last_idx: usize,
    pub batch_errors: u64,
    /// content pick: evaluations that had to hash-compare the whole plane (history too old / unknown)
    pub full_filters: std::cell::Cell<u64>,
    pub evals: std::cell::Cell<u64>,
    tiles: Vec<u32>,
    cand: Box<Plane>,
    best: Box<Plane>,
}

impl V2 {
    pub fn new(e: &mut Engine, spec: Spec) -> V2 {
        let layer = CALayer::new();
        layer.setContentsScale(2.0);
        layer.setOpaque(true);
        layer.setFrame(objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(W as f64 / 2.0, H as f64 / 2.0),
        ));
        let surfs = (0..spec.n)
            .map(|_| {
                let io = make_iosurface();
                let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, W as usize, H as usize, false) };
                d.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
                d.setStorageMode(MTLStorageMode::Shared);
                let tex = e.gpu.device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0).expect("iosurface texture");
                Surf { io, tex, last: None, zh: vec![0; if spec.pick == Pick::Content { TX * TY } else { 0 }], salt: 0 }
            })
            .collect();
        if spec.pick == Pick::Content && e.scene.zob.is_empty() {
            e.scene.enable_zobrist();
        }
        let mut v = V2 {
            spec,
            layer,
            surfs,
            next: 0,
            shown: None,
            last_cmd: None,
            pending: None,
            need_frame: false,
            ev: None,
            listener: None,
            armed_upto: 0,
            batches: VecDeque::new(),
            gpu_frames: 0,
            sh: Arc::new(Shared::default()),
            drawn_in_use: 0,
            deferred: 0,
            skipped_gpu: 0,
            skipped_all: 0,
            free_hist: [0; 5],
            tiles_last: 0,
            tiles_total: 0,
            last_idx: 0,
            batch_errors: 0,
            full_filters: std::cell::Cell::new(0),
            evals: std::cell::Cell::new(0),
            tiles: Vec::with_capacity(TX * TY),
            cand: Box::new(Plane::new()),
            best: Box::new(Plane::new()),
        };
        if let Raster::Armed(k) = spec.raster {
            v.ev = Some(e.gpu.device.newSharedEvent().expect("shared event"));
            if spec.publish != Pub::NextTick {
                v.listener = Some(MTLSharedEventListener::new());
            }
            v.arm(&e.gpu, k as u64);
        }
        v
    }

    /// Pre-encode GPU frames armed_upto+1 ..= armed_upto+k into ONE command buffer and commit it. Each frame:
    /// wait(ev >= 2f-1); indirect dispatch from slot f%2; signal(ev = 2f).
    fn arm(&mut self, g: &Gpu, k: u64) {
        let ev = self.ev.as_ref().unwrap();
        let cmd = g.queue.commandBuffer().unwrap();
        let t: Vec<&Tex> = self.surfs.iter().map(|s| &s.tex).collect();
        let targets = [t[0], t[1 % t.len()], t[2 % t.len()], t[3 % t.len()]];
        for f in self.armed_upto + 1..=self.armed_upto + k {
            cmd.encodeWaitForEvent_value(ProtocolObject::from_ref(&**ev), 2 * f - 1);
            g.encode_compute(&cmd, FRAME_OFF + (f % 2) as usize * SLOT_BYTES, targets, 0, true);
            cmd.encodeSignalEvent_value(ProtocolObject::from_ref(&**ev), 2 * f);
        }
        cmd.commit();
        self.armed_upto += k;
        self.batches.push_back(cmd);
        while self.batches.len() > 3 {
            let b = self.batches.pop_front().unwrap();
            if b.status() == MTLCommandBufferStatus::Error {
                self.batch_errors += 1;
            }
        }
    }

    fn gpu_done(&self) -> bool {
        match self.spec.raster {
            Raster::Armed(_) => self.ev.as_ref().unwrap().signaledValue() >= 2 * self.gpu_frames,
            _ => self.last_cmd.as_ref().map(|c| c.status() == MTLCommandBufferStatus::Completed).unwrap_or(true),
        }
    }

    /// Repair plane for surface `s` at the current scene state; returns the tile count. Content pick filters the
    /// history's candidates by Zobrist hash equality.
    fn repair_plane(&self, e: &Engine, s: usize, out: &mut Plane) -> usize {
        let sf = &self.surfs[s];
        let since = if e.sabotage { sf.last.map(|x| (x + 1).min(e.hist.epoch)) } else { sf.last };
        let z_salt = self.spec.pick == Pick::Content;
        // Hashes decide only "does a surface already hold ALL of it" (zero_diff); the target's repair set is the
        // plain history union (no per-tile filter: that cost ~35 instr per damaged tile and taxed scroll/scatter).
        let z = false;
        if z_salt && sf.salt != e.scene.salt {
            out.clear();
            out.add_full();
            return TX * TY;
        }
        let ok = e.hist.repair(since, out);
        self.evals.set(self.evals.get() + 1);
        if !ok || out.full {
            out.clear();
            out.add_full();
            if !z {
                return TX * TY;
            }
            self.full_filters.set(self.full_filters.get() + 1);
            out.full = false; // filter below
        }
        if !out.any {
            return 0;
        }
        let mut n = 0usize;
        let (r0, r1) = (out.r0 as usize, out.r1 as usize);
        for ty in r0..=r1 {
            for w in 0..2 {
                let mut b = out.rows[ty][w];
                if z {
                    let mut keep = 0u64;
                    while b != 0 {
                        let bit = b.trailing_zeros() as usize;
                        b &= b - 1;
                        let tx = w * 64 + bit;
                        if tx < TX && e.scene.zob[ty * TX + tx] != sf.zh[ty * TX + tx] {
                            keep |= 1 << bit;
                        }
                    }
                    out.rows[ty][w] = keep;
                    n += keep.count_ones() as usize;
                } else {
                    n += b.count_ones() as usize;
                }
            }
        }
        if n == 0 {
            out.clear();
        }
        n
    }

    /// Content pick: does surface `s` already hold every pixel of the current state? Early exit on the first
    /// mismatching tile, so a non-matching surface costs O(1) in the common case.
    fn zero_diff(&self, e: &Engine, s: usize, tmp: &mut Plane) -> bool {
        let sf = &self.surfs[s];
        if sf.salt != e.scene.salt || sf.last.is_none() || e.sabotage {
            return false;
        }
        self.evals.set(self.evals.get() + 1);
        if !e.hist.repair(sf.last, tmp) || tmp.full {
            self.full_filters.set(self.full_filters.get() + 1);
            return e.scene.zob == sf.zh; // memcmp, stops at the first difference
        }
        if !tmp.any {
            return true;
        }
        for ty in tmp.r0 as usize..=tmp.r1 as usize {
            for w in 0..2 {
                let mut b = tmp.rows[ty][w];
                while b != 0 {
                    let tx = w * 64 + b.trailing_zeros() as usize;
                    b &= b - 1;
                    if tx < TX && e.scene.zob[ty * TX + tx] != sf.zh[ty * TX + tx] {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn plane_to_tiles(p: &Plane, out: &mut Vec<u32>) {
        out.clear();
        if !p.any || p.full {
            return; // full: the slot's flag makes the threadgroup index the tile
        }
        for ty in p.r0 as usize..=p.r1 as usize {
            for w in 0..2 {
                let mut b = p.rows[ty][w];
                while b != 0 {
                    let tx = w * 64 + b.trailing_zeros() as usize;
                    b &= b - 1;
                    if tx < TX {
                        out.push((ty as u32) << 16 | tx as u32);
                    }
                }
            }
        }
    }

    /// TimerPoll: publish the pending frame if its GPU work is done, else look again in 0.25 ms.
    fn timer_poll(&mut self) {
        if let Some((idx, f)) = self.pending {
            if self.gpu_done() {
                self.pending = None;
                self.publish_now(idx, f);
            } else {
                self.arm_poll(250);
            }
        }
    }

    fn arm_poll(&mut self, us: u64) {
        let me = SendPtr(self as *mut V2);
        let when = dispatch2::DispatchTime::try_from(std::time::Duration::from_micros(us)).unwrap();
        let _ = dispatch2::DispatchQueue::main().after(when, move || {
            let me = me;
            unsafe { (*me.0).timer_poll() };
        });
    }

    fn publish_now(&mut self, idx: usize, frame: u64) {
        publish_txn(&self.layer, &self.surfs[idx].io, &self.sh, frame);
    }

    /// Record that surface `s` now holds the scene at `epoch` (tiles of `p` were just repaired).
    fn mark_drawn(&mut self, e: &Engine, s: usize, p: &Plane, epoch: u64) {
        let sf = &mut self.surfs[s];
        sf.last = Some(epoch);
        if self.spec.pick == Pick::Content {
            if p.full || sf.salt != e.scene.salt {
                sf.zh.copy_from_slice(&e.scene.zob);
                sf.salt = e.scene.salt;
            } else if p.any {
                // Copy whole row spans: tiles in the span but outside the repair set already hold equal hashes
                // (unchanged since this surface's epoch), so a memcpy is exact and ~20x cheaper than per tile.
                for ty in p.r0 as usize..=p.r1 as usize {
                    let (a, b) = (p.rows[ty][0], p.rows[ty][1]);
                    if a | b == 0 {
                        continue;
                    }
                    let lo = if a != 0 { a.trailing_zeros() as usize } else { 64 + b.trailing_zeros() as usize };
                    let hi = (if b != 0 { 128 - b.leading_zeros() as usize } else { 64 - a.leading_zeros() as usize }).min(TX);
                    let r = ty * TX;
                    sf.zh[r + lo..r + hi].copy_from_slice(&e.scene.zob[r + lo..r + hi]);
                }
            }
        }
    }
}

impl Presenter for V2 {
    fn name(&self) -> String {
        let r = match self.spec.raster {
            Raster::Render => "rp".to_string(),
            Raster::VClip => "vc".to_string(),
            Raster::Compute => "cs".to_string(),
            Raster::Armed(k) => format!("arm{k}"),
        };
        let p = match self.spec.publish {
            Pub::Handler => "h",
            Pub::HandlerMain => "hm",
            Pub::NextTick => "nt",
            Pub::TimerPoll => "tp",
        };
        let k = match self.spec.pick {
            Pick::RoundRobin => "rr",
            Pick::Free => "free",
            Pick::Content => "z",
        };
        format!("{r}:{p}:{k}:{}", self.spec.n)
    }

    fn sync(&mut self) {
        match self.spec.raster {
            Raster::Armed(_) => {
                if self.gpu_frames > 0 {
                    let ev = self.ev.as_ref().unwrap();
                    if ev.signaledValue() < 2 * self.gpu_frames {
                        ev.waitUntilSignaledValue_timeoutMS(2 * self.gpu_frames, 5000);
                    }
                }
            }
            _ => {
                if let Some(c) = &self.last_cmd {
                    if c.status() != MTLCommandBufferStatus::Completed {
                        c.waitUntilCompleted();
                    }
                }
            }
        }
    }

    fn poll(&mut self, e: &mut Engine) {
        if let Some((idx, f)) = self.pending {
            if self.gpu_done() {
                self.pending = None;
                self.publish_now(idx, f);
            }
        }
        if self.need_frame && !e.scene.acc.any {
            let ep = e.hist.epoch;
            self.present(e, ep);
        }
    }

    fn busy(&self) -> bool {
        self.pending.is_some() || self.need_frame
    }

    fn present(&mut self, e: &mut Engine, epoch: u64) {
        self.need_frame = false;
        // A drawn-but-unpublished frame goes out first (sync() already guaranteed its GPU work is done).
        if let Some((idx, f)) = self.pending.take() {
            self.sync();
            self.publish_now(idx, f);
        }
        let n = self.surfs.len();
        // ---- pick a target -------------------------------------------------------------------------------
        let mut idx = usize::MAX;
        let mut plane = std::mem::replace(&mut self.cand, Box::new(Plane::new()));
        let mut best = std::mem::replace(&mut self.best, Box::new(Plane::new()));
        let mut ntiles = 0usize;
        match self.spec.pick {
            Pick::RoundRobin => {
                idx = self.next;
                ntiles = self.repair_plane(e, idx, &mut best);
            }
            Pick::Free | Pick::Content => {
                let mut free = 0;
                let z = self.spec.pick == Pick::Content;
                if z {
                    if let Some(s) = self.shown {
                        if self.zero_diff(e, s, &mut plane) {
                            // The surface on screen already shows exactly this state: nothing to do at all.
                            self.surfs[s].last = Some(epoch);
                            self.skipped_all += 1;
                            self.cand = plane;
                            self.best = best;
                            return;
                        }
                    }
                }
                let mut first_free = usize::MAX;
                let mut zero = false;
                for k in 0..n {
                    let c = (self.next + k) % n;
                    if Some(c) == self.shown || self.surfs[c].io.is_in_use() {
                        continue;
                    }
                    free += 1;
                    if first_free == usize::MAX {
                        first_free = c;
                    }
                    if z && idx == usize::MAX && self.zero_diff(e, c, &mut plane) {
                        idx = c; // holds every pixel already: publish without GPU
                        zero = true;
                    }
                }
                if idx == usize::MAX && first_free != usize::MAX {
                    idx = first_free;
                }
                if zero {
                    best.clear();
                    ntiles = 0;
                } else if idx != usize::MAX {
                    ntiles = self.repair_plane(e, idx, &mut best);
                }
                self.free_hist[free.min(4)] += 1;
            }
        }
        self.cand = plane;
        if idx == usize::MAX {
            // Every surface is on screen or held by the compositor: draw nothing now, retry next tick.
            self.deferred += 1;
            self.need_frame = true;
            self.best = best;
            return;
        }
        if self.surfs[idx].io.is_in_use() {
            self.drawn_in_use += 1; // only round robin can get here
        }
        self.next = (idx + 1) % n;
        self.last_idx = idx;
        self.tiles_last = ntiles;
        self.tiles_total += ntiles as u64;
        if ntiles == 0 {
            // Content pick found a free surface that already holds every pixel: publish it, no GPU at all.
            self.skipped_gpu += 1;
            self.mark_drawn(e, idx, &best, epoch);
            self.shown = Some(idx);
            self.best = best;
            self.publish_now(idx, self.gpu_frames);
            return;
        }
        // ---- encode ------------------------------------------------------------------------------------------
        self.gpu_frames += 1;
        let f = self.gpu_frames;
        self.sh.produced_at[(f % 8) as usize].store(unsafe { CACurrentMediaTime() }.to_bits(), Relaxed);
        match self.spec.raster {
            Raster::Render => {
                let cmd = e.gpu.queue.commandBuffer().unwrap();
                if best.full {
                    e.region.set_full();
                } else {
                    e.region.from_plane(&best);
                }
                e.gpu.encode_repair(&cmd, &self.surfs[idx].tex, &e.scene, &e.region, e.mode);
                self.last_cmd = Some(cmd);
            }
            Raster::VClip => {
                let cmd = e.gpu.queue.commandBuffer().unwrap();
                if best.full {
                    e.region.set_full();
                    e.gpu.encode_repair(&cmd, &self.surfs[idx].tex, &e.scene, &e.region, DamageMode::Bbox);
                } else {
                    e.gpu.encode_vclip(&cmd, &self.surfs[idx].tex, &e.scene, &best);
                }
                self.last_cmd = Some(cmd);
            }
            Raster::Compute => {
                Self::plane_to_tiles(&best, &mut self.tiles);
                let off = e.gpu.write_slot_ex((f % 2) as usize, 0, &self.tiles, best.full);
                let cmd = e.gpu.queue.commandBuffer().unwrap();
                let t = &self.surfs[idx].tex;
                e.gpu.encode_compute(&cmd, off, [t, t, t, t], if best.full { TX * TY } else { self.tiles.len() }, false);
                self.last_cmd = Some(cmd);
            }
            Raster::Armed(_) => {
                Self::plane_to_tiles(&best, &mut self.tiles);
                e.gpu.write_slot_ex((f % 2) as usize, (idx % 4) as u32, &self.tiles, best.full);
            }
        }
        self.mark_drawn(e, idx, &best, epoch);
        self.best = best;
        self.shown = Some(idx);
        // ---- submit + publish ----------------------------------------------------------------------------
        match self.spec.raster {
            Raster::Armed(k) => {
                let ev = self.ev.clone().unwrap();
                match self.spec.publish {
                    Pub::NextTick => self.pending = Some((idx, f)),
                    Pub::TimerPoll => {
                        self.pending = Some((idx, f));
                        self.arm_poll(300);
                    }
                    Pub::Handler | Pub::HandlerMain => {
                        let to_main = self.spec.publish == Pub::HandlerMain;
                        let (layer, io, sh) = (self.layer.clone(), self.surfs[idx].io.clone(), self.sh.clone());
                        let blk = RcBlock::new(move |_e: NonNull<ProtocolObject<dyn MTLSharedEvent>>, _v: u64| {
                            if to_main {
                                let b = SendPtr((layer.clone(), io.clone(), sh.clone()));
                                dispatch2::DispatchQueue::main().exec_async(move || {
                                    let b = b;
                                    publish_txn(&b.0 .0, &b.0 .1, &b.0 .2, f);
                                });
                            } else {
                                publish_txn(&layer, &io, &sh, f);
                            }
                        });
                        unsafe { ev.notifyListener_atValue_block(self.listener.as_ref().unwrap(), 2 * f, RcBlock::as_ptr(&blk)) };
                    }
                }
                ev.setSignaledValue(2 * f - 1); // release the pre-encoded frame
                e.gpu.msgs.set(e.gpu.msgs.get() + 1);
                if self.armed_upto < f + k as u64 {
                    self.arm(&e.gpu, k as u64);
                }
            }
            _ => {
                let cmd = self.last_cmd.clone().unwrap();
                let mut poll = false;
                match self.spec.publish {
                    Pub::NextTick => self.pending = Some((idx, f)),
                    Pub::TimerPoll => {
                        self.pending = Some((idx, f));
                        poll = true;
                    }
                    Pub::Handler | Pub::HandlerMain => {
                        let to_main = self.spec.publish == Pub::HandlerMain;
                        let (layer, io, sh) = (self.layer.clone(), self.surfs[idx].io.clone(), self.sh.clone());
                        let blk = RcBlock::new(move |_c: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                            if to_main {
                                let b = SendPtr((layer.clone(), io.clone(), sh.clone()));
                                dispatch2::DispatchQueue::main().exec_async(move || {
                                    let b = b;
                                    publish_txn(&b.0 .0, &b.0 .1, &b.0 .2, f);
                                });
                            } else {
                                publish_txn(&layer, &io, &sh, f);
                            }
                        });
                        unsafe { cmd.addCompletedHandler(RcBlock::as_ptr(&blk)) };
                        e.gpu.msgs.set(e.gpu.msgs.get() + 1);
                    }
                }
                cmd.commit();
                e.gpu.msgs.set(e.gpu.msgs.get() + 2);
                if poll {
                    self.arm_poll(300);
                }
            }
        }
    }

    fn gpu_times(&mut self) -> Option<(f64, f64)> {
        self.last_cmd.as_ref().filter(|c| c.status() == MTLCommandBufferStatus::Completed).map(|c| (c.GPUStartTime(), c.GPUEndTime()))
    }

    fn presented_pixels(&mut self, _e: &Engine) -> Vec<u8> {
        // Make sure the last frame is published (NextTick) and read the surface the compositor was given.
        if let Some((idx, f)) = self.pending.take() {
            self.publish_now(idx, f);
        }
        let s = &self.surfs[self.shown.unwrap_or(self.last_idx)];
        let n = W as usize * 4 * H as usize;
        let mut v = vec![0u8; n];
        unsafe {
            s.io.lock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
            let bpr = s.io.bytes_per_row();
            let base = s.io.base_address().as_ptr() as *const u8;
            for y in 0..H as usize {
                std::ptr::copy_nonoverlapping(base.add(y * bpr), v.as_mut_ptr().add(y * W as usize * 4), W as usize * 4);
            }
            s.io.unlock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        }
        v
    }

    fn reference(&self, e: &Engine) -> Vec<u8> {
        match self.spec.raster {
            Raster::Render | Raster::VClip => e.gpu.reference(&e.scene),
            _ => e.gpu.reference_compute(),
        }
    }

    fn layer(&self) -> Retained<CALayer> {
        self.layer.clone()
    }

    fn surface_bytes(&self) -> u64 {
        self.surfs.iter().map(|s| s.io.alloc_size() as u64).sum()
    }

    fn last_surface_id(&self) -> usize {
        self.surfs[self.shown.unwrap_or(self.last_idx)].io.id() as usize
    }

    fn diag(&self) -> String {
        format!(
            "[{}] content evals {} (whole-plane {}), gpu frames {}, tiles/frame {:.1}, skipped-gpu {}, skipped-all {}, deferred (no free surface) {}, drawn-while-in-use {}, published-while-in-use {}, free-candidates hist {:?}, armed batch errors {}",
            self.name(),
            self.evals.get(),
            self.full_filters.get(),
            self.gpu_frames,
            self.tiles_total as f64 / self.gpu_frames.max(1) as f64,
            self.skipped_gpu,
            self.skipped_all,
            self.deferred,
            self.drawn_in_use,
            self.sh.in_use_at_publish.load(Relaxed),
            self.free_hist,
            self.batch_errors
        )
    }
}
