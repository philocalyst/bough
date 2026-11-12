//! Hardware counters of the calling thread, one call on both hosts (bench lane, round 7;
//! `docs/lanes/bench/MEASUREMENT.md`). Measurement only: nothing on a frame path calls this.
//!
//! * **macOS**: `thread_selfcounts` gives instructions and cycles of the calling thread (user and kernel) and
//!   nothing else. Cache and TLB events need kperf (root and a private entitlement), so they are refused.
//! * **Linux**: one `perf_event_open` group on the calling thread (`pid 0, cpu -1`), read with one `read(2)`
//!   (`PERF_FORMAT_GROUP`). The group is scheduled on the PMU as a unit, so every value in a [`Reading`] covers
//!   the same instructions; if the PMU could not hold the group the whole time, [`Reading::complete`] is false
//!   and the values are partial (never scaled). User mode only unless `BOUGH_PMU_KERNEL=1`, so the `read(2)`
//!   bracketing a cell does not count itself. Needs `perf_event_paranoid <= 2` for user-mode counting of
//!   one's own thread, or root.
//!
//! Zen 2 (ilo-prod, Ryzen 7 3700X) has six core counters and the NMI watchdog takes one: a group of more than
//! five events never schedules. The generic `cache-references` / `cache-misses` are L2 accesses and L2 misses
//! there (`0xff60`, `0x0964`), not last-level-cache events; Zen 2 has no generic LLC events, and its L3 counters
//! live in the uncore PMU (`amd_l3`), which this kernel does not expose. The data-cache refill sources
//! (`dc-fill-*`, raw event `0x43`) say where an L1 miss was served from, which is what a "cache lines from DRAM"
//! question needs.
//!
//! `BOUGH_PMU=l1d-misses,cache-misses,dc-fill-dram` adds up to three events after the two every thread group
//! starts with (instructions, cycles); see [`Ev::parse`] for the names.

use std::fmt;

/// One countable event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ev {
    Instructions,
    Cycles,
    BranchMisses,
    /// L1 data-cache read accesses (generic `L1-dcache-loads`).
    L1dLoads,
    /// L1 data-cache read misses (generic `L1-dcache-load-misses`; on Zen 2: requests from the data cache
    /// to L2, which counts store misses too).
    L1dMisses,
    /// Generic `cache-references` (Zen 2: all L2 requests).
    CacheRefs,
    /// Generic `cache-misses` (Zen 2: L2 misses of data and instruction fetches).
    CacheMisses,
    DtlbMisses,
    /// A model-specific raw event (`PERF_TYPE_RAW` config) with the name it prints under.
    Raw(u64, &'static str),
}

impl Ev {
    /// The names `BOUGH_PMU` accepts: the generic events by perf's names (`instructions`, `cycles`,
    /// `branch-misses`, `l1d-loads`, `l1d-misses`, `cache-refs`, `cache-misses`, `dtlb-misses`), the Zen 2 data
    /// cache refill sources (`dc-fill-l2`, `dc-fill-ccx` = L3 or another core's L2 in the same CCX,
    /// `dc-fill-dram`, `dc-fill-remote` = another CCX's caches), and `raw:0x<config>`.
    pub fn parse(s: &str) -> Option<Ev> {
        Some(match s.trim() {
            "instructions" | "ins" => Ev::Instructions,
            "cycles" | "cyc" => Ev::Cycles,
            "branch-misses" => Ev::BranchMisses,
            "l1d-loads" => Ev::L1dLoads,
            "l1d-misses" => Ev::L1dMisses,
            "cache-refs" | "cache-references" => Ev::CacheRefs,
            "cache-misses" => Ev::CacheMisses,
            "dtlb-misses" => Ev::DtlbMisses,
            // Zen 2 PMC 0x43 LsRefillsFromSys, unit masks: 0x01 local L2, 0x02 local cache (L3 or another L2 of
            // the CCX), 0x08 local DRAM, 0x10 remote cache (another CCX).
            "dc-fill-l2" => Ev::Raw(0x0143, "dc-fill-l2"),
            "dc-fill-ccx" => Ev::Raw(0x0243, "dc-fill-ccx"),
            "dc-fill-dram" => Ev::Raw(0x0843, "dc-fill-dram"),
            "dc-fill-remote" => Ev::Raw(0x1043, "dc-fill-remote"),
            s => {
                let hex = s.strip_prefix("raw:0x")?;
                Ev::Raw(u64::from_str_radix(hex, 16).ok()?, "raw")
            }
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Ev::Instructions => "instructions",
            Ev::Cycles => "cycles",
            Ev::BranchMisses => "branch-misses",
            Ev::L1dLoads => "l1d-loads",
            Ev::L1dMisses => "l1d-misses",
            Ev::CacheRefs => "cache-refs",
            Ev::CacheMisses => "cache-misses",
            Ev::DtlbMisses => "dtlb-misses",
            Ev::Raw(_, n) => n,
        }
    }
}

/// Most events in one group (the PMU's limit on Zen 2 with the NMI watchdog on; Apple silicon offers two here).
pub const MAX: usize = 5;

/// Counter values of one group at one moment (or a difference of two, [`Reading::since`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub v: [u64; MAX],
    pub n: usize,
    /// Nanoseconds the group was enabled and actually counting (Linux; equal on macOS).
    pub enabled: u64,
    pub running: u64,
}

impl Reading {
    /// Every value covers the whole interval (the PMU held the group throughout).
    pub fn complete(&self) -> bool {
        self.running == self.enabled
    }
    pub fn since(&self, earlier: &Reading) -> Reading {
        let mut d = *self;
        for i in 0..self.n {
            d.v[i] = self.v[i].wrapping_sub(earlier.v[i]);
        }
        d.enabled = self.enabled.wrapping_sub(earlier.enabled);
        d.running = self.running.wrapping_sub(earlier.running);
        d
    }
}

/// A group of counters on the thread that opened it. Read it only from that thread.
pub struct Group {
    evs: Vec<Ev>,
    #[cfg(target_os = "linux")]
    fds: Vec<i32>,
}

impl fmt::Debug for Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.evs.iter().map(|e| e.name())).finish()
    }
}

impl Group {
    /// Open `evs` (at most [`MAX`]) on the calling thread, counting from now.
    pub fn open(evs: &[Ev]) -> Result<Group, String> {
        if evs.is_empty() || evs.len() > MAX {
            return Err(format!("pmu: {} events (1..={MAX} fit one group)", evs.len()));
        }
        sys::open(evs)
    }

    /// The events this group counts, in [`Reading::v`] order.
    pub fn events(&self) -> &[Ev] {
        &self.evs
    }

    pub fn read(&self) -> Reading {
        sys::read(self)
    }

    /// Instructions, cycles, then the events named in `BOUGH_PMU` (comma separated, at most three; unknown
    /// names and events this host refuses are reported once on stderr and left out).
    pub fn from_env() -> Group {
        let mut evs = vec![Ev::Instructions, Ev::Cycles];
        if let Ok(list) = std::env::var("BOUGH_PMU") {
            for name in list.split(',').filter(|s| !s.trim().is_empty()) {
                match Ev::parse(name) {
                    Some(e) if evs.len() < MAX && !evs.contains(&e) => evs.push(e),
                    Some(_) => eprintln!("pmu: {name} left out (duplicate, or more than {MAX} events)"),
                    None => eprintln!("pmu: unknown event {name}"),
                }
            }
        }
        loop {
            match Group::open(&evs) {
                Ok(g) => return g,
                Err(e) if evs.len() > 2 => {
                    eprintln!("pmu: {e}; dropping {}", evs[evs.len() - 1].name());
                    evs.pop();
                }
                Err(e) => panic!("{e}"),
            }
        }
    }
}

thread_local! {
    static THREAD: Group = Group::from_env();
}

/// The calling thread's counters (opened on first use: instructions, cycles, then `BOUGH_PMU`'s events).
pub fn sample() -> Reading {
    THREAD.with(|g| g.read())
}

/// The events [`sample`] reports on this thread, in order.
pub fn sample_events() -> Vec<Ev> {
    THREAD.with(|g| g.events().to_vec())
}

/// Instructions and cycles retired by the calling thread so far (counted from the first call on Linux).
#[inline]
pub fn instructions_cycles() -> (u64, u64) {
    let r = sample();
    (r.v[0], r.v[1])
}

#[cfg(target_os = "macos")]
mod sys {
    use super::{Ev, Group, Reading};

    unsafe extern "C" {
        fn thread_selfcounts(kind: i32, buf: *mut u64, nbytes: usize) -> i32;
    }

    pub fn open(evs: &[Ev]) -> Result<Group, String> {
        if let Some(e) = evs.iter().find(|e| !matches!(e, Ev::Instructions | Ev::Cycles)) {
            return Err(format!("pmu: {} needs kperf on macOS (only instructions and cycles are offered)", e.name()));
        }
        Ok(Group { evs: evs.to_vec() })
    }

    pub fn read(g: &Group) -> Reading {
        let mut c = [0u64; 2];
        // SAFETY: THSC_CPI (1) writes two u64s (instructions, cycles) into a 16-byte buffer.
        unsafe { thread_selfcounts(1, c.as_mut_ptr(), 16) };
        let mut r = Reading { n: g.evs.len(), ..Reading::default() };
        for (i, e) in g.evs.iter().enumerate() {
            r.v[i] = if *e == Ev::Instructions { c[0] } else { c[1] };
        }
        r
    }
}

#[cfg(target_os = "linux")]
mod sys {
    use super::{Ev, Group, Reading, MAX};
    use std::ffi::{c_int, c_long, c_ulong, c_void};

    unsafe extern "C" {
        fn syscall(n: c_long, ...) -> c_long;
        fn ioctl(fd: c_int, req: c_ulong, ...) -> c_int;
        fn read(fd: c_int, buf: *mut c_void, n: usize) -> isize;
        fn close(fd: c_int) -> c_int;
    }

    #[cfg(target_arch = "x86_64")]
    const SYS_PERF_EVENT_OPEN: c_long = 298;
    #[cfg(target_arch = "aarch64")]
    const SYS_PERF_EVENT_OPEN: c_long = 241;

    /// `perf_event_attr` up to `config1` (PERF_ATTR_SIZE_VER0, 64 bytes; the kernel takes older sizes).
    #[repr(C)]
    #[derive(Default)]
    struct Attr {
        ty: u32,
        size: u32,
        config: u64,
        sample_period: u64,
        sample_type: u64,
        read_format: u64,
        flags: u64,
        wakeup_events: u32,
        bp_type: u32,
        config1: u64,
    }
    const _: () = assert!(std::mem::size_of::<Attr>() == 64);

    const DISABLED: u64 = 1 << 0;
    const EXCLUDE_KERNEL: u64 = 1 << 5;
    const EXCLUDE_HV: u64 = 1 << 6;
    const FORMAT: u64 = 1 | 2 | 8; // TOTAL_TIME_ENABLED | TOTAL_TIME_RUNNING | GROUP
    const IOC_ENABLE: c_ulong = 0x2400;
    const IOC_RESET: c_ulong = 0x2403;
    const IOC_FLAG_GROUP: c_ulong = 1;
    const FD_CLOEXEC: c_ulong = 8;

    fn code(e: Ev) -> (u32, u64) {
        const fn cache(id: u64, miss: bool) -> u64 {
            id | (0 << 8) | ((miss as u64) << 16) // op READ
        }
        match e {
            Ev::Cycles => (0, 0),
            Ev::Instructions => (0, 1),
            Ev::CacheRefs => (0, 2),
            Ev::CacheMisses => (0, 3),
            Ev::BranchMisses => (0, 5),
            Ev::L1dLoads => (3, cache(0, false)),
            Ev::L1dMisses => (3, cache(0, true)),
            Ev::DtlbMisses => (3, cache(3, true)),
            Ev::Raw(c, _) => (4, c),
        }
    }

    pub fn open(evs: &[Ev]) -> Result<Group, String> {
        let kernel = std::env::var("BOUGH_PMU_KERNEL").is_ok_and(|v| v == "1");
        let mut fds: Vec<i32> = Vec::with_capacity(evs.len());
        for (i, &e) in evs.iter().enumerate() {
            let (ty, config) = code(e);
            let mut flags = EXCLUDE_HV;
            if !kernel {
                flags |= EXCLUDE_KERNEL;
            }
            if i == 0 {
                flags |= DISABLED;
            }
            let attr = Attr { ty, size: 64, config, read_format: FORMAT, flags, ..Attr::default() };
            let leader: c_int = if i == 0 { -1 } else { fds[0] };
            // SAFETY: perf_event_open(attr, pid 0 = this thread, cpu -1 = any, group leader, flags); `attr` is a
            // valid PERF_ATTR_SIZE_VER0 struct that outlives the call.
            let fd = unsafe { syscall(SYS_PERF_EVENT_OPEN, &attr as *const Attr, 0 as c_int, -1 as c_int, leader, FD_CLOEXEC) };
            if fd < 0 {
                let err = std::io::Error::last_os_error();
                for &f in &fds {
                    // SAFETY: an fd this function opened.
                    unsafe { close(f) };
                }
                return Err(format!("pmu: perf_event_open({}) failed: {err} (perf_event_paranoid? event on this CPU?)", e.name()));
            }
            fds.push(fd as i32);
        }
        // SAFETY: the leader's fd; reset and enable the whole group.
        unsafe {
            ioctl(fds[0], IOC_RESET, IOC_FLAG_GROUP);
            ioctl(fds[0], IOC_ENABLE, IOC_FLAG_GROUP);
        }
        Ok(Group { evs: evs.to_vec(), fds })
    }

    pub fn read(g: &Group) -> Reading {
        let mut buf = [0u64; 3 + MAX];
        let want = (3 + g.evs.len()) * 8;
        // SAFETY: reading a PERF_FORMAT_GROUP record (nr, time_enabled, time_running, values[nr]) into a buffer
        // large enough for MAX values.
        let got = unsafe { read(g.fds[0], buf.as_mut_ptr() as *mut c_void, std::mem::size_of_val(&buf)) };
        assert_eq!(got as usize, want, "pmu: short read of the counter group");
        let mut r = Reading { n: buf[0] as usize, enabled: buf[1], running: buf[2], ..Reading::default() };
        r.v[..r.n].copy_from_slice(&buf[3..3 + r.n]);
        r
    }

    impl Drop for Group {
        fn drop(&mut self) {
            for &f in self.fds.iter().rev() {
                // SAFETY: fds this group opened and still owns.
                unsafe { close(f) };
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod sys {
    use super::{Ev, Group, Reading};
    pub fn open(_: &[Ev]) -> Result<Group, String> {
        Err("pmu: no counters on this OS".into())
    }
    pub fn read(_: &Group) -> Reading {
        Reading::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_a_loop() {
        let g = Group::open(&[Ev::Instructions, Ev::Cycles]).expect("counters");
        let a = g.read();
        let mut x = 0u64;
        for i in 0..1_000_000u64 {
            x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(i));
        }
        let d = g.read().since(&a);
        assert!(d.v[0] > 1_000_000, "instructions {}", d.v[0]);
        assert!(d.v[1] > 0);
        assert!(d.complete());
    }

    #[test]
    fn parses_names() {
        assert_eq!(Ev::parse("dc-fill-dram"), Some(Ev::Raw(0x0843, "dc-fill-dram")));
        assert_eq!(Ev::parse("raw:0x0964"), Some(Ev::Raw(0x0964, "raw")));
        assert_eq!(Ev::parse("l1d-misses"), Some(Ev::L1dMisses));
        assert_eq!(Ev::parse("nope"), None);
    }
}
