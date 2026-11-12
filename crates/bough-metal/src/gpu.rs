//! Device, pipelines, the shared pool buffer, and the repair encoder (one uber-shader draw).

use crate::damage::Region;
use crate::instance::*;
use crate::scene::{make_atlas, Scene, ATLAS_H, ATLAS_W};
use dispatch2::DispatchData;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{ns_string, NSString};
use objc2_metal::*;
use std::cell::Cell;
use std::ptr::NonNull;
use std::time::Instant;

pub type Dev = Retained<ProtocolObject<dyn MTLDevice>>;
pub type Tex = Retained<ProtocolObject<dyn MTLTexture>>;
pub type Cmd = Retained<ProtocolObject<dyn MTLCommandBuffer>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageMode {
    /// One scissored draw over the bounding box of the repair region.
    Bbox,
    /// One scissored draw per tile-rect, all sharing the bbox-culled draw list.
    MultiShared,
    /// One scissored draw per tile-rect, each with its own CPU-culled draw list.
    MultiCulled,
    /// Stencil mask written from the rect list (memoryless stencil), then ONE draw.
    Stencil,
}
impl DamageMode {
    pub const ALL: [DamageMode; 4] = [DamageMode::Bbox, DamageMode::MultiShared, DamageMode::MultiCulled, DamageMode::Stencil];
    pub fn name(&self) -> &'static str {
        match self {
            DamageMode::Bbox => "bbox",
            DamageMode::MultiShared => "multi-shared",
            DamageMode::MultiCulled => "multi-culled",
            DamageMode::Stencil => "stencil",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ShaderSource {
    /// Pre-compiled .metallib embedded in the binary (built by build.rs).
    Metallib,
    /// MSL source compiled at startup via newLibraryWithSource.
    Runtime,
}

#[cfg(feature = "mutant_cert_skip")]
const M_SKIP: &str = "#define MUTANT_CERT_SKIP 1\n";
#[cfg(not(feature = "mutant_cert_skip"))]
const M_SKIP: &str = "";
#[cfg(feature = "mutant_cert_stale")]
const M_STALE: &str = "#define MUTANT_CERT_STALE 1\n";
#[cfg(not(feature = "mutant_cert_stale"))]
const M_STALE: &str = "";
#[cfg(feature = "mutant_cert_tile")]
const M_TILE: &str = "#define MUTANT_CERT_TILE 1\n";
#[cfg(not(feature = "mutant_cert_tile"))]
const M_TILE: &str = "";

pub static SHADER_SRC: &str = include_str!("shader.metal");
pub static METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shader.metallib"));

/// `#define`s of the gate mutants this build enables (none in any shipped build), prepended to the source.
fn mutant_defines() -> String {
    [M_SKIP, M_STALE, M_TILE].concat()
}

/// The shader library: the embedded metallib, or, when the build had no Metal compiler (empty `METALLIB`),
/// `shader.metal` compiled from source.
pub fn load_library(device: &ProtocolObject<dyn MTLDevice>) -> Retained<ProtocolObject<dyn MTLLibrary>> {
    // measurement knob (A/B of a kernel change on one binary): MSL source from a file, compiled as the default
    if let Some(p) = std::env::var_os("BOUGH_METAL_SRC") {
        let src = std::fs::read_to_string(&p).expect("BOUGH_METAL_SRC: readable MSL source");
        return device.newLibraryWithSource_options_error(&NSString::from_str(&src), None).expect("msl compile");
    }
    if METALLIB.is_empty() {
        // a build without a Metal compiler (kernel development): `BOUGH_MSL` names a source to compile instead
        let over = std::env::var_os("BOUGH_MSL").map(|p| std::fs::read_to_string(p).expect("BOUGH_MSL"));
        let s = NSString::from_str(&format!("{}{}", mutant_defines(), over.as_deref().unwrap_or(SHADER_SRC)));
        device.newLibraryWithSource_options_error(&s, None).expect("msl compile")
    } else {
        let data = DispatchData::from_static_bytes(METALLIB);
        device.newLibraryWithData_error(&data).expect("metallib load")
    }
}

#[derive(Default, Clone, Debug)]
pub struct StartupTimes {
    pub device_ms: f64,
    pub library_ms: f64,
    pub pipelines_ms: f64,
    pub buffers_atlas_ms: f64,
}

/// Optional instruction-count phase profiler (uses proc_pid_rusage; off by default).
pub struct Prof {
    pub on: Cell<bool>,
    pub acc: [Cell<u64>; 10],
    last: Cell<u64>,
    ovh: Cell<u64>,
}
impl Prof {
    pub fn new() -> Self {
        Prof { on: Cell::new(false), acc: Default::default(), last: Cell::new(0), ovh: Cell::new(0) }
    }
    pub fn enable(&self) {
        self.on.set(true);
        self.ovh.set(crate::perf::snap_overhead());
    }
    #[inline]
    pub fn reset(&self) {
        if self.on.get() {
            self.last.set(crate::perf::snap().instrs);
        }
    }
    /// Attribute instructions since the last mark to phase `i`.
    #[inline]
    pub fn mark(&self, i: usize) {
        if self.on.get() {
            let s = crate::perf::snap().instrs;
            self.acc[i].set(self.acc[i].get() + (s - self.last.get()).saturating_sub(self.ovh.get()));
            self.last.set(crate::perf::snap().instrs);
        }
    }
    pub fn take(&self) -> [u64; 10] {
        let mut o = [0; 10];
        for i in 0..10 {
            o[i] = self.acc[i].replace(0);
        }
        o
    }
}

pub struct Gpu {
    pub lib: Retained<ProtocolObject<dyn MTLLibrary>>,
    pub prof: Prof,
    pub device: Dev,
    pub queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    /// second queue for readback/reference work, so it never queues behind a pre-armed (event-waiting) batch
    pub aux: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pub pso_cs: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    pub pso_cs32: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    pub pso_bin32: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    pub pso_tile32b: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    /// row bins of the binned compute repair (GPU-private): 45 rows x 4096 x (instance, ink) and per-row counts
    pub bins: Retained<ProtocolObject<dyn MTLBuffer>>,
    pub bin_counts: Retained<ProtocolObject<dyn MTLBuffer>>,
    pso_main: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pso_vclip: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// entries emitted by the last vclip cull (diagnostic)
    pub vclip_entries: Cell<usize>,
    pso_main_st: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pso_stencil: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    ds_write: Retained<ProtocolObject<dyn MTLDepthStencilState>>,
    ds_test: Retained<ProtocolObject<dyn MTLDepthStencilState>>,
    pub pool: Retained<ProtocolObject<dyn MTLBuffer>>,
    pub pool_ptr: *mut u8,
    pub atlas: Tex,
    #[allow(dead_code)]
    stencil_tex: Tex,
    pd_plain: Retained<MTLRenderPassDescriptor>,
    pd_stencil: Retained<MTLRenderPassDescriptor>,
    c0_plain: Retained<MTLRenderPassColorAttachmentDescriptor>,
    c0_stencil: Retained<MTLRenderPassColorAttachmentDescriptor>,
    last_load: Cell<[i8; 2]>,
    pub times: StartupTimes,
    /// Number of ObjC message sends issued by the encode path (counted at call sites).
    pub msgs: Cell<u32>,
    pub draws: Cell<u32>,
}

/// 16-px plane -> list of damaged 32x32 tiles ((ty << 16) | tx), a 32-px tile being damaged iff any of its four
/// 16-px tiles is. (The bough-core ring is 32-px natively; this adapter exists only for this crate's 16-px history.)
pub fn tiles32(p: &crate::damage::Plane, out: &mut Vec<u32>) {
    out.clear();
    if !p.any {
        return;
    }
    const EVEN: u64 = 0x5555_5555_5555_5555;
    let (r0, r1) = (p.r0 as usize, p.r1 as usize);
    for ty in r0 / 2..=r1 / 2 {
        let mut m = [0u64; 2];
        for y in [2 * ty, 2 * ty + 1] {
            if y >= r0 && y <= r1 {
                m[0] |= p.rows[y][0];
                m[1] |= p.rows[y][1];
            }
        }
        for w in 0..2 {
            let mut b = (m[w] | m[w] >> 1) & EVEN; // bit 2k set iff 16-px tile 2k or 2k+1 is damaged
            while b != 0 {
                let tx = (w * 64 + b.trailing_zeros() as usize) / 2;
                b &= b - 1;
                if tx < 60 {
                    out.push((ty as u32) << 16 | tx as u32);
                }
            }
        }
    }
}

/// Runs per tile row the vertex-clipped stream draws (must match `VK` in shader.metal).
pub const VK: u32 = 4;

/// Fill the smallest gaps of a 128-tile row until it has at most `k` runs of set bits. The result depends only on
/// the row, so every instance sees the same superset (which keeps the repair exact).
#[inline]
pub fn canon_row(mut b: u128, k: u32) -> u128 {
    while (b & !(b << 1)).count_ones() > k {
        let (mut best_len, mut best_at) = (u32::MAX, 0u32);
        let mut pos = b.trailing_zeros();
        loop {
            let end = pos + (b >> pos).trailing_ones();
            if end >= 128 || (b >> end) == 0 {
                break;
            }
            let next = end + (b >> end).trailing_zeros();
            if next - end < best_len {
                best_len = next - end;
                best_at = end;
            }
            pos = next;
        }
        b |= ((1u128 << best_len) - 1) << best_at;
    }
    b
}

/// Bits [a, b) of the 64-bit word starting at tile `base`.
#[inline]
fn span64(a: u32, b: u32, base: u32) -> u64 {
    let (s, e) = (a.max(base), b.min(base + 64));
    if s >= e {
        0
    } else if e - s == 64 {
        !0
    } else {
        ((1u64 << (e - s)) - 1) << (s - base)
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

impl Gpu {
    pub fn new(src: ShaderSource) -> Gpu {
        let mut times = StartupTimes::default();
        let t = Instant::now();
        let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
        let queue = device.newCommandQueue().expect("queue");
        let aux = device.newCommandQueue().expect("aux queue");
        times.device_ms = ms(t);

        let t = Instant::now();
        let lib = match src {
            ShaderSource::Metallib => load_library(&device),
            ShaderSource::Runtime => {
                let s = NSString::from_str(SHADER_SRC);
                device.newLibraryWithSource_options_error(&s, None).expect("msl compile")
            }
        };
        times.library_ms = ms(t);

        let t = Instant::now();
        let vs = lib.newFunctionWithName(ns_string!("vs_main")).unwrap();
        let fs = lib.newFunctionWithName(ns_string!("fs_main")).unwrap();
        let vs_r = lib.newFunctionWithName(ns_string!("vs_rects")).unwrap();
        let vs_c = lib.newFunctionWithName(ns_string!("vs_vclip")).unwrap();
        let mk2 = |vsf: &ProtocolObject<dyn MTLFunction>, stencil: bool| {
            let d = MTLRenderPipelineDescriptor::new();
            d.setVertexFunction(Some(vsf));
            d.setFragmentFunction(Some(&fs));
            let ca = unsafe { d.colorAttachments().objectAtIndexedSubscript(0) };
            ca.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            ca.setBlendingEnabled(true);
            ca.setSourceRGBBlendFactor(MTLBlendFactor::One);
            ca.setSourceAlphaBlendFactor(MTLBlendFactor::One);
            ca.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
            ca.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
            if stencil {
                d.setStencilAttachmentPixelFormat(MTLPixelFormat::Stencil8);
            }
            device.newRenderPipelineStateWithDescriptor_error(&d).expect("pso")
        };
        let mk = |stencil: bool| mk2(&vs, stencil);
        let pso_main = mk(false);
        let pso_main_st = mk(true);
        let pso_vclip = mk2(&vs_c, false);
        let pso_stencil = {
            let d = MTLRenderPipelineDescriptor::new();
            d.setVertexFunction(Some(&vs_r));
            let ca = unsafe { d.colorAttachments().objectAtIndexedSubscript(0) };
            ca.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            ca.setWriteMask(MTLColorWriteMask::None);
            d.setStencilAttachmentPixelFormat(MTLPixelFormat::Stencil8);
            device.newRenderPipelineStateWithDescriptor_error(&d).expect("pso stencil")
        };
        let ds = |write: bool| {
            let sd = MTLStencilDescriptor::new();
            if write {
                sd.setStencilCompareFunction(MTLCompareFunction::Always);
                sd.setDepthStencilPassOperation(MTLStencilOperation::Replace);
                sd.setWriteMask(0xff);
            } else {
                sd.setStencilCompareFunction(MTLCompareFunction::Equal);
                sd.setDepthStencilPassOperation(MTLStencilOperation::Keep);
                sd.setWriteMask(0);
            }
            sd.setReadMask(0xff);
            let d = MTLDepthStencilDescriptor::new();
            d.setFrontFaceStencil(Some(&sd));
            d.setBackFaceStencil(Some(&sd));
            device.newDepthStencilStateWithDescriptor(&d).expect("ds")
        };
        let cs = lib.newFunctionWithName(ns_string!("cs_repair")).unwrap();
        let pso_cs = device.newComputePipelineStateWithFunction_error(&cs).expect("pso cs");
        let cs32 = lib.newFunctionWithName(ns_string!("cs_repair32")).unwrap();
        let pso_cs32 = device.newComputePipelineStateWithFunction_error(&cs32).expect("pso cs32");
        let f = lib.newFunctionWithName(ns_string!("cs_bin32")).unwrap();
        let pso_bin32 = device.newComputePipelineStateWithFunction_error(&f).expect("pso bin32");
        let f = lib.newFunctionWithName(ns_string!("cs_tile32b")).unwrap();
        let pso_tile32b = device.newComputePipelineStateWithFunction_error(&f).expect("pso tile32b");
        let bins = device.newBufferWithLength_options(45 * 8192 * 16, MTLResourceOptions::StorageModePrivate).expect("bins");
        let bin_counts = device.newBufferWithLength_options(64 * 4, MTLResourceOptions::StorageModePrivate).expect("bin counts");
        let ds_write = ds(true);
        let ds_test = ds(false);
        times.pipelines_ms = ms(t);

        let t = Instant::now();
        let pool = device
            .newBufferWithLength_options(POOL_BYTES, MTLResourceOptions::StorageModeShared)
            .expect("pool");
        let pool_ptr = pool.contents().as_ptr() as *mut u8;
        let ad = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::R8Unorm, ATLAS_W, ATLAS_H, false) };
        let atlas = device.newTextureWithDescriptor(&ad).expect("atlas");
        let bytes = make_atlas();
        unsafe {
            atlas.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                MTLRegion { origin: MTLOrigin { x: 0, y: 0, z: 0 }, size: MTLSize { width: ATLAS_W, height: ATLAS_H, depth: 1 } },
                0,
                NonNull::new(bytes.as_ptr() as *mut _).unwrap(),
                ATLAS_W,
            );
        }
        let sd = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::Stencil8, W as usize, H as usize, false) };
        sd.setStorageMode(MTLStorageMode::Memoryless);
        sd.setUsage(MTLTextureUsage::RenderTarget);
        let stencil_tex = device.newTextureWithDescriptor(&sd).expect("stencil tex");
        times.buffers_atlas_ms = ms(t);

        let mkpd = |stencil: bool| {
            let pd = MTLRenderPassDescriptor::renderPassDescriptor();
            if stencil {
                let sa = pd.stencilAttachment();
                sa.setTexture(Some(&stencil_tex));
                sa.setLoadAction(MTLLoadAction::Clear);
                sa.setStoreAction(MTLStoreAction::DontCare);
                sa.setClearStencil(0);
            }
            pd
        };
        let pd_plain = mkpd(false);
        let pd_stencil = mkpd(true);
        let mkc0 = |pd: &Retained<MTLRenderPassDescriptor>| {
            let c0 = unsafe { pd.colorAttachments().objectAtIndexedSubscript(0) };
            c0.setStoreAction(MTLStoreAction::Store);
            c0
        };
        let c0_plain = mkc0(&pd_plain);
        let c0_stencil = mkc0(&pd_stencil);

        Gpu {
            lib,
            device,
            queue,
            aux,
            pso_cs,
            pso_cs32,
            pso_bin32,
            pso_tile32b,
            bins,
            bin_counts,
            pso_main,
            pso_vclip,
            vclip_entries: Cell::new(0),
            pso_main_st,
            pso_stencil,
            ds_write,
            ds_test,
            pool,
            pool_ptr,
            atlas,
            stencil_tex,
            pd_plain,
            pd_stencil,
            c0_plain,
            c0_stencil,
            last_load: Cell::new([-1, -1]),
            times,
            prof: Prof::new(),
            msgs: Cell::new(0),
            draws: Cell::new(0),
        }
    }

    pub fn new_texture(&self, storage: MTLStorageMode, framebuffer_readable: bool) -> Tex {
        let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, W as usize, H as usize, false) };
        d.setStorageMode(storage);
        d.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderWrite | if framebuffer_readable { MTLTextureUsage::ShaderRead } else { MTLTextureUsage::empty() });
        self.device.newTextureWithDescriptor(&d).expect("texture")
    }

    #[inline]
    fn bump(&self, n: u32) {
        self.msgs.set(self.msgs.get() + n);
    }

    /// CPU-cull instance indices overlapping `r` into list[pos..]; returns count. Paint order preserved.
    #[inline]
    fn cull(&self, scene: &Scene, r: &IRect, pos: usize) -> usize {
        let lp = scene.list_ptr();
        let mut o = pos;
        for (pg, pi) in scene.page_ink.iter().enumerate().take((scene.n + 63) / 64) {
            if pi.is_empty() || !pi.overlaps(r) {
                continue;
            }
            let (lo, hi) = (pg * 64, ((pg + 1) * 64).min(scene.n));
            for (i, k) in scene.ink[lo..hi].iter().enumerate() {
                unsafe { *lp.add(o) = (lo + i) as u32 };
                o += (!k.is_empty() && k.overlaps(r)) as usize;
            }
        }
        o - pos
    }

    /// Encode the repair of `region` into `target` (colour attachment 0, BGRA8).
    /// Returns number of draw calls emitted. Message sends are counted into `self.msgs`.
    pub fn encode_repair(&self, cmd: &Cmd, target: &Tex, scene: &Scene, region: &Region, mode: DamageMode) -> u32 {
        let lp = scene.list_ptr();
        let mut draws = 0u32;
        let full = region.full;
        let mode = if full { DamageMode::Bbox } else { mode };
        let stencil = mode == DamageMode::Stencil && region.rects.len() > 1;
        let pd = if stencil { &self.pd_stencil } else { &self.pd_plain };

        // --- CPU: draw list(s) in the shared pool ------------------------------------------------
        let mut ranges: [(usize, usize); 1] = [(0, 0)];
        let mut multi_ranges: Vec<(usize, usize)> = Vec::new();
        let per_rect_culled = mode == DamageMode::MultiCulled && region.rects.len() * scene.n <= MAX_LIST;
        if full {
            for i in 0..scene.n {
                unsafe { *lp.add(i) = i as u32 };
            }
            ranges[0] = (0, scene.n);
        } else if per_rect_culled {
            let mut pos = 0;
            for r in &region.rects {
                let c = self.cull(scene, r, pos);
                multi_ranges.push((pos, c));
                pos += c;
            }
        } else {
            let c = self.cull(scene, &region.bbox, 0);
            ranges[0] = (0, c);
        }
        if stencil {
            let rp = scene.rects_ptr();
            for (k, r) in region.rects.iter().enumerate() {
                unsafe { *rp.add(k) = [r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32] };
            }
        }

        self.prof.mark(1); // draw-list cull
        // --- encode --------------------------------------------------------------------------------
        // The descriptor and its colour attachment are cached; only the texture (and, when it changes, the load action) is set.
        let c0 = if stencil { &self.c0_stencil } else { &self.c0_plain };
        c0.setTexture(Some(target));
        self.bump(1);
        let want = if full { 0i8 } else { 1 };
        let li = stencil as usize;
        let mut ll = self.last_load.get();
        if ll[li] != want {
            c0.setLoadAction(if full { MTLLoadAction::DontCare } else { MTLLoadAction::Load });
            ll[li] = want;
            self.last_load.set(ll);
            self.bump(1);
        }
        let enc = cmd.renderCommandEncoderWithDescriptor(pd).expect("encoder");
        self.bump(1);
        unsafe {
            if stencil {
                enc.setRenderPipelineState(&self.pso_stencil);
                enc.setDepthStencilState(Some(&self.ds_write));
                enc.setStencilReferenceValue(1);
                enc.setVertexBuffer_offset_atIndex(Some(&self.pool), 0, 0);
                enc.setScissorRect(MTLScissorRect { x: 0, y: 0, width: W as usize, height: H as usize });
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount(MTLPrimitiveType::TriangleStrip, 0, 4, region.rects.len());
                enc.setRenderPipelineState(&self.pso_main_st);
                enc.setDepthStencilState(Some(&self.ds_test));
                self.bump(6);
                draws += 1;
            } else {
                enc.setRenderPipelineState(&self.pso_main);
                enc.setVertexBuffer_offset_atIndex(Some(&self.pool), 0, 0);
                self.bump(2);
            }
            enc.setFragmentTexture_atIndex(Some(&self.atlas), 0);
            self.bump(1);
            if mode == DamageMode::MultiShared || per_rect_culled {
                for (k, r) in region.rects.iter().enumerate() {
                    let (start, count) = if per_rect_culled { multi_ranges[k] } else { ranges[0] };
                    if count == 0 {
                        continue;
                    }
                    enc.setScissorRect(MTLScissorRect { x: r.x0 as usize, y: r.y0 as usize, width: (r.x1 - r.x0) as usize, height: (r.y1 - r.y0) as usize });
                    enc.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(MTLPrimitiveType::TriangleStrip, 0, 4, count, start);
                    self.bump(2);
                    draws += 1;
                }
            } else {
                let (start, count) = ranges[0];
                let b = region.bbox;
                enc.setScissorRect(MTLScissorRect { x: b.x0 as usize, y: b.y0 as usize, width: (b.x1 - b.x0) as usize, height: (b.y1 - b.y0) as usize });
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(MTLPrimitiveType::TriangleStrip, 0, 4, count, start);
                self.bump(2);
                draws += 1;
            }
            enc.endEncoding();
            self.bump(1);
        }
        self.prof.mark(2); // encode
        self.draws.set(self.draws.get() + draws);
        draws
    }

    /// Vertex-clipped repair (format C expanded on the GPU): upload the 16-px plane, cull instances against it
    /// (entry = instance | first-row << 16 per 8 tile rows that meet damage in the instance's x-span), one draw of
    /// entries x 32 sub-quads, loadAction = Load, no stencil, no scissor.
    pub fn encode_vclip(&self, cmd: &Cmd, target: &Tex, scene: &Scene, p: &crate::damage::Plane) -> u32 {
        use crate::damage::TY;
        let pl = unsafe { self.pool_ptr.add(PLANE_OFF) as *mut [u64; 2] };
        unsafe { std::ptr::write_bytes(pl, 0, TY) };
        if !p.any {
            return 0;
        }
        let (r0, r1) = (p.r0 as usize, p.r1 as usize);
        let mut xo = [0u64; 2];
        let mut rows = [[0u64; 2]; TY];
        for y in r0..=r1 {
            let b = canon_row((p.rows[y][0] as u128) | (p.rows[y][1] as u128) << 64, VK);
            rows[y] = [b as u64, (b >> 64) as u64];
            unsafe { *pl.add(y) = rows[y] };
            xo[0] |= rows[y][0];
            xo[1] |= rows[y][1];
        }
        let lo_t = if xo[0] != 0 { xo[0].trailing_zeros() } else { 64 + xo[1].trailing_zeros() } as i32;
        let hi_t = if xo[1] != 0 { 128 - xo[1].leading_zeros() } else { 64 - xo[0].leading_zeros() } as i32;
        let bb = IRect::new(lo_t * 16, r0 as i32 * 16, hi_t * 16, (r1 as i32 + 1) * 16);
        self.prof.mark(0);
        let lp = scene.list_ptr();
        let mut o = 0usize;
        for (pg, pi) in scene.page_ink.iter().enumerate().take((scene.n + 63) / 64) {
            if pi.is_empty() || !pi.overlaps(&bb) {
                continue;
            }
            let (lo, hi) = (pg * 64, ((pg + 1) * 64).min(scene.n));
            for i in lo..hi {
                let k = scene.ink[i];
                if k.is_empty() || !k.overlaps(&bb) {
                    continue;
                }
                let (tx0, tx1) = ((k.x0 >> 4) as u32, (((k.x1 - 1) >> 4) + 1) as u32); // [tx0, tx1)
                let m0 = span64(tx0, tx1, 0);
                let m1 = span64(tx0, tx1, 64);
                let (ty0, ty1) = ((k.y0 >> 4) as usize, ((k.y1 - 1) >> 4) as usize);
                let mut c = ty0;
                while c <= ty1 {
                    let (a, b) = (c.max(r0), (c + 7).min(ty1).min(r1));
                    let mut hit = false;
                    let mut y = a;
                    while y <= b && !hit {
                        hit = rows[y][0] & m0 | rows[y][1] & m1 != 0;
                        y += 1;
                    }
                    if hit {
                        unsafe { *lp.add(o) = i as u32 | (c as u32) << 16 };
                        o += 1;
                    }
                    c += 8;
                }
            }
        }
        self.vclip_entries.set(o);
        self.prof.mark(1);
        self.c0_plain.setTexture(Some(target));
        let mut ll = self.last_load.get();
        if ll[0] != 1 {
            self.c0_plain.setLoadAction(MTLLoadAction::Load);
            ll[0] = 1;
            self.last_load.set(ll);
            self.bump(1);
        }
        let enc = cmd.renderCommandEncoderWithDescriptor(&self.pd_plain).expect("encoder");
        unsafe {
            enc.setRenderPipelineState(&self.pso_vclip);
            enc.setVertexBuffer_offset_atIndex(Some(&self.pool), 0, 0);
            enc.setFragmentTexture_atIndex(Some(&self.atlas), 0);
            if o > 0 {
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount(MTLPrimitiveType::TriangleStrip, 0, 4, o * 32);
            }
        }
        enc.endEncoding();
        self.bump(7);
        self.prof.mark(2);
        self.draws.set(self.draws.get() + 1);
        1
    }

    /// Write a tile list into frame slot `slot` (header: indirect args, target, count). Returns the slot's pool offset.
    pub fn write_slot(&self, slot: usize, target: u32, tiles: &[u32]) -> usize {
        self.write_slot_ex(slot, target, tiles, false)
    }

    /// `full`: dispatch every tile (threadgroup index = tile), no list is written.
    pub fn write_slot_ex(&self, slot: usize, target: u32, tiles: &[u32], full: bool) -> usize {
        self.write_slot_w(slot, target, tiles, full, false)
    }

    /// `wide`: tiles are 32x32 (60 x 45 grid) for `cs_repair32`.
    pub fn write_slot_w(&self, slot: usize, target: u32, tiles: &[u32], full: bool, wide: bool) -> usize {
        let off = FRAME_OFF + slot * SLOT_BYTES;
        if full {
            unsafe {
                let h = self.pool_ptr.add(off) as *mut u32;
                let n = if wide { 60 * 45 } else { (crate::damage::TX * crate::damage::TY) as u32 };
                *h = n;
                *h.add(1) = 1;
                *h.add(2) = 1;
                *h.add(3) = target;
                *h.add(4) = n;
                *h.add(5) = 1;
            }
            return off;
        }
        debug_assert!(32 + tiles.len() * 4 <= SLOT_BYTES);
        unsafe {
            let h = self.pool_ptr.add(off) as *mut u32;
            *h = tiles.len() as u32;
            *h.add(1) = 1;
            *h.add(2) = 1;
            *h.add(3) = target;
            *h.add(4) = tiles.len() as u32;
            *h.add(5) = 0;
            std::ptr::copy_nonoverlapping(tiles.as_ptr(), self.pool_ptr.add(off + 32) as *mut u32, tiles.len());
        }
        off
    }

    /// Encode one compute tile repair: textures = [atlas, t0..t3]; the kernel picks t[target] from the slot.
    /// `indirect` reads the threadgroup count from the slot (constant command stream); otherwise it is immediate.
    pub fn encode_compute(&self, cmd: &ProtocolObject<dyn MTLCommandBuffer>, slot_off: usize, targets: [&Tex; 4], ntiles: usize, indirect: bool) {
        self.encode_compute_w(cmd, slot_off, targets, ntiles, indirect, false)
    }

    pub fn encode_compute_w(&self, cmd: &ProtocolObject<dyn MTLCommandBuffer>, slot_off: usize, targets: [&Tex; 4], ntiles: usize, indirect: bool, wide: bool) {
        let enc = cmd.computeCommandEncoder().expect("compute encoder");
        enc.setComputePipelineState(if wide { &self.pso_cs32 } else { &self.pso_cs });
        let off = slot_off as u32;
        unsafe {
            enc.setBuffer_offset_atIndex(Some(&self.pool), 0, 0);
            enc.setBytes_length_atIndex(NonNull::new(&off as *const u32 as *mut _).unwrap(), 4, 1);
            let texs: [*const ProtocolObject<dyn MTLTexture>; 5] = [
                Retained::as_ptr(&self.atlas), Retained::as_ptr(targets[0]), Retained::as_ptr(targets[1]), Retained::as_ptr(targets[2]), Retained::as_ptr(targets[3]),
            ];
            enc.setTextures_withRange(NonNull::new(texs.as_ptr() as *mut _).unwrap(), objc2_foundation::NSRange::new(0, 5));
            let tg = MTLSize { width: 16, height: 16, depth: 1 };
            if indirect {
                enc.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(&self.pool, slot_off, tg);
            } else {
                enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: ntiles, height: 1, depth: 1 }, tg);
            }
        }
        enc.endEncoding();
        self.bump(6);
    }

    /// Binned 32x32 repair: also writes the damaged-row list (ty | tx0 << 8 | tx1 << 16) into the slot tail and the
    /// bin pass's indirect args. `tiles` are 32-px tiles in row-major order (as `tiles32` emits them).
    pub fn write_rows(&self, slot: usize, tiles: &[u32], full: bool) {
        let off = FRAME_OFF + slot * SLOT_BYTES + SLOT_BYTES - 256;
        unsafe {
            let h = self.pool_ptr.add(off) as *mut u32;
            let rows = h.add(4);
            let mut n = 0usize;
            if full {
                for ty in 0..45u32 {
                    *rows.add(n) = ty | 60 << 16;
                    n += 1;
                }
            } else {
                let mut i = 0;
                while i < tiles.len() {
                    let ty = tiles[i] >> 16;
                    let tx0 = tiles[i] & 0xffff;
                    let mut tx1 = tx0 + 1;
                    while i < tiles.len() && tiles[i] >> 16 == ty {
                        tx1 = (tiles[i] & 0xffff) + 1;
                        i += 1;
                    }
                    *rows.add(n) = ty | tx0 << 8 | tx1 << 16;
                    n += 1;
                }
            }
            *h = n as u32;
            *h.add(1) = 1;
            *h.add(2) = 1;
        }
    }

    /// Two dispatches in one encoder: bin pass (one threadgroup per damaged row), buffer barrier, tile pass.
    pub fn encode_binned(&self, cmd: &ProtocolObject<dyn MTLCommandBuffer>, slot_off: usize, targets: [&Tex; 4], nrows: usize, ntiles: usize, indirect: bool) {
        let enc = cmd.computeCommandEncoder().expect("compute encoder");
        let off = slot_off as u32;
        let tg = MTLSize { width: 256, height: 1, depth: 1 };
        unsafe {
            enc.setBuffer_offset_atIndex(Some(&self.pool), 0, 0);
            enc.setBytes_length_atIndex(NonNull::new(&off as *const u32 as *mut _).unwrap(), 4, 1);
            enc.setBuffer_offset_atIndex(Some(&self.bins), 0, 2);
            enc.setBuffer_offset_atIndex(Some(&self.bin_counts), 0, 3);
            enc.setComputePipelineState(&self.pso_bin32);
            if indirect {
                enc.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(&self.pool, slot_off + SLOT_BYTES - 256, tg);
            } else {
                enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: nrows, height: 1, depth: 1 }, tg);
            }
            enc.memoryBarrierWithScope(MTLBarrierScope::Buffers);
            enc.setComputePipelineState(&self.pso_tile32b);
            let texs: [*const ProtocolObject<dyn MTLTexture>; 5] = [
                Retained::as_ptr(&self.atlas), Retained::as_ptr(targets[0]), Retained::as_ptr(targets[1]), Retained::as_ptr(targets[2]), Retained::as_ptr(targets[3]),
            ];
            enc.setTextures_withRange(NonNull::new(texs.as_ptr() as *mut _).unwrap(), objc2_foundation::NSRange::new(0, 5));
            let tg2 = MTLSize { width: 16, height: 16, depth: 1 };
            if indirect {
                enc.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(&self.pool, slot_off, tg2);
            } else {
                enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: ntiles, height: 1, depth: 1 }, tg2);
            }
        }
        enc.endEncoding();
        self.bump(11);
    }

    /// Full-redraw reference through the BINNED 32x32 kernels (explicit list of all 2700 tiles).
    pub fn reference_binned(&self) -> Vec<u8> {
        let tex = self.new_texture(MTLStorageMode::Shared, true);
        let all: Vec<u32> = (0..45u32).flat_map(|y| (0..60u32).map(move |x| y << 16 | x)).collect();
        let off = self.write_slot_w(2, 0, &all, false, true);
        self.write_rows(2, &all, false);
        let cmd = self.aux.commandBuffer().unwrap();
        self.encode_binned(&cmd, off, [&tex, &tex, &tex, &tex], 45, all.len(), false);
        cmd.commit();
        cmd.waitUntilCompleted();
        self.read_texture(&tex)
    }

    /// Full-redraw reference through the 32x32 COMPUTE kernel (explicit list of all 2700 tiles).
    pub fn reference_compute32(&self) -> Vec<u8> {
        let tex = self.new_texture(MTLStorageMode::Shared, true);
        let all: Vec<u32> = (0..45u32).flat_map(|y| (0..60u32).map(move |x| y << 16 | x)).collect();
        let off = self.write_slot_w(2, 0, &all, false, true);
        let cmd = self.aux.commandBuffer().unwrap();
        self.encode_compute_w(&cmd, off, [&tex, &tex, &tex, &tex], all.len(), false, true);
        cmd.commit();
        cmd.waitUntilCompleted();
        self.read_texture(&tex)
    }

    /// Full-redraw reference through the COMPUTE kernel (all tiles), returned as bytes.
    pub fn reference_compute(&self) -> Vec<u8> {
        let tex = self.new_texture(MTLStorageMode::Shared, true);
        // the reference walks an explicit list (not the `full` flag), so the flag path is itself under test
        let all: Vec<u32> = (0..crate::damage::TY as u32).flat_map(|y| (0..crate::damage::TX as u32).map(move |x| y << 16 | x)).collect();
        let off = self.write_slot(2, 0, &all);
        let cmd = self.aux.commandBuffer().unwrap();
        self.encode_compute(&cmd, off, [&tex, &tex, &tex, &tex], all.len(), false);
        cmd.commit();
        cmd.waitUntilCompleted();
        self.read_texture(&tex)
    }

    /// Readback helper: copy a texture into a fresh shared buffer (tight pitch). Waits for completion.
    pub fn read_texture(&self, tex: &Tex) -> Vec<u8> {
        let n = W as usize * 4 * H as usize;
        let buf = self.device.newBufferWithLength_options(n, MTLResourceOptions::StorageModeShared).unwrap();
        let cmd = self.aux.commandBuffer().unwrap();
        let blit = cmd.blitCommandEncoder().unwrap();
        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                tex, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: W as usize, height: H as usize, depth: 1 },
                &buf, 0, W as usize * 4, n,
            );
        }
        blit.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        let mut v = vec![0u8; n];
        unsafe { std::ptr::copy_nonoverlapping(buf.contents().as_ptr() as *const u8, v.as_mut_ptr(), n) };
        v
    }

    /// Full-redraw reference of the current scene into a private scratch texture, returned as bytes.
    pub fn reference(&self, scene: &Scene) -> Vec<u8> {
        let tex = self.new_texture(MTLStorageMode::Shared, true);
        let cmd = self.aux.commandBuffer().unwrap();
        let mut full = Region::full();
        full.set_full();
        self.encode_repair(&cmd, &tex, scene, &full, DamageMode::Bbox);
        cmd.commit();
        cmd.waitUntilCompleted();
        self.read_texture(&tex)
    }
}
