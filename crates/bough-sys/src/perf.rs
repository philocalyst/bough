//! Process-level and thread-level counters: retired instructions, cycles, CPU time, memory.
//!
//! * macOS: `proc_pid_rusage` (V4: instructions and cycles of the whole process, footprint) and
//!   `thread_selfcounts` (the calling thread).
//! * Linux: `perf_event_open` hardware counters, user space only (`exclude_kernel`, allowed at
//!   `perf_event_paranoid` 2): a per-thread counter for the calling thread, and a process counter opened on the
//!   first `snap()` with `inherit`, so it counts the thread that opened it and every thread created after (not
//!   threads that already existed). Memory from `/proc/self/status`: `rss` = VmRSS, `footprint` = RssAnon +
//!   RssShmem (the dirty private and shared memory; the closest Linux notion to macOS's phys_footprint).
//!   Without a PMU (a VM, a container without perf) the instruction and cycle fields read 0.
use std::time::Instant;

#[derive(Clone, Copy, Default, Debug)]
pub struct Snap {
    pub instrs: u64,
    pub cycles: u64,
    pub cpu_ns: u64,
    pub rss: u64,
    pub footprint: u64,
    /// package-idle + interrupt wakeups (process lifetime; macOS only, 0 elsewhere)
    pub wakeups: u64,
    /// CPU time other processes billed to us (vouchers), ns (macOS only, 0 elsewhere)
    pub billed_ns: u64,
}

/// The calling thread's retired instructions (an alias of [`tinstr`], under the name bough-metal used).
#[inline]
pub fn thread_instructions() -> u64 {
    tinstr()
}

/// Cost of one `tinstr()` call (subtract once from a bracketed delta).
pub fn tinstr_overhead() -> u64 {
    let _ = tinstr();
    let mut v: Vec<u64> = (0..200)
        .map(|_| {
            let a = tinstr();
            let b = tinstr();
            b - a
        })
        .collect();
    v.sort();
    v[v.len() / 2]
}

/// Cost of one `snap()` call in instructions (subtract from deltas that bracket work).
pub fn snap_overhead() -> u64 {
    let _ = snap();
    let mut v: Vec<u64> = (0..200)
        .map(|_| {
            let a = snap();
            let b = snap();
            b.instrs - a.instrs
        })
        .collect();
    v.sort();
    v[v.len() / 2]
}

pub fn now_ns() -> u64 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

pub fn pct(v: &mut Vec<f64>, p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let i = ((v.len() - 1) as f64 * p).round() as usize;
    v[i]
}

#[cfg(target_os = "macos")]
pub use mac::*;
#[cfg(target_os = "macos")]
mod mac {
    #![allow(deprecated)]
    use super::Snap;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct RusageInfoV4 {
        pub ri_uuid: [u8; 16],
        pub ri_user_time: u64,
        pub ri_system_time: u64,
        pub ri_pkg_idle_wkups: u64,
        pub ri_interrupt_wkups: u64,
        pub ri_pageins: u64,
        pub ri_wired_size: u64,
        pub ri_resident_size: u64,
        pub ri_phys_footprint: u64,
        pub ri_proc_start_abstime: u64,
        pub ri_proc_exit_abstime: u64,
        pub ri_child_user_time: u64,
        pub ri_child_system_time: u64,
        pub ri_child_pkg_idle_wkups: u64,
        pub ri_child_interrupt_wkups: u64,
        pub ri_child_pageins: u64,
        pub ri_child_elapsed_abstime: u64,
        pub ri_diskio_bytesread: u64,
        pub ri_diskio_byteswritten: u64,
        pub ri_cpu_time_qos_default: u64,
        pub ri_cpu_time_qos_maintenance: u64,
        pub ri_cpu_time_qos_background: u64,
        pub ri_cpu_time_qos_utility: u64,
        pub ri_cpu_time_qos_legacy: u64,
        pub ri_cpu_time_qos_user_initiated: u64,
        pub ri_cpu_time_qos_user_interactive: u64,
        pub ri_billed_system_time: u64,
        pub ri_serviced_system_time: u64,
        pub ri_logical_writes: u64,
        pub ri_lifetime_max_phys_footprint: u64,
        pub ri_instructions: u64,
        pub ri_cycles: u64,
        pub ri_billed_energy: u64,
        pub ri_serviced_energy: u64,
        pub ri_interval_max_phys_footprint: u64,
        pub ri_runnable_time: u64,
    }

    unsafe extern "C" {
        fn proc_pid_rusage(pid: libc::c_int, flavor: libc::c_int, buffer: *mut libc::c_void) -> libc::c_int;
        fn thread_selfcounts(kind: libc::c_int, buf: *mut u64, nbytes: usize) -> libc::c_int;
    }

    /// This process's `rusage_info_v4`.
    pub fn rusage_v4() -> RusageInfoV4 {
        let mut r = RusageInfoV4::default();
        // SAFETY: flavor 4 (RUSAGE_INFO_V4) fills one rusage_info_v4, which `r` is.
        let rc = unsafe { proc_pid_rusage(libc::getpid(), 4, &mut r as *mut _ as *mut libc::c_void) };
        assert_eq!(rc, 0, "proc_pid_rusage failed");
        r
    }

    pub fn snap() -> Snap {
        let r = rusage_v4();
        let tb = timebase();
        Snap {
            instrs: r.ri_instructions,
            cycles: r.ri_cycles,
            cpu_ns: (r.ri_user_time + r.ri_system_time) * tb.0 / tb.1,
            rss: r.ri_resident_size,
            footprint: r.ri_phys_footprint,
            wakeups: r.ri_pkg_idle_wkups + r.ri_interrupt_wkups,
            billed_ns: r.ri_billed_system_time * tb.0 / tb.1,
        }
    }

    /// Retired instructions of the CALLING THREAD (libsystem_kernel `thread_selfcounts`, ~1.4 K instr per call,
    /// 4x cheaper than `proc_pid_rusage`). Used for in-tick and per-thread (completion handler) attribution.
    #[inline]
    pub fn tinstr() -> u64 {
        let mut b = [0u64; 2];
        // SAFETY: kind 1 fills two u64s (instructions, cycles); `b` has 16 bytes.
        unsafe { thread_selfcounts(1, b.as_mut_ptr(), 16) };
        b[0]
    }

    fn timebase() -> (u64, u64) {
        static TB: std::sync::OnceLock<(u64, u64)> = std::sync::OnceLock::new();
        *TB.get_or_init(|| {
            let mut t = libc::mach_timebase_info { numer: 0, denom: 0 };
            // SAFETY: fills the struct.
            unsafe { libc::mach_timebase_info(&mut t) };
            (t.numer as u64, t.denom as u64)
        })
    }

    /// Wall time since this process was exec'd (the kernel's start time; microsecond resolution), ns.
    pub fn since_exec_ns() -> u64 {
        // SAFETY: plain old data.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let sz = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
        // SAFETY: PROC_PIDTBSDINFO fills one proc_bsdinfo; `info` is that size.
        let rc = unsafe { libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut libc::c_void, sz) };
        assert_eq!(rc, sz, "proc_pidinfo failed");
        let start = std::time::UNIX_EPOCH + std::time::Duration::new(info.pbi_start_tvsec, info.pbi_start_tvusec as u32 * 1000);
        std::time::SystemTime::now().duration_since(start).map(|d| d.as_nanos() as u64).unwrap_or(0)
    }
}

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "linux")]
mod linux {
    use super::Snap;
    use std::sync::OnceLock;

    /// `struct perf_event_attr` up to `PERF_ATTR_SIZE_VER5` (112 bytes).
    #[repr(C)]
    #[derive(Default)]
    struct Attr {
        ty: u32,
        size: u32,
        config: u64,
        sample_period: u64,
        sample_type: u64,
        read_format: u64,
        /// The bit fields: disabled 0, inherit 1, pinned 2, exclusive 3, exclude_user 4, exclude_kernel 5,
        /// exclude_hv 6, ...
        flags: u64,
        wakeup_events: u32,
        bp_type: u32,
        config1: u64,
        config2: u64,
        branch_sample_type: u64,
        sample_regs_user: u64,
        sample_stack_user: u32,
        clockid: i32,
        sample_regs_intr: u64,
        aux_watermark: u32,
        sample_max_stack: u16,
        reserved: u16,
    }
    const _: () = assert!(size_of::<Attr>() == 112);
    const PERF_TYPE_HARDWARE: u32 = 0;
    const HW_CPU_CYCLES: u64 = 0;
    const HW_INSTRUCTIONS: u64 = 1;
    const INHERIT: u64 = 1 << 1;
    const EXCLUDE_KERNEL: u64 = 1 << 5;
    const EXCLUDE_HV: u64 = 1 << 6;
    const PERF_FLAG_FD_CLOEXEC: libc::c_ulong = 1 << 3;

    /// A user-space hardware counter on the calling thread (`inherit`: and the threads it creates later), or -1.
    fn open(config: u64, inherit: bool) -> i32 {
        let a = Attr { ty: PERF_TYPE_HARDWARE, size: size_of::<Attr>() as u32, config, flags: EXCLUDE_KERNEL | EXCLUDE_HV | if inherit { INHERIT } else { 0 }, ..Attr::default() };
        // SAFETY: a valid perf_event_attr of its own size; pid 0 / cpu -1 = the calling thread on any CPU; no group.
        let fd = unsafe { libc::syscall(libc::SYS_perf_event_open, &a as *const Attr, 0, -1, -1, PERF_FLAG_FD_CLOEXEC) };
        fd as i32
    }

    fn read(fd: i32) -> u64 {
        if fd < 0 {
            return 0;
        }
        let mut v = 0u64;
        // SAFETY: a counter fd without read_format flags reads one u64.
        let n = unsafe { libc::read(fd, &mut v as *mut u64 as *mut libc::c_void, 8) };
        if n == 8 { v } else { 0 }
    }

    /// A thread's counter, closed when the thread exits.
    struct Fd(i32);
    impl Drop for Fd {
        fn drop(&mut self) {
            if self.0 >= 0 {
                // SAFETY: the fd this struct opened and alone owns.
                unsafe { libc::close(self.0) };
            }
        }
    }

    thread_local! {
        static THREAD: Fd = Fd(open(HW_INSTRUCTIONS, false));
    }

    /// Retired user-space instructions of the CALLING THREAD since its first call (a per-thread counter opened
    /// on first use; 0 without a PMU).
    #[inline]
    pub fn tinstr() -> u64 {
        THREAD.with(|fd| read(fd.0))
    }

    /// The process counters (instructions, cycles): opened on the first call with `inherit`.
    fn proc_fds() -> (i32, i32) {
        static P: OnceLock<(i32, i32)> = OnceLock::new();
        *P.get_or_init(|| (open(HW_INSTRUCTIONS, true), open(HW_CPU_CYCLES, true)))
    }

    fn status_kb(s: &str, key: &str) -> u64 {
        s.lines().find(|l| l.starts_with(key)).and_then(|l| l[key.len()..].split_whitespace().next()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1024
    }

    pub fn snap() -> Snap {
        let (fi, fc) = proc_fds();
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: fills `ts`.
        unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) };
        let st = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        Snap {
            instrs: read(fi),
            cycles: read(fc),
            cpu_ns: ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64,
            rss: status_kb(&st, "VmRSS:"),
            footprint: status_kb(&st, "RssAnon:") + status_kb(&st, "RssShmem:"),
            wakeups: 0,
            billed_ns: 0,
        }
    }

    /// Wall time since this process started (procfs start time in clock ticks since boot, against
    /// CLOCK_BOOTTIME; tick resolution), ns.
    pub fn since_exec_ns() -> u64 {
        let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else { return 0 };
        // field 22 (starttime) counts from after the parenthesised command name
        let Some(rest) = stat.rfind(')').map(|i| &stat[i + 2..]) else { return 0 };
        let Some(start) = rest.split_whitespace().nth(19).and_then(|v| v.parse::<u64>().ok()) else { return 0 };
        // SAFETY: sysconf has no preconditions.
        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: fills `ts`.
        unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
        let now = ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64;
        now.saturating_sub(start * 1_000_000_000 / hz)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod other {
    use super::Snap;
    pub fn snap() -> Snap {
        Snap::default()
    }
    pub fn tinstr() -> u64 {
        0
    }
    pub fn since_exec_ns() -> u64 {
        0
    }
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub use other::*;
