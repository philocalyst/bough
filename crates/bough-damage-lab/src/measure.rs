//! Instruction and time counters. macOS: `ri_instructions` is per task (all threads, user + kernel), so timing
//! runs single-threaded and the counter pair's own cost is subtracted (`overhead`). Linux (round 7, bench lane):
//! the calling thread's perf_event group from `bough_mem::pmu` (user mode only unless `BOUGH_PMU_KERNEL=1`), so
//! the same examples run on ilo-prod; `docs/lanes/bench/MEASUREMENT.md` says how the two compare.

#[cfg(target_os = "macos")]
pub use mac::*;
#[cfg(not(target_os = "macos"))]
pub use portable::*;

#[cfg(target_os = "macos")]
mod mac {
    #[repr(C)]
    #[derive(Default)]
    struct RusageInfoV4 {
        ri_uuid: [u8; 16],
        f: [u64; 29],
        ri_instructions: u64,
        ri_cycles: u64,
        rest: [u64; 4],
    }

    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV4) -> i32;
        fn getpid() -> i32;
        fn mach_absolute_time() -> u64;
        fn pthread_set_qos_class_self_np(qos: u32, rel: i32) -> i32;
    }

    /// (instructions, cycles) retired by this task so far.
    #[inline(never)]
    pub fn counters() -> (u64, u64) {
        let mut ri = RusageInfoV4::default();
        // SAFETY: the buffer is a correctly sized rusage_info_v4 (flavor 4).
        let rc = unsafe { proc_pid_rusage(getpid(), 4, &mut ri) };
        assert_eq!(rc, 0);
        (ri.ri_instructions, ri.ri_cycles)
    }

    #[inline(always)]
    pub fn ticks() -> u64 {
        // SAFETY: plain libSystem call.
        unsafe { mach_absolute_time() }
    }

    /// Apple silicon timebase is 125/3 ns per tick.
    pub fn ticks_to_ns(t: u64) -> f64 {
        t as f64 * 125.0 / 3.0
    }

    /// Prefer performance cores: QOS_CLASS_USER_INTERACTIVE.
    pub fn prefer_p_cores() {
        // SAFETY: plain libSystem call on the current thread.
        unsafe {
            pthread_set_qos_class_self_np(0x21, 0);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod portable {
    use std::sync::OnceLock;
    use std::time::Instant;

    /// (instructions, cycles) retired by the calling thread since its counters opened.
    #[inline(never)]
    pub fn counters() -> (u64, u64) {
        bough_mem::pmu::instructions_cycles()
    }

    /// Monotonic nanoseconds since the first call (the macOS build returns mach ticks; `ticks_to_ns` converts).
    #[inline(always)]
    pub fn ticks() -> u64 {
        static T0: OnceLock<Instant> = OnceLock::new();
        T0.get_or_init(Instant::now).elapsed().as_nanos() as u64
    }

    pub fn ticks_to_ns(t: u64) -> f64 {
        t as f64
    }

    /// No QoS classes here; pin with `taskset` instead (MEASUREMENT.md).
    pub fn prefer_p_cores() {}
}

/// Instructions consumed by one `counters()` pair with nothing between.
pub fn overhead() -> u64 {
    let mut best = u64::MAX;
    for _ in 0..50 {
        let a = counters().0;
        let b = counters().0;
        best = best.min(b - a);
    }
    best
}
