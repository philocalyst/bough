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

pub static SHADER_SRC: &str = include_str!("shader.metal");
static METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shader.metallib"));

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
    pub prof: Prof,
    pub device: Dev,
    pub queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pso_main: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
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

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

impl Gpu {
    pub fn new(src: ShaderSource) -> Gpu {
        let mut times = StartupTimes::default();
        let t = Instant::now();
        let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
        let queue = device.newCommandQueue().expect("queue");
        times.device_ms = ms(t);

        let t = Instant::now();
        let lib = match src {
            ShaderSource::Metallib => {
                let data = DispatchData::from_static_bytes(METALLIB);
                device.newLibraryWithData_error(&data).expect("metallib load")
            }
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
        let mk = |stencil: bool| {
            let d = MTLRenderPipelineDescriptor::new();
            d.setVertexFunction(Some(&vs));
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
        let pso_main = mk(false);
        let pso_main_st = mk(true);
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
            device,
            queue,
            pso_main,
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
        d.setUsage(MTLTextureUsage::RenderTarget | if framebuffer_readable { MTLTextureUsage::ShaderRead } else { MTLTextureUsage::empty() });
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

    /// Readback helper: copy a texture into a fresh shared buffer (tight pitch). Waits for completion.
    pub fn read_texture(&self, tex: &Tex) -> Vec<u8> {
        let n = W as usize * 4 * H as usize;
        let buf = self.device.newBufferWithLength_options(n, MTLResourceOptions::StorageModeShared).unwrap();
        let cmd = self.queue.commandBuffer().unwrap();
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
        let cmd = self.queue.commandBuffer().unwrap();
        let mut full = Region::full();
        full.set_full();
        self.encode_repair(&cmd, &tex, scene, &full, DamageMode::Bbox);
        cmd.commit();
        cmd.waitUntilCompleted();
        self.read_texture(&tex)
    }
}
