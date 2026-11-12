//! The window's memory ledger (AUDIT R4, BRIEF §5.3 item 6): every holder of the runtime's memory, by name,
//! with the bytes it uses, reserves, and has resident, and what its size follows (content or window).
//!
//! Diagnostics only: built on demand ([`App::ledger`], the host's `BOUGH_LEDGER` timer), never on a frame
//! path. Region columns are measured page by page (`mincore`); heap tables are their capacities (measured
//! with `mincore` when they span whole pages); GPU objects are Metal's and IOSurface's own sizes. The
//! process side ([`Proc`]) is what the system charges: `phys_footprint`, resident size, the malloc zones,
//! the Rust heap (when the counting allocator `bough_mem::count::Counting` is installed) and Metal's
//! `currentAllocatedSize`, so the named holders can be checked against the total and the residual stated.
use crate::app::{App, View};
/// The counting allocator (opt-in per binary: `#[global_allocator] static A: bough::ledger::Counting =
/// bough::ledger::Counting;`) and its counters.
pub use bough_mem::count::{self, Counting};
pub use bough_mem::{Ledger as MemLedger, Line, Scales as MemScales, Space as MemSpace};
use bough_mem::Ledger;
#[cfg(target_os = "macos")]
use bough_mem::{Scales, Space};

/// What the process holds as the system counts it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Proc {
    pub phys_footprint: u64,
    pub phys_footprint_peak: u64,
    pub resident: u64,
    /// All malloc zones: bytes in live blocks, bytes the zones hold from the VM.
    pub malloc_in_use: u64,
    pub malloc_allocated: u64,
    pub malloc_blocks: u64,
    /// Live Rust heap bytes and allocation count (0 without the counting allocator).
    pub rust_live: u64,
    pub rust_allocs: u64,
    /// `MTLDevice.currentAllocatedSize` (0 headless).
    pub metal_allocated: u64,
}

#[repr(C)]
#[derive(Default)]
struct MallocStats {
    blocks_in_use: u32,
    size_in_use: usize,
    max_size_in_use: usize,
    size_allocated: usize,
}

#[cfg(target_os = "macos")]
fn malloc_stats() -> MallocStats {
    unsafe extern "C" {
        fn malloc_zone_statistics(zone: *const u8, stats: *mut MallocStats);
    }
    let mut m = MallocStats::default();
    // SAFETY: a null zone sums every zone; `m` is a malloc_statistics_t.
    unsafe { malloc_zone_statistics(std::ptr::null(), &mut m) };
    m
}
/// glibc's arenas (`mallinfo2`): bytes in use, bytes held from the system (arenas + mmapped blocks).
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn malloc_stats() -> MallocStats {
    // SAFETY: mallinfo2 has no preconditions.
    let m = unsafe { libc::mallinfo2() };
    MallocStats { blocks_in_use: (m.ordblks + m.hblks) as u32, size_in_use: m.uordblks + m.hblkhd, max_size_in_use: 0, size_allocated: m.arena + m.hblkhd }
}
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
fn malloc_stats() -> MallocStats {
    MallocStats::default()
}

impl Proc {
    pub fn now() -> Proc {
        let m = malloc_stats();
        let (fp, peak, rss) = rusage();
        Proc {
            phys_footprint: fp,
            phys_footprint_peak: peak,
            resident: rss,
            malloc_in_use: m.size_in_use as u64,
            malloc_allocated: m.size_allocated as u64,
            malloc_blocks: m.blocks_in_use as u64,
            rust_live: bough_mem::count::live() as u64,
            rust_allocs: bough_mem::count::allocs() as u64,
            metal_allocated: 0,
        }
    }
}

#[cfg(target_os = "macos")]
fn rusage() -> (u64, u64, u64) {
    let mut r = bough_sys::perf::RusageInfoV4::default();
    // SAFETY: flavor 4 (RUSAGE_INFO_V4) fills a rusage_info_v4.
    unsafe {
        unsafe extern "C" {
            fn proc_pid_rusage(pid: libc::c_int, flavor: libc::c_int, buffer: *mut libc::c_void) -> libc::c_int;
        }
        proc_pid_rusage(libc::getpid(), 4, &mut r as *mut _ as *mut libc::c_void);
    }
    (r.ri_phys_footprint, r.ri_lifetime_max_phys_footprint, r.ri_resident_size)
}
/// (footprint, its peak, resident): bough-sys's footprint (RssAnon + RssShmem), VmHWM as the peak (of resident:
/// Linux keeps no footprint peak), VmRSS.
#[cfg(not(target_os = "macos"))]
fn rusage() -> (u64, u64, u64) {
    let s = bough_sys::perf::snap();
    let hwm = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|t| t.lines().find(|l| l.starts_with("VmHWM:")).and_then(|l| l[6..].split_whitespace().next().and_then(|v| v.parse::<u64>().ok())))
        .unwrap_or(0)
        * 1024;
    (s.footprint, hwm, s.rss)
}

impl<V: View> App<V> {
    /// Every holder of this window's memory (see the module docs).
    pub fn ledger(&self) -> (Ledger, Proc) {
        let mut l = Ledger::default();
        self.rt.ledger(&mut l);
        crate::text::fonts_ledger(&mut l);
        self.ledger_app(&mut l);
        #[allow(unused_mut)]
        let mut p = Proc::now();
        #[cfg(all(target_os = "macos", not(feature = "wgpu")))]
        if let Some(g) = self.gpu.as_ref() {
            let m = g.gpu_mem();
            p.metal_allocated = m.device_allocated as u64;
            for (i, &(bytes, touched, volatile, in_use)) in m.surfaces.iter().enumerate() {
                let st = match (touched, volatile) {
                    (false, _) => "never drawn",
                    (true, true) => "drawn, purgeable",
                    (true, false) if in_use => "drawn, held by the compositor",
                    (true, false) => "drawn",
                };
                // an IOSurface owns no pages until it is first written; a volatile one may have been purged
                let res = if touched { bytes } else { 0 };
                l.push("gpu", format!("surface {i} ({st})"), Space::Gpu, Scales::Window, bytes, bytes, Some(res), Some(if volatile { 0 } else { res }));
            }
            if let Some((bytes, ..)) = m.scratch {
                l.push("gpu", "reference-render surface", Space::Gpu, Scales::Window, bytes, bytes, Some(bytes), Some(bytes));
            }
            l.push("gpu", format!("glyph atlas texture {}x{}", m.atlas_dims.0, m.atlas_dims.1), Space::Gpu, Scales::Content, m.atlas, m.atlas, None, None);
            if m.image_atlas > 0 {
                l.push("gpu", "image atlas texture", Space::Gpu, Scales::Content, m.image_atlas, m.image_atlas, None, None);
            }
            l.push("gpu", format!("row bins ({} per tile row)", m.bin_cap), Space::Gpu, Scales::Window, m.bins, m.bins, None, None);
            l.push("gpu", "bin counts", Space::Gpu, Scales::Window, m.counts, m.counts, None, None);
            if m.warm > 0 {
                l.push("gpu", "keep-warm buffer", Space::Gpu, Scales::Fixed, m.warm, m.warm, None, None);
            }
            // the region buffer is the `region` lines; its Metal-side size is recorded for the check
            l.push("gpu", "(region buffer, Metal's allocatedSize: counted in `region`)", Space::Gpu, Scales::Content, 0, m.region_allocated, Some(0), Some(0));
        }
        (l, p)
    }
}

/// JSON of a ledger and its process totals (no dependencies: names are plain ASCII from this crate).
pub fn to_json(l: &Ledger, p: &Proc) -> String {
    use std::fmt::Write;
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let opt = |v: Option<usize>| v.map_or("null".to_string(), |x| x.to_string());
    let mut s = String::new();
    let _ = write!(
        s,
        "{{\"proc\":{{\"phys_footprint\":{},\"phys_footprint_peak\":{},\"resident\":{},\"malloc_in_use\":{},\"malloc_allocated\":{},\"malloc_blocks\":{},\"rust_live\":{},\"rust_allocs\":{},\"metal_allocated\":{}}},\"lines\":[",
        p.phys_footprint, p.phys_footprint_peak, p.resident, p.malloc_in_use, p.malloc_allocated, p.malloc_blocks, p.rust_live, p.rust_allocs, p.metal_allocated
    );
    for (i, x) in l.lines.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(
            s,
            "{{\"group\":\"{}\",\"name\":\"{}\",\"space\":\"{:?}\",\"scales\":\"{:?}\",\"used\":{},\"reserved\":{},\"resident\":{},\"dirty\":{}}}",
            esc(x.group),
            esc(&x.name),
            x.space,
            x.scales,
            x.used,
            x.reserved,
            opt(x.resident),
            opt(x.dirty)
        );
    }
    s.push_str("]}");
    s
}
