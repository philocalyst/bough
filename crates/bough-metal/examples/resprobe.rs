//! Footprint probe for the fused presenter's memory decisions:
//!   resprobe buf      : is a storageModeShared MTLBuffer backed lazily (untouched pages free)?
//!   resprobe resid    : does a residency set over untouched IOSurface textures make them resident?
//!   resprobe compute  : driver footprint after a compute-only frame vs a render pass
use bough_metal::perf::snap;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{ns_string, NSDictionary, NSNumber, NSString};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::*;

fn mb() -> f64 {
    snap().footprint as f64 / 1048576.0
}

fn surface(device: &ProtocolObject<dyn MTLDevice>, w: usize, h: usize) -> (objc2_core_foundation::CFRetained<IOSurfaceRef>, Retained<ProtocolObject<dyn MTLTexture>>) {
    let keys: [&NSString; 4] = [ns_string!("IOSurfaceWidth"), ns_string!("IOSurfaceHeight"), ns_string!("IOSurfaceBytesPerElement"), ns_string!("IOSurfacePixelFormat")];
    let vals = [NSNumber::new_i32(w as i32), NSNumber::new_i32(h as i32), NSNumber::new_i32(4), NSNumber::new_u32(0x4247_5241)];
    let refs: Vec<&AnyObject> = vals.iter().map(|v| v.as_ref() as &AnyObject).collect();
    let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&keys, &refs);
    let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&dict) as *const CFDictionary) };
    let io = unsafe { IOSurfaceRef::new(cf) }.unwrap();
    let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, w, h, false) };
    d.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
    d.setStorageMode(MTLStorageMode::Shared);
    let t = device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0).unwrap();
    (io, t)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let what = a.get(1).map(|s| s.as_str()).unwrap_or("buf");
    println!("start {:.1}", mb());
    let device = MTLCreateSystemDefaultDevice().unwrap();
    let queue = device.newCommandQueue().unwrap();
    println!("device+queue {:.1}", mb());
    match what {
        "buf" => {
            let b = device.newBufferWithLength_options(64 << 20, MTLResourceOptions::StorageModeShared).unwrap();
            println!("64 MB shared buffer created {:.1}", mb());
            let p = b.contents().as_ptr() as *mut u8;
            unsafe { std::ptr::write_bytes(p, 1, 1 << 20) };
            println!("1 MB touched {:.1}", mb());
            unsafe { std::ptr::write_bytes(p.add(32 << 20), 1, 4 << 20) };
            println!("+4 MB touched {:.1}", mb());
            let d = MTLResidencySetDescriptor::new();
            let set = device.newResidencySetWithDescriptor_error(&d).unwrap();
            set.addAllocation(ProtocolObject::from_ref(&*b));
            set.commit();
            set.requestResidency();
            queue.addResidencySet(&set);
            println!("residency set over the buffer {:.1}", mb());
            let c = queue.commandBuffer().unwrap();
            let e = c.blitCommandEncoder().unwrap();
            e.fillBuffer_range_value(&b, objc2_foundation::NSRange::new(0, 4), 0);
            e.endEncoding();
            c.commit();
            c.waitUntilCompleted();
            println!("after a blit touching 4 B {:.1}", mb());
        }
        "resid" => {
            let s: Vec<_> = (0..4).map(|_| surface(&device, 1920, 1440)).collect();
            println!("4 surfaces 1920x1440 {:.1}", mb());
            let d = MTLResidencySetDescriptor::new();
            let set = device.newResidencySetWithDescriptor_error(&d).unwrap();
            for (_, t) in &s {
                set.addAllocation(ProtocolObject::from_ref(&**t));
            }
            set.commit();
            set.requestResidency();
            queue.addResidencySet(&set);
            println!("residency set over them {:.1}", mb());
            let c = queue.commandBuffer().unwrap();
            let e = c.blitCommandEncoder().unwrap();
            let tiny = device.newBufferWithLength_options(16, MTLResourceOptions::StorageModePrivate).unwrap();
            e.fillBuffer_range_value(&tiny, objc2_foundation::NSRange::new(0, 4), 0);
            e.endEncoding();
            c.commit();
            c.waitUntilCompleted();
            println!("after a blit {:.1}", mb());
            std::thread::sleep(std::time::Duration::from_millis(2500));
            println!("2.5 s idle {:.1}", mb());
        }
        _ => {
            let (_io, t) = surface(&device, 1920, 1440);
            let c = queue.commandBuffer().unwrap();
            let e = c.blitCommandEncoder().unwrap();
            let tiny = device.newBufferWithLength_options(16, MTLResourceOptions::StorageModePrivate).unwrap();
            e.fillBuffer_range_value(&tiny, objc2_foundation::NSRange::new(0, 4), 0);
            e.endEncoding();
            c.commit();
            c.waitUntilCompleted();
            println!("blit only {:.1}", mb());
            let _ = t;
            std::thread::sleep(std::time::Duration::from_millis(2500));
            println!("2.5 s idle {:.1}", mb());
        }
    }
}
