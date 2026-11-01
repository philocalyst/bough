//! Engine (scene + damage history + pool) and the three presentation strategies.

use crate::damage::{History, Plane, Region};
use crate::gpu::*;
use crate::instance::*;
use crate::scene::Scene;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_core_foundation::{CFDictionary, CFRetained};
use objc2_foundation::{ns_string, NSDictionary, NSNumber, NSString};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::*;
use objc2_quartz_core::{CALayer, CAMetalDrawable, CAMetalLayer, CATransaction};
use std::collections::HashMap;
use std::ptr::NonNull;

pub struct Engine {
    pub gpu: Gpu,
    pub scene: Scene,
    pub hist: History,
    pub mode: DamageMode,
    pub region: Region,
    pub plane: Box<Plane>,
    pub capture: bool,
    cap_buf: Option<Retained<ProtocolObject<dyn MTLBuffer>>>,
    pub frames_presented: u64,
    pub sabotage: bool,
}

impl Engine {
    pub fn new(src: ShaderSource) -> Engine {
        let gpu = Gpu::new(src);
        let scene = Scene::new(gpu.pool_ptr);
        Engine {
            gpu,
            scene,
            hist: History::new(),
            mode: DamageMode::Bbox,
            region: Region::default(),
            plane: Box::new(Plane::new()),
            capture: false,
            cap_buf: None,
            frames_presented: 0,
            sabotage: std::env::var_os("BOUGH_SABOTAGE").is_some(),
        }
    }

    /// Commit the scene's accumulated damage as a new epoch and present it. `None` = idle (no work at all).
    pub fn frame(&mut self, p: &mut dyn Presenter) -> Option<u64> {
        if !self.scene.acc.any {
            return None;
        }
        let epoch = self.hist.push(&mut self.scene.acc);
        p.present(self, epoch);
        self.frames_presented += 1;
        Some(epoch)
    }

    /// Fill `self.region` with the repair for a buffer last drawn at epoch `since`.
    pub fn build_region(&mut self, since: Option<u64>) {
        // Test hook: BOUGH_SABOTAGE=1 makes ages one epoch too young; correctness must then FAIL.
        let since = if self.sabotage { since.map(|s| (s + 1).min(self.hist.epoch)) } else { since };
        if self.hist.repair(since, &mut self.plane) && !self.plane.full {
            self.region.from_plane(&self.plane);
            if self.region.bbox.is_empty() {
                self.region.rects.clear();
            }
        } else {
            self.region.set_full();
        }
    }

    /// Turn on pixel capture (allocates an 11 MB readback buffer; only tests do this).
    pub fn enable_capture(&mut self) {
        self.capture = true;
        self.cap_buf = Some(
            self.gpu.device.newBufferWithLength_options(W as usize * 4 * H as usize, MTLResourceOptions::StorageModeShared).unwrap(),
        );
    }

    pub fn encode_capture(&self, cmd: &Cmd, tex: &Tex) {
        let blit = cmd.blitCommandEncoder().unwrap();
        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                tex, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: W as usize, height: H as usize, depth: 1 },
                self.cap_buf.as_ref().unwrap(), 0, W as usize * 4, W as usize * 4 * H as usize,
            );
        }
        blit.endEncoding();
    }
    pub fn captured(&self) -> Vec<u8> {
        let n = W as usize * 4 * H as usize;
        let mut v = vec![0u8; n];
        unsafe { std::ptr::copy_nonoverlapping(self.cap_buf.as_ref().unwrap().contents().as_ptr() as *const u8, v.as_mut_ptr(), n) };
        v
    }
}

pub trait Presenter {
    fn name(&self) -> String;
    /// Ensure the GPU is finished with the shared pool (called before the CPU mutates instances).
    fn sync(&mut self);
    fn present(&mut self, e: &mut Engine, epoch: u64);
    /// (GPUStartTime, GPUEndTime) in seconds (CACurrentMediaTime domain) of the most recent completed frame.
    fn gpu_times(&mut self) -> Option<(f64, f64)>;
    /// Pixels of what was just presented (valid after `sync()`), only when `e.capture`.
    fn presented_pixels(&mut self, e: &Engine) -> Vec<u8>;
    /// Layer to attach to a window.
    fn layer(&self) -> Retained<CALayer>;
    /// Texture/IOSurface memory attributable to this strategy, bytes.
    fn surface_bytes(&self) -> u64;
    fn diag(&self) -> String {
        String::new()
    }
    fn surface_ids(&self) -> Vec<usize> {
        vec![]
    }
    fn last_surface_id(&self) -> usize {
        0
    }
}

fn wait(cmd: &Cmd) {
    if cmd.status() != MTLCommandBufferStatus::Completed {
        cmd.waitUntilCompleted();
    }
}
fn gpu_times(cmd: &Cmd) -> (f64, f64) {
    (cmd.GPUStartTime(), cmd.GPUEndTime())
}

// =========================================================================== (a) CONTROL
pub struct Control {
    pub layer: Retained<CAMetalLayer>,
    retained: Tex,
    drawn: bool,
    last: Option<Cmd>,
    pub framebuffer_only: bool,
}

pub fn make_metal_layer(device: &Dev, framebuffer_only: bool) -> Retained<CAMetalLayer> {
    let l = CAMetalLayer::new();
    l.setDevice(Some(device));
    l.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    l.setFramebufferOnly(framebuffer_only);
    l.setDrawableSize(objc2_core_foundation::CGSize { width: W as f64, height: H as f64 });
    l.setContentsScale(2.0);
    l.setOpaque(true);
    l.setFrame(objc2_core_foundation::CGRect::new(
        objc2_core_foundation::CGPoint::new(0.0, 0.0),
        objc2_core_foundation::CGSize::new(W as f64 / 2.0, H as f64 / 2.0),
    ));
    l.setMaximumDrawableCount(3);
    l
}

impl Control {
    pub fn new(e: &Engine) -> Self {
        // zgx/wgpu: the drawable must be a copy destination => framebufferOnly = false.
        let layer = make_metal_layer(&e.gpu.device, false);
        let retained = e.gpu.new_texture(MTLStorageMode::Private, true);
        Control { layer, retained, drawn: false, last: None, framebuffer_only: false }
    }
}

impl Presenter for Control {
    fn name(&self) -> String {
        "a-control".into()
    }
    fn sync(&mut self) {
        if let Some(c) = &self.last {
            wait(c);
        }
    }
    fn present(&mut self, e: &mut Engine, epoch: u64) {
        e.gpu.prof.reset();
        if self.drawn {
            e.build_region(Some(epoch - 1)); // retained texture is always up to date: age 1
        } else {
            e.region.set_full();
        }
        e.gpu.prof.mark(0);
        let drawable = self.layer.nextDrawable().expect("drawable");
        e.gpu.prof.mark(7); // nextDrawable
        let dtex = drawable.texture();
        let cmd = e.gpu.queue.commandBuffer().unwrap();
        e.gpu.encode_repair(&cmd, &self.retained, &e.scene, &e.region, e.mode);
        // whole-texture copy to the drawable (the cost this project exists to remove)
        let blit = cmd.blitCommandEncoder().unwrap();
        unsafe { blit.copyFromTexture_toTexture(&self.retained, &dtex) };
        blit.endEncoding();
        e.gpu.prof.mark(8); // blit copy encode
        e.gpu.msgs.set(e.gpu.msgs.get() + 3); // blitCommandEncoder, copyFromTexture:toTexture:, endEncoding
        if e.capture {
            e.encode_capture(&cmd, &dtex);
        }
        cmd.presentDrawable(ProtocolObject::from_ref(&*drawable));
        cmd.commit();
        e.gpu.prof.mark(3); // present+commit
        e.gpu.msgs.set(e.gpu.msgs.get() + 5); // nextDrawable, texture, commandBuffer, presentDrawable, commit
        self.drawn = true;
        self.last = Some(cmd);
    }
    fn gpu_times(&mut self) -> Option<(f64, f64)> {
        self.last.as_ref().filter(|c| c.status() == MTLCommandBufferStatus::Completed).map(gpu_times)
    }
    fn presented_pixels(&mut self, e: &Engine) -> Vec<u8> {
        e.captured()
    }
    fn layer(&self) -> Retained<CALayer> {
        Retained::into_super(self.layer.clone())
    }
    fn surface_bytes(&self) -> u64 {
        (W as u64 * 4 * H as u64) * 1 // retained texture (drawables are system-owned: +3 more, see report)
    }
}

// =========================================================================== (c) CAMETALLAYER AGE
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrawableId {
    TexturePtr,
    IOSurface,
}

pub struct MetalAge {
    pub layer: Retained<CAMetalLayer>,
    ages: HashMap<usize, Option<u64>>,
    id_kind: DrawableId,
    last: Option<Cmd>,
    pub framebuffer_only: bool,
    pub distinct_ids: usize,
    /// when set, every drawable's presentedTime (CACurrentMediaTime domain) is appended here
    pub presented_log: Option<std::sync::Arc<std::sync::Mutex<Vec<f64>>>>,
}

pub fn drawable_id(tex: &Tex, kind: DrawableId) -> usize {
    match kind {
        DrawableId::TexturePtr => Retained::as_ptr(tex) as *const u8 as usize,
        DrawableId::IOSurface => tex.iosurface().map(|s| s.id() as usize).unwrap_or(0),
    }
}

impl MetalAge {
    pub fn new(e: &Engine, framebuffer_only: bool, id_kind: DrawableId) -> Self {
        MetalAge {
            layer: make_metal_layer(&e.gpu.device, framebuffer_only),
            ages: HashMap::new(),
            id_kind,
            last: None,
            framebuffer_only,
            distinct_ids: 0,
            presented_log: None,
        }
    }
}

impl Presenter for MetalAge {
    fn name(&self) -> String {
        "c-metal-age".into()
    }
    fn sync(&mut self) {
        if let Some(c) = &self.last {
            wait(c);
        }
    }
    fn present(&mut self, e: &mut Engine, epoch: u64) {
        e.gpu.prof.reset();
        let drawable = self.layer.nextDrawable().expect("drawable");
        e.gpu.prof.mark(7);
        let dtex = drawable.texture();
        let id = drawable_id(&dtex, self.id_kind);
        let since = self.ages.get(&id).copied().flatten();
        e.build_region(since);
        e.gpu.prof.mark(0);
        let cmd = e.gpu.queue.commandBuffer().unwrap();
        e.gpu.encode_repair(&cmd, &dtex, &e.scene, &e.region, e.mode);
        if e.capture {
            e.encode_capture(&cmd, &dtex);
        }
        if let Some(log) = &self.presented_log {
            let log = log.clone();
            let blk = RcBlock::new(move |d: NonNull<ProtocolObject<dyn MTLDrawable>>| {
                let t = unsafe { d.as_ref() }.presentedTime();
                log.lock().unwrap().push(t);
            });
            unsafe { drawable.addPresentedHandler(RcBlock::as_ptr(&blk)) };
        }
        cmd.presentDrawable(ProtocolObject::from_ref(&*drawable));
        cmd.commit();
        e.gpu.prof.mark(3);
        e.gpu.msgs.set(e.gpu.msgs.get() + 5); // nextDrawable, texture, commandBuffer, presentDrawable, commit
        self.ages.insert(id, Some(epoch));
        self.distinct_ids = self.ages.len();
        self.last = Some(cmd);
    }
    fn gpu_times(&mut self) -> Option<(f64, f64)> {
        self.last.as_ref().filter(|c| c.status() == MTLCommandBufferStatus::Completed).map(gpu_times)
    }
    fn presented_pixels(&mut self, e: &Engine) -> Vec<u8> {
        e.captured()
    }
    fn layer(&self) -> Retained<CALayer> {
        Retained::into_super(self.layer.clone())
    }
    fn diag(&self) -> String {
        format!("distinct drawable ids seen (by {:?}): {}", self.id_kind, self.ages.len())
    }
    fn surface_bytes(&self) -> u64 {
        (W as u64 * 4 * H as u64) * self.ages.len().max(1) as u64
    }
}

// =========================================================================== (b) IOSURFACE RING
pub struct Surface {
    pub io: CFRetained<IOSurfaceRef>,
    pub tex: Tex,
    pub last: Option<u64>,
}

pub fn make_iosurface() -> CFRetained<IOSurfaceRef> {
    let keys: [&NSString; 4] = [ns_string!("IOSurfaceWidth"), ns_string!("IOSurfaceHeight"), ns_string!("IOSurfaceBytesPerElement"), ns_string!("IOSurfacePixelFormat")];
    let vals = [NSNumber::new_i32(W), NSNumber::new_i32(H), NSNumber::new_i32(4), NSNumber::new_u32(0x4247_5241)]; // 'BGRA'
    let refs: Vec<&AnyObject> = vals.iter().map(|v| v.as_ref() as &AnyObject).collect();
    let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&keys, &refs);
    let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&dict) as *const CFDictionary) };
    unsafe { IOSurfaceRef::new(cf) }.expect("IOSurface")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishSync {
    /// waitUntilCompleted inline, then set contents + commit transaction on the calling thread.
    Wait,
    /// waitUntilScheduled inline (GPU work ordered before the compositor's reads), then publish.
    Scheduled,
    /// GPU completion handler publishes contents (no CPU wait; handler runs on a Metal thread).
    Handler,
}

pub struct IoRing {
    pub layer: Retained<CALayer>,
    pub surfaces: Vec<Surface>,
    next: usize,
    last: Option<Cmd>,
    pub sync_mode: PublishSync,
    pub in_use_skips: u64,
    pub skip_in_use: bool,
    /// histogram of IOSurfaceIsInUse bit patterns (bit k = surface idx+k in use) sampled before each present
    pub use_hist: [u64; 8],
    pub last_idx: usize,
}

impl IoRing {
    pub fn new(e: &Engine, n: usize, sync_mode: PublishSync) -> Self {
        let layer = CALayer::new();
        layer.setContentsScale(2.0);
        layer.setOpaque(true);
        layer.setFrame(objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(W as f64 / 2.0, H as f64 / 2.0),
        ));
        let surfaces = (0..n)
            .map(|_| {
                let io = make_iosurface();
                let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, W as usize, H as usize, false) };
                d.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
                d.setStorageMode(MTLStorageMode::Shared);
                let tex = e.gpu.device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0).expect("iosurface texture");
                Surface { io, tex, last: None }
            })
            .collect();
        IoRing { layer, surfaces, next: 0, last: None, sync_mode, in_use_skips: 0, skip_in_use: false, use_hist: [0; 8], last_idx: 0 }
    }

    fn publish(layer: &CALayer, io: &IOSurfaceRef) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        unsafe { layer.setContents(Some(&*(io as *const IOSurfaceRef as *const AnyObject))) };
        CATransaction::commit();
    }
}

impl Presenter for IoRing {
    fn name(&self) -> String {
        format!("b-iosurface-{:?}", self.sync_mode).to_lowercase()
    }
    fn sync(&mut self) {
        if let Some(c) = &self.last {
            wait(c);
        }
    }
    fn present(&mut self, e: &mut Engine, epoch: u64) {
        let n = self.surfaces.len();
        let mut bits = 0usize;
        for k in 0..n.min(3) {
            bits |= (self.surfaces[(self.next + k) % n].io.is_in_use() as usize) << k;
        }
        self.use_hist[bits] += 1;
        let mut idx = self.next;
        if self.skip_in_use {
            for k in 0..n {
                let c = (self.next + k) % n;
                if !self.surfaces[c].io.is_in_use() {
                    idx = c;
                    break;
                }
                self.in_use_skips += 1;
            }
        }
        self.next = (idx + 1) % n;
        self.last_idx = idx;
        let s = &mut self.surfaces[idx];
        e.gpu.prof.reset();
        e.build_region(s.last);
        e.gpu.prof.mark(0); // region build
        let cmd = e.gpu.queue.commandBuffer().unwrap();
        e.gpu.encode_repair(&cmd, &s.tex, &e.scene, &e.region, e.mode);
        s.last = Some(epoch);
        e.gpu.msgs.set(e.gpu.msgs.get() + 1); // commandBuffer
        match self.sync_mode {
            PublishSync::Wait => {
                cmd.commit();
                e.gpu.prof.mark(3); // commit
                cmd.waitUntilCompleted();
                e.gpu.prof.mark(4); // wait
                Self::publish(&self.layer, &s.io);
                e.gpu.prof.mark(5); // publish
                e.gpu.msgs.set(e.gpu.msgs.get() + 2 + 4); // commit, waitUntilCompleted, CATransaction begin/setDisableActions/setContents/commit
            }
            PublishSync::Scheduled => {
                cmd.commit();
                e.gpu.prof.mark(3);
                cmd.waitUntilScheduled();
                e.gpu.prof.mark(4);
                Self::publish(&self.layer, &s.io);
                e.gpu.prof.mark(5);
                e.gpu.msgs.set(e.gpu.msgs.get() + 2 + 4);
            }
            PublishSync::Handler => {
                let layer = self.layer.clone();
                let io = s.io.clone();
                let blk = RcBlock::new(move |_c: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                    IoRing::publish(&layer, &io);
                });
                unsafe { cmd.addCompletedHandler(RcBlock::as_ptr(&blk)) };
                e.gpu.prof.mark(6); // handler block
                cmd.commit();
                e.gpu.prof.mark(3);
                e.gpu.msgs.set(e.gpu.msgs.get() + 2); // addCompletedHandler, commit (handler's 4 happen on a Metal thread)
            }
        }
        self.last = Some(cmd);
    }
    fn gpu_times(&mut self) -> Option<(f64, f64)> {
        self.last.as_ref().filter(|c| c.status() == MTLCommandBufferStatus::Completed).map(gpu_times)
    }
    fn presented_pixels(&mut self, _e: &Engine) -> Vec<u8> {
        // Read the IOSurface memory directly (what the compositor would scan out).
        let s = &self.surfaces[self.last_idx];
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
    fn layer(&self) -> Retained<CALayer> {
        self.layer.clone()
    }
    fn surface_ids(&self) -> Vec<usize> {
        self.surfaces.iter().map(|s| s.io.id() as usize).collect()
    }
    fn last_surface_id(&self) -> usize {
        self.surfaces[self.last_idx].io.id() as usize
    }
    fn diag(&self) -> String {
        format!(
            "IOSurfaceIsInUse before each present, histogram by bit pattern (bit0 = surface about to be drawn, bit1 = next, bit2 = the one presented last frame): {:?}; skips {}",
            self.use_hist, self.in_use_skips
        )
    }
    fn surface_bytes(&self) -> u64 {
        self.surfaces.iter().map(|s| s.io.alloc_size() as u64).sum()
    }
}
