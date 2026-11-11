//! (Round 5 memory lane's probe, left uncommitted in its worktree; copied unchanged into round 6.)
//! The GPU driver's first-submission footprint, piece by piece (round 5, memory).
//!
//!   floorprobe <step> <step> ...
//!
//! Every step prints `phys_footprint` and the task's VM ledgers (TASK_VM_INFO rev 6: internal, compressed,
//! graphics-tagged footprint, purgeable). Steps run in order on one device:
//!
//! * `q`              another command queue (the first exists after `dev`)
//! * `blit[:k]`       a 4-byte fill on queue k (default 0), waited
//! * `comp[:k]`       a 1-threadgroup compute dispatch (runtime-compiled MSL), waited
//! * `comptg`         the same with 32 KB of threadgroup memory
//! * `render`         a clear-only render pass on a 64x64 texture, waited
//! * `armed`          commit a command buffer that waits on a shared event (not signalled) and then blits
//! * `signal`         signal that event, wait for the armed buffer
//! * `resid:MB`       a residency set (on queue 0) over a fresh MB-sized shared buffer, 1 page touched
//! * `heap:MB`        an MTLHeap of MB (private), one buffer placed in it
//! * `surf:w:h[:fmt]` an IOSurface-backed BGRA8 texture (fmt: BGRA | &BGA | -BGA via CoreVideo)
//! * `gtile`          a compute write of one 32x32 tile into the last surface (waited)
//! * `gfull`          a compute write of the whole last surface (waited)
//! * `rfull`          a render-pass clear of the whole last surface (waited; RenderTarget usage)
//! * `vol` / `nonvol` the last surface's IOSurface purgeable state (prints the old state)
//! * `idle:ms`        sleep, sampling every 250 ms
//! * `hold:s`         print the pid and sleep (run `footprint -v <pid>` / `vmmap --summary <pid>` meanwhile)
//! * `drop`           release every surface, heap, residency set and queue but the first
use bough_metal::perf::snap;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_core_foundation::{CFDictionary, CFRetained};
use objc2_foundation::{ns_string, NSDictionary, NSNumber, NSString};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::*;
use std::ffi::c_void;

#[repr(C, packed(4))]
#[derive(Default, Clone, Copy)]
struct TaskVmInfo {
    virtual_size: u64,
    region_count: i32,
    page_size: i32,
    resident_size: u64,
    resident_size_peak: u64,
    device: u64,
    device_peak: u64,
    internal: u64,
    internal_peak: u64,
    external: u64,
    external_peak: u64,
    reusable: u64,
    reusable_peak: u64,
    purgeable_volatile_pmap: u64,
    purgeable_volatile_resident: u64,
    purgeable_volatile_virtual: u64,
    compressed: u64,
    compressed_peak: u64,
    compressed_lifetime: u64,
    phys_footprint: u64,
    min_address: u64,
    max_address: u64,
    ledger_phys_footprint_peak: i64,
    ledger_purgeable_nonvolatile: i64,
    ledger_purgeable_novolatile_compressed: i64,
    ledger_purgeable_volatile: i64,
    ledger_purgeable_volatile_compressed: i64,
    ledger_tag_network_nonvolatile: i64,
    ledger_tag_network_nonvolatile_compressed: i64,
    ledger_tag_network_volatile: i64,
    ledger_tag_network_volatile_compressed: i64,
    ledger_tag_media_footprint: i64,
    ledger_tag_media_footprint_compressed: i64,
    ledger_tag_media_nofootprint: i64,
    ledger_tag_media_nofootprint_compressed: i64,
    ledger_tag_graphics_footprint: i64,
    ledger_tag_graphics_footprint_compressed: i64,
    ledger_tag_graphics_nofootprint: i64,
    ledger_tag_graphics_nofootprint_compressed: i64,
    ledger_tag_neural_footprint: i64,
    ledger_tag_neural_footprint_compressed: i64,
    ledger_tag_neural_nofootprint: i64,
    ledger_tag_neural_nofootprint_compressed: i64,
    limit_bytes_remaining: u64,
    decompressions: i32,
    ledger_swapins: i64,
}

unsafe extern "C" {
    fn task_info(task: u32, flavor: i32, out: *mut c_void, count: *mut u32) -> i32;
    static mach_task_self_: u32;
}

fn vm() -> TaskVmInfo {
    let mut v = TaskVmInfo::default();
    let mut n = (size_of::<TaskVmInfo>() / 4) as u32;
    // SAFETY: TASK_VM_INFO (22) fills at most `n` natural_t words of `v`.
    let kr = unsafe { task_info(mach_task_self_, 22, &mut v as *mut _ as *mut c_void, &mut n) };
    assert_eq!(kr, 0, "task_info");
    v
}

fn line(tag: &str) {
    let v = vm();
    let m = |x: i64| x as f64 / 1048576.0;
    let (fp, int, cmp, gfx, gfxn, pnv, pv, dev) = (
        v.phys_footprint as i64,
        v.internal as i64,
        v.compressed as i64,
        v.ledger_tag_graphics_footprint,
        v.ledger_tag_graphics_nofootprint,
        v.ledger_purgeable_nonvolatile,
        v.ledger_purgeable_volatile,
        v.device as i64,
    );
    println!(
        "{tag:28} footprint {:7.1}  internal {:6.1}  compressed {:5.1}  gfx-fp {:6.1}  gfx-nofp {:6.1}  purg-nv {:6.1}  purg-v {:6.1}  device {:6.1}  rest {:6.1}",
        m(fp),
        m(int),
        m(cmp),
        m(gfx),
        m(gfxn),
        m(pnv),
        m(pv),
        m(dev),
        m(fp - int - cmp - gfx - pnv)
    );
    let _ = snap();
}

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    fn CVPixelBufferCreate(alloc: *const c_void, w: usize, h: usize, fmt: u32, attrs: *const c_void, out: *mut *mut c_void) -> i32;
    fn CVPixelBufferGetIOSurface(pb: *const c_void) -> *mut IOSurfaceRef;
    static kCVPixelBufferIOSurfacePropertiesKey: *const c_void;
    static kCVPixelBufferMetalCompatibilityKey: *const c_void;
}

fn fourcc(s: &str) -> u32 {
    let b = s.as_bytes();
    (b[0] as u32) << 24 | (b[1] as u32) << 16 | (b[2] as u32) << 8 | b[3] as u32
}

fn iosurface(w: usize, h: usize, fmt: &str) -> CFRetained<IOSurfaceRef> {
    if fmt == "BGRA" {
        let keys: [&NSString; 4] = [ns_string!("IOSurfaceWidth"), ns_string!("IOSurfaceHeight"), ns_string!("IOSurfaceBytesPerElement"), ns_string!("IOSurfacePixelFormat")];
        let vals = [NSNumber::new_i32(w as i32), NSNumber::new_i32(h as i32), NSNumber::new_i32(4), NSNumber::new_u32(fourcc("BGRA"))];
        let refs: Vec<&AnyObject> = vals.iter().map(|v| v.as_ref() as &AnyObject).collect();
        let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&keys, &refs);
        // SAFETY: toll-free bridged; a well-formed property dictionary.
        let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&dict) as *const CFDictionary) };
        return unsafe { IOSurfaceRef::new(cf) }.expect("IOSurface");
    }
    // CoreVideo knows the compressed formats' plane layout: allocate through a CVPixelBuffer
    let empty: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();
    let yes = NSNumber::new_bool(true);
    // SAFETY: CoreVideo's CFString keys, bridged to NSString.
    let (k_io, k_mtl) = unsafe { (&*(kCVPixelBufferIOSurfacePropertiesKey as *const NSString), &*(kCVPixelBufferMetalCompatibilityKey as *const NSString)) };
    let attrs: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&[k_io, k_mtl], &[empty.as_ref() as &AnyObject, yes.as_ref() as &AnyObject]);
    let mut pb: *mut c_void = std::ptr::null_mut();
    // SAFETY: valid attributes; `pb` receives a +1 CVPixelBuffer (leaked: the probe keeps it for its lifetime).
    let r = unsafe { CVPixelBufferCreate(std::ptr::null(), w, h, fourcc(fmt), Retained::as_ptr(&attrs) as *const c_void, &mut pb) };
    assert_eq!(r, 0, "CVPixelBufferCreate({fmt}) failed: {r}");
    // SAFETY: an IOSurface-backed pixel buffer returns its (unretained) surface; retain it.
    let io = unsafe { CVPixelBufferGetIOSurface(pb) };
    assert!(!io.is_null(), "no IOSurface behind the pixel buffer");
    let io = unsafe { CFRetained::retain(std::ptr::NonNull::new(io).unwrap()) };
    println!("  {fmt} surface: alloc {} B, bytes/row {}, uncompressed {} B", io.alloc_size(), io.bytes_per_row(), w * h * 4);
    io
}

const MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
kernel void k_tiny(device uint* b [[buffer(0)]], uint i [[thread_position_in_grid]]) { b[i & 3] = i; }
kernel void k_tg(device uint* b [[buffer(0)]], uint i [[thread_position_in_grid]], uint l [[thread_index_in_threadgroup]]) {
    threadgroup uint s[8192];
    s[l * 31 & 8191] = i;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    b[i & 3] = s[(l * 7) & 8191];
}
kernel void k_fill(texture2d<float, access::write> t [[texture(0)]], constant uint4& r [[buffer(0)]], uint2 p [[thread_position_in_grid]]) {
    uint2 q = p + r.xy;
    if (q.x < r.z && q.y < r.w) t.write(float4(0.2, 0.4, 0.6, 1.0), q);
}
"#;

struct Probe {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queues: Vec<Retained<ProtocolObject<dyn MTLCommandQueue>>>,
    lib: Option<Retained<ProtocolObject<dyn MTLLibrary>>>,
    buf: Retained<ProtocolObject<dyn MTLBuffer>>,
    ev: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    armed: Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    surfs: Vec<(CFRetained<IOSurfaceRef>, Retained<ProtocolObject<dyn MTLTexture>>)>,
    keep_tex: Vec<Retained<ProtocolObject<dyn MTLTexture>>>,
    keep_buf: Vec<Retained<ProtocolObject<dyn MTLBuffer>>>,
    keep_heap: Vec<Retained<ProtocolObject<dyn MTLHeap>>>,
    resid: Vec<Retained<ProtocolObject<dyn MTLResidencySet>>>,
}

impl Probe {
    fn pso(&mut self, name: &str) -> Retained<ProtocolObject<dyn MTLComputePipelineState>> {
        if self.lib.is_none() {
            self.lib = Some(self.device.newLibraryWithSource_options_error(&NSString::from_str(MSL), None).expect("msl"));
        }
        let f = self.lib.as_ref().unwrap().newFunctionWithName(&NSString::from_str(name)).expect("fn");
        self.device.newComputePipelineStateWithFunction_error(&f).expect("pso")
    }
    fn compute(&mut self, name: &str, q: usize) {
        let p = self.pso(name);
        let c = self.queues[q].commandBuffer().unwrap();
        let e = c.computeCommandEncoder().unwrap();
        e.setComputePipelineState(&p);
        unsafe { e.setBuffer_offset_atIndex(Some(&self.buf), 0, 0) };
        e.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: 1, height: 1, depth: 1 }, MTLSize { width: 32, height: 1, depth: 1 });
        e.endEncoding();
        c.commit();
        c.waitUntilCompleted();
    }
    fn fill(&mut self, full: bool) {
        let p = self.pso("k_fill");
        let (_, t) = self.surfs.last().expect("no surface");
        let (w, h) = (t.width() as u32, t.height() as u32);
        let r: [u32; 4] = if full { [0, 0, w, h] } else { [64, 64, 96, 96] };
        let c = self.queues[0].commandBuffer().unwrap();
        let e = c.computeCommandEncoder().unwrap();
        e.setComputePipelineState(&p);
        unsafe {
            e.setTexture_atIndex(Some(t), 0);
            e.setBytes_length_atIndex(std::ptr::NonNull::new(r.as_ptr() as *mut c_void).unwrap(), 16, 0);
        }
        let (gw, gh) = (r[2] - r[0], r[3] - r[1]);
        e.dispatchThreadgroups_threadsPerThreadgroup(
            MTLSize { width: gw.div_ceil(16) as usize, height: gh.div_ceil(16) as usize, depth: 1 },
            MTLSize { width: 16, height: 16, depth: 1 },
        );
        e.endEncoding();
        c.commit();
        c.waitUntilCompleted();
        if let Some(err) = c.error() {
            println!("  command buffer error: {err:?}");
        }
    }
}

fn main() {
    line("start");
    let device = MTLCreateSystemDefaultDevice().unwrap();
    line("device");
    let q0 = device.newCommandQueue().unwrap();
    line("queue");
    let buf = device.newBufferWithLength_options(4096, MTLResourceOptions::StorageModeShared).unwrap();
    let ev = device.newSharedEvent().unwrap();
    let mut p = Probe { device, queues: vec![q0], lib: None, buf, ev, armed: None, surfs: vec![], keep_tex: vec![], keep_buf: vec![], keep_heap: vec![], resid: vec![] };
    for step in std::env::args().skip(1) {
        let parts: Vec<&str> = step.split(':').collect();
        let arg = |i: usize, d: usize| parts.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
        let t0 = std::time::Instant::now();
        match parts[0] {
            "q" => p.queues.push(p.device.newCommandQueue().unwrap()),
            "blit" => {
                let c = p.queues[arg(1, 0)].commandBuffer().unwrap();
                let e = c.blitCommandEncoder().unwrap();
                e.fillBuffer_range_value(&p.buf, objc2_foundation::NSRange::new(0, 4), 7);
                e.endEncoding();
                c.commit();
                c.waitUntilCompleted();
            }
            "comp" => p.compute("k_tiny", arg(1, 0)),
            "comptg" => p.compute("k_tg", arg(1, 0)),
            "render" => {
                let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, 64, 64, false) };
                d.setUsage(MTLTextureUsage::RenderTarget);
                d.setStorageMode(MTLStorageMode::Private);
                let t = p.device.newTextureWithDescriptor(&d).unwrap();
                let pd = MTLRenderPassDescriptor::renderPassDescriptor();
                let c0 = unsafe { pd.colorAttachments().objectAtIndexedSubscript(0) };
                c0.setTexture(Some(&t));
                c0.setLoadAction(MTLLoadAction::Clear);
                c0.setStoreAction(MTLStoreAction::Store);
                let c = p.queues[0].commandBuffer().unwrap();
                let e = c.renderCommandEncoderWithDescriptor(&pd).unwrap();
                e.endEncoding();
                c.commit();
                c.waitUntilCompleted();
                p.keep_tex.push(t);
            }
            "rfull" => {
                let (_, t) = p.surfs.last().expect("no surface");
                let pd = MTLRenderPassDescriptor::renderPassDescriptor();
                let c0 = unsafe { pd.colorAttachments().objectAtIndexedSubscript(0) };
                c0.setTexture(Some(t));
                c0.setLoadAction(MTLLoadAction::Clear);
                c0.setClearColor(MTLClearColor { red: 0.2, green: 0.4, blue: 0.6, alpha: 1.0 });
                c0.setStoreAction(MTLStoreAction::Store);
                let c = p.queues[0].commandBuffer().unwrap();
                let e = c.renderCommandEncoderWithDescriptor(&pd).unwrap();
                e.endEncoding();
                c.commit();
                c.waitUntilCompleted();
                if let Some(err) = c.error() {
                    println!("  command buffer error: {err:?}");
                }
            }
            "armed" => {
                let c = p.queues[0].commandBuffer().unwrap();
                c.encodeWaitForEvent_value(ProtocolObject::from_ref(&*p.ev), 1);
                let e = c.blitCommandEncoder().unwrap();
                e.fillBuffer_range_value(&p.buf, objc2_foundation::NSRange::new(0, 4), 9);
                e.endEncoding();
                c.commit();
                p.armed = Some(c);
            }
            "signal" => {
                p.ev.setSignaledValue(1);
                if let Some(c) = p.armed.take() {
                    c.waitUntilCompleted();
                }
            }
            "resid" => {
                let b = p.device.newBufferWithLength_options(arg(1, 1) << 20, MTLResourceOptions::StorageModeShared).unwrap();
                unsafe { *(b.contents().as_ptr() as *mut u8) = 1 };
                let d = MTLResidencySetDescriptor::new();
                let set = p.device.newResidencySetWithDescriptor_error(&d).unwrap();
                set.addAllocation(ProtocolObject::from_ref(&*b));
                set.commit();
                set.requestResidency();
                p.queues[0].addResidencySet(&set);
                p.resid.push(set);
                p.keep_buf.push(b);
            }
            "heap" => {
                let d = MTLHeapDescriptor::new();
                d.setSize(arg(1, 16) << 20);
                d.setStorageMode(MTLStorageMode::Private);
                let h = p.device.newHeapWithDescriptor(&d).unwrap();
                let b = h.newBufferWithLength_options(4096, MTLResourceOptions::StorageModePrivate).unwrap();
                p.keep_buf.push(b);
                p.keep_heap.push(h);
            }
            "surf" => {
                let (w, h) = (arg(1, 1920), arg(2, 1440));
                let fmt = parts.get(3).copied().unwrap_or("BGRA");
                let io = iosurface(w, h, fmt);
                let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, w, h, false) };
                let usage = match parts.get(4).copied() {
                    Some("rt") => MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget,
                    _ => MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite | MTLTextureUsage::RenderTarget,
                };
                d.setUsage(usage);
                d.setStorageMode(MTLStorageMode::Shared);
                match p.device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0) {
                    Some(t) => {
                        println!("  texture {}x{} usage {:?} compression {:?}", t.width(), t.height(), t.usage(), t.compressionType());
                        p.surfs.push((io, t));
                    }
                    None => println!("  newTextureWithDescriptor:iosurface: returned nil for {fmt}"),
                }
            }
            "gtile" => p.fill(false),
            "gfull" => p.fill(true),
            "cpu" => {
                let (io, _) = p.surfs.last().expect("no surface");
                unsafe {
                    io.lock(objc2_io_surface::IOSurfaceLockOptions::empty(), std::ptr::null_mut());
                    std::ptr::write_bytes(io.base_address().as_ptr() as *mut u8, 0x55, io.alloc_size());
                    io.unlock(objc2_io_surface::IOSurfaceLockOptions::empty(), std::ptr::null_mut());
                }
            }
            "vol" | "nonvol" => {
                let (io, _) = p.surfs.last().expect("no surface");
                let mut old = 99u32;
                let kr = unsafe { io.set_purgeable(if parts[0] == "vol" { 1 } else { 0 }, &mut old) };
                println!("  set_purgeable({}) kr {kr} old {old}", parts[0]);
            }
            "idle" => {
                let ms = arg(1, 2000);
                let mut t = 0;
                while t < ms {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    t += 250;
                    line(&format!("  idle {t} ms"));
                }
                continue;
            }
            "hold" => {
                println!("  pid {} holding {} s", std::process::id(), arg(1, 5));
                std::thread::sleep(std::time::Duration::from_secs(arg(1, 5) as u64));
            }
            "drop" => {
                p.surfs.clear();
                p.keep_tex.clear();
                p.keep_buf.clear();
                p.keep_heap.clear();
                for s in p.resid.drain(..) {
                    p.queues[0].removeResidencySet(&s);
                }
                p.queues.truncate(1);
            }
            s => panic!("unknown step {s}"),
        }
        line(&format!("{step} ({:.1} ms)", t0.elapsed().as_secs_f64() * 1e3));
    }
}
