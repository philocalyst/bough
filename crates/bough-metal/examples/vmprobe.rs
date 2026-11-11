//! Does a GPU-visible buffer cost its capacity or its touched pages? (lane memory, round 6: "capacity as address
//! space"). Steps run in order on one device and print, after each, the process footprint, its resident size, the
//! resident / dirty pages of the last buffer's range (mincore) and the task's graphics ledger:
//!
//!   vmprobe <step> <step> ...
//!
//! * `shared:MB`    `newBufferWithLength` (storageModeShared)
//! * `nocopy:MB`    an anonymous `mmap` of MB wrapped by `newBufferWithBytesNoCopy` (no deallocator)
//! * `heapalloc:MB` `alloc_zeroed` of MB (what `bough_mem::Region::heap` does), wrapped no-copy
//! * `touch:KB`     the CPU writes the first KB of the last buffer
//! * `touchat:MB:KB` the CPU writes KB at MB into the last buffer
//! * `resid`        add the last buffer to a residency set: commit, requestResidency, attach to the queue
//! * `unresid`      detach and drop the residency sets
//! * `bind`         a compute dispatch that binds the last buffer (setBuffer) and writes 4 bytes of it, waited
//! * `blit`         a 4-byte fill of the last buffer, waited
//! * `wrap:MB`      a second no-copy buffer over the first MB of the last mmap (same base address): the growth step
//!                  of a column whose capacity is address space and whose GPU view is its committed prefix
//! * `mmap:MB`      an anonymous `mmap` of MB with no buffer over it (only `wrap` views make parts GPU-visible)
//! * `drop`         release the last buffer (and its mmap's GPU views)
//! * `idle:ms`      sleep
use bough_metal::perf::snap;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::*;
use std::ffi::c_void;
use std::ptr::NonNull;

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
}

unsafe extern "C" {
    fn task_info(task: u32, flavor: i32, out: *mut c_void, count: *mut u32) -> i32;
    static mach_task_self_: u32;
    fn mach_host_self() -> u32;
    fn host_statistics64(host: u32, flavor: i32, out: *mut c_void, count: *mut u32) -> i32;
}

/// System-wide (wired, free + speculative) pages (HOST_VM_INFO64): physical memory the whole machine has wired
/// and free, to see memory a process's own ledgers do not charge it for. Noisy (every process), so read as a
/// delta across one step on an otherwise steady machine.
fn host_pages() -> (u64, u64) {
    let mut v = [0u32; 64];
    let mut n = 38u32; // HOST_VM_INFO64_COUNT
    // SAFETY: HOST_VM_INFO64 (4) fills at most `n` natural_t words.
    let kr = unsafe { host_statistics64(mach_host_self(), 4, v.as_mut_ptr() as *mut c_void, &mut n) };
    assert_eq!(kr, 0);
    // vm_statistics64: free_count u32 @0, active @1, inactive @2, wire_count @3, ..., speculative_count @... (u32 @ index 18)
    (v[3] as u64, v[0] as u64)
}

fn vm() -> TaskVmInfo {
    let mut v = TaskVmInfo::default();
    let mut n = (size_of::<TaskVmInfo>() / 4) as u32;
    // SAFETY: TASK_VM_INFO (22) fills at most `n` natural_t words of `v`.
    let kr = unsafe { task_info(mach_task_self_, 22, &mut v as *mut _ as *mut c_void, &mut n) };
    assert_eq!(kr, 0, "task_info");
    v
}

/// Resident and dirty bytes of [p, p + len) (mincore).
fn residency(p: *const u8, len: usize) -> (usize, usize) {
    let pg = 16384usize;
    let start = p as usize & !(pg - 1);
    let end = (p as usize + len).next_multiple_of(pg);
    let mut v = vec![0u8; (end - start) / pg];
    // SAFETY: page-aligned start, one byte per page.
    if unsafe { libc::mincore(start as *const c_void, end - start, v.as_mut_ptr() as *mut _) } != 0 {
        return (usize::MAX, usize::MAX);
    }
    (v.iter().filter(|&&b| b & 1 != 0).count() * pg, v.iter().filter(|&&b| b & 1 != 0 && b & 0x14 != 0).count() * pg)
}

struct Buf {
    /// The whole range as one buffer (`None` for a bare `mmap`: only its prefix views are GPU-visible).
    b: Option<Retained<ProtocolObject<dyn MTLBuffer>>>,
    ptr: *mut u8,
    len: usize,
    /// The mmap behind a no-copy buffer (unmapped on drop, after its views).
    map: Option<(*mut u8, usize)>,
    views: Vec<Retained<ProtocolObject<dyn MTLBuffer>>>,
}

fn line(tag: &str, last: Option<&Buf>) {
    let v = vm();
    let s = snap();
    let m = |x: i64| x as f64 / 1048576.0;
    let (r, d) = last.map_or((0, 0), |b| residency(b.ptr, b.len));
    let gfx = v.ledger_tag_graphics_footprint;
    let (wired, free) = host_pages();
    println!(
        "{tag:24} footprint {:7.1}  rss {:7.1}  internal {:6.1}  gfx-fp {:6.1}  buffer resident {:6.1} dirty {:6.1}  | system wired {:8.1} free {:8.1}",
        m(s.footprint as i64),
        m(s.rss as i64),
        m(v.internal as i64),
        m(gfx),
        m(r as i64),
        m(d as i64),
        (wired * 16384) as f64 / 1048576.0,
        (free * 16384) as f64 / 1048576.0
    );
}

const MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
kernel void k_touch(device uint* b [[buffer(0)]], uint i [[thread_position_in_grid]]) { if (i == 0) b[0] = 7; }
"#;

fn main() {
    let device = MTLCreateSystemDefaultDevice().unwrap();
    let queue = device.newCommandQueue().unwrap();
    line("device + queue", None);
    let lib = device.newLibraryWithSource_options_error(&NSString::from_str(MSL), None).expect("msl");
    let pso = device.newComputePipelineStateWithFunction_error(&lib.newFunctionWithName(&NSString::from_str("k_touch")).unwrap()).unwrap();
    let mut bufs: Vec<Buf> = Vec::new();
    let mut sets: Vec<Retained<ProtocolObject<dyn MTLResidencySet>>> = Vec::new();
    for step in std::env::args().skip(1) {
        let parts: Vec<&str> = step.split(':').collect();
        let arg = |i: usize, d: usize| parts.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
        match parts[0] {
            "shared" => {
                let len = arg(1, 64) << 20;
                let b = device.newBufferWithLength_options(len, MTLResourceOptions::StorageModeShared).unwrap();
                let ptr = b.contents().as_ptr() as *mut u8;
                bufs.push(Buf { b: Some(b), ptr, len, map: None, views: vec![] });
            }
            "mmap" => {
                let len = arg(1, 64) << 20;
                // SAFETY: an anonymous private mapping of `len` bytes.
                let p = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_ANON | libc::MAP_PRIVATE, -1, 0) };
                assert_ne!(p, libc::MAP_FAILED);
                bufs.push(Buf { b: None, ptr: p as *mut u8, len, map: Some((p as *mut u8, len)), views: vec![] });
            }
            "nocopy" | "heapalloc" => {
                let len = arg(1, 64) << 20;
                let p = if parts[0] == "nocopy" {
                    // SAFETY: an anonymous private mapping of `len` bytes.
                    let p = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_ANON | libc::MAP_PRIVATE, -1, 0) };
                    assert_ne!(p, libc::MAP_FAILED);
                    p as *mut u8
                } else {
                    // SAFETY: non-zero size, page alignment (leaked: the probe keeps it).
                    unsafe { std::alloc::alloc_zeroed(std::alloc::Layout::from_size_align(len, 16384).unwrap()) }
                };
                // SAFETY: page-aligned, `len` bytes, alive for the probe's life (unmapped only after its views).
                let b = unsafe { device.newBufferWithBytesNoCopy_length_options_deallocator(NonNull::new(p as *mut c_void).unwrap(), len, MTLResourceOptions::StorageModeShared, None) }
                    .expect("no-copy buffer");
                bufs.push(Buf { b: Some(b), ptr: p, len, map: (parts[0] == "nocopy").then_some((p, len)), views: vec![] });
            }
            "touch" | "touchat" => {
                let b = bufs.last().expect("no buffer");
                let (at, kb) = if parts[0] == "touch" { (0, arg(1, 1024)) } else { (arg(1, 0) << 20, arg(2, 1024)) };
                // SAFETY: inside the buffer (asserted).
                assert!(at + (kb << 10) <= b.len);
                unsafe { std::ptr::write_bytes(b.ptr.add(at), 1, kb << 10) };
            }
            "resid" => {
                let b = bufs.last().expect("no buffer");
                let d = MTLResidencySetDescriptor::new();
                let set = device.newResidencySetWithDescriptor_error(&d).unwrap();
                if let Some(bb) = &b.b {
                    set.addAllocation(ProtocolObject::from_ref(&**bb));
                }
                for v in &b.views {
                    set.addAllocation(ProtocolObject::from_ref(&**v));
                }
                set.commit();
                set.requestResidency();
                queue.addResidencySet(&set);
                sets.push(set);
            }
            "unresid" => {
                for s in sets.drain(..) {
                    queue.removeResidencySet(&s);
                    s.endResidency();
                }
            }
            "bind" | "blit" => {
                let b = bufs.last().expect("no buffer");
                let target = b.views.last().or(b.b.as_ref()).expect("no GPU view").clone();
                let c = queue.commandBuffer().unwrap();
                if parts[0] == "bind" {
                    let e = c.computeCommandEncoder().unwrap();
                    e.setComputePipelineState(&pso);
                    unsafe { e.setBuffer_offset_atIndex(Some(&target), 0, 0) };
                    e.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: 1, height: 1, depth: 1 }, MTLSize { width: 32, height: 1, depth: 1 });
                    e.endEncoding();
                } else {
                    let e = c.blitCommandEncoder().unwrap();
                    e.fillBuffer_range_value(&target, objc2_foundation::NSRange::new(0, 4), 3);
                    e.endEncoding();
                }
                c.commit();
                c.waitUntilCompleted();
            }
            "wrap" => {
                let b = bufs.last_mut().expect("no buffer");
                let len = (arg(1, 1) << 20).min(b.len);
                // SAFETY: a page-aligned prefix of the live mapping.
                let v = unsafe { device.newBufferWithBytesNoCopy_length_options_deallocator(NonNull::new(b.ptr as *mut c_void).unwrap(), len, MTLResourceOptions::StorageModeShared, None) }
                    .expect("prefix view");
                b.views.push(v);
            }
            "drop" => {
                for s in sets.drain(..) {
                    queue.removeResidencySet(&s);
                }
                if let Some(b) = bufs.pop() {
                    let map = b.map;
                    drop(b);
                    if let Some((p, len)) = map {
                        // SAFETY: the mapping made above; its buffers are released.
                        unsafe { libc::munmap(p as *mut c_void, len) };
                    }
                }
            }
            "idle" => std::thread::sleep(std::time::Duration::from_millis(arg(1, 2000) as u64)),
            s => panic!("unknown step {s}"),
        }
        line(&step, bufs.last());
    }
}
