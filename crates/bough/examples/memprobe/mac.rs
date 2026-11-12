//! Where does the GPU driver's ~100 MB first-submission footprint come from? Stage by stage.
//!   cargo run --release -p bough --example memprobe -- <stage> [w h]
use block2::RcBlock;
use bough_sys::perf::snap;
use dispatch2::DispatchData;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::rc::Retained;
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{ns_string, NSDictionary, NSNumber, NSString};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::*;

static METALLIB: &[u8] = bough_metal::gpu::METALLIB;
fn mb() -> f64 {
    snap().footprint as f64 / 1048576.0
}
pub fn main() {
    let a: Vec<String> = std::env::args().collect();
    let stage: u32 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(9);
    let w: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(2560);
    let h: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(1600);
    println!("start {:.1}", mb());
    let device = MTLCreateSystemDefaultDevice().unwrap();
    println!("device {:.1}", mb());
    let queue = device.newCommandQueue().unwrap();
    println!("queue {:.1}", mb());
    if stage == 0 {
        return;
    }
    // surface-backed render target
    let keys: [&NSString; 4] = [ns_string!("IOSurfaceWidth"), ns_string!("IOSurfaceHeight"), ns_string!("IOSurfaceBytesPerElement"), ns_string!("IOSurfacePixelFormat")];
    let vals = [NSNumber::new_i32(w as i32), NSNumber::new_i32(h as i32), NSNumber::new_i32(4), NSNumber::new_u32(0x4247_5241)];
    let refs: Vec<&AnyObject> = vals.iter().map(|v| v.as_ref() as &AnyObject).collect();
    let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&keys, &refs);
    let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&dict) as *const CFDictionary) };
    let io = unsafe { IOSurfaceRef::new(cf) }.unwrap();
    let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, w, h, false) };
    d.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    d.setStorageMode(MTLStorageMode::Shared);
    let tex = if std::env::var_os("PLAIN").is_some() {
        d.setStorageMode(MTLStorageMode::Private);
        device.newTextureWithDescriptor(&d).unwrap()
    } else {
        device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0).unwrap()
    };
    println!("target {:.1}", mb());
    if std::env::var_os("PURGE").is_some() {
        unsafe {
            io.lock(objc2_io_surface::IOSurfaceLockOptions::empty(), std::ptr::null_mut());
            std::ptr::write_bytes(io.base_address().as_ptr() as *mut u8, 0x55, io.alloc_size());
            io.unlock(objc2_io_surface::IOSurfaceLockOptions::empty(), std::ptr::null_mut());
        }
        println!("touched {:.1} (alloc {} B)", mb(), io.alloc_size());
        let mut old = 99u32;
        let r = unsafe { io.set_purgeable(1, &mut old) };
        println!("volatile: kr {r} old {old} -> {:.1}", mb());
        let r = unsafe { io.set_purgeable(0, &mut old) };
        println!("nonvolatile: kr {r} old {old} -> {:.1}", mb());
        unsafe {
            io.lock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
            println!("first byte {:#x}", *(io.base_address().as_ptr() as *const u8));
            io.unlock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        }
        return;
    }
    if stage == 7 {
        // blit-only context
        let buf = device.newBufferWithLength_options(w * 4 * 16, MTLResourceOptions::StorageModeShared).unwrap();
        let cmd = queue.commandBuffer().unwrap();
        let enc = cmd.blitCommandEncoder().unwrap();
        unsafe {
            enc.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                &buf, 0, w * 4, w * 4 * 16, MTLSize { width: w, height: 16, depth: 1 }, &tex, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 });
        }
        enc.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        println!("blit pass {:.1}", mb());
        return;
    }
    let pd = MTLRenderPassDescriptor::renderPassDescriptor();
    let c0 = unsafe { pd.colorAttachments().objectAtIndexedSubscript(0) };
    c0.setTexture(Some(&tex));
    c0.setLoadAction(MTLLoadAction::Clear);
    c0.setStoreAction(MTLStoreAction::Store);
    if stage == 1 {
        // an empty command buffer
        let cmd = queue.commandBuffer().unwrap();
        cmd.commit();
        cmd.waitUntilCompleted();
        println!("empty cmd {:.1}", mb());
        return;
    }
    let cmd = queue.commandBuffer().unwrap();
    let enc = cmd.renderCommandEncoderWithDescriptor(&pd).unwrap();
    if stage >= 3 {
        let data = DispatchData::from_static_bytes(METALLIB);
        let lib = device.newLibraryWithData_error(&data).unwrap();
        let vs = lib.newFunctionWithName(ns_string!("vs_main")).unwrap();
        let fs = lib.newFunctionWithName(ns_string!("fs_main")).unwrap();
        let pdsc = MTLRenderPipelineDescriptor::new();
        pdsc.setVertexFunction(Some(&vs));
        pdsc.setFragmentFunction(Some(&fs));
        let ca = unsafe { pdsc.colorAttachments().objectAtIndexedSubscript(0) };
        ca.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        if stage >= 4 {
            ca.setBlendingEnabled(true);
            ca.setSourceRGBBlendFactor(MTLBlendFactor::One);
            ca.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        }
        let pso = device.newRenderPipelineStateWithDescriptor_error(&pdsc).unwrap();
        println!("pso {:.1}", mb());
        if stage >= 5 {
            let buf = device.newBufferWithLength_options(900_000, MTLResourceOptions::StorageModeShared).unwrap();
            unsafe {
                enc.setRenderPipelineState(&pso);
                enc.setVertexBuffer_offset_atIndex(Some(&buf), 0, 0);
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount(MTLPrimitiveType::TriangleStrip, 0, 4, 1);
            }
        }
    }
    enc.endEncoding();
    let done = RcBlock::new(|_c: std::ptr::NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {});
    unsafe { cmd.addCompletedHandler(RcBlock::as_ptr(&done)) };
    cmd.commit();
    cmd.waitUntilCompleted();
    println!("first pass {:.1}", mb());
    if std::env::var_os("PURGE2").is_some() {
        let mut old = 99u32;
        let r = unsafe { io.set_purgeable(1, &mut old) };
        println!("io volatile kr {r} old {old} -> {:.1}", mb());
        let r = unsafe { io.set_purgeable(0, &mut old) };
        println!("io nonvolatile kr {r} old {old} -> {:.1}", mb());
        let st = tex.setPurgeableState(MTLPurgeableState::Volatile);
        println!("tex volatile old {:?} -> {:.1}", st, mb());
        let st = tex.setPurgeableState(MTLPurgeableState::NonVolatile);
        println!("tex nonvolatile old {:?} -> {:.1}", st, mb());
        let r = unsafe { io.set_purgeable(1, &mut old) };
        let st = tex.setPurgeableState(MTLPurgeableState::Volatile);
        println!("both volatile: io kr {r} tex old {:?} -> {:.1}", st, mb());
        std::thread::sleep(std::time::Duration::from_millis(1500));
        println!("1.5 s later -> {:.1}", mb());
        return;
    }
    if std::env::var_os("WAIT").is_some() {
        for sec in 1..=20 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            println!("idle {sec} s: {:.1}", mb());
        }
    }
    for i in 0..3 {
        let cmd = queue.commandBuffer().unwrap();
        let enc = cmd.renderCommandEncoderWithDescriptor(&pd).unwrap();
        enc.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        println!("pass {} {:.1}", i + 2, mb());
    }
}
