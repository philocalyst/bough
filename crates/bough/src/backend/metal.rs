//! The winning presenter of bough-metal's RESULTS (IOSurface ring + completion-handler publish,
//! stencil-masked repair, ONE instanced draw), re-implemented here at a runtime window size over
//! the façade's pool layout (bough-metal's version has 1920x1440 and 4,096 instances compiled in
//! and its own scene type). Shader: `shader.metal` (bough-text's instance layout).
use crate::rt::*;
use block2::RcBlock;
use bough_text::atlas::Atlas;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_core_foundation::{CFDictionary, CFRetained};
use objc2_foundation::{ns_string, NSDictionary, NSNumber, NSString};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::*;
use objc2_quartz_core::{CALayer, CATransaction};
use dispatch2::DispatchData;
use std::ptr::NonNull;

pub type Dev = Retained<ProtocolObject<dyn MTLDevice>>;
pub type Tex = Retained<ProtocolObject<dyn MTLTexture>>;
pub type Cmd = Retained<ProtocolObject<dyn MTLCommandBuffer>>;

static METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shader.metallib"));

pub struct Surface {
    pub io: CFRetained<IOSurfaceRef>,
    pub tex: Tex,
}

pub struct Metal {
    pub device: Dev,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pso_main: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pso_main_st: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pso_stencil: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    ds_write: Retained<ProtocolObject<dyn MTLDepthStencilState>>,
    ds_test: Retained<ProtocolObject<dyn MTLDepthStencilState>>,
    pool: Retained<ProtocolObject<dyn MTLBuffer>>,
    pool_ptr: *mut u8,
    atlas: Tex,
    atlas_dims: (u32, u32),
    #[allow(dead_code)]
    stencil_tex: Tex,
    pd_plain: Retained<MTLRenderPassDescriptor>,
    pd_stencil: Retained<MTLRenderPassDescriptor>,
    c0_plain: Retained<MTLRenderPassColorAttachmentDescriptor>,
    c0_stencil: Retained<MTLRenderPassColorAttachmentDescriptor>,
    last_load: [i8; 2],
    pub ring: Vec<Surface>,
    pub layer: Retained<CALayer>,
    last: Option<Cmd>,
    pub w: usize,
    pub h: usize,
    /// Publish surfaces to `layer` from the completion handler (off for headless runs).
    pub publish: bool,
    pub last_idx: usize,
    scratch: Option<Surface>,
    pub gpu_frames: u64,
}

fn make_iosurface(w: usize, h: usize) -> CFRetained<IOSurfaceRef> {
    let keys: [&NSString; 4] = [ns_string!("IOSurfaceWidth"), ns_string!("IOSurfaceHeight"), ns_string!("IOSurfaceBytesPerElement"), ns_string!("IOSurfacePixelFormat")];
    let vals = [NSNumber::new_i32(w as i32), NSNumber::new_i32(h as i32), NSNumber::new_i32(4), NSNumber::new_u32(0x4247_5241)]; // 'BGRA'
    let refs: Vec<&AnyObject> = vals.iter().map(|v| v.as_ref() as &AnyObject).collect();
    let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&keys, &refs);
    let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&dict) as *const CFDictionary) };
    unsafe { IOSurfaceRef::new(cf) }.expect("IOSurface")
}

impl Metal {
    pub fn new(w: usize, h: usize, scale: f64, ring: usize) -> Metal {
        let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
        let queue = device.newCommandQueue().expect("queue");
        let data = DispatchData::from_static_bytes(METALLIB);
        let lib = device.newLibraryWithData_error(&data).expect("metallib load");
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

        let pool = device.newBufferWithLength_options(POOL_BYTES, MTLResourceOptions::StorageModeShared).expect("pool");
        let pool_ptr = pool.contents().as_ptr() as *mut u8;
        let atlas = Self::new_atlas(&device, 1024, 512);
        let sd = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::Stencil8, w, h, false) };
        sd.setStorageMode(MTLStorageMode::Memoryless);
        sd.setUsage(MTLTextureUsage::RenderTarget);
        let stencil_tex = device.newTextureWithDescriptor(&sd).expect("stencil tex");
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

        let mk_surface = || {
            let io = make_iosurface(w, h);
            let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, w, h, false) };
            d.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
            d.setStorageMode(MTLStorageMode::Shared);
            let tex = device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0).expect("iosurface texture");
            Surface { io, tex }
        };
        let ring: Vec<Surface> = (0..ring).map(|_| mk_surface()).collect();
        let layer = CALayer::new();
        layer.setContentsScale(scale);
        layer.setOpaque(true);
        layer.setFrame(objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(w as f64 / scale, h as f64 / scale),
        ));
        Metal {
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
            atlas_dims: (1024, 512),
            stencil_tex,
            pd_plain,
            pd_stencil,
            c0_plain,
            c0_stencil,
            last_load: [-1, -1],
            ring,
            layer,
            last: None,
            w,
            h,
            publish: false,
            last_idx: 0,
            scratch: None,
            gpu_frames: 0,
        }
    }

    fn new_atlas(device: &Dev, w: u32, h: u32) -> Tex {
        let ad = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::R8Unorm, w as usize, h as usize, false) };
        device.newTextureWithDescriptor(&ad).expect("atlas")
    }

    pub fn pool_ptr(&self) -> *mut u8 {
        self.pool_ptr
    }

    /// Wait for the previous frame (the CPU is about to write the shared pool).
    #[inline]
    pub fn sync(&mut self) {
        if let Some(c) = &self.last {
            if c.status() != MTLCommandBufferStatus::Completed {
                c.waitUntilCompleted();
            }
        }
    }

    pub fn is_in_use(&self, i: usize) -> bool {
        self.ring[i].io.is_in_use()
    }

    /// Upload the glyph atlas if it changed since the last upload.
    pub fn sync_atlas(&mut self, a: &mut Atlas) {
        if a.dirty_bytes == 0 && (a.width, a.height) == self.atlas_dims {
            return;
        }
        if (a.width, a.height) != self.atlas_dims {
            self.atlas = Self::new_atlas(&self.device, a.width, a.height);
            self.atlas_dims = (a.width, a.height);
        }
        unsafe {
            self.atlas.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                MTLRegion { origin: MTLOrigin { x: 0, y: 0, z: 0 }, size: MTLSize { width: a.width as usize, height: a.height as usize, depth: 1 } },
                0,
                NonNull::new(a.pixels.as_ptr() as *mut _).unwrap(),
                a.width as usize,
            );
        }
        a.dirty_bytes = 0;
    }

    fn publish_surface(layer: &CALayer, io: &IOSurfaceRef) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        unsafe { layer.setContents(Some(&*(io as *const IOSurfaceRef as *const AnyObject))) };
        CATransaction::commit();
    }

    /// Encode the repair into `target` (`full`: one unscissored pass, loadAction DontCare; one rect:
    /// scissored plain pass; several rects: the stencil mask written from the rect table in the pool,
    /// then ONE draw). The draw list and rect table are already in the pool. Commits; when
    /// `publish_idx` is set the completion handler publishes that ring surface to the layer.
    pub fn encode(&mut self, target: &Tex, full: bool, rects: &[[i32; 4]], list_len: usize, publish_idx: Option<usize>) {
        let stencil = !full && rects.len() > 1;
        let pd = if stencil { &self.pd_stencil } else { &self.pd_plain };
        let cmd = self.queue.commandBuffer().unwrap();
        let c0 = if stencil { &self.c0_stencil } else { &self.c0_plain };
        c0.setTexture(Some(target));
        let want = if full { 0i8 } else { 1 };
        let li = stencil as usize;
        if self.last_load[li] != want {
            c0.setLoadAction(if full { MTLLoadAction::DontCare } else { MTLLoadAction::Load });
            self.last_load[li] = want;
        }
        let enc = cmd.renderCommandEncoderWithDescriptor(pd).expect("encoder");
        unsafe {
            if stencil {
                enc.setRenderPipelineState(&self.pso_stencil);
                enc.setDepthStencilState(Some(&self.ds_write));
                enc.setStencilReferenceValue(1);
                enc.setVertexBuffer_offset_atIndex(Some(&self.pool), 0, 0);
                enc.setScissorRect(MTLScissorRect { x: 0, y: 0, width: self.w, height: self.h });
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount(MTLPrimitiveType::TriangleStrip, 0, 4, rects.len());
                enc.setRenderPipelineState(&self.pso_main_st);
                enc.setDepthStencilState(Some(&self.ds_test));
            } else {
                enc.setRenderPipelineState(&self.pso_main);
                enc.setVertexBuffer_offset_atIndex(Some(&self.pool), 0, 0);
            }
            enc.setFragmentTexture_atIndex(Some(&self.atlas), 0);
            let b = if full { [0, 0, self.w as i32, self.h as i32] } else { union_rects(rects) };
            enc.setScissorRect(MTLScissorRect { x: b[0] as usize, y: b[1] as usize, width: (b[2] - b[0]) as usize, height: (b[3] - b[1]) as usize });
            enc.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(MTLPrimitiveType::TriangleStrip, 0, 4, list_len, 0);
            enc.endEncoding();
        }
        if let (true, Some(i)) = (self.publish, publish_idx) {
            let layer = self.layer.clone();
            let io = self.ring[i].io.clone();
            let blk = RcBlock::new(move |_c: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                Metal::publish_surface(&layer, &io);
            });
            unsafe { cmd.addCompletedHandler(RcBlock::as_ptr(&blk)) };
        }
        cmd.commit();
        self.last = Some(cmd);
        self.gpu_frames += 1;
    }

    /// Pixels of ring surface `i` (BGRA, tight rows), after the GPU is done with it.
    pub fn surface_pixels(&mut self, i: usize) -> Vec<u8> {
        self.sync();
        read_io(&self.ring[i].io, self.w, self.h)
    }

    /// Full redraw of the whole draw list into a private scratch surface; its pixels.
    pub fn reference_pixels(&mut self, list_len: usize) -> Vec<u8> {
        self.sync();
        if self.scratch.is_none() {
            let io = make_iosurface(self.w, self.h);
            let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, self.w, self.h, false) };
            d.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
            d.setStorageMode(MTLStorageMode::Shared);
            let tex = self.device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0).expect("scratch");
            self.scratch = Some(Surface { io, tex });
        }
        let tex = self.scratch.as_ref().unwrap().tex.clone();
        let (publish, full) = (self.publish, true);
        self.publish = false;
        self.last_load = [-1, -1];
        self.encode(&tex, full, &[[0, 0, self.w as i32, self.h as i32]], list_len, None);
        self.publish = publish;
        self.sync();
        read_io(&self.scratch.as_ref().unwrap().io, self.w, self.h)
    }

    pub fn surface_bytes(&self) -> u64 {
        self.ring.iter().map(|s| s.io.alloc_size() as u64).sum()
    }
}

fn union_rects(r: &[[i32; 4]]) -> [i32; 4] {
    let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for q in r {
        b = [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[2]), b[3].max(q[3])];
    }
    b
}

fn read_io(io: &IOSurfaceRef, w: usize, h: usize) -> Vec<u8> {
    let mut v = vec![0u8; w * 4 * h];
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
