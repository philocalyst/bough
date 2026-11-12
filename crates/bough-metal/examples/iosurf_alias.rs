//! E9 (compute lane, round 7): can a compute kernel store pixels through a buffer that aliases an IOSurface's
//! memory? Then both executors write the surface as words (`out[y * stride + x] = bgra`), with no texture unit
//! between the kernel's integer result and the bytes the compositor reads.
//! Headless. Prints what Metal accepted and whether each view of the memory saw the kernel's words.
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::*;
use std::ptr::NonNull;

const MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
kernel void fill(device uint* out [[buffer(0)]], uint3 gid [[thread_position_in_grid]]) {
    uint i = gid.y * 256u + gid.x;   // bytes per row / 4 is passed implicitly: the probe surface is 256 px wide
    out[i] = 0xff000000u | (gid.x << 16) | (gid.y << 8) | ((gid.x ^ gid.y) & 255u);
}
"#;

fn main() {
    let (w, h) = (256usize, 128usize);
    let device = MTLCreateSystemDefaultDevice().expect("device");
    let queue = device.newCommandQueue().unwrap();
    let io = bough_metal::fused::make_iosurface(w, h);
    let (bpr, alloc) = (io.bytes_per_row(), io.alloc_size());
    let base = io.base_address().as_ptr() as *mut u8;
    let page = 16384usize;
    println!("surface {w}x{h}: bytes/row {bpr}, alloc {alloc}, base {base:p} (page aligned: {}), alloc % page = {}", base as usize % page == 0, alloc % page);
    let len = alloc.next_multiple_of(page);
    // SAFETY: the surface's memory, kept alive by `io` for the probe's duration; no deallocator.
    let buf = unsafe { device.newBufferWithBytesNoCopy_length_options_deallocator(NonNull::new(base as *mut _).unwrap(), len, MTLResourceOptions::StorageModeShared, None) };
    let Some(buf) = buf else {
        println!("RESULT: newBufferWithBytesNoCopy refused the surface's memory (len {len})");
        return;
    };
    println!("no-copy buffer accepted: length {}", buf.length());
    let lib = device.newLibraryWithSource_options_error(&NSString::from_str(MSL), None).expect("msl");
    let f = lib.newFunctionWithName(&NSString::from_str("fill")).unwrap();
    let pso = device.newComputePipelineStateWithFunction_error(&f).unwrap();
    // a texture view of the same surface, as the presenter makes
    let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, w, h, false) };
    d.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
    d.setStorageMode(MTLStorageMode::Shared);
    let tex: Retained<ProtocolObject<dyn MTLTexture>> = device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0).expect("texture");
    let seed0 = io.seed();
    let cmd = queue.commandBuffer().unwrap();
    let enc = cmd.computeCommandEncoder().unwrap();
    enc.setComputePipelineState(&pso);
    unsafe { enc.setBuffer_offset_atIndex(Some(&buf), 0, 0) };
    assert_eq!(bpr, w * 4, "the probe assumes an unpadded row");
    enc.dispatchThreads_threadsPerThreadgroup(MTLSize { width: w, height: h, depth: 1 }, MTLSize { width: 16, height: 16, depth: 1 });
    enc.endEncoding();
    cmd.commit();
    cmd.waitUntilCompleted();
    println!("kernel status {:?} error {:?}", cmd.status(), cmd.error());
    let want = |x: usize, y: usize| 0xff00_0000u32 | (x as u32) << 16 | (y as u32) << 8 | ((x ^ y) as u32 & 255);
    // (a) the CPU's view of the surface
    let mut bad_cpu = 0;
    unsafe { io.lock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()) };
    for y in 0..h {
        for x in 0..w {
            let v = unsafe { (base.add(y * bpr + x * 4) as *const u32).read_unaligned() };
            bad_cpu += (v != want(x, y)) as u32;
        }
    }
    unsafe { io.unlock(objc2_io_surface::IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()) };
    // (b) the GPU's texture view of the surface
    let mut px = vec![0u32; w * h];
    unsafe {
        tex.getBytes_bytesPerRow_fromRegion_mipmapLevel(NonNull::new(px.as_mut_ptr() as *mut _).unwrap(), w * 4, MTLRegion { origin: MTLOrigin { x: 0, y: 0, z: 0 }, size: MTLSize { width: w, height: h, depth: 1 } }, 0)
    };
    let bad_tex = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| px[y * w + x] != want(x, y)).count();
    println!("RESULT: words wrong through the surface's base address: {bad_cpu} of {}; through its texture view: {bad_tex}; seed {seed0} -> {} (a lock/unlock bumps it)", w * h, io.seed());
}
