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
//! * **Submission: one pre-armed stream for every frame** (RESULTS Part IV). K frames are encoded ahead into
//!   one command buffer, each `wait(ev >= 2f-1) -> bin, tile -> signal(ev = 2f)`, and committed long before
//!   they run. A frame is: write its slot, `ev.signaledValue = 2f-1` (the GPU starts at once: no encoder,
//!   no commit, no submission on the critical path). Encoders are **bindless**: they bind only the region
//!   and the slot offset; surfaces, atlas, bins and counts are Metal 3 resource ids / GPU addresses in the
//!   header's bind table (`FBIND_OFF`), all in the queue's residency set. So a pending frame names no
//!   surface (IOSurfaceIsInUse reports only the compositor) and growing the bins is a header write.
//! * **Publish (windowed): the publisher thread.** The input thread only writes the slot, signals the event
//!   and posts the surface; the publisher waits for `ev >= 2f`, commits the surface in its own explicit
//!   Core Animation transaction, and keeps the stream armed (`Kern`). No GPU wait, CA transaction, encode or
//!   commit on the input thread, and no added latency. The CPU never writes the region while the GPU reads
//!   it (`sync` is a 72-instruction read of the event when the GPU is done).
//! * **Resize:** fresh surfaces, bins and counts at the new size; the layer's new frame is committed with the
//!   first frame drawn at that size.
use crate::gpu::{Dev, Tex};
use crate::instance::Instance;
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
    /// Per space: the affine of a transformed subtree (`[f32; 8]`: M - I, pivot, translation; all zero is
    /// the identity), indexed like `spaces` (round 3c, input agent).
    pub xforms_off: u32,
    /// Three frame slots (two in flight, one for the reference render): header + damaged-row records.
    pub slot: [u32; 3],
    /// Capacity masks (capacity - 1, every column's capacity a power of two): the kernels AND every index
    /// they read from region data with its column's mask, so no value in the region can make the GPU read
    /// outside the column (bough-metal RESULTS Part IV). `Layout::default()` (probes: empty scene) masks to 0.
    /// With column views (`Fused::new_views`) the masks the kernels see are the views' committed capacities.
    pub masks: Masks,
    /// The GPU-read columns (order `COL_*`): where each starts, its element size, its mask group and the bytes
    /// the region reserves for it. A presenter with column views wraps each one's used prefix.
    pub cols: [ColumnDesc; NCOLS],
}

/// One GPU-read column of the region (`Layout::cols`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColumnDesc {
    /// Byte offset in the region (a VM page boundary when the presenter makes views).
    pub off: u32,
    pub elem: u32,
    /// Bytes reserved for the column (its carved capacity).
    pub reserve: u32,
    /// Mask group (`G_*`): the columns indexed by one index share one mask and so one capacity.
    pub group: u8,
}

/// The column table's order (`FCols` in shader.metal).
pub const COL_INST: usize = 0;
pub const COL_SPACES: usize = 1;
pub const COL_CLIPS: usize = 2;
pub const COL_DIR_ID: usize = 3;
pub const COL_DIR_INK: usize = 4;
pub const COL_PAGES: usize = 5;
pub const COL_GX: usize = 6;
pub const COL_GENT: usize = 7;
pub const COL_RUNS: usize = 8;
pub const COL_TOKS: usize = 9;
pub const COL_XFORMS: usize = 10;
pub const NCOLS: usize = 11;
/// Mask groups, in `Masks` order: an index read from region data is ANDed with its group's mask, so every
/// column of a group must hold the group's capacity.
pub const G_INST: usize = 0;
pub const G_PAGE: usize = 1;
pub const G_GLYPH: usize = 2;
pub const G_RUN: usize = 3;
pub const G_SPACE: usize = 4;
pub const G_CLIP: usize = 5;
pub const G_STRIKE: usize = 6;
pub const NGROUPS: usize = 7;

/// Power-of-two capacity minus one, per GPU-read column.
#[derive(Clone, Copy, Debug, Default)]
pub struct Masks {
    pub inst: u32,
    pub page: u32,
    pub glyph: u32,
    pub run: u32,
    pub space: u32,
    pub clip: u32,
    pub strike: u32,
}

impl Layout {
    pub fn tiles_x(&self) -> u32 {
        self.w.div_ceil(32)
    }
    pub fn tiles_y(&self) -> u32 {
        self.h.div_ceil(32)
    }
    /// Bytes of one frame slot for a `w`-by-`h` window: a 64-byte header (`FSlot`) and one 32-byte record per
    /// tile row (`FRow`, at +64).
    pub fn slot_bytes(_w: u32, h: u32) -> usize {
        (SLOT_HDR + 32 * h.div_ceil(32) as usize).next_multiple_of(256)
    }
}

/// Bytes of a surface's certificate plane: one 72-byte `FCert` per 32 px tile.
fn cert_bytes(w: usize, h: usize) -> usize {
    w.div_ceil(32) * h.div_ceil(32) * 72
}

/// `SlotCopy::src` bit: the copy leaves the destination's band certificates as they are (kernel timing).
pub const CERT_FROZEN: u32 = 0x100;

/// Bytes of a frame slot's header (`FSlot`): the three dispatches' indirect args, target, rows, the copy.
pub const SLOT_HDR: usize = 64;

/// Scroll by copy (RESULTS §15): this frame first copies `rect` (target px, `[x0, y0, x1, y1)`, empty = no
/// copy) from surface `src` at `p - (dx, dy)` into the target, skipping the tiles the repair writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotCopy {
    pub src: u32,
    pub rect: [i32; 4],
    pub dx: i32,
    pub dy: i32,
}
impl SlotCopy {
    pub const NONE: SlotCopy = SlotCopy { src: 0, rect: [0; 4], dx: 0, dy: 0 };
    pub fn is_none(&self) -> bool {
        self.rect[0] >= self.rect[2] || self.rect[1] >= self.rect[3]
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
    masks: [u32; 7],
    xforms_off: u32,
}
/// Bytes reserved for the header at the start of the region: `FHdr` @0, the bind table @`FBIND_OFF`, the
/// column table @`FCOL_OFF`.
pub const HDR_BYTES: usize = 512;
const _: () = assert!(size_of::<FHdr>() <= HDR_BYTES);
/// Byte offset of the column table (`FCols` in shader.metal): each GPU-read column's GPU address. The
/// kernels read every column through it (round 6, lane memory): a column is either a range of the one region
/// buffer (`Fused::new`, `new_on`) or its own no-copy view of the region's memory (`new_views`).
pub const FCOL_OFF: usize = 256;
const _: () = assert!(FCOL_OFF + 8 * NCOLS <= HDR_BYTES);
const _: () = assert!(size_of::<Instance>() == 48);

pub struct Surface {
    pub io: CFRetained<IOSurfaceRef>,
    pub tex: Tex,
    /// The surface's row certificates (`FCert` in shader.metal): per 32 x 4 px band a colour and a mask of
    /// its rows known to hold it; zero = nothing known. Written by the kernels with the pixels; zeroed here whenever the pixels may have changed
    /// without them (a trim, a purge).
    cert: Retained<ProtocolObject<dyn MTLBuffer>>,
    /// Written at least once (an untouched IOSurface owns no pages).
    pub touched: Cell<bool>,
    /// Marked purgeable-volatile by a calm trim (made non-volatile before the next draw).
    pub volatile: Cell<bool>,
    /// Written by the CPU (`write_begin`) at least once: its pages exist although the GPU never drew it.
    pub cpu_written: Cell<bool>,
}

pub fn make_iosurface(w: usize, h: usize) -> CFRetained<IOSurfaceRef> {
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

// ------------------------------------------------------------------ the frame stream's encoder

/// The three pipelines of one generation of the kernels (`fs_bin`, `fs_tile`, `fs_copy`): the presenter's own,
/// or the frozen original ones the gates compare against (`oracle.metal`).
struct Psos {
    bin: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    tile: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    copy: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
}

impl Psos {
    fn new(device: &Dev, lib: &ProtocolObject<dyn MTLLibrary>) -> Psos {
        let pso = |name: &NSString| {
            let f = lib.newFunctionWithName(name).expect("kernel");
            device.newComputePipelineStateWithFunction_error(&f).expect("compute pso")
        };
        Psos { bin: pso(ns_string!("fs_bin")), tile: pso(ns_string!("fs_tile")), copy: pso(ns_string!("fs_copy")) }
    }
}

/// What encoding a frame needs, shared by the main thread and the publisher thread (which keeps the stream
/// armed, so no encode or commit runs on the input thread): the stream queue and event, the region, both
/// pipelines, the slot offsets, and the arming state behind one lock (two threads never arm overlapping
/// frames).
struct Kern {
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    ev: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    pool: Retained<ProtocolObject<dyn MTLBuffer>>,
    psos: Psos,
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
}
// SAFETY: Metal queues, events, buffers, pipelines and command buffers are thread-safe objects; the arming
// state is behind a mutex.
unsafe impl Send for Kern {}
unsafe impl Sync for Kern {}

impl Kern {
    /// One frame: the bin pass then the tile pass over slot `k`, in one compute encoder that binds only the
    /// region and the slot offset (everything else is in the bind table). `indirect`: the dispatch sizes come
    /// from the slot (pre-armed frames); otherwise `nrows` / `ntiles`.
    fn encode(&self, cmd: &ProtocolObject<dyn MTLCommandBuffer>, k: usize, nrows: u32, ntiles: u32, indirect: bool) {
        self.encode_with(&self.psos, cmd, k, nrows, ntiles, indirect);
    }

    /// `encode` through the pipelines `psos` (the presenter's own, or the oracle's).
    fn encode_with(&self, psos: &Psos, cmd: &ProtocolObject<dyn MTLCommandBuffer>, k: usize, nrows: u32, ntiles: u32, indirect: bool) {
        let slot = self.slots[k].load(Relaxed);
        let enc = cmd.computeCommandEncoder().expect("compute encoder");
        let offs = [slot, 0];
        unsafe {
            enc.setBuffer_offset_atIndex(Some(&self.pool), 0, 0);
            enc.setBytes_length_atIndex(NonNull::new(offs.as_ptr() as *mut _).unwrap(), 8, 1);
            // the scroll copy (0 threadgroups without one): disjoint from the tiles the repair writes, so no
            // barrier orders it against the tile pass
            enc.setComputePipelineState(&psos.copy);
            let tgc = MTLSize { width: 32, height: 8, depth: 1 };
            enc.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(&self.pool, slot as usize + 32, tgc);
            enc.setComputePipelineState(&psos.bin);
            let tg = MTLSize { width: 256, height: 1, depth: 1 };
            if indirect {
                enc.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(&self.pool, slot as usize, tg);
            } else {
                enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: nrows as usize, height: 1, depth: 1 }, tg);
            }
            enc.memoryBarrierWithScope(MTLBarrierScope::Buffers);
            enc.setComputePipelineState(&psos.tile);
            let tg2 = MTLSize { width: 16, height: 16, depth: 1 };
            if indirect {
                enc.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(&self.pool, slot as usize + 12, tg2);
            } else {
                enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: ntiles as usize, height: 1, depth: 1 }, tg2);
            }
        }
        enc.endEncoding();
    }

    /// Encode and commit the next `batch` frames: each waits for its release (`ev >= 2f-1`), runs the bin and
    /// tile passes over slot `f % 2` with sizes read from the slot, and signals `ev = 2f`.
    fn arm_locked(&self, st: &mut ArmState) {
        let batch = self.batch.load(Relaxed) as u64;
        let cmd = self.queue.commandBufferWithUnretainedReferences().expect("command buffer");
        for f in st.armed_upto + 1..=st.armed_upto + batch {
            cmd.encodeWaitForEvent_value(ProtocolObject::from_ref(&*self.ev), 2 * f - 1);
            self.encode(&cmd, (f % 2) as usize, 0, 0, true);
            cmd.encodeSignalEvent_value(ProtocolObject::from_ref(&*self.ev), 2 * f);
        }
        cmd.commit();
        if st.batches.len() == 2 {
            st.batches.pop_front();
        }
        st.batches.push_back(cmd);
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

/// Counters.
#[derive(Default, Clone, Copy, Debug)]
pub struct Stats {
    pub gpu_frames: u64,
    pub tiles: u64,
    pub ca_only: u64,
    pub warm_blits: u64,
    pub resident_surfaces: usize,
    /// Pre-armed batches drained because the residency set grew.
    pub drains: u64,
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
    /// Keeps the memory of a wrapped region alive (`new_on`).
    _pool_keep: Option<Box<dyn std::any::Any>>,
    /// Premultiplied RGBA images (icons, gradient ramps; `IMG_W` x `IMG_H`), created at the first image.
    img: Option<Tex>,
    /// A clock-driven frame drawn but not yet committed (published at the next `sync`).
    pending: Option<usize>,
    /// The surface the last calling-thread commit put on screen.
    last_main: usize,
    /// Column views (`new_views`): `None` when every column is a range of the one region buffer.
    views: Option<Views>,
    /// The frozen original kernels (`oracle.metal`), compiled at the first reference render.
    oracle: Option<Psos>,
    /// `BOUGH_AB` set: time the kernels against the oracle's at draws 60, 120 and 180 (`ab_probe`).
    ab: bool,
    ab_draw: u32,
}

/// The GPU's views of the region's columns: per column a no-copy buffer over its used prefix, per mask group
/// the committed capacity (a power of two of elements; 0 before the first frame).
#[derive(Default)]
struct Views {
    bufs: [Option<Retained<ProtocolObject<dyn MTLBuffer>>>; NCOLS],
    cap: [u32; NGROUPS],
    /// Views made so far (growth steps).
    growths: u32,
}

/// Byte offset of the bind table in the region header (`FBind` in shader.metal).
pub const FBIND_OFF: usize = 128;
/// The bind table: 4 target surfaces, the atlas (resource ids), bins and counts (GPU addresses), the image
/// atlas (resource id; the glyph atlas stands in until an image exists).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FBind {
    t: [u64; 4],
    atlas: u64,
    bins: u64,
    counts: u64,
    /// The same 4 surfaces as read handles (the scroll copy's source); appended (round 3c), table = 216 B.
    r: [u64; 4],
    img: u64,
    /// Each surface's band certificates (GPU addresses), appended in round 6: the table fills the header.
    cert: [u64; 4],
}
const _: () = assert!(size_of::<FHdr>() <= FBIND_OFF && FBIND_OFF + size_of::<FBind>() <= HDR_BYTES);

/// The image atlas's size (premultiplied RGBA8 texels).
pub const IMG_W: u32 = 1024;
pub const IMG_H: u32 = 1024;

/// The device-side state of a presenter: the device (the driver's loading: 22–29 ms on this Mac, most of a
/// cold start), the queues, the library and the two pipelines, plus the window-sized resources when the
/// window is known. `prepare_in_background` builds it on another thread from process entry while the main
/// thread mounts, shapes and lays out (round 3b, input agent).
pub struct GpuPrep {
    device: Dev,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    aux: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    psos: Psos,
    res: Option<PrepRes>,
    /// Instructions the building thread retired (thread_selfcounts) and its wall time.
    pub instructions: u64,
    pub nanos: u64,
}

/// ((w, h, surfaces, bin_cap), atlas, bins, counts, surfaces).
type PrepRes = ((usize, usize, usize, u32), Tex, Retained<ProtocolObject<dyn MTLBuffer>>, Retained<ProtocolObject<dyn MTLBuffer>>, Vec<Surface>);

// SAFETY: Metal devices, command queues, pipeline states, buffers, textures and IOSurfaces are thread-safe
// objects; the struct only moves them from the thread that built them to the one that uses them.
unsafe impl Send for GpuPrep {}

unsafe extern "C" {
    fn thread_selfcounts(kind: i32, buffer: *mut u64, size: usize) -> i32;
}

/// The calling thread's retired instructions (xnu's per-thread counters).
pub fn thread_instructions() -> u64 {
    let mut c = [0u64; 2];
    // SAFETY: kind 1 fills two u64s (instructions, cycles); `c` has 16 bytes.
    unsafe { thread_selfcounts(1, c.as_mut_ptr(), 16) };
    c[0]
}

impl GpuPrep {
    /// The device state, plus the window-sized resources (atlas, bins, surfaces) when `dims` names the
    /// window: (w, h px, surfaces, bin capacity).
    pub fn new(dims: Option<(usize, usize, usize, u32)>) -> GpuPrep {
        let (i0, t0) = (thread_instructions(), std::time::Instant::now());
        let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
        let queue = device.newCommandQueue().expect("queue");
        let aux = device.newCommandQueue().expect("aux queue");
        let lib = crate::gpu::load_library(&device);
        let psos = Psos::new(&device, &lib);
        let res = dims.map(|(w, h, surfaces, bin_cap)| {
            let (atlas, bins, counts) = Fused::new_bins(&device, h, bin_cap);
            let surfs = (0..surfaces.clamp(1, MAX_SURFACES)).map(|_| Fused::new_surface(&device, w, h)).collect();
            ((w, h, surfaces, bin_cap), atlas, bins, counts, surfs)
        });
        GpuPrep { device, queue, aux, psos, res, instructions: thread_instructions() - i0, nanos: t0.elapsed().as_nanos() as u64 }
    }
}

static PREP: std::sync::Mutex<Option<std::thread::JoinHandle<GpuPrep>>> = std::sync::Mutex::new(None);

/// Start building the GPU's device state (and, with `dims`, the window's resources) on another thread (call
/// at process entry). The next presenter created takes it, waiting for it if it is not done.
pub fn prepare_in_background(dims: Option<(usize, usize, usize, u32)>) {
    let h = std::thread::Builder::new().name("bough-gpu-prep".into()).spawn(move || GpuPrep::new(dims)).expect("gpu prep thread");
    *PREP.lock().unwrap() = Some(h);
}

/// Is a background preparation pending (so a presenter should be created after the mount)?
pub fn prep_pending() -> bool {
    PREP.lock().unwrap().is_some()
}

fn take_prep() -> GpuPrep {
    match PREP.lock().unwrap().take() {
        Some(h) => h.join().expect("gpu prep thread"),
        None => GpuPrep::new(None),
    }
}

/// The region a presenter draws from: a fresh shared buffer, or memory the caller already carved (wrapped
/// without a copy: page-aligned, a whole number of pages; kept alive by `keep` as long as the buffer).
pub enum PoolSrc {
    Alloc(usize),
    Wrap { ptr: *mut u8, len: usize, keep: Box<dyn std::any::Any> },
    /// The region is `len` bytes at `ptr`; the GPU buffer bound to the encoders covers its first `base` bytes
    /// (header and frame slots) and each column gets its own view (`ensure_columns`).
    Views { ptr: *mut u8, len: usize, keep: Box<dyn std::any::Any>, base: usize },
}

impl Fused {
    /// A presenter for a `w` x `h` px window whose region is a `region_len`-byte shared buffer.
    pub fn new(w: usize, h: usize, scale: f64, region_len: usize, surfaces: usize, bin_cap: u32) -> Fused {
        Self::with_pool(take_prep(), PoolSrc::Alloc(region_len), w, h, scale, surfaces, bin_cap)
    }

    /// A presenter over memory already carved and written (the mount ran while the device was built): the
    /// memory becomes the shared buffer without a copy. Returns the preparation thread's instructions and ns.
    ///
    /// # Safety
    /// `ptr` is page-aligned, valid for `len` bytes (a multiple of the page size) and stays valid while `keep`
    /// lives; nothing else frees it.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn new_on(ptr: *mut u8, len: usize, keep: Box<dyn std::any::Any>, w: usize, h: usize, scale: f64, surfaces: usize, bin_cap: u32) -> (Fused, u64, u64) {
        let prep = take_prep();
        let (i, ns) = (prep.instructions, prep.nanos);
        (Self::with_pool(prep, PoolSrc::Wrap { ptr, len, keep }, w, h, scale, surfaces, bin_cap), i, ns)
    }

    /// A presenter over a region whose columns the GPU sees through views of their used prefixes (round 6,
    /// lane memory): the buffer bound to the encoders wraps the region's first `base` bytes (header and frame
    /// slots), and `ensure_columns` wraps, before each frame, as much of each column as its mask group's
    /// high-water mark needs (a power of two of elements, page-rounded). Pages of the region outside every view
    /// are never wired: a column's capacity is address space. Returns the preparation thread's instructions
    /// and ns.
    ///
    /// # Safety
    /// As `new_on`; and every column named by the layout later passed to `set_layout` lies inside the
    /// region, starts on a VM page boundary, and is followed by unused region bytes up to the next page.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn new_views(ptr: *mut u8, len: usize, keep: Box<dyn std::any::Any>, base: usize, w: usize, h: usize, scale: f64, surfaces: usize, bin_cap: u32) -> (Fused, u64, u64) {
        let prep = take_prep();
        let (i, ns) = (prep.instructions, prep.nanos);
        (Self::with_pool(prep, PoolSrc::Views { ptr, len, keep, base }, w, h, scale, surfaces, bin_cap), i, ns)
    }

    fn with_pool(prep: GpuPrep, src: PoolSrc, w: usize, h: usize, scale: f64, surfaces: usize, bin_cap: u32) -> Fused {
        let GpuPrep { device, queue, aux, psos, res, .. } = prep;
        let mut views = None;
        let (pool, region_len, pool_keep) = match src {
            PoolSrc::Alloc(len) => (device.newBufferWithLength_options(len, MTLResourceOptions::StorageModeShared).expect("region buffer"), len, None),
            PoolSrc::Wrap { ptr, len, keep } => {
                // SAFETY: the caller's contract (page-aligned, `len` bytes, alive while `keep` is; no deallocator).
                let b = unsafe { device.newBufferWithBytesNoCopy_length_options_deallocator(NonNull::new(ptr as *mut _).unwrap(), len, MTLResourceOptions::StorageModeShared, None) }.expect("wrapped region buffer");
                (b, len, Some(keep))
            }
            PoolSrc::Views { ptr, len, keep, base } => {
                let base = base.next_multiple_of(vm_page()).min(len);
                // SAFETY: the caller's contract (page-aligned, `len` >= `base` bytes, alive while `keep` is): the
                // buffer is the region's first `base` bytes (header and frame slots), no deallocator.
                let b = unsafe { device.newBufferWithBytesNoCopy_length_options_deallocator(NonNull::new(ptr as *mut _).unwrap(), base, MTLResourceOptions::StorageModeShared, None) }.expect("region base buffer");
                views = Some(Views::default());
                (b, len, Some(keep))
            }
        };
        let pool_ptr = pool.contents().as_ptr() as *mut u8;
        let (atlas, bins, counts, surfs) = match res {
            Some((d, atlas, bins, counts, surfs)) if d == (w, h, surfaces, bin_cap) => (atlas, bins, counts, surfs),
            _ => {
                let (atlas, bins, counts) = Self::new_bins(&device, h, bin_cap);
                (atlas, bins, counts, (0..surfaces.clamp(1, MAX_SURFACES)).map(|_| Self::new_surface(&device, w, h)).collect())
            }
        };
        let device_ev = device.newSharedEvent().expect("shared event");
        let kern = Arc::new(Kern {
            queue: queue.clone(),
            ev: device_ev.clone(),
            pool: pool.clone(),
            psos,
            slots: Default::default(),
            batch: std::sync::atomic::AtomicUsize::new(16),
            armed: AtomicU64::new(0),
            arm: std::sync::Mutex::new(ArmState { armed_upto: 0, batches: std::collections::VecDeque::with_capacity(3) }),
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
            _pool_keep: pool_keep,
            img: None,
            pending: None,
            last_main: NONE_SURF,
            views,
            oracle: None,
            ab: std::env::var_os("BOUGH_AB").is_some(),
            ab_draw: 0,
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

    /// The atlas, the bins (one row of `bin_cap` per tile row of a `h` px surface) and their counts.
    #[allow(clippy::type_complexity)]
    fn new_bins(device: &Dev, h: usize, bin_cap: u32) -> (Tex, Retained<ProtocolObject<dyn MTLBuffer>>, Retained<ProtocolObject<dyn MTLBuffer>>) {
        let atlas = Self::new_atlas(device, 1024, 512);
        let ty = h.div_ceil(32);
        let bins = device.newBufferWithLength_options(ty * bin_cap as usize * 16, MTLResourceOptions::StorageModePrivate).expect("bins");
        let counts = device.newBufferWithLength_options((ty + 1) * 4, MTLResourceOptions::StorageModeShared).expect("bin counts");
        (atlas, bins, counts)
    }

    fn new_surface(device: &Dev, w: usize, h: usize) -> Surface {
        let io = make_iosurface(w, h);
        let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, w, h, false) };
        d.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
        d.setStorageMode(MTLStorageMode::Shared);
        let tex = device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0).expect("iosurface texture");
        // zero-filled by Metal (nothing known), and no page of it is touched until the surface is drawn
        let cert = device.newBufferWithLength_options(cert_bytes(w, h), MTLResourceOptions::StorageModeShared).expect("band certificates");
        Surface { io, tex, cert, touched: Cell::new(false), volatile: Cell::new(false), cpu_written: Cell::new(false) }
    }

    /// The region's memory (the shared buffer's contents) and its length, plus a keep-alive handle.
    pub fn region(&self) -> (*mut u8, usize, Retained<ProtocolObject<dyn MTLBuffer>>) {
        (self.pool_ptr, self.pool_len, self.pool.clone())
    }

    /// Write the kernels' header (offsets of every column, window size, bin capacity) and the column table.
    pub fn set_layout(&mut self, l: Layout) {
        self.lay = l;
        // with column views the kernels' masks are the views' committed capacities, not the reservations
        let masks = match &self.views {
            Some(v) => view_masks(&v.cap),
            None => [l.masks.inst, l.masks.page, l.masks.glyph, l.masks.run, l.masks.space, l.masks.clip, l.masks.strike],
        };
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
            masks,
            xforms_off: l.xforms_off,
        };
        // SAFETY: the region starts with HDR_BYTES reserved for the header; no GPU work reads it now.
        unsafe { (self.pool_ptr as *mut FHdr).write(h) };
        self.write_cols();
        for (k, &o) in l.slot.iter().enumerate() {
            self.kern.slots[k].store(o, Relaxed);
        }
        self.write_binds();
        self.kern.ensure(self.released + 1);
    }

    /// Write the column table: each column's GPU address (its view's, or the region buffer's address plus the
    /// column's offset; 0 for a column without a view yet, which no frame reads: views are made before a frame).
    fn write_cols(&mut self) {
        let mut t = [0u64; NCOLS];
        let base = self.pool.gpuAddress();
        let l = self.lay;
        let offs = [l.inst_off, l.spaces_off, l.clips_off, l.dir_id_off, l.dir_ink_off, l.pages_off, l.gx_off, l.gent_off, l.runs_off, l.toks_off, l.xforms_off];
        for (k, a) in t.iter_mut().enumerate() {
            *a = match &self.views {
                Some(v) => v.bufs[k].as_ref().map_or(0, |b| b.gpuAddress()),
                None => base + offs[k] as u64,
            };
        }
        // SAFETY: the column table lies inside the reserved header (const-asserted); the GPU is idle.
        unsafe { (self.pool_ptr.add(FCOL_OFF) as *mut [u64; NCOLS]).write(t) };
    }

    /// Make the GPU's views of the region's columns hold at least `need[g]` elements of every column of mask
    /// group `g` (column views only; a no-op for a one-buffer region). The common case is seven compares;
    /// a group past its committed capacity gets views of the next power of two (at least double), page-rounded,
    /// a new column table and masks, and the residency set's update (which drains the pre-armed frames, as a bin
    /// growth does). The CPU's addresses never move. Call before a frame is drawn (`App::draw`).
    #[inline]
    pub fn ensure_columns(&mut self, need: &[usize; NGROUPS]) {
        let Some(v) = &self.views else { return };
        if (0..NGROUPS).all(|g| need[g] <= v.cap[g] as usize && v.cap[g] != 0) {
            return;
        }
        self.grow_views(need);
    }

    #[cold]
    #[inline(never)]
    fn grow_views(&mut self, need: &[usize; NGROUPS]) {
        self.sync();
        let page = vm_page();
        let l = self.lay;
        let v = self.views.as_mut().unwrap();
        let mut cap = v.cap;
        let mut changed = [false; NGROUPS];
        for g in 0..NGROUPS {
            // the largest capacity every column of the group can hold (its reservation, a power of two)
            let limit = l.cols.iter().filter(|c| c.group as usize == g && c.elem > 0).map(|c| (c.reserve / c.elem) as usize).min().unwrap_or(0);
            let want = need[g].max(1).next_power_of_two();
            if cap[g] != 0 && want <= cap[g] as usize {
                continue;
            }
            // the gates' sabotage: views never grow past their first size (the capacity test must fail)
            #[cfg(feature = "sabotage_views")]
            if cap[g] != 0 {
                continue;
            }
            // at least double (a mount that grows one row at a time makes log2 views, not one per row), but
            // start from one page's worth
            let first = |c: &ColumnDesc| (page / c.elem.max(1) as usize).max(1);
            let floor = l.cols.iter().filter(|c| c.group as usize == g && c.elem > 0).map(first).min().unwrap_or(1).next_power_of_two() / 2;
            let new = want.max(cap[g] as usize * 2).max(floor.max(1)).min(limit.max(1));
            if new as u32 != cap[g] {
                cap[g] = new as u32;
                changed[g] = true;
            }
        }
        let mut old = Vec::new();
        for (k, c) in l.cols.iter().enumerate() {
            let g = c.group as usize;
            if !changed[g] || c.elem == 0 {
                continue;
            }
            let bytes = (cap[g] as usize * c.elem as usize).next_multiple_of(page).min((c.reserve as usize).next_multiple_of(page)).min(self.pool_len - c.off as usize);
            debug_assert!(c.off as usize % page == 0, "column {k} is not on a page boundary");
            // SAFETY: `[off, off + bytes)` lies inside the region (clamped to its length), starts on a page
            // boundary and is a whole number of pages (`new_views`' contract); the region outlives the view (the
            // presenter holds its keep-alive); no deallocator.
            let b = unsafe {
                self.device.newBufferWithBytesNoCopy_length_options_deallocator(NonNull::new(self.pool_ptr.add(c.off as usize) as *mut _).unwrap(), bytes, MTLResourceOptions::StorageModeShared, None)
            }
            .expect("column view");
            if let Some(r) = &self.resid {
                r.addAllocation(ProtocolObject::from_ref(&*b));
            }
            if let Some(o) = v.bufs[k].replace(b) {
                old.push(o);
            }
        }
        if let Some(r) = &self.resid {
            for o in &old {
                r.removeAllocation(ProtocolObject::from_ref(&**o));
            }
            r.commit();
        }
        v.cap = cap;
        v.growths += 1;
        let masks = view_masks(&cap);
        // SAFETY: header field `masks`; the GPU is idle (synced above).
        unsafe { (*(self.pool_ptr as *mut FHdr)).masks = masks };
        self.write_cols();
        self.residency_changed();
        drop(old);
    }

    /// Write the bind table (resource ids of the surfaces and the atlas, GPU addresses of bins and counts).
    /// Only while the GPU is idle (`sync`), like every other header write.
    fn write_binds(&mut self) {
        let n = self.surfs.len();
        let b = FBind {
            t: [rid(&self.surfs[0].tex), rid(&self.surfs[1 % n].tex), rid(&self.surfs[2 % n].tex), rid(&self.surfs[3 % n].tex)],
            atlas: rid(&self.atlas),
            bins: self.bins.gpuAddress(),
            counts: self.counts.gpuAddress(),
            r: [rid(&self.surfs[0].tex), rid(&self.surfs[1 % n].tex), rid(&self.surfs[2 % n].tex), rid(&self.surfs[3 % n].tex)],
            img: rid(self.img.as_ref().unwrap_or(&self.atlas)),
            cert: [self.surfs[0].cert.gpuAddress(), self.surfs[1 % n].cert.gpuAddress(), self.surfs[2 % n].cert.gpuAddress(), self.surfs[3 % n].cert.gpuAddress()],
        };
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
        let cmd = self.aux.commandBuffer().expect("command buffer");
        self.encode(&cmd, k, nr, nt, false);
        cmd.commit();
        cmd.waitUntilCompleted();
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

    /// Why surface `i` is in use (diagnostics, the cold path of a ring growth): bit 0 the publisher is
    /// committing it, 1 it is queued for the publisher, 2 the publisher's last commit put it on screen, 3 the
    /// calling thread's last commit or a paced frame holds it, 4 `IOSurfaceIsInUse` (the compositor).
    pub fn in_use_why(&self, i: usize) -> u8 {
        let p = self.publisher.as_ref();
        let b = |c: bool, k: u8| if c { 1u8 << k } else { 0 };
        b(p.is_some_and(|p| p.sh.in_flight.load(Relaxed) == i), 0)
            | b(p.is_some_and(|p| p.sh.queued.load(Relaxed) == i), 1)
            | b(p.is_some_and(|p| p.sh.last.load(Relaxed) == i), 2)
            | b(self.last_main == i || self.pending == Some(i), 3)
            | b(self.surfs[i].io.is_in_use(), 4)
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
                r.removeAllocation(ProtocolObject::from_ref(&*s.cert));
            }
            r.removeAllocation(ProtocolObject::from_ref(&*old_bins));
            r.removeAllocation(ProtocolObject::from_ref(&*old_counts));
            r.addAllocation(ProtocolObject::from_ref(&*self.bins));
            r.addAllocation(ProtocolObject::from_ref(&*self.counts));
            r.commit();
        }
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

    /// Upload rows `rows` of the image atlas (`IMG_W` premultiplied RGBA texels per row) from `pixels` (the
    /// whole atlas). The GPU must be idle (sync()). The first call creates the texture, adds it to the
    /// residency set (draining the pre-armed frames, as any new allocation) and names it in the bind table.
    pub fn sync_images(&mut self, pixels: &[u8], rows: std::ops::Range<u32>) {
        assert_eq!(pixels.len(), (IMG_W * IMG_H * 4) as usize);
        if self.img.is_none() {
            let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::RGBA8Unorm, IMG_W as usize, IMG_H as usize, false) };
            let t = self.device.newTextureWithDescriptor(&d).expect("image atlas");
            if let Some(r) = &self.resid {
                r.addAllocation(ProtocolObject::from_ref(&*t));
                r.commit();
            }
            self.img = Some(t);
            self.write_binds();
            self.residency_changed();
        }
        let rows = rows.start.min(IMG_H)..rows.end.min(IMG_H);
        if rows.is_empty() {
            return;
        }
        let t = self.img.as_ref().unwrap();
        let off = (rows.start * IMG_W * 4) as usize;
        // SAFETY: the rows lie inside the atlas (clamped; the size asserted); the GPU is idle.
        unsafe {
            t.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                MTLRegion { origin: MTLOrigin { x: 0, y: rows.start as usize, z: 0 }, size: MTLSize { width: IMG_W as usize, height: (rows.end - rows.start) as usize, depth: 1 } },
                0,
                NonNull::new(pixels.as_ptr().add(off) as *mut _).unwrap(),
                (IMG_W * 4) as usize,
            );
        }
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

    /// Surface `i` non-volatile again (true if the system purged it meanwhile: its contents are gone), without
    /// touching the residency set (a CPU-repaired frame must not cause GPU work).
    pub fn make_nonvolatile(&mut self, i: usize) -> bool {
        let s = &self.surfs[i];
        if !s.volatile.get() {
            return false;
        }
        let mut old = 0u32;
        // SAFETY: a valid surface; 0 = kIOSurfacePurgeableNonVolatile.
        unsafe { s.io.set_purgeable(0, &mut old) };
        s.volatile.set(false);
        old == 2
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
            // first use, or back from a trim (its pixels may be gone): nothing is known about them
            // SAFETY: the plane is `cert_bytes` long, in shared storage, and no frame that names this
            // surface is pending (it was not in the residency set).
            unsafe { std::ptr::write_bytes(s.cert.contents().as_ptr() as *mut u8, 0, cert_bytes(self.w, self.h)) };
            if let Some(r) = &self.resid {
                r.addAllocation(ProtocolObject::from_ref(&*s.cert));
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
            if !want(i) || s.volatile.get() || !(s.touched.get() || s.cpu_written.get()) || s.io.is_in_use() {
                continue;
            }
            if s.touched.get() {
                if let Some(r) = &self.resid {
                    r.removeAllocation(ProtocolObject::from_ref(&*s.tex));
                    r.removeAllocation(ProtocolObject::from_ref(&*s.cert));
                    r.commit();
                    self.stats.resident_surfaces -= 1;
                }
            }
            s.touched.set(false);
            s.cpu_written.set(false);
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

    fn encode(&self, cmd: &ProtocolObject<dyn MTLCommandBuffer>, k: usize, nrows: u32, ntiles: u32, indirect: bool) {
        self.kern.encode(cmd, k, nrows, ntiles, indirect);
    }

    /// Repair surface `target` from the damaged tile rows (`full`: everything) with `n_dir` directory
    /// entries, commit, and (windowed) publish it once the GPU is done. Returns the tile count.
    /// `PACED`: a clock-driven turn: published at the next `sync` on the calling thread (one refresh later,
    /// no thread wake, no completion wait); otherwise the publisher thread commits it in this refresh.
    /// `copy`: a scroll copy from another surface first (RESULTS §15; `SlotCopy::NONE` for none).
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
        if self.ab {
            self.ab_probe(target, rows, full, n_dir, copy);
        }
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

    /// Surface `i` for the CPU to write (`CpuRaster`, lane memory round 6): the GPU done with it, non-volatile
    /// again, locked. Returns its base address and bytes per row, and whether the system purged it while volatile
    /// (its contents are gone: the caller repaints it whole). Pair with `write_end`.
    pub fn write_begin(&mut self, i: usize) -> (*mut u8, usize, bool) {
        self.sync();
        let s = &self.surfs[i];
        let mut purged = false;
        if s.volatile.get() {
            let mut old = 0u32;
            // SAFETY: a valid surface; 0 = kIOSurfacePurgeableNonVolatile.
            unsafe { s.io.set_purgeable(0, &mut old) };
            s.volatile.set(false);
            purged = old == 2;
        }
        // SAFETY: a valid surface, locked for writing until `write_end`; the GPU finished every frame (`sync`).
        unsafe { s.io.lock(objc2_io_surface::IOSurfaceLockOptions::empty(), std::ptr::null_mut()) };
        s.cpu_written.set(true);
        (s.io.base_address().as_ptr() as *mut u8, s.io.bytes_per_row(), purged)
    }

    /// The certificate hook (compute lane, round 7; asked for by the gpu and memory handoffs): the pixels of
    /// these tiles of surface `i` were written without the kernels, so nothing is known about them any more.
    /// `rows` is the frame's damage (a bit per tile column, per tile row); `full`: every tile. A surface the
    /// kernels have not touched since its plane was last zeroed needs nothing (`prepare_target` zeroes it at
    /// first use). Call between `write_begin` and `write_end`.
    pub fn forget_tiles(&mut self, i: usize, rows: &[u128], full: bool) {
        let s = &self.surfs[i];
        if !s.touched.get() {
            return;
        }
        let (tx, ty) = (self.w.div_ceil(32), self.h.div_ceil(32));
        let p = s.cert.contents().as_ptr() as *mut u8;
        // SAFETY: the plane is `cert_bytes` long in shared storage; `write_begin` synced the GPU, so no frame
        // reads or writes it; every index is below tx * ty.
        unsafe {
            if full {
                std::ptr::write_bytes(p, 0, cert_bytes(self.w, self.h));
                return;
            }
            for (y, &r) in rows.iter().enumerate().take(ty) {
                let mut m = if tx >= 128 { r } else { r & ((1u128 << tx) - 1) };
                while m != 0 {
                    let x = m.trailing_zeros() as usize;
                    m &= m - 1;
                    std::ptr::write_bytes(p.add((y * tx + x) * 72), 0, 72);
                }
            }
        }
    }

    /// Surface `i`'s memory for an executor that writes or reads it in place: base address, bytes per row,
    /// allocation size. Valid while the surface exists (a resize replaces it); lock it (`write_begin`,
    /// `read_begin`) around CPU access.
    pub fn surface_mem(&self, i: usize) -> (*mut u8, usize, usize) {
        let io = &self.surfs[i].io;
        (io.base_address().as_ptr() as *mut u8, io.bytes_per_row(), io.alloc_size())
    }
    /// Lock surface `i` for CPU reads (a scroll copy's source), the GPU done with it.
    pub fn read_begin(&mut self, i: usize) -> (*const u8, usize) {
        self.sync();
        let io = &self.surfs[i].io;
        // SAFETY: a valid surface, read-locked until `read_end`.
        unsafe { io.lock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()) };
        (io.base_address().as_ptr() as *const u8, io.bytes_per_row())
    }
    pub fn read_end(&mut self, i: usize) {
        // SAFETY: locked by `read_begin`.
        unsafe { self.surfs[i].io.unlock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()) };
    }

    /// Unlock surface `i` after `write_begin`.
    pub fn write_end(&mut self, i: usize) {
        // SAFETY: locked by `write_begin`.
        unsafe { self.surfs[i].io.unlock(objc2_io_surface::IOSurfaceLockOptions::empty(), std::ptr::null_mut()) };
    }

    /// Has the process's GPU queue run work since it was made (the driver's per-queue block exists)?
    pub fn gpu_used(&self) -> bool {
        self.stats.gpu_frames > 0 || self.bin_growths > 0
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

    /// The frozen original kernels (the shader as of the round-6 base, `oracle.metal`), compiled on first use.
    /// They read the same region, slots and bind table: every layout change since is an append.
    fn oracle(&mut self) {
        if self.oracle.is_none() {
            let src = NSString::from_str(include_str!("oracle.metal"));
            let lib = self.device.newLibraryWithSource_options_error(&src, None).expect("oracle msl compile");
            self.oracle = Some(Psos::new(&self.device, &lib));
        }
    }

    /// A from-scratch full redraw of the current region into a private surface, through the presenter's
    /// kernels AND through the original ones (`oracle.metal`): the two must agree byte for byte (a panic
    /// otherwise), so a gate that compares a retained frame with this compares it with the original raster,
    /// and no mechanism added to the kernels can hide a dropped or reordered entry in both of its own renders.
    pub fn reference_pixels(&mut self, n_dir: u32) -> Vec<u8> {
        let ours = self.full_redraw(n_dir, false);
        let orig = self.full_redraw(n_dir, true);
        if ours != orig {
            let n = ours.chunks(4).zip(orig.chunks(4)).filter(|(a, b)| a != b).count();
            let first = ours.chunks(4).zip(orig.chunks(4)).position(|(a, b)| a != b).unwrap();
            panic!("the kernels' full redraw differs from the original kernels' in {n} pixels (first at {}, {})", first % self.w, first / self.w);
        }
        orig
    }

    /// The original kernels' full redraw alone (the independent raster oracle).
    pub fn oracle_pixels(&mut self, n_dir: u32) -> Vec<u8> {
        self.full_redraw(n_dir, true)
    }

    fn full_redraw(&mut self, n_dir: u32, oracle: bool) -> Vec<u8> {
        self.sync();
        if oracle {
            self.oracle();
        }
        if self.scratch.is_none() {
            self.scratch = Some(Self::new_surface(&self.device, self.w, self.h));
        }
        unsafe { (*(self.pool_ptr as *mut FHdr)).n_dir = n_dir };
        // the scratch surface takes the bind table's target 0 for this one render (the GPU is idle)
        let s = self.scratch.as_ref().unwrap().tex.clone();
        let sc = self.scratch.as_ref().unwrap().cert.clone();
        if let Some(r) = &self.resid {
            r.addAllocation(ProtocolObject::from_ref(&*s));
            r.addAllocation(ProtocolObject::from_ref(&*sc));
            r.commit();
        }
        // SAFETY: the bind table lies inside the reserved header; the GPU is idle (synced above). The scratch
        // surface brings its own certificate plane: surface 0's describes surface 0's pixels, not these.
        unsafe {
            (*(self.pool_ptr.add(FBIND_OFF) as *mut FBind)).t[0] = rid(&s);
            (*(self.pool_ptr.add(FBIND_OFF) as *mut FBind)).cert[0] = sc.gpuAddress();
        }
        let (nt, nr) = self.write_slot(2, 0, &[], true, &SlotCopy::NONE);
        loop {
            let cmd = self.aux.commandBuffer().expect("command buffer");
            self.kern.encode_with(if oracle { self.oracle.as_ref().unwrap() } else { &self.kern.psos }, &cmd, 2, nr, nt, false);
            cmd.commit();
            cmd.waitUntilCompleted();
            // a row bin overflowed (frames drawn by another backend never grew them): grow and render again
            let ty = self.lay.tiles_y() as usize;
            // SAFETY: `counts` holds ty + 1 u32s in shared storage; the GPU is idle (waited).
            let need = unsafe { *(self.counts.contents().as_ptr() as *const u32).add(ty) };
            if need == 0 {
                break;
            }
            // SAFETY: as above.
            unsafe { *(self.counts.contents().as_ptr() as *mut u32).add(ty) = 0 };
            self.replace_bins(need.next_power_of_two().max(self.bin_cap * 2));
            // SAFETY: the bind table lies inside the reserved header; the GPU is idle (drained by the growth).
            unsafe {
                (*(self.pool_ptr.add(FBIND_OFF) as *mut FBind)).t[0] = rid(&s);
                (*(self.pool_ptr.add(FBIND_OFF) as *mut FBind)).cert[0] = sc.gpuAddress();
            }
            self.write_slot(2, 0, &[], true, &SlotCopy::NONE);
        }
        self.write_binds();
        if let Some(r) = &self.resid {
            r.removeAllocation(ProtocolObject::from_ref(&*s));
            r.removeAllocation(ProtocolObject::from_ref(&*sc));
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
        self.bench_frames_with(false, target, rows, full, n_dir, reps, debug, copy)
    }

    /// `bench_frames` through the presenter's kernels or the original ones (`oracle`).
    #[allow(clippy::too_many_arguments)]
    pub fn bench_frames_with(&mut self, oracle: bool, target: usize, rows: &[u128], full: bool, n_dir: u32, reps: usize, debug: u32, copy: &SlotCopy) -> f64 {
        self.sync();
        if oracle {
            self.oracle();
        }
        self.prepare_target(target);
        // SAFETY: header fields `n_dir`, `debug`; the GPU is idle (synced above).
        unsafe {
            (*(self.pool_ptr as *mut FHdr)).n_dir = n_dir;
            (*(self.pool_ptr as *mut FHdr)).debug = debug;
        }
        if !copy.is_none() {
            self.prepare_target((copy.src & 3) as usize);
        }
        let (nt, nr) = self.write_slot(2, target as u32, rows, full, copy);
        let cmd = self.aux.commandBuffer().expect("command buffer");
        for _ in 0..reps {
            self.kern.encode_with(if oracle { self.oracle.as_ref().unwrap() } else { &self.kern.psos }, &cmd, 2, nr, nt, false);
        }
        cmd.commit();
        cmd.waitUntilCompleted();
        // SAFETY: as above; the GPU is idle again.
        unsafe { (*(self.pool_ptr as *mut FHdr)).debug = 0 };
        (cmd.GPUEndTime() - cmd.GPUStartTime()) * 1e3 / reps as f64
    }

    /// Kernel A/B on the frame about to be drawn (`BOUGH_AB`, draws 60 / 120 / 180): the presenter's kernels
    /// against the original ones, interleaved in alternating order, 100 back-to-back frames each (the GPU
    /// saturated: its clock is steady, so the ratio holds under load); whole frame and repair without the
    /// scroll copy. The two retained results must be the same bytes.
    #[cold]
    fn ab_probe(&mut self, target: usize, rows: &[u128], full: bool, n_dir: u32, copy: &SlotCopy) {
        self.ab_draw += 1;
        if ![60, 120, 180].contains(&self.ab_draw) {
            return;
        }
        let tiles: u32 = if full { self.lay.tiles_x() * self.lay.tiles_y() } else { rows.iter().map(|r| r.count_ones()).sum() };
        eprintln!("AB frame={} size={}x{} full={} tiles={} copy={}", self.ab_draw, self.w, self.h, full, tiles, !copy.is_none() && !full);
        if std::env::var_os("BOUGH_AB_BANDS").is_some() && !copy.is_none() && !full {
            self.band_stats(target, rows, copy);
        }
        // The target's certificates as the frame finds them: restored before every run of the presenter's
        // kernels, and frozen during the timed repetitions, so each repetition decides as the real frame does.
        let cb = cert_bytes(self.w, self.h);
        // SAFETY: the plane is `cb` bytes of shared storage; the GPU is idle (the caller synced).
        let saved = unsafe { std::slice::from_raw_parts(self.surfs[target].cert.contents().as_ptr() as *const u8, cb) }.to_vec();
        let frozen = SlotCopy { src: copy.src | CERT_FROZEN, ..*copy };
        let cycles: usize = std::env::var("BOUGH_AB").ok().and_then(|v| v.parse().ok()).filter(|&n| n > 1).unwrap_or(6);
        for cycle in 0..cycles {
            let mut cases = vec![(false, "whole"), (true, "whole"), (false, "repair"), (true, "repair")];
            if cycle % 2 != 0 {
                cases.reverse();
            }
            for (oracle, name) in cases {
                let cp = if name == "whole" { &frozen } else { &SlotCopy::NONE };
                self.sync();
                // SAFETY: as above.
                unsafe { std::ptr::copy_nonoverlapping(saved.as_ptr(), self.surfs[target].cert.contents().as_ptr() as *mut u8, cb) };
                let ms = self.bench_frames_with(oracle, target, rows, full, n_dir, 100, 0, cp);
                eprintln!("AB_TIME frame={} cycle={} k={} case={} us={:.3}", self.ab_draw, cycle, if oracle { "ref" } else { "new" }, name, ms * 1e3);
            }
        }
        self.bench_frames_with(true, target, rows, full, n_dir, 1, 0, copy);
        let a = read_io(&self.surfs[target].io, self.w, self.h);
        // The frame as the stream will draw it: the target holds it when the probe returns. Every band the
        // frame writes already holds the frame's pixels (the renders above), so whichever of skip, fill and
        // move the restored certificates select leaves the frame, and the certificates the copy stores are
        // true of it.
        // SAFETY: as above.
        unsafe { std::ptr::copy_nonoverlapping(saved.as_ptr(), self.surfs[target].cert.contents().as_ptr() as *mut u8, cb) };
        self.bench_frames_with(false, target, rows, full, n_dir, 1, 0, copy);
        let b = read_io(&self.surfs[target].io, self.w, self.h);
        let bad = a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x != y).count();
        eprintln!("AB_PIXEL frame={} different_pixels={}", self.ab_draw, bad);
        // `BOUGH_AB_LOOSE`: a timing experiment that is not meant to be exact (a floor, a disabled stage)
        assert!(bad == 0 || std::env::var_os("BOUGH_AB_LOOSE").is_some(), "the kernels' retained frame differs from the original kernels'");
    }

    /// Measurement (`BOUGH_AB_BANDS`): of the blocks the scroll copy moves (undamaged tiles, block wholly
    /// inside the copy rect), how many are one colour in the source at `p - d` and already that colour in the
    /// target as it stands (the copy could skip them), one colour in the source only (a fill), or neither.
    #[cold]
    fn band_stats(&mut self, target: usize, rows: &[u128], copy: &SlotCopy) {
        let dst = read_io(&self.surfs[target].io, self.w, self.h);
        let src = read_io(&self.surfs[copy.src as usize].io, self.w, self.h);
        let px = |b: &[u8], x: i32, y: i32| -> [u8; 4] { b[(y as usize * self.w + x as usize) * 4..][..4].try_into().unwrap() };
        let [x0, y0, x1, y1] = copy.rect;
        // what the certificates know: rows (32 px of a tile) certified in the source / in the target as it stands
        let cb = cert_bytes(self.w, self.h);
        let tx = self.w.div_ceil(32);
        // SAFETY: both planes are `cb` bytes of shared storage; the GPU is idle.
        let (cs, cd) = unsafe { (std::slice::from_raw_parts(self.surfs[(copy.src & 3) as usize].cert.contents().as_ptr() as *const u8, cb), std::slice::from_raw_parts(self.surfs[target].cert.contents().as_ptr() as *const u8, cb)) };
        let row = |c: &[u8], x: i32, y: i32| -> Option<[u8; 8]> {
            let e = &c[((y as usize >> 5) * tx + (x as usize >> 5)) * 72..][..72];
            let b = (y as usize >> 2) & 7;
            (e[64 + b] >> (y & 3) & 1 != 0).then(|| e[b * 8..b * 8 + 8].try_into().unwrap())
        };
        let (mut known_src, mut known_dst, mut rows_n, mut truth) = (0u32, 0u32, 0u32, 0u32);
        for y in y0..y1 {
            for x in ((x0 + 31) & !31..x1 & !31).step_by(32) {
                rows_n += 1;
                known_src += row(cs, x, y).is_some() as u32;
                known_dst += row(cd, x, y).is_some() as u32;
                let c = px(&src, x, y);
                truth += (x..x + 32).all(|xx| px(&src, xx, y) == c) as u32;
            }
        }
        eprintln!("AB_CERT frame={} d=({}, {}) rect={:?} rows={} src_certified={:.3} dst_certified={:.3} src_truly_one_colour={:.3}", self.ab_draw, copy.dx, copy.dy, copy.rect, rows_n, known_src as f64 / rows_n as f64, known_dst as f64 / rows_n as f64, truth as f64 / rows_n as f64);
        for (bw, bh) in [(32, 32), (32, 16), (32, 8), (32, 4), (16, 4), (32, 1)] {
            let (mut skip, mut fill, mut mv) = (0u32, 0u32, 0u32);
            for ty in (y0 >> 5)..((y1 + 31) >> 5) {
                for tx in (x0 >> 5)..((x1 + 31) >> 5) {
                    if rows.get(ty as usize).copied().unwrap_or(0) >> tx & 1 != 0 {
                        continue;
                    }
                    for by in (ty * 32..ty * 32 + 32).step_by(bh) {
                        for bx in (tx * 32..tx * 32 + 32).step_by(bw) {
                            let (ex, ey) = (bx + bw as i32, by + bh as i32);
                            if bx < x0 || by < y0 || ex > x1 || ey > y1 {
                                mv += 1;
                                continue;
                            }
                            let c = px(&src, bx - copy.dx, by - copy.dy);
                            let src_const = (by..ey).all(|y| (bx..ex).all(|x| px(&src, x - copy.dx, y - copy.dy) == c));
                            let dst_const = (by..ey).all(|y| (bx..ex).all(|x| px(&dst, x, y) == c));
                            match (src_const, dst_const) {
                                (true, true) => skip += 1,
                                (true, false) => fill += 1,
                                _ => mv += 1,
                            }
                        }
                    }
                }
            }
            let n = (skip + fill + mv).max(1) as f64;
            eprintln!("AB_BANDS frame={} block={}x{} skip={:.3} fill={:.3} copy={:.3} blocks={}", self.ab_draw, bw, bh, skip as f64 / n, fill as f64 / n, mv as f64 / n, skip + fill + mv);
        }
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

    /// The presenter's GPU objects for a memory ledger (diagnostics; a few Metal property reads).
    pub fn gpu_mem(&self) -> GpuMem {
        let surf = |s: &Surface| (s.io.alloc_size(), s.touched.get(), s.volatile.get(), s.io.is_in_use());
        GpuMem {
            surfaces: self.surfs.iter().map(surf).collect(),
            scratch: self.scratch.as_ref().map(surf),
            region_len: self.pool_len,
            region_allocated: MTLResource::allocatedSize(&*self.pool),
            atlas: MTLResource::allocatedSize(&*self.atlas),
            atlas_dims: self.atlas_dims,
            image_atlas: self.img.as_ref().map_or(0, |t| MTLResource::allocatedSize(&**t)),
            bins: MTLResource::allocatedSize(&*self.bins),
            bin_cap: self.bin_cap,
            counts: MTLResource::allocatedSize(&*self.counts),
            warm: self.warm.as_ref().map_or(0, |w| MTLResource::allocatedSize(&*w.tiny)),
            device_allocated: self.device.currentAllocatedSize(),
            views: self.views.as_ref().map(|v| (v.bufs.iter().flatten().map(|b| b.length()).sum(), v.growths, v.cap)),
            base_len: self.pool.length(),
        }
    }
}

/// [`Fused::gpu_mem`]: bytes of each GPU object the presenter holds.
#[derive(Clone, Debug, Default)]
pub struct GpuMem {
    /// Per ring surface: (IOSurface allocation size, drawn at least once, purgeable-volatile, in use by the
    /// compositor).
    pub surfaces: Vec<(usize, bool, bool, bool)>,
    /// The reference render's private surface, if one was made.
    pub scratch: Option<(usize, bool, bool, bool)>,
    pub region_len: usize,
    pub region_allocated: usize,
    pub atlas: usize,
    pub atlas_dims: (u32, u32),
    pub image_atlas: usize,
    pub bins: usize,
    pub bin_cap: u32,
    pub counts: usize,
    pub warm: usize,
    /// `MTLDevice.currentAllocatedSize`: every allocation the device made for this process.
    pub device_allocated: usize,
    /// Column views (`new_views`): bytes wrapped, growth steps, committed capacity per mask group.
    pub views: Option<(usize, u32, [u32; NGROUPS])>,
    /// Bytes of the buffer bound to the encoders (the whole region, or its header and slots with views).
    pub base_len: usize,
}

/// The kernels' masks for committed capacities (`cap - 1`; a group with no view yet masks to 0).
fn view_masks(cap: &[u32; NGROUPS]) -> [u32; 7] {
    let mut m = [0u32; 7];
    for g in 0..NGROUPS {
        m[g] = cap[g].saturating_sub(1);
    }
    m
}

/// The VM page size (no-copy buffers are whole pages at page-aligned addresses).
fn vm_page() -> usize {
    // SAFETY: sysconf has no preconditions.
    (unsafe { libc::sysconf(libc::_SC_PAGESIZE) }) as usize
}

/// Copy the OR-ed tile rows (`rows[ty]`, bit tx = tile) into frame slot `k` as damaged-row records (see the
/// module docs): O(damaged rows). `full`: every tile of the window. Returns (tiles, rows). This is the whole
/// CPU side of a frame's raster input (headless runs call it too, so RENDER=0 measures it).
///
/// # Safety
/// `region` must be a region laid out by `l` (slots reserved with `Layout::slot_bytes` for its window), not
/// read by the GPU while this writes.
pub unsafe fn write_slot(region: *mut u8, l: &Layout, k: usize, target: u32, rows: &[u128], full: bool, copy: &SlotCopy) -> (u32, u32) {
    let (tx, ty) = (l.tiles_x(), l.tiles_y());
    let all = if tx >= 128 { u128::MAX } else { (1u128 << tx) - 1 };
    // SAFETY: see the function contract.
    unsafe {
        let s = region.add(l.slot[k] as usize) as *mut u32;
        // the scroll copy: one threadgroup per tile of the destination's tile span (row-major), its origin and
        // width in `cspan`; the destination rect and the move as u16 / i16 pairs
        let c = if full { &SlotCopy::NONE } else { copy };
        if c.is_none() {
            *s.add(8) = 0;
        } else {
            let [x0, y0, x1, y1] = c.rect;
            let (cx0, cy0) = ((x0 >> 5) as u32, (y0 >> 5) as u32);
            let (cw, ch) = (((x1 + 31) >> 5) as u32 - cx0, ((y1 + 31) >> 5) as u32 - cy0);
            *s.add(8) = cw * ch;
            *s.add(11) = c.src;
            *s.add(12) = x0 as u32 | (y0 as u32) << 16;
            *s.add(13) = x1 as u32 | (y1 as u32) << 16;
            *s.add(14) = c.dx as i16 as u16 as u32 | (c.dy as i16 as u16 as u32) << 16;
            *s.add(15) = cx0 | cy0 << 8 | cw << 16;
        }
        *s.add(9) = 1;
        *s.add(10) = 1;
        let rec = s.add(SLOT_HDR / 4);
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
