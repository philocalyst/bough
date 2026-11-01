//! The fused presenter: Parts II-III of RESULTS.md as ONE runtime-sized presenter over the fused system's
//! shared region (INTERFACE.md, "fused layout").
//!
//! * **Raster:** the binned 32x32 compute repair (`fs_bin` + `fs_tile`): one compute encoder, two dispatches,
//!   no render pass (so the driver never allocates a tiler parameter buffer). The kernels read bough-core's
//!   pages, directory and clip table, bough-text's glyph slots / run headers / token tables and the façade's
//!   quad instances *where the CPU wrote them*: the region is this presenter's `storageModeShared` buffer.
//! * **Damage in:** the damage IS the dispatch. The caller's OR-ed 32 px tile rows are copied into the
//!   frame slot as one 32-byte record per damaged row (row, x-span, prefix count of damaged tiles, the row's
//!   128 bits): O(damaged rows), no tile list. The bin pass runs per record; each tile threadgroup finds its
//!   tile by a binary search over the prefix counts and a select of the k-th set bit.
//! * **Surfaces:** up to 4 IOSurface-backed BGRA8 textures, all bound to every dispatch (the slot names the
//!   target). An IOSurface costs nothing until first written; *which* surface to draw is the caller's policy
//!   (`App::pick` in the façade: content-addressed, zero skips).
//! * **Bins sized by need:** the row bins start small (1024 entries per tile row); a bin pass that finds more
//!   hits than fit records the largest count, and the CPU (which checks after every frame, before the surface
//!   is published) grows the bins to it and redraws the frame: exact, and memory is what the scene needs.
//! * **Residency:** one `MTLResidencySet` on the queue holding the region, the atlas, the bins and the
//!   surfaces that have been drawn (an untouched surface is never added: residency makes every page of an
//!   allocation resident, measured by `examples/resprobe.rs`).
//! * **Keep-warm (bounded):** while GPU frames keep coming at intervals under the 10 s window, a 4-byte blit
//!   whenever the GPU has idled 0.8 s keeps it out of its power-down state (RESULTS 13.3); a frame served
//!   from a surface without the GPU (content-addressed) cancels it, and nothing is kept warm after a single
//!   isolated frame (mount): idle stays zero and the driver returns its memory.
//! * **Publish:** on the calling (main) thread once the GPU is done (`setContents` joins the run loop turn's
//!   implicit transaction): no thread hop, one commit per turn, and the CPU never writes the region while the
//!   GPU reads it.
//! * **Throughput mode:** `arm(k)` pre-encodes K frames into one command buffer, each waiting on a shared
//!   event; `release` writes the slot and signals (RESULTS 12.8). For non-input-driven animation.
use crate::gpu::{Cmd, Dev, Tex, METALLIB};
use crate::instance::Instance;
use dispatch2::DispatchData;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_core_foundation::{CFDictionary, CFRetained};
use objc2_foundation::{ns_string, NSDictionary, NSNumber, NSString};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::*;
use objc2_quartz_core::{CACurrentMediaTime, CALayer, CATransaction};
use std::cell::Cell;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;

pub const MAX_SURFACES: usize = 4;
/// Payload bit of a text record: runs `[p & 0xFFFFF, + ((p >> 20) & 0x7FF) + 1)`.
pub const PAY_RUN: u32 = 0x8000_0000;
/// Payload bit of an instance-run record: instances `[p & 0x3FFFF, + ((p >> 18) & 0xFFF) + 1)`.
pub const PAY_IRUN: u32 = 0x4000_0000;

#[inline]
pub const fn pay_runs(first: u32, n: u32) -> u32 {
    debug_assert!(first < (1 << 20) && n >= 1 && n <= 2048);
    PAY_RUN | (n - 1) << 20 | first
}
#[inline]
pub const fn pay_irun(first: u32, n: u32) -> u32 {
    debug_assert!(first < (1 << 18) && n >= 1 && n <= 4096);
    PAY_IRUN | (n - 1) << 18 | first
}

/// Region offsets and sizes the kernels read (written into the header once; `n_dir` per frame).
#[derive(Clone, Copy, Debug, Default)]
pub struct Layout {
    pub w: u32,
    pub h: u32,
    pub inst_off: u32,
    pub spaces_off: u32,
    pub clips_off: u32,
    pub dir_id_off: u32,
    pub dir_ink_off: u32,
    pub pages_off: u32,
    pub gx_off: u32,
    pub gent_off: u32,
    pub runs_off: u32,
    pub toks_off: u32,
    pub tok_stride: u32,
    /// Three frame slots (two in flight, one for the reference render): header + damaged-row records.
    pub slot: [u32; 3],
}

impl Layout {
    pub fn tiles_x(&self) -> u32 {
        self.w.div_ceil(32)
    }
    pub fn tiles_y(&self) -> u32 {
        self.h.div_ceil(32)
    }
    /// Bytes of one frame slot for a `w`-by-`h` window: a 32-byte header and one 32-byte record per tile row.
    pub fn slot_bytes(_w: u32, h: u32) -> usize {
        (32 + 32 * h.div_ceil(32) as usize).next_multiple_of(256)
    }
}

/// The kernels' header (`FHdr` in shader.metal), at region offset 0.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FHdr {
    viewport: [f32; 2],
    tiles_x: u32,
    tiles_y: u32,
    inst_off: u32,
    spaces_off: u32,
    clips_off: u32,
    dir_id_off: u32,
    dir_ink_off: u32,
    n_dir: u32,
    pages_off: u32,
    gx_off: u32,
    gent_off: u32,
    runs_off: u32,
    toks_off: u32,
    tok_stride: u32,
    bin_cap: u32,
    debug: u32,
}
/// Bytes reserved for the header at the start of the region.
pub const HDR_BYTES: usize = 256;
const _: () = assert!(size_of::<FHdr>() <= HDR_BYTES);
const _: () = assert!(size_of::<Instance>() == 48);

pub struct Surface {
    pub io: CFRetained<IOSurfaceRef>,
    pub tex: Tex,
    /// Written at least once (an untouched IOSurface owns no pages).
    pub touched: Cell<bool>,
    /// Marked purgeable-volatile by a calm trim (made non-volatile before the next draw).
    pub volatile: Cell<bool>,
}

fn make_iosurface(w: usize, h: usize) -> CFRetained<IOSurfaceRef> {
    let keys: [&NSString; 4] = [ns_string!("IOSurfaceWidth"), ns_string!("IOSurfaceHeight"), ns_string!("IOSurfaceBytesPerElement"), ns_string!("IOSurfacePixelFormat")];
    let vals = [NSNumber::new_i32(w as i32), NSNumber::new_i32(h as i32), NSNumber::new_i32(4), NSNumber::new_u32(0x4247_5241)]; // 'BGRA'
    let refs: Vec<&AnyObject> = vals.iter().map(|v| v.as_ref() as &AnyObject).collect();
    let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&keys, &refs);
    // SAFETY: NSDictionary is toll-free bridged to CFDictionary.
    let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&dict) as *const CFDictionary) };
    // SAFETY: a well-formed property dictionary.
    unsafe { IOSurfaceRef::new(cf) }.expect("IOSurface")
}

/// Keep-warm state shared with its main-queue timer (no pointer to the presenter is captured).
struct Warm {
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    tiny: Retained<ProtocolObject<dyn MTLBuffer>>,
    period: f64,
    window: f64,
    last_gpu: AtomicU64,
    last_blit: AtomicU64,
    armed: AtomicBool,
    cancelled: AtomicBool,
    blits: AtomicU64,
}
struct SendWarm(Arc<Warm>);
// SAFETY: Metal command queues and buffers are thread-safe objects; the timer only runs on the main queue.
unsafe impl Send for SendWarm {}

fn now() -> f64 {
    CACurrentMediaTime()
}

impl Warm {
    fn arm(self: &Arc<Self>, after: f64) {
        if self.armed.swap(true, Relaxed) {
            return;
        }
        let w = SendWarm(self.clone());
        let when = dispatch2::DispatchTime::try_from(std::time::Duration::from_secs_f64(after.max(0.02))).unwrap();
        let _ = dispatch2::DispatchQueue::main().after(when, move || {
            let w = w;
            w.0.tick();
        });
    }
    fn tick(self: &Arc<Self>) {
        self.armed.store(false, Relaxed);
        let t = now();
        let last = f64::from_bits(self.last_gpu.load(Relaxed));
        if self.cancelled.load(Relaxed) || t - last > self.window {
            return; // toggling between held surfaces, or a long idle: let the GPU sleep
        }
        let lb = f64::from_bits(self.last_blit.load(Relaxed)).max(last);
        if t - lb >= self.period * 0.9 {
            if let Some(c) = self.queue.commandBuffer() {
                if let Some(b) = c.blitCommandEncoder() {
                    b.fillBuffer_range_value(&self.tiny, objc2_foundation::NSRange::new(0, 4), 0);
                    b.endEncoding();
                }
                c.commit();
            }
            self.blits.fetch_add(1, Relaxed);
            self.last_blit.store(t.to_bits(), Relaxed);
        }
        let lb = f64::from_bits(self.last_blit.load(Relaxed)).max(last);
        self.arm(lb + self.period - t);
    }
}

/// Counters.
#[derive(Default, Clone, Copy, Debug)]
pub struct Stats {
    pub gpu_frames: u64,
    pub tiles: u64,
    pub ca_only: u64,
    pub warm_blits: u64,
    pub resident_surfaces: usize,
}

pub struct Fused {
    pub device: Dev,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    /// Reference renders (tests/checks): never queued behind a pre-armed batch waiting on its event.
    aux: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pso_bin: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    pso_tile: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    pool: Retained<ProtocolObject<dyn MTLBuffer>>,
    pool_ptr: *mut u8,
    pool_len: usize,
    atlas: Tex,
    atlas_dims: (u32, u32),
    bins: Retained<ProtocolObject<dyn MTLBuffer>>,
    counts: Retained<ProtocolObject<dyn MTLBuffer>>,
    pub bin_cap: u32,
    pub surfs: Vec<Surface>,
    pub layer: Retained<CALayer>,
    pub w: usize,
    pub h: usize,
    /// Publish drawn surfaces to `layer` (windowed); off headless.
    pub publish: bool,
    last: Option<Cmd>,
    lay: Layout,
    resid: Option<Retained<ProtocolObject<dyn MTLResidencySet>>>,
    warm: Option<Arc<Warm>>,
    prev_frame_at: f64,
    scratch: Option<Surface>,
    pub stats: Stats,
    /// The last GPU frame: (slot, target, rows, tiles), redrawn if its bins overflowed.
    last_frame: Option<(usize, usize, u32, u32)>,
    pub bin_growths: u32,
    // throughput mode (pre-armed command stream)
    ev: Option<Retained<ProtocolObject<dyn MTLSharedEvent>>>,
    armed_upto: u64,
    released: u64,
    batch: usize,
}

impl Fused {
    /// A presenter for a `w` x `h` px window whose region is a `region_len`-byte shared buffer.
    pub fn new(w: usize, h: usize, scale: f64, region_len: usize, surfaces: usize, bin_cap: u32) -> Fused {
        let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
        let queue = device.newCommandQueue().expect("queue");
        let aux = device.newCommandQueue().expect("aux queue");
        let data = DispatchData::from_static_bytes(METALLIB);
        let lib = device.newLibraryWithData_error(&data).expect("metallib load");
        let pso = |name: &NSString| {
            let f = lib.newFunctionWithName(name).expect("kernel");
            device.newComputePipelineStateWithFunction_error(&f).expect("compute pso")
        };
        let pso_bin = pso(ns_string!("fs_bin"));
        let pso_tile = pso(ns_string!("fs_tile"));
        let pool = device.newBufferWithLength_options(region_len, MTLResourceOptions::StorageModeShared).expect("region buffer");
        let pool_ptr = pool.contents().as_ptr() as *mut u8;
        let atlas = Self::new_atlas(&device, 1024, 512);
        let ty = h.div_ceil(32);
        let bins = device.newBufferWithLength_options(ty * bin_cap as usize * 16, MTLResourceOptions::StorageModePrivate).expect("bins");
        let counts = device.newBufferWithLength_options((ty + 1) * 4, MTLResourceOptions::StorageModeShared).expect("bin counts");
        let surfs = (0..surfaces.clamp(1, MAX_SURFACES)).map(|_| Self::new_surface(&device, w, h)).collect();
        let layer = CALayer::new();
        layer.setContentsScale(scale);
        layer.setOpaque(true);
        layer.setFrame(objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(w as f64 / scale, h as f64 / scale),
        ));
        let mut f = Fused {
            device,
            queue,
            aux,
            pso_bin,
            pso_tile,
            pool,
            pool_ptr,
            pool_len: region_len,
            atlas,
            atlas_dims: (1024, 512),
            bins,
            counts,
            bin_cap,
            surfs,
            layer,
            w,
            h,
            publish: false,
            last: None,
            lay: Layout::default(),
            resid: None,
            warm: None,
            prev_frame_at: f64::NEG_INFINITY,
            scratch: None,
            stats: Stats::default(),
            last_frame: None,
            bin_growths: 0,
            ev: None,
            armed_upto: 0,
            released: 0,
            batch: 0,
        };
        if std::env::var_os("BOUGH_NORESIDENCY").is_none() {
            let d = MTLResidencySetDescriptor::new();
            unsafe { d.setInitialCapacity(8) };
            if let Ok(set) = f.device.newResidencySetWithDescriptor_error(&d) {
                set.addAllocation(ProtocolObject::from_ref(&*f.pool));
                set.addAllocation(ProtocolObject::from_ref(&*f.atlas));
                set.addAllocation(ProtocolObject::from_ref(&*f.bins));
                set.addAllocation(ProtocolObject::from_ref(&*f.counts));
                set.commit();
                set.requestResidency();
                f.queue.addResidencySet(&set);
                f.resid = Some(set);
            }
        }
        let warm = std::env::var("BOUGH_KEEPWARM").ok().map_or(Some((0.8, 10.0)), |s| {
            let mut it = s.split(',').map(|x| x.parse::<f64>().ok());
            match (it.next().flatten(), it.next().flatten()) {
                (Some(p), Some(w)) if p > 0.0 => Some((p / 1e3, w)),
                _ => None,
            }
        });
        if let Some((period, window)) = warm {
            let tiny = f.device.newBufferWithLength_options(16, MTLResourceOptions::StorageModePrivate).expect("keep-warm buffer");
            f.warm = Some(Arc::new(Warm {
                queue: f.queue.clone(),
                tiny,
                period,
                window,
                last_gpu: AtomicU64::new(0f64.to_bits()),
                last_blit: AtomicU64::new(0f64.to_bits()),
                armed: AtomicBool::new(false),
                cancelled: AtomicBool::new(false),
                blits: AtomicU64::new(0),
            }));
        }
        f
    }

    fn new_atlas(device: &Dev, w: u32, h: u32) -> Tex {
        let ad = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::R8Unorm, w as usize, h as usize, false) };
        device.newTextureWithDescriptor(&ad).expect("atlas")
    }

    fn new_surface(device: &Dev, w: usize, h: usize) -> Surface {
        let io = make_iosurface(w, h);
        let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, w, h, false) };
        d.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
        d.setStorageMode(MTLStorageMode::Shared);
        let tex = device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0).expect("iosurface texture");
        Surface { io, tex, touched: Cell::new(false), volatile: Cell::new(false) }
    }

    /// The region's memory (the shared buffer's contents) and its length, plus a keep-alive handle.
    pub fn region(&self) -> (*mut u8, usize, Retained<ProtocolObject<dyn MTLBuffer>>) {
        (self.pool_ptr, self.pool_len, self.pool.clone())
    }

    /// Write the kernels' header (offsets of every column, window size, bin capacity).
    pub fn set_layout(&mut self, l: Layout) {
        self.lay = l;
        let h = FHdr {
            viewport: [l.w as f32, l.h as f32],
            tiles_x: l.tiles_x(),
            tiles_y: l.tiles_y(),
            inst_off: l.inst_off,
            spaces_off: l.spaces_off,
            clips_off: l.clips_off,
            dir_id_off: l.dir_id_off,
            dir_ink_off: l.dir_ink_off,
            n_dir: 0,
            pages_off: l.pages_off,
            gx_off: l.gx_off,
            gent_off: l.gent_off,
            runs_off: l.runs_off,
            toks_off: l.toks_off,
            tok_stride: l.tok_stride,
            bin_cap: self.bin_cap,
            debug: 0,
        };
        // SAFETY: the region starts with HDR_BYTES reserved for the header; no GPU work reads it now.
        unsafe { (self.pool_ptr as *mut FHdr).write(h) };
    }

    /// Wait for the GPU's last frame (the CPU is about to write the region); redraw it if a bin overflowed.
    #[inline]
    pub fn sync(&mut self) {
        if let Some(c) = &self.last {
            if c.status() != MTLCommandBufferStatus::Completed {
                c.waitUntilCompleted();
            }
        }
        if let Some(ev) = &self.ev {
            if self.released > 0 && ev.signaledValue() < 2 * self.released {
                ev.waitUntilSignaledValue_timeoutMS(2 * self.released, 5000);
            }
        }
        self.check_bins();
    }

    /// After a GPU frame completed: if a row bin overflowed, grow the bins to the recorded need and redraw
    /// that frame (same slot, same target) before anyone sees it.
    fn check_bins(&mut self) {
        let ty = self.lay.tiles_y() as usize;
        // SAFETY: `counts` holds ty + 1 u32s in shared storage; the GPU is idle (the caller waited).
        let need = unsafe { *(self.counts.contents().as_ptr() as *const u32).add(ty) };
        if need == 0 {
            return;
        }
        let Some((k, target, nr, nt)) = self.last_frame else { return };
        unsafe { *(self.counts.contents().as_ptr() as *mut u32).add(ty) = 0 };
        let streaming = self.ev.is_some();
        if streaming {
            // draining reuses both slots: keep the overflowed frame's
            let sb = Layout::slot_bytes(self.lay.w, self.lay.h);
            let mut keep = vec![0u8; sb];
            // SAFETY: slot k lies inside the region (reserved with `slot_bytes`); the GPU is idle.
            unsafe { std::ptr::copy_nonoverlapping(self.pool_ptr.add(self.lay.slot[k] as usize), keep.as_mut_ptr(), sb) };
            self.drain_stream();
            unsafe { std::ptr::copy_nonoverlapping(keep.as_ptr(), self.pool_ptr.add(self.lay.slot[k] as usize), sb) };
        }
        let cap = need.next_power_of_two().max(self.bin_cap * 2);
        self.bins = self.device.newBufferWithLength_options(ty * cap as usize * 16, MTLResourceOptions::StorageModePrivate).expect("bins");
        if let Some(r) = &self.resid {
            r.addAllocation(ProtocolObject::from_ref(&*self.bins));
            r.commit();
        }
        self.bin_cap = cap;
        self.bin_growths += 1;
        // SAFETY: header field `bin_cap`; the GPU is idle.
        unsafe { (*(self.pool_ptr as *mut FHdr)).bin_cap = cap };
        let cmd = self.aux.commandBuffer().expect("command buffer");
        if streaming {
            self.encode(&cmd, k, self.targets(), nr, nt, false);
        } else {
            let t = &self.surfs[target].tex;
            self.encode(&cmd, k, [t, t, t, t], nr, nt, false);
        }
        cmd.commit();
        cmd.waitUntilCompleted();
        if streaming {
            self.arm_batch();
        }
        self.check_bins();
    }

    /// Replace the row bins by ones of `cap` entries per row (tests; not while streaming).
    pub fn reset_bins(&mut self, cap: u32) {
        assert!(self.ev.is_none());
        let ty = self.lay.tiles_y() as usize;
        self.bins = self.device.newBufferWithLength_options(ty * cap as usize * 16, MTLResourceOptions::StorageModePrivate).expect("bins");
        self.bin_cap = cap;
        // SAFETY: header field `bin_cap`; the GPU is idle (the caller synced).
        unsafe { (*(self.pool_ptr as *mut FHdr)).bin_cap = cap };
    }

    /// Release every pre-armed frame with an empty slot (no tiles) and wait: the stream is idle and can be
    /// re-armed (with new bins).
    fn drain_stream(&mut self) {
        let ev = self.ev.clone().unwrap();
        let empty = [0u128; 0];
        for f in self.released + 1..=self.armed_upto {
            // SAFETY: the region and its slots; the frame `f - 1` finished (waited below each time).
            unsafe { write_slot(self.pool_ptr, &self.lay, (f % 2) as usize, 0, &empty, false) };
            ev.setSignaledValue(2 * f - 1);
            ev.waitUntilSignaledValue_timeoutMS(2 * f, 5000);
        }
        self.released = self.armed_upto;
    }

    pub fn is_in_use(&self, i: usize) -> bool {
        self.surfs[i].io.is_in_use()
    }

    /// Upload the glyph atlas (R8, `width` x `height`) if it changed since the last upload (`dirty`).
    pub fn sync_atlas(&mut self, width: u32, height: u32, pixels: &[u8], dirty: bool) -> bool {
        if !dirty && (width, height) == self.atlas_dims {
            return false;
        }
        assert_eq!(pixels.len(), (width * height) as usize);
        if (width, height) != self.atlas_dims {
            let old = std::mem::replace(&mut self.atlas, Self::new_atlas(&self.device, width, height));
            self.atlas_dims = (width, height);
            if let Some(r) = &self.resid {
                r.removeAllocation(ProtocolObject::from_ref(&*old));
                r.addAllocation(ProtocolObject::from_ref(&*self.atlas));
                r.commit();
            }
        }
        // SAFETY: `pixels` holds width * height bytes (asserted); the GPU is idle (sync()).
        unsafe {
            self.atlas.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                MTLRegion { origin: MTLOrigin { x: 0, y: 0, z: 0 }, size: MTLSize { width: width as usize, height: height as usize, depth: 1 } },
                0,
                NonNull::new(pixels.as_ptr() as *mut _).unwrap(),
                width as usize,
            );
        }
        true
    }

    /// Make surface `i` ready to be drawn: non-volatile again (returns true if the system purged it, so its
    /// contents are gone) and, on first use, part of the residency set.
    pub fn prepare_target(&mut self, i: usize) -> bool {
        let s = &self.surfs[i];
        let mut purged = false;
        if s.volatile.get() {
            let mut old = 0u32;
            unsafe { s.io.set_purgeable(0, &mut old) };
            s.volatile.set(false);
            purged = old == 2;
        }
        if !s.touched.get() {
            s.touched.set(true);
            if let Some(r) = &self.resid {
                r.addAllocation(ProtocolObject::from_ref(&*s.tex));
                r.commit();
                self.stats.resident_surfaces += 1;
            }
        }
        purged
    }

    /// Make every surface `want(i)` selects purgeable-volatile (unless the compositor holds it); count.
    pub fn trim_where(&mut self, want: impl Fn(usize) -> bool) -> usize {
        let mut n = 0;
        for i in 0..self.surfs.len() {
            let s = &self.surfs[i];
            if !want(i) || s.volatile.get() || !s.touched.get() || s.io.is_in_use() {
                continue;
            }
            if let Some(r) = &self.resid {
                r.removeAllocation(ProtocolObject::from_ref(&*s.tex));
                r.commit();
                self.stats.resident_surfaces -= 1;
            }
            s.touched.set(false);
            let mut old = 0u32;
            if unsafe { s.io.set_purgeable(1, &mut old) } == 0 {
                s.volatile.set(true);
                n += 1;
            }
        }
        n
    }

    fn write_slot(&self, k: usize, target: u32, rows: &[u128], full: bool) -> (u32, u32) {
        // SAFETY: the region is this presenter's buffer, laid out by `lay`; the GPU is idle (sync()).
        unsafe { write_slot(self.pool_ptr, &self.lay, k, target, rows, full) }
    }

    fn encode(&self, cmd: &ProtocolObject<dyn MTLCommandBuffer>, k: usize, targets: [&Tex; 4], nrows: u32, ntiles: u32, indirect: bool) {
        let l = &self.lay;
        let enc = cmd.computeCommandEncoder().expect("compute encoder");
        let offs = [l.slot[k], 0];
        unsafe {
            enc.setBuffer_offset_atIndex(Some(&self.pool), 0, 0);
            enc.setBytes_length_atIndex(NonNull::new(offs.as_ptr() as *mut _).unwrap(), 8, 1);
            enc.setBuffer_offset_atIndex(Some(&self.bins), 0, 2);
            enc.setBuffer_offset_atIndex(Some(&self.counts), 0, 3);
            enc.setComputePipelineState(&self.pso_bin);
            let tg = MTLSize { width: 256, height: 1, depth: 1 };
            if indirect {
                enc.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(&self.pool, l.slot[k] as usize, tg);
            } else {
                enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: nrows as usize, height: 1, depth: 1 }, tg);
            }
            enc.memoryBarrierWithScope(MTLBarrierScope::Buffers);
            enc.setComputePipelineState(&self.pso_tile);
            let texs: [*const ProtocolObject<dyn MTLTexture>; 5] =
                [Retained::as_ptr(&self.atlas), Retained::as_ptr(targets[0]), Retained::as_ptr(targets[1]), Retained::as_ptr(targets[2]), Retained::as_ptr(targets[3])];
            enc.setTextures_withRange(NonNull::new(texs.as_ptr() as *mut _).unwrap(), objc2_foundation::NSRange::new(0, 5));
            let tg2 = MTLSize { width: 16, height: 16, depth: 1 };
            if indirect {
                enc.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(&self.pool, l.slot[k] as usize + 12, tg2);
            } else {
                enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: ntiles as usize, height: 1, depth: 1 }, tg2);
            }
        }
        enc.endEncoding();
    }

    fn targets(&self) -> [&Tex; 4] {
        let n = self.surfs.len();
        [&self.surfs[0].tex, &self.surfs[1 % n].tex, &self.surfs[2 % n].tex, &self.surfs[3 % n].tex]
    }

    /// Repair surface `target` from the damaged tile rows (`full`: everything) with `n_dir` directory
    /// entries, commit, and (windowed) publish it once the GPU is done. Returns the tile count.
    pub fn draw(&mut self, target: usize, rows: &[u128], full: bool, n_dir: u32) -> u32 {
        self.sync();
        let k = if self.ev.is_some() { ((self.released + 1) % 2) as usize } else { (self.stats.gpu_frames % 2) as usize };
        // SAFETY: header field `n_dir` (u32) of the region's header; the GPU is idle (sync()).
        unsafe { (*(self.pool_ptr as *mut FHdr)).n_dir = n_dir };
        if self.ev.is_some() {
            // the pre-encoded stream binds every surface; the slot names the target
            let (nt, _) = self.write_slot(k, target as u32, rows, full);
            return self.release(k, nt);
        }
        // only the target is bound (at every slot): binding a surface makes all its pages resident, so a
        // surface the ring never draws must never be bound
        let (nt, nr) = self.write_slot(k, 0, rows, full);
        let cmd = self.queue.commandBuffer().expect("command buffer");
        let t = &self.surfs[target].tex;
        self.encode(&cmd, k, [t, t, t, t], nr, nt, false);
        cmd.commit();
        self.after_gpu_frame(nt);
        self.last_frame = Some((k, target, nr, nt));
        self.last = Some(cmd);
        if self.publish {
            self.sync();
            self.publish_surface(target);
        }
        nt
    }

    fn after_gpu_frame(&mut self, nt: u32) {
        self.stats.gpu_frames += 1;
        self.stats.tiles += nt as u64;
        let t = now();
        if let Some(w) = &self.warm {
            // keep warm only what has proven to be interactive: a frame within the window of the previous one
            w.last_gpu.store(t.to_bits(), Relaxed);
            w.cancelled.store(false, Relaxed);
            if t - self.prev_frame_at < w.window {
                w.arm(w.period);
            }
            self.stats.warm_blits = w.blits.load(Relaxed);
        }
        self.prev_frame_at = t;
    }

    /// Put surface `i` on screen without any GPU work (it already holds the current state).
    pub fn show_held(&mut self, i: usize) {
        self.stats.ca_only += 1;
        if let Some(w) = &self.warm {
            w.cancelled.store(true, Relaxed);
        }
        if self.publish {
            self.publish_surface(i);
        }
    }

    fn publish_surface(&self, i: usize) {
        CATransaction::setDisableActions(true);
        // SAFETY: an IOSurfaceRef is a valid CALayer `contents` object.
        unsafe { self.layer.setContents(Some(&*(&*self.surfs[i].io as *const IOSurfaceRef as *const AnyObject))) };
    }

    // ------------------------------------------------------------------ throughput mode

    /// Switch to the pre-armed command stream: `k` frames per batch, each waiting on a shared event.
    pub fn arm_stream(&mut self, k: usize) {
        self.ev = Some(self.device.newSharedEvent().expect("shared event"));
        self.batch = k.max(2);
        self.arm_batch();
    }

    fn arm_batch(&mut self) {
        let ev = self.ev.clone().unwrap();
        let cmd = self.queue.commandBuffer().expect("command buffer");
        for f in self.armed_upto + 1..=self.armed_upto + self.batch as u64 {
            cmd.encodeWaitForEvent_value(ProtocolObject::from_ref(&*ev), 2 * f - 1);
            self.encode(&cmd, (f % 2) as usize, self.targets(), 0, 0, true);
            cmd.encodeSignalEvent_value(ProtocolObject::from_ref(&*ev), 2 * f);
        }
        cmd.commit();
        self.armed_upto += self.batch as u64;
    }

    fn release(&mut self, k: usize, nt: u32) -> u32 {
        let f = self.released + 1;
        // the slot names the target; a redraw after a bin overflow reads it back from there
        let target = unsafe { *(self.pool_ptr.add(self.lay.slot[k] as usize) as *const u32).add(6) } as usize;
        let nr = unsafe { *(self.pool_ptr.add(self.lay.slot[k] as usize) as *const u32).add(7) };
        self.last_frame = Some((k, target, nr, nt));
        debug_assert_eq!((f % 2) as usize, k);
        self.released = f;
        let ev = self.ev.clone().unwrap();
        ev.setSignaledValue(2 * f - 1);
        self.after_gpu_frame(nt);
        if self.armed_upto < f + self.batch as u64 / 2 {
            self.arm_batch();
        }
        nt
    }

    /// Throughput mode: has the last released frame finished on the GPU?
    pub fn released_done(&self) -> bool {
        self.ev.as_ref().is_none_or(|e| e.signaledValue() >= 2 * self.released)
    }

    /// Throughput mode: publish surface `i` (its frame finished; called from the next tick).
    pub fn publish_later(&self, i: usize) {
        if self.publish {
            self.publish_surface(i);
        }
    }

    pub fn streaming(&self) -> bool {
        self.ev.is_some()
    }

    // ------------------------------------------------------------------ readback (tests, checks)

    /// Pixels of surface `i` (BGRA, tight rows), after the GPU is done with it.
    pub fn surface_pixels(&mut self, i: usize) -> Vec<u8> {
        self.sync();
        read_io(&self.surfs[i].io, self.w, self.h)
    }

    /// A from-scratch full redraw of the current region through the same kernels into a private surface.
    pub fn reference_pixels(&mut self, n_dir: u32) -> Vec<u8> {
        self.sync();
        if self.scratch.is_none() {
            self.scratch = Some(Self::new_surface(&self.device, self.w, self.h));
        }
        unsafe { (*(self.pool_ptr as *mut FHdr)).n_dir = n_dir };
        let (nt, nr) = self.write_slot(2, 0, &[], true);
        let s = &self.scratch.as_ref().unwrap().tex;
        let cmd = self.aux.commandBuffer().expect("command buffer");
        self.encode(&cmd, 2, [s, s, s, s], nr, nt, false);
        cmd.commit();
        cmd.waitUntilCompleted();
        read_io(&self.scratch.as_ref().unwrap().io, self.w, self.h)
    }

    /// Did a row bin overflow in the last frame and stay unrepaired? (tests: always false after a sync, since
    /// an overflow grows the bins and redraws.)
    pub fn bins_overflowed(&self) -> bool {
        let ty = self.lay.tiles_y() as usize;
        // SAFETY: `counts` holds ty + 1 u32s; shared storage; the GPU is idle after sync.
        unsafe { *(self.counts.contents().as_ptr() as *const u32).add(ty) != 0 }
    }

    pub fn surface_bytes(&self) -> u64 {
        self.surfs.iter().filter(|s| s.touched.get()).map(|s| s.io.alloc_size() as u64).sum()
    }
}

/// Copy the OR-ed tile rows (`rows[ty]`, bit tx = tile) into frame slot `k` as damaged-row records (see the
/// module docs): O(damaged rows). `full`: every tile of the window. Returns (tiles, rows). This is the whole
/// CPU side of a frame's raster input (headless runs call it too, so RENDER=0 measures it).
///
/// # Safety
/// `region` must be a region laid out by `l` (slots reserved with `Layout::slot_bytes` for its window), not
/// read by the GPU while this writes.
pub unsafe fn write_slot(region: *mut u8, l: &Layout, k: usize, target: u32, rows: &[u128], full: bool) -> (u32, u32) {
    let (tx, ty) = (l.tiles_x(), l.tiles_y());
    let all = if tx >= 128 { u128::MAX } else { (1u128 << tx) - 1 };
    // SAFETY: see the function contract.
    unsafe {
        let s = region.add(l.slot[k] as usize) as *mut u32;
        let rec = s.add(8);
        let (mut nt, mut nr) = (0u32, 0u32);
        for y in 0..ty as usize {
            let row = if full { all } else { rows.get(y).copied().unwrap_or(0) & all };
            if row == 0 {
                continue;
            }
            let r = rec.add(nr as usize * 8);
            *r = y as u32;
            *r.add(1) = row.trailing_zeros() | (128 - row.leading_zeros()) << 8;
            *r.add(2) = nt;
            (r.add(4) as *mut [u32; 4]).write_unaligned(std::mem::transmute::<u128, [u32; 4]>(row));
            nt += row.count_ones();
            nr += 1;
        }
        *s = nr;
        *s.add(1) = 1;
        *s.add(2) = 1;
        *s.add(3) = nt;
        *s.add(4) = 1;
        *s.add(5) = 1;
        *s.add(6) = target;
        *s.add(7) = nr;
        (nt, nr)
    }
}

fn read_io(io: &IOSurfaceRef, w: usize, h: usize) -> Vec<u8> {
    let mut v = vec![0u8; w * 4 * h];
    // SAFETY: the surface is locked for reading; rows are `bytes_per_row` apart and `w * 4` long.
    unsafe {
        io.lock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        let bpr = io.bytes_per_row();
        let base = io.base_address().as_ptr() as *const u8;
        for y in 0..h {
            std::ptr::copy_nonoverlapping(base.add(y * bpr), v.as_mut_ptr().add(y * w * 4), w * 4);
        }
        io.unlock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
    }
    v
}
