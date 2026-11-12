//! The fused presenter on the vendored **wgpu-hal** (ROUND4 §8): `bough_metal::fused` with its frame encoder
//! moved onto the hal. Same region, header, bind table, slots, surfaces, residency set, shared-event stream,
//! publisher thread, paced commits, keep-warm, bin growth and resize (see bough-metal's fused.rs for the design;
//! this file is that file with the Metal encoder replaced). What changed:
//! * kernels are wgsl-rs (`crate::shaders::raster`, `copy`), compiled at build time into `crate::METALLIB`, loaded
//!   through `ShaderInput::MetalLib`; pipelines and layouts are hal objects;
//! * a frame is one hal compute pass (copy, bin, tile) binding group 0 (region, bins, counts) and group 1 (the
//!   slot); surfaces and the atlas come from the region's bind table (build.rs's bindless transform);
//! * the pre-armed batches are hal command buffers: `begin_encoding`, per frame the shared-event wait and signal on
//!   `raw_command_buffer()` around the pass, `end_encoding`, `Queue::submit`;
//! * the device is `MTLCreateSystemDefaultDevice` wrapped by `Device::device_from_raw` (no instance, adapter
//!   enumeration or validation), the queues by `Queue::queue_from_raw`, the region by `Device::buffer_from_raw`.
use bough_metal::instance::Instance;
use wgpu_hal as hal;
use wgpu_hal::{CommandEncoder as _, Device as _, Queue as _};
use wgpu_types as wgt;

type Dev = Retained<ProtocolObject<dyn MTLDevice>>;
type Tex = Retained<ProtocolObject<dyn MTLTexture>>;
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

pub use bough_metal::fused::{pay_irun, pay_runs, write_slot, Layout, Masks, SlotCopy, Stats, MAX_SURFACES, PAY_IRUN, PAY_RUN, SLOT_HDR};

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
    masks: [u32; 7],
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

// ------------------------------------------------------------------ the frame stream's encoder (wgpu-hal)

/// What encoding a frame needs, shared by the main thread and the publisher thread (which keeps the stream
/// armed, so no encode or commit runs on the input thread): the hal device and queues, the event, the region,
/// the three pipelines, the slot bind groups, and the arming state behind one lock (two threads
/// never arm overlapping frames). Everything a frame binds is the region, the bins, the counts and the slot: the
/// surfaces and the atlas are read from the region's bind table (build.rs's bindless transform).
struct Kern {
    dev: hal::metal::Device,
    queue: hal::metal::Queue,
    aux: hal::metal::Queue,
    ev: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    region_raw: Retained<ProtocolObject<dyn MTLBuffer>>,
    pso_bin: hal::metal::ComputePipeline,
    pso_tile: hal::metal::ComputePipeline,
    pso_copy: hal::metal::ComputePipeline,
    /// One 16-byte uniform per frame slot (x = the slot's byte offset), shared storage written in place.
    slot_raw: [Retained<ProtocolObject<dyn MTLBuffer>>; 3],
    slots: [std::sync::atomic::AtomicU32; 3],
    batch: std::sync::atomic::AtomicUsize,
    /// Mirror of `arm.armed_upto` for a lock-free "already armed?" test.
    armed: AtomicU64,
    arm: std::sync::Mutex<ArmState>,
}
struct ArmState {
    armed_upto: u64,
    /// The last two pre-armed command buffers (diagnostics if a frame never finishes).
    batches: std::collections::VecDeque<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    enc: hal::metal::CommandEncoder,
    fence: hal::metal::Fence,
    fence_value: u64,
    aux_enc: hal::metal::CommandEncoder,
    aux_fence: hal::metal::Fence,
    aux_value: u64,
    /// The current bins and counts (replaced under the lock when they grow or the window resizes).
    bc: (Retained<ProtocolObject<dyn MTLBuffer>>, Retained<ProtocolObject<dyn MTLBuffer>>),
}
// SAFETY: Metal and wgpu-hal Metal objects are thread-safe; the arming state is behind a mutex.
unsafe impl Send for Kern {}
unsafe impl Sync for Kern {}

impl Kern {
    /// One frame: the scroll copy, the bin pass, then the tile pass over slot `k`, in one hal compute pass that
    /// binds the region, bins, counts and the slot (serial dispatch orders bin before tile). `indirect`: sizes
    /// from the slot (pre-armed frames); otherwise `nrows` / `ntiles` (the copy is always indirect).
    ///
    /// # Safety
    /// `enc` is recording; `bg0` is the current group 0.
    unsafe fn encode(&self, enc: &mut hal::metal::CommandEncoder, bc: &(Retained<ProtocolObject<dyn MTLBuffer>>, Retained<ProtocolObject<dyn MTLBuffer>>), k: usize, nrows: u32, ntiles: u32, indirect: bool) {
        let slot = self.slots[k].load(Relaxed) as u64;
        // SAFETY: see the function contract; every object is live and of this device.
        // The pass is the hal's; its bindings and dispatches go straight to the pass's Metal encoder (vendored
        // `raw_compute_encoder` / `raw_pipeline`): the same calls as the direct presenter, without the hal's
        // per-call binding bookkeeping (measured: +5..15 % instructions per frame through `set_bind_group` /
        // `set_compute_pipeline`). The buffer indices are the pipeline layout's (build.rs).
        unsafe {
            enc.begin_compute_pass(&hal::ComputePassDescriptor { label: None, timestamp_writes: None });
            let ce = enc.raw_compute_encoder().unwrap();
            let (bins, counts) = (&bc.0, &bc.1);
            ce.setBuffer_offset_atIndex(Some(&self.region_raw), 0, 0);
            ce.setBuffer_offset_atIndex(Some(bins), 0, 1);
            ce.setBuffer_offset_atIndex(Some(counts), 0, 2);
            ce.setBuffer_offset_atIndex(Some(&self.slot_raw[k]), 0, 3);
            let tgc = MTLSize { width: 32, height: 8, depth: 1 };
            let tg = MTLSize { width: 256, height: 1, depth: 1 };
            ce.setComputePipelineState(self.pso_copy.raw_pipeline());
            ce.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(&self.region_raw, slot as usize + 32, tgc);
            ce.setComputePipelineState(self.pso_bin.raw_pipeline());
            if indirect {
                ce.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(&self.region_raw, slot as usize, tg);
            } else {
                ce.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: nrows as usize, height: 1, depth: 1 }, tg);
            }
            ce.setComputePipelineState(self.pso_tile.raw_pipeline());
            if indirect {
                ce.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(&self.region_raw, slot as usize + 12, tg);
            } else {
                ce.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: ntiles as usize, height: 1, depth: 1 }, tg);
            }
            enc.end_compute_pass();
        }
    }

    /// Encode and commit the next `batch` frames: each waits for its release (`ev >= 2f-1`), runs slot `f % 2`
    /// with sizes read from the slot, and signals `ev = 2f` (event waits through the hal's raw command buffer).
    fn arm_locked(&self, st: &mut ArmState) {
        let batch = self.batch.load(Relaxed) as u64;
        let ev: &ProtocolObject<dyn MTLEvent> = ProtocolObject::from_ref(&*self.ev);
        let ArmState { enc, bc, .. } = st;
        // SAFETY: the encoder is idle (only armed under the lock); objects are live.
        let raw = unsafe {
            enc.begin_encoding(None).unwrap();
            let raw: Retained<ProtocolObject<dyn MTLCommandBuffer>> = Retained::from(enc.raw_command_buffer().unwrap());
            for f in st.armed_upto + 1..=st.armed_upto + batch {
                raw.encodeWaitForEvent_value(ev, 2 * f - 1);
                self.encode(enc, bc, (f % 2) as usize, 0, 0, true);
                raw.encodeSignalEvent_value(ev, 2 * f);
            }
            raw
        };
        // SAFETY: as above.
        unsafe {
            let cb = st.enc.end_encoding().unwrap();
            st.fence_value += 1;
            self.queue.submit(&[&cb], &[], (&mut st.fence, st.fence_value)).unwrap();
            st.enc.reset_all(std::iter::once(cb));
        }
        if st.batches.len() == 2 {
            st.batches.pop_front();
        }
        st.batches.push_back(raw);
        st.armed_upto += batch;
        self.armed.store(st.armed_upto, std::sync::atomic::Ordering::Release);
    }

    /// Make sure frames up to `upto` are armed (a lock-free read when they are).
    fn ensure(&self, upto: u64) {
        if self.armed.load(std::sync::atomic::Ordering::Acquire) >= upto {
            return;
        }
        let mut st = self.arm.lock().unwrap();
        while st.armed_upto < upto {
            self.arm_locked(&mut st);
        }
    }

    /// `reps` direct-sized frames of slot `k` in one command buffer on the aux queue, waited for; returns the
    /// command buffer's GPU time in ms (reference renders, bin-overflow redraws, kernel A/B).
    fn run_aux(&self, k: usize, nrows: u32, ntiles: u32, reps: usize) -> f64 {
        let mut st = self.arm.lock().unwrap();
        let ArmState { aux_enc, bc, .. } = &mut *st;
        // SAFETY: the aux encoder is idle (used only under the lock); objects are live; the caller synced.
        let raw = unsafe {
            aux_enc.begin_encoding(None).unwrap();
            let raw: Retained<ProtocolObject<dyn MTLCommandBuffer>> = Retained::from(aux_enc.raw_command_buffer().unwrap());
            for _ in 0..reps {
                self.encode(aux_enc, bc, k, nrows, ntiles, false);
            }
            raw
        };
        // SAFETY: as above.
        unsafe {
            let cb = st.aux_enc.end_encoding().unwrap();
            st.aux_value += 1;
            let v = st.aux_value;
            self.aux.submit(&[&cb], &[], (&mut st.aux_fence, v)).unwrap();
            self.dev.wait(&st.aux_fence, v, None).unwrap();
            st.aux_enc.reset_all(std::iter::once(cb));
        }
        (raw.GPUEndTime() - raw.GPUStartTime()) * 1e3
    }

    /// Group 0 over the current bins and counts (under the arm lock: no frame is being encoded).
    fn rebind(&self, bins: &Retained<ProtocolObject<dyn MTLBuffer>>, counts: &Retained<ProtocolObject<dyn MTLBuffer>>) {
        self.arm.lock().unwrap().bc = (bins.clone(), counts.clone());
    }
}


/// The pipelines of the hal frame path (kernels from the build-time metallib).
struct Pipes {
    pso_bin: hal::metal::ComputePipeline,
    pso_tile: hal::metal::ComputePipeline,
    pso_copy: hal::metal::ComputePipeline,
}

fn make_pipes(dev: &hal::metal::Device) -> Pipes {
    let buf = |binding: u32, ty: wgt::BufferBindingType| wgt::BindGroupLayoutEntry {
        binding,
        visibility: wgt::ShaderStages::COMPUTE,
        ty: wgt::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    };
    let st = |read_only| wgt::BufferBindingType::Storage { read_only };
    let e0 = [buf(0, st(true)), buf(1, st(false)), buf(2, st(false))];
    let e1 = [buf(0, wgt::BufferBindingType::Uniform)];
    // SAFETY: descriptors reference live objects of this device; build.rs mirrors the hal's index assignment.
    unsafe {
        let bgl0 = dev.create_bind_group_layout(&hal::BindGroupLayoutDescriptor { label: None, flags: hal::BindGroupLayoutFlags::empty(), entries: &e0 }).unwrap();
        let bgl1 = dev.create_bind_group_layout(&hal::BindGroupLayoutDescriptor { label: None, flags: hal::BindGroupLayoutFlags::empty(), entries: &e1 }).unwrap();
        let layout = dev.create_pipeline_layout(&hal::PipelineLayoutDescriptor { label: None, flags: hal::PipelineLayoutFlags::empty(), bind_group_layouts: &[Some(&bgl0), Some(&bgl1)], immediate_size: 0 }).unwrap();
        let module = |wg: (u32, u32, u32)| dev.create_shader_module(&hal::ShaderModuleDescriptor { label: None, runtime_checks: wgt::ShaderRuntimeChecks::unchecked() }, hal::ShaderInput::MetalLib { file: crate::METALLIB, num_workgroups: wg }).unwrap();
        let (m256, m32x8) = (module((256, 1, 1)), module((32, 8, 1)));
        let consts = Default::default();
        let pso = |m: &hal::metal::ShaderModule, entry: &str| {
            dev.create_compute_pipeline(&hal::ComputePipelineDescriptor {
                label: None,
                layout: &layout,
                stage: hal::ProgrammableStage { module: m, entry_point: entry, constants: &consts, zero_initialize_workgroup_memory: false },
                cache: None,
            })
            .unwrap()
        };
        let (pso_bin, pso_tile, pso_copy) = (pso(&m256, "fs_bin"), pso(&m256, "fs_tile"), pso(&m32x8, "fs_copy"));
        Pipes { pso_bin, pso_tile, pso_copy }
    }
}

// ------------------------------------------------------------------ the publisher thread

const NONE_SURF: usize = usize::MAX;

struct SendIo(CFRetained<IOSurfaceRef>);
// SAFETY: IOSurface objects are thread-safe (reference counted, used across processes).
unsafe impl Send for SendIo {}

struct PubJob {
    surf: usize,
    io: SendIo,
    /// Shared-event value that says the frame is drawn (0: already drawn, a held surface).
    wait: u64,
    /// A new layer frame (window resize), committed in the same transaction as the new contents.
    frame: Option<objc2_core_foundation::CGRect>,
    /// Address of the bin-overflow word (shared storage): a frame that overflowed is not published; the
    /// main thread's next `sync` redraws it and posts it again.
    overflow: usize,
    /// After the commit, keep the stream armed up to this frame (encode and commit off the input thread).
    ahead: u64,
}

struct PubShared {
    m: std::sync::Mutex<PubState>,
    cv: std::sync::Condvar,
    idle: std::sync::Condvar,
    /// The surface being committed now, and the surface the last commit put on screen: both count as in use
    /// (the compositor's own in-use flag lags a commit by up to one server turn).
    in_flight: std::sync::atomic::AtomicUsize,
    last: std::sync::atomic::AtomicUsize,
    queued: std::sync::atomic::AtomicUsize,
    commits: AtomicU64,
    held_back: AtomicU64,
}
struct PubState {
    job: Option<PubJob>,
    quit: bool,
}

/// Owns what the thread touches: the event it waits on and the layer it commits to.
struct PubObjs {
    kern: Arc<Kern>,
    layer: Retained<CALayer>,
}
// SAFETY: MTLSharedEvent is thread-safe; a CALayer may be modified from any thread inside an explicit
// CATransaction, and only this thread modifies this layer after the first publication.
unsafe impl Send for PubObjs {}

/// The publisher (RESULTS Part IV): a thread that waits for a released frame on the shared event and puts
/// its surface on the layer in its own explicit Core Animation transaction. The input (main) thread only
/// writes the slot, signals the event and posts the job: no GPU wait, no transaction. One job slot: a job
/// posted while another is queued supersedes it (frames are in order; the newer one shows everything).
struct Publisher {
    sh: Arc<PubShared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Publisher {
    fn spawn(kern: Arc<Kern>, layer: Retained<CALayer>) -> Publisher {
        use std::sync::atomic::AtomicUsize;
        let sh = Arc::new(PubShared {
            m: std::sync::Mutex::new(PubState { job: None, quit: false }),
            cv: std::sync::Condvar::new(),
            idle: std::sync::Condvar::new(),
            in_flight: AtomicUsize::new(NONE_SURF),
            last: AtomicUsize::new(NONE_SURF),
            queued: AtomicUsize::new(NONE_SURF),
            commits: AtomicU64::new(0),
            held_back: AtomicU64::new(0),
        });
        let (s2, objs) = (sh.clone(), PubObjs { kern, layer });
        let thread = std::thread::Builder::new()
            .name("bough-publish".into())
            .spawn(move || {
                let objs = objs;
                Publisher::run(&s2, &objs)
            })
            .expect("publisher thread");
        Publisher { sh, thread: Some(thread) }
    }

    fn run(sh: &PubShared, o: &PubObjs) {
        loop {
            let job = {
                let mut g = sh.m.lock().unwrap();
                loop {
                    if g.quit {
                        return;
                    }
                    if let Some(j) = g.job.take() {
                        sh.in_flight.store(j.surf, Relaxed);
                        sh.queued.store(NONE_SURF, Relaxed);
                        break j;
                    }
                    sh.idle.notify_all();
                    g = sh.cv.wait(g).unwrap();
                }
            };
            let ev = &o.kern.ev;
            let drawn = job.wait == 0 || ev.signaledValue() >= job.wait || ev.waitUntilSignaledValue_timeoutMS(job.wait, 5000);
            // SAFETY: the overflow word lies in the live counts buffer (the main thread replaces it only after
            // `publisher_idle`); shared storage, written by the GPU before it signalled the event.
            let overflowed = unsafe { std::ptr::read_volatile(job.overflow as *const u32) } != 0;
            if drawn && !overflowed {
                CATransaction::begin();
                CATransaction::setDisableActions(true);
                if let Some(f) = job.frame {
                    o.layer.setFrame(f);
                }
                // SAFETY: an IOSurfaceRef is a valid CALayer `contents` object.
                unsafe { o.layer.setContents(Some(&*(&*job.io.0 as *const IOSurfaceRef as *const AnyObject))) };
                CATransaction::commit();
                sh.last.store(job.surf, Relaxed);
                sh.commits.fetch_add(1, Relaxed);
            } else {
                sh.held_back.fetch_add(1, Relaxed);
            }
            sh.in_flight.store(NONE_SURF, Relaxed);
            // the next batch is encoded and committed here, between frames: never on the input thread
            o.kern.ensure(job.ahead);
        }
    }

    fn post(&self, job: PubJob) {
        let mut g = self.sh.m.lock().unwrap();
        self.sh.queued.store(job.surf, Relaxed);
        g.job = Some(job);
        self.sh.cv.notify_one();
    }

    /// Queued, being committed, or the last commit: the publisher (or a lagging compositor flag) holds it.
    fn holds(&self, i: usize) -> bool {
        self.sh.in_flight.load(Relaxed) == i || self.sh.last.load(Relaxed) == i || self.sh.queued.load(Relaxed) == i
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        self.sh.m.lock().unwrap().quit = true;
        self.sh.cv.notify_one();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub struct Fused {
    pub device: Dev,
    /// The frame stream's queue: always holds a pre-armed batch waiting on `ev`.
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    /// Reference renders, bin-overflow redraws and keep-warm blits: never queued behind the waiting stream.
    aux: Retained<ProtocolObject<dyn MTLCommandQueue>>,
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
    scale: f64,
    /// Publish drawn surfaces to `layer` (windowed); off headless.
    pub publish: bool,
    lay: Layout,
    resid: Option<Retained<ProtocolObject<dyn MTLResidencySet>>>,
    warm: Option<Arc<Warm>>,
    prev_frame_at: f64,
    scratch: Option<Surface>,
    pub stats: Stats,
    /// The last GPU frame: (slot, target, rows, tiles), redrawn if its bins overflowed.
    last_frame: Option<(usize, usize, u32, u32)>,
    pub bin_growths: u32,
    // the frame stream (pre-armed command buffers released by `ev`)
    ev: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    /// Encoder objects and arming state, shared with the publisher thread.
    kern: Arc<Kern>,
    released: u64,
    /// The publisher thread (windowed: spawned at the first publication).
    publisher: Option<Publisher>,
    /// A layer frame to commit with the next publication (set by `resize`).
    pending_frame: Option<objc2_core_foundation::CGRect>,
    /// A clock-driven frame drawn but not yet committed (published at the next `sync`).
    pending: Option<usize>,
    /// The surface the last calling-thread commit put on screen.
    last_main: usize,
    /// The region's memory when it was adopted (`Fused::adopt`): the LAST field, so it is freed after the
    /// buffer over it and everything that encodes into it.
    _mem: Option<Arc<RegionMem>>,
}

/// Page-aligned, zero-filled memory for the region, allocated before the device exists (`Fused::adopt`).
pub struct RegionMem {
    pub ptr: *mut u8,
    pub len: usize,
}
// SAFETY: plain memory; access is governed by the region's write rule.
unsafe impl Send for RegionMem {}
unsafe impl Sync for RegionMem {}
impl RegionMem {
    pub fn new(len: usize) -> RegionMem {
        let len = len.next_multiple_of(16384).max(16384);
        // SAFETY: anonymous private mapping; zero-filled by the kernel and lazily backed.
        let p = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANON, -1, 0) };
        assert!(p != libc::MAP_FAILED, "region mmap");
        RegionMem { ptr: p as *mut u8, len }
    }
}
impl Drop for RegionMem {
    fn drop(&mut self) {
        // SAFETY: mapped in `new` with this length; the buffer over it was released before (field order).
        unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
    }
}

enum Pool {
    New(usize),
    Adopt(Arc<RegionMem>),
}

/// What a presenter needs that does not depend on the window or the region: the device (the OS's driver load,
/// ~22 ms cold, RESULTS.md), the queues and the pipelines. `Boot::spawn` makes it on another thread at the top of
/// `App::new`, so it overlaps mounting, layout and shaping.
pub struct Boot {
    device: Dev,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    aux: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    hdev: hal::metal::Device,
    pipes: Pipes,
}
// SAFETY: Metal devices, queues, pipelines and wgpu-hal's Metal objects are thread-safe; a `Boot` is moved once.
unsafe impl Send for Boot {}

impl Boot {
    pub fn new() -> Boot {
        let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
        let queue = device.newCommandQueue().expect("queue");
        let aux = device.newCommandQueue().expect("aux queue");
        // SAFETY: a live MTLDevice (no instance, no adapter enumeration, no validation layer).
        let hdev = unsafe { hal::metal::Device::device_from_raw(device.clone(), wgt::Features::empty()) };
        let pipes = make_pipes(&hdev);
        Boot { device, queue, aux, hdev, pipes }
    }
    pub fn spawn() -> std::thread::JoinHandle<Boot> {
        std::thread::Builder::new().name("bough-boot".into()).spawn(Boot::new).expect("boot thread")
    }
}

/// Byte offset of the bind table in the region header (`FBind` in shader.metal).
pub const FBIND_OFF: usize = 128;
/// The bind table: 4 target surfaces, the atlas (resource ids), bins and counts (GPU addresses).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FBind {
    t: [u64; 4],
    atlas: u64,
    bins: u64,
    counts: u64,
    /// The same 4 surfaces as read handles (the scroll copy's source); appended (round 3c), table = 216 B.
    r: [u64; 4],
}
const _: () = assert!(size_of::<FHdr>() <= FBIND_OFF && FBIND_OFF + size_of::<FBind>() <= HDR_BYTES);

impl Fused {
    /// A presenter for a `w` x `h` px window whose region is a `region_len`-byte shared buffer.
    pub fn new(w: usize, h: usize, scale: f64, region_len: usize, surfaces: usize, bin_cap: u32) -> Fused {
        Self::from_boot(Boot::new(), w, h, scale, Pool::New(region_len), surfaces, bin_cap)
    }

    /// The presenter over memory the CPU already filled (`Boot` + `RegionMem`): the region becomes a no-copy
    /// shared MTLBuffer over `mem`, so the view can be mounted, laid out and shaped into the region while the
    /// device boots on another thread (`Boot::spawn`), and nothing is copied when they meet.
    pub fn adopt(boot: Boot, w: usize, h: usize, scale: f64, mem: Arc<RegionMem>, surfaces: usize, bin_cap: u32) -> Fused {
        Self::from_boot(boot, w, h, scale, Pool::Adopt(mem), surfaces, bin_cap)
    }

    fn from_boot(boot: Boot, w: usize, h: usize, scale: f64, region: Pool, surfaces: usize, bin_cap: u32) -> Fused {
        let Boot { device, queue, aux, hdev, pipes } = boot;
        let (pool, region_len, mem) = match region {
            Pool::New(len) => (device.newBufferWithLength_options(len, MTLResourceOptions::StorageModeShared).expect("region buffer"), len, None),
            Pool::Adopt(m) => {
                // SAFETY: `m` is page-aligned, page-multiple, zero-initialised memory that outlives the buffer
                // (the presenter keeps `m`); no deallocator: `RegionMem` frees it.
                let b = unsafe {
                    device.newBufferWithBytesNoCopy_length_options_deallocator(NonNull::new(m.ptr as *mut std::ffi::c_void).unwrap(), m.len, MTLResourceOptions::StorageModeShared, None)
                }
                .expect("no-copy region buffer");
                (b, m.len, Some(m))
            }
        };
        let pool_ptr = pool.contents().as_ptr() as *mut u8;
        let atlas = Self::new_atlas(&device, 1024, 512);
        let ty = h.div_ceil(32);
        let bins = device.newBufferWithLength_options(ty * bin_cap as usize * 16, MTLResourceOptions::StorageModePrivate).expect("bins");
        let counts = device.newBufferWithLength_options((ty + 1) * 4, MTLResourceOptions::StorageModeShared).expect("bin counts");
        let surfs = (0..surfaces.clamp(1, MAX_SURFACES)).map(|_| Self::new_surface(&device, w, h)).collect();
        let device_ev = device.newSharedEvent().expect("shared event");
        // SAFETY: live queues of this device.
        let (hq, haux) = unsafe { (hal::metal::Queue::queue_from_raw(queue.clone(), 1.0), hal::metal::Queue::queue_from_raw(aux.clone(), 1.0)) };
        let slot_raw: [Retained<ProtocolObject<dyn MTLBuffer>>; 3] = std::array::from_fn(|_| device.newBufferWithLength_options(16, MTLResourceOptions::StorageModeShared).expect("slot uniform"));
        // SAFETY: device-owned objects.
        let (enc, fence, aux_enc, aux_fence) = unsafe {
            (
                hdev.create_command_encoder(&hal::CommandEncoderDescriptor { label: None, queue: &hq }).unwrap(),
                hdev.create_fence().unwrap(),
                hdev.create_command_encoder(&hal::CommandEncoderDescriptor { label: None, queue: &haux }).unwrap(),
                hdev.create_fence().unwrap(),
            )
        };
        let kern = Arc::new(Kern {
            dev: hdev,
            queue: hq,
            aux: haux,
            ev: device_ev.clone(),
            region_raw: pool.clone(),
            pso_bin: pipes.pso_bin,
            pso_tile: pipes.pso_tile,
            pso_copy: pipes.pso_copy,
            slot_raw,
            slots: Default::default(),
            batch: std::sync::atomic::AtomicUsize::new(16),
            armed: AtomicU64::new(0),
            arm: std::sync::Mutex::new(ArmState {
                armed_upto: 0,
                batches: std::collections::VecDeque::with_capacity(3),
                enc,
                fence,
                fence_value: 0,
                aux_enc,
                aux_fence,
                aux_value: 0,
                bc: (bins.clone(), counts.clone()),
            }),
        });
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
            scale,
            publish: false,
            lay: Layout::default(),
            resid: None,
            warm: None,
            prev_frame_at: f64::NEG_INFINITY,
            scratch: None,
            stats: Stats::default(),
            last_frame: None,
            bin_growths: 0,
            kern: kern.clone(),
            ev: device_ev,
            released: 0,
            publisher: None,
            pending_frame: None,
            pending: None,
            last_main: NONE_SURF,
            _mem: mem,
        };
        // the residency set is what makes the bind table's resources reachable (encoders bind nothing else)
        {
            let d = MTLResidencySetDescriptor::new();
            unsafe { d.setInitialCapacity(8) };
            let set = f.device.newResidencySetWithDescriptor_error(&d).expect("residency set");
            set.addAllocation(ProtocolObject::from_ref(&*f.pool));
            set.addAllocation(ProtocolObject::from_ref(&*f.atlas));
            set.addAllocation(ProtocolObject::from_ref(&*f.bins));
            set.addAllocation(ProtocolObject::from_ref(&*f.counts));
            set.commit();
            set.requestResidency();
            f.queue.addResidencySet(&set);
            f.aux.addResidencySet(&set);
            f.resid = Some(set);
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
                queue: f.aux.clone(),
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
            masks: [l.masks.inst, l.masks.page, l.masks.glyph, l.masks.run, l.masks.space, l.masks.clip, l.masks.strike],
        };
        // SAFETY: the region starts with HDR_BYTES reserved for the header; no GPU work reads it now.
        unsafe { (self.pool_ptr as *mut FHdr).write(h) };
        for (k, &o) in l.slot.iter().enumerate() {
            self.kern.slots[k].store(o, Relaxed);
            // SAFETY: the slot uniform is 16 bytes of shared storage; the GPU is idle.
            unsafe { *(self.kern.slot_raw[k].contents().as_ptr() as *mut u32) = o };
        }
        self.write_binds();
        self.kern.ensure(self.released + 1);
    }

    /// Write the bind table (resource ids of the surfaces and the atlas, GPU addresses of bins and counts).
    /// Only while the GPU is idle (`sync`), like every other header write.
    fn write_binds(&mut self) {
        let n = self.surfs.len();
        let t = [rid(&self.surfs[0].tex), rid(&self.surfs[1 % n].tex), rid(&self.surfs[2 % n].tex), rid(&self.surfs[3 % n].tex)];
        let b = FBind { t, atlas: rid(&self.atlas), bins: self.bins.gpuAddress(), counts: self.counts.gpuAddress(), r: t };
        // SAFETY: the bind table lies inside the reserved header (const-asserted); the GPU is idle.
        unsafe { (self.pool_ptr.add(FBIND_OFF) as *mut FBind).write(b) };
    }

    /// Wait for the GPU's last frame (the CPU is about to write the region); redraw it if a bin overflowed.
    /// When the GPU is done (the common case) this is one read of the event's shared value.
    #[inline]
    pub fn sync(&mut self) {
        let want = 2 * self.released;
        if self.ev.signaledValue() < want && !self.ev.waitUntilSignaledValue_timeoutMS(want, 5000) {
            let err = self.kern.arm.lock().unwrap().batches.iter().map(|c| format!("{:?} {:?}", c.status(), c.error())).collect::<Vec<_>>();
            panic!("GPU frame {} did not finish within 5 s (stream batches: {err:?})", self.released);
        }
        self.check_bins();
        if let Some(i) = self.pending.take() {
            // a clock-driven frame, drawn (and repaired if a bin overflowed): on screen now
            self.commit_now(i);
        }
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
        let Some((k, _target, nr, nt)) = self.last_frame else { return };
        // the publisher must have read the overflow word (and held the frame back) before it is cleared
        self.publisher_idle();
        // SAFETY: `counts` holds ty + 1 u32s in shared storage; the GPU is idle.
        unsafe { *(self.counts.contents().as_ptr() as *mut u32).add(ty) = 0 };
        // The bins are named only by the bind table: growing them is a new buffer and a header write, and the
        // pre-armed frames read the new address when they run. The overflowed frame is redrawn from its slot
        // (which names the target) on the aux queue before anyone sees it.
        let cap = need.next_power_of_two().max(self.bin_cap * 2);
        // growing the bins drains the pre-armed frames (`residency_changed`), which reuses both slots: keep
        // the overflowed frame's
        let sb = Layout::slot_bytes(self.lay.w, self.lay.h);
        let mut keep = vec![0u8; sb];
        // SAFETY: slot k lies inside the region (reserved for at least `sb` bytes); the GPU is idle.
        unsafe { std::ptr::copy_nonoverlapping(self.pool_ptr.add(self.lay.slot[k] as usize), keep.as_mut_ptr(), sb) };
        self.replace_bins(cap);
        // SAFETY: as above.
        unsafe { std::ptr::copy_nonoverlapping(keep.as_ptr(), self.pool_ptr.add(self.lay.slot[k] as usize), sb) };
        self.bin_growths += 1;
        self.kern.run_aux(k, nr, nt, 1);
        self.check_bins();
        if self.publish {
            // the publisher held the overflowed frame back: it is whole now
            self.post(_target, 0);
        }
    }

    /// New row bins of `cap` entries per row, named in the bind table (the GPU is idle).
    fn replace_bins(&mut self, cap: u32) {
        let ty = self.lay.tiles_y() as usize;
        let old = std::mem::replace(&mut self.bins, self.device.newBufferWithLength_options(ty * cap as usize * 16, MTLResourceOptions::StorageModePrivate).expect("bins"));
        if let Some(r) = &self.resid {
            r.removeAllocation(ProtocolObject::from_ref(&*old));
            r.addAllocation(ProtocolObject::from_ref(&*self.bins));
            r.commit();
        }
        self.bin_cap = cap;
        self.kern.rebind(&self.bins, &self.counts);
        // SAFETY: header field `bin_cap`; the GPU is idle.
        unsafe { (*(self.pool_ptr as *mut FHdr)).bin_cap = cap };
        self.write_binds();
        self.residency_changed();
    }

    /// Replace the row bins by ones of `cap` entries per row (tests: forces the grow-and-redraw path).
    pub fn reset_bins(&mut self, cap: u32) {
        self.sync();
        self.replace_bins(cap);
    }

    /// In use by the compositor, or held by the publisher (queued, being committed, or committed last).
    pub fn is_in_use(&self, i: usize) -> bool {
        self.publisher.as_ref().is_some_and(|p| p.holds(i)) || self.last_main == i || self.pending == Some(i) || self.surfs[i].io.is_in_use()
    }

    /// The window is now `w` x `h` px: fresh surfaces of that size (unbacked until first drawn), row bins
    /// and counts for its tile rows, and the layer's new frame committed with the next publication, so the
    /// first frame at the new size and the new size reach the screen in one transaction (no frame dropped,
    /// none shown stretched). The caller then writes the new layout (`set_layout`) and draws a full frame.
    /// The surface on screen stays alive in the layer until it is replaced.
    pub fn resize(&mut self, w: usize, h: usize) {
        self.sync();
        self.publisher_idle();
        if (w, h) == (self.w, self.h) {
            return;
        }
        let n = self.surfs.len();
        let old = std::mem::replace(&mut self.surfs, (0..n).map(|_| Self::new_surface(&self.device, w, h)).collect());
        let ty = h.div_ceil(32);
        let bins = self.device.newBufferWithLength_options(ty * self.bin_cap as usize * 16, MTLResourceOptions::StorageModePrivate).expect("bins");
        let counts = self.device.newBufferWithLength_options((ty + 1) * 4, MTLResourceOptions::StorageModeShared).expect("bin counts");
        let (old_bins, old_counts) = (std::mem::replace(&mut self.bins, bins), std::mem::replace(&mut self.counts, counts));
        if let Some(r) = &self.resid {
            for s in old.iter().filter(|s| s.touched.get()) {
                r.removeAllocation(ProtocolObject::from_ref(&*s.tex));
            }
            r.removeAllocation(ProtocolObject::from_ref(&*old_bins));
            r.removeAllocation(ProtocolObject::from_ref(&*old_counts));
            r.addAllocation(ProtocolObject::from_ref(&*self.bins));
            r.addAllocation(ProtocolObject::from_ref(&*self.counts));
            r.commit();
        }
        self.kern.rebind(&self.bins, &self.counts);
        self.residency_changed();
        self.stats.resident_surfaces = 0;
        self.last_frame = None;
        self.last_main = NONE_SURF;
        self.scratch = None; // the reference render's surface follows too
        if let Some(p) = &self.publisher {
            p.sh.last.store(NONE_SURF, Relaxed); // the old surfaces are gone; none of the new ones is on screen
        }
        (self.w, self.h) = (w, h);
        self.pending_frame = Some(objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(w as f64 / self.scale, h as f64 / self.scale),
        ));
        if !self.publish {
            self.layer.setFrame(self.pending_frame.take().unwrap());
        }
        self.write_binds();
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
            self.write_binds();
            self.residency_changed();
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
            self.residency_changed();
        }
        purged
    }

    /// An allocation joined the residency set: frames already submitted were made resident without it (the
    /// set is resolved when a command buffer is submitted, and the pre-armed batch is submitted long before
    /// its frames run), so the pending frames are drained (released with empty slots) and a new batch is
    /// armed. Rare: a surface's first use, a resize, a bin or atlas growth.
    fn residency_changed(&mut self) {
        let want = 2 * self.released;
        if self.ev.signaledValue() < want {
            self.ev.waitUntilSignaledValue_timeoutMS(want, 5000);
        }
        // the arming lock is held throughout: the publisher cannot arm frames in between
        let kern = self.kern.clone();
        let mut st = kern.arm.lock().unwrap();
        for f in self.released + 1..=st.armed_upto {
            // SAFETY: slot f % 2 lies in the region; frame f - 1 (the other slot) is done or waited below.
            unsafe { write_slot(self.pool_ptr, &self.lay, (f % 2) as usize, 0, &[], false, &SlotCopy::NONE) };
            self.ev.setSignaledValue(2 * f - 1);
            self.ev.waitUntilSignaledValue_timeoutMS(2 * f, 5000);
        }
        self.released = st.armed_upto;
        self.stats.drains += 1;
        kern.arm_locked(&mut st);
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

    fn write_slot(&self, k: usize, target: u32, rows: &[u128], full: bool, copy: &SlotCopy) -> (u32, u32) {
        // SAFETY: the region is this presenter's buffer, laid out by `lay`; the GPU is idle (sync()).
        unsafe { write_slot(self.pool_ptr, &self.lay, k, target, rows, full, copy) }
    }

    /// Repair surface `target` from the damaged tile rows (`full`: everything) with `n_dir` directory
    /// entries, commit, and (windowed) publish it once the GPU is done. Returns the tile count.
    /// `PACED`: a clock-driven turn: published at the next `sync` on the calling thread (one refresh later,
    /// no thread wake, no completion wait); otherwise the publisher thread commits it in this refresh.
    /// `copy`: a scroll copy from another surface first (RESULTS §15; `SlotCopy::NONE` for none).
    /// `draw` for a caller that knows the first tile row that may be set (bough-metal `Fused::draw_in`); this
    /// presenter's slot writer visits every row of `rows`.
    pub fn draw_in<const PACED: bool>(&mut self, target: usize, rows: &[u128], full: bool, n_dir: u32, copy: &SlotCopy, _y0: usize) -> u32 {
        self.draw::<PACED>(target, rows, full, n_dir, copy)
    }

    pub fn draw<const PACED: bool>(&mut self, target: usize, rows: &[u128], full: bool, n_dir: u32, copy: &SlotCopy) -> u32 {
        self.sync();
        // a bindless frame reaches its target only through the residency set
        if !self.surfs[target].touched.get() {
            self.prepare_target(target);
        }
        let k = ((self.released + 1) % 2) as usize;
        // SAFETY: header field `n_dir` (u32) of the region's header; the GPU is idle (sync()).
        unsafe { (*(self.pool_ptr as *mut FHdr)).n_dir = n_dir };
        // the slot names the target; the pre-encoded frame names none
        debug_assert!(copy.is_none() || (copy.src as usize != target && self.surfs[copy.src as usize].touched.get()));
        let (nt, _) = self.write_slot(k, target as u32, rows, full, copy);
        self.release(k, nt);
        if self.publish {
            if PACED {
                self.pending = Some(target);
            } else {
                // the publisher thread waits for the frame and commits it: no GPU wait and no Core Animation
                // transaction on this (input) thread
                let wait = 2 * self.released;
                self.post(target, wait);
            }
        }
        nt
    }

    /// Commit surface `i` now, on the calling thread, in its own transaction (after anything the publisher
    /// still holds, so commits stay in frame order).
    fn commit_now(&mut self, i: usize) {
        self.publisher_idle();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if let Some(f) = self.pending_frame.take() {
            self.layer.setFrame(f);
        }
        // SAFETY: an IOSurfaceRef is a valid CALayer `contents` object.
        unsafe { self.layer.setContents(Some(&*(&*self.surfs[i].io as *const IOSurfaceRef as *const AnyObject))) };
        CATransaction::commit();
        self.last_main = i;
        if let Some(p) = &self.publisher {
            p.sh.last.store(i, Relaxed);
        }
    }

    /// Hand surface `i` to the publisher thread, to be put on screen once `ev >= wait` (0: now).
    fn post(&mut self, i: usize, wait: u64) {
        let frame = self.pending_frame.take();
        let overflow = self.counts.contents().as_ptr() as usize + 4 * self.lay.tiles_y() as usize;
        let ahead = self.released + self.kern.batch.load(Relaxed) as u64 / 2;
        let job = PubJob { surf: i, io: SendIo(self.surfs[i].io.clone()), wait, frame, overflow, ahead };
        self.publisher.get_or_insert_with(|| Publisher::spawn(self.kern.clone(), self.layer.clone())).post(job);
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
    pub fn show_held<const PACED: bool>(&mut self, i: usize) {
        self.stats.ca_only += 1;
        if let Some(w) = &self.warm {
            w.cancelled.store(true, Relaxed);
        }
        if self.publish {
            self.pending = None;
            if PACED {
                self.commit_now(i);
            } else {
                self.post(i, 0);
            }
        }
    }

    /// Publisher counters: (transactions committed, frames held back by a bin overflow).
    pub fn publish_stats(&self) -> (u64, u64) {
        self.publisher.as_ref().map_or((0, 0), |p| (p.sh.commits.load(Relaxed), p.sh.held_back.load(Relaxed)))
    }

    /// Wait until the publisher has nothing queued or in flight (resize, tests).
    pub fn publisher_idle(&self) {
        if let Some(p) = &self.publisher {
            let mut g = p.sh.m.lock().unwrap();
            while g.job.is_some() || p.sh.in_flight.load(Relaxed) != NONE_SURF {
                g = p.sh.idle.wait(g).unwrap();
            }
        }
    }

    // ------------------------------------------------------------------ the frame stream

    /// Frames per pre-armed command buffer (default 16); takes effect at the next batch.
    pub fn set_batch(&mut self, k: usize) {
        self.kern.batch.store(k.max(2), Relaxed);
    }

    fn release(&mut self, k: usize, nt: u32) {
        let f = self.released + 1;
        // SAFETY: slot k's header (u32 x 8) lies inside the region, just written; the GPU is idle.
        let (target, nr) = unsafe {
            let s = self.pool_ptr.add(self.lay.slot[k] as usize) as *const u32;
            (*s.add(6) as usize, *s.add(7))
        };
        // a redraw after a bin overflow re-encodes this frame from its slot
        self.last_frame = Some((k, target, nr, nt));
        debug_assert_eq!((f % 2) as usize, k);
        // normally armed long ago (one atomic read); only a stream that outran its arming arms here
        self.kern.ensure(f);
        self.released = f;
        self.ev.setSignaledValue(2 * f - 1);
        self.after_gpu_frame(nt);
        if self.publisher.is_none() {
            // headless: the next batch is encoded and committed here, after the release (off the frame's
            // critical path); windowed, the publisher thread does it between frames
            self.kern.ensure(f + self.kern.batch.load(Relaxed) as u64 / 2);
        }
    }

    /// Has the last released frame finished on the GPU? (one read of the event's shared value)
    pub fn released_done(&self) -> bool {
        self.ev.signaledValue() >= 2 * self.released
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
        // the scratch surface takes the bind table's target 0 for this one render (the GPU is idle)
        let s = self.scratch.as_ref().unwrap().tex.clone();
        if let Some(r) = &self.resid {
            r.addAllocation(ProtocolObject::from_ref(&*s));
            r.commit();
        }
        // SAFETY: the bind table lies inside the reserved header; the GPU is idle (synced above).
        unsafe { (*(self.pool_ptr.add(FBIND_OFF) as *mut FBind)).t[0] = rid(&s) };
        let (nt, nr) = self.write_slot(2, 0, &[], true, &SlotCopy::NONE);
        self.kern.run_aux(2, nr, nt, 1);
        self.write_binds();
        if let Some(r) = &self.resid {
            r.removeAllocation(ProtocolObject::from_ref(&*s));
            r.commit();
        }
        read_io(&self.scratch.as_ref().unwrap().io, self.w, self.h)
    }

    /// GPU time of one frame's repair (`rows` / `full` into surface `target`), measured as one command
    /// buffer of `reps` back-to-back copies on the aux queue: (GPUEndTime - GPUStartTime) / reps, in ms.
    /// The GPU is saturated, so its clock is high and steady (kernel A/B, not a frame cost). `debug` goes to
    /// the header's debug word for the run (kernel variants), then back to 0.
    /// `copy`: a scroll copy first (RESULTS §15; `SlotCopy::NONE` for none).
    #[allow(clippy::too_many_arguments)]
    pub fn bench_frames(&mut self, target: usize, rows: &[u128], full: bool, n_dir: u32, reps: usize, debug: u32, copy: &SlotCopy) -> f64 {
        self.sync();
        self.prepare_target(target);
        // SAFETY: header fields `n_dir`, `debug`; the GPU is idle (synced above).
        unsafe {
            (*(self.pool_ptr as *mut FHdr)).n_dir = n_dir;
            (*(self.pool_ptr as *mut FHdr)).debug = debug;
        }
        if !copy.is_none() {
            self.prepare_target(copy.src as usize);
        }
        let (nt, nr) = self.write_slot(2, target as u32, rows, full, copy);
        let ms = self.kern.run_aux(2, nr, nt, reps);
        // SAFETY: as above; the GPU is idle again.
        unsafe { (*(self.pool_ptr as *mut FHdr)).debug = 0 };
        ms / reps as f64
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

/// A texture's Metal 3 resource id, as the shader's bind table holds it.
fn rid(t: &Tex) -> u64 {
    // SAFETY: MTLResourceID is a 64-bit handle (`{ _impl: u64 }`, repr(C)).
    unsafe { std::mem::transmute::<MTLResourceID, u64>(t.gpuResourceID()) }
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
