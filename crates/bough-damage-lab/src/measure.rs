//! Instruction and time counters. macOS: `ri_instructions` is per task (all threads, user + kernel). Linux:
//! `perf_event_open` on the calling thread, user space only (`instructions:u`, `cycles:u`, which
//! `perf_event_paranoid` ≤ 2 allows). Either way timing runs single-threaded and the counter pair's own cost is
//! subtracted (`overhead`). The two platforms count different ISAs; compare a count only with one from the same one.

#[cfg(target_os = "macos")]
mod os {
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

#[cfg(target_os = "linux")]
mod os {
    use core::ffi::{c_int, c_long, c_void};

    unsafe extern "C" {
        fn syscall(n: c_long, ...) -> c_long;
        fn read(fd: c_int, buf: *mut c_void, n: usize) -> isize;
        fn clock_gettime(clk: c_int, ts: *mut [i64; 2]) -> c_int;
    }

    #[cfg(target_arch = "x86_64")]
    const SYS_PERF_EVENT_OPEN: c_long = 298;
    #[cfg(target_arch = "aarch64")]
    const SYS_PERF_EVENT_OPEN: c_long = 241;
    const PERF_TYPE_HARDWARE: u32 = 0;
    const PERF_COUNT_HW_CPU_CYCLES: u64 = 0;
    const PERF_COUNT_HW_INSTRUCTIONS: u64 = 1;
    const PERF_FORMAT_GROUP: u64 = 1 << 3;
    // attr.flags bits: exclude_kernel (5), exclude_hv (6).
    const EXCLUDE_KERNEL_HV: u64 = (1 << 5) | (1 << 6);

    /// `struct perf_event_attr` (PERF_ATTR_SIZE_VER5, 112 bytes). `disabled` (flags bit 0) is clear, so a counter
    /// runs from the moment it is opened.
    #[repr(C)]
    struct Attr {
        ty: u32,
        size: u32,
        config: u64,
        sample_period: u64,
        sample_type: u64,
        read_format: u64,
        flags: u64,
        rest: [u64; 8],
    }

    fn open(config: u64, group: c_int) -> c_int {
        let attr = Attr {
            ty: PERF_TYPE_HARDWARE,
            size: size_of::<Attr>() as u32,
            config,
            sample_period: 0,
            sample_type: 0,
            read_format: PERF_FORMAT_GROUP,
            flags: EXCLUDE_KERNEL_HV,
            rest: [0; 8],
        };
        // SAFETY: perf_event_open(attr, pid = 0 (this thread), cpu = -1 (any), group_fd, flags = 0) with a fully
        // initialized attr whose `size` names its own length.
        let fd = unsafe { syscall(SYS_PERF_EVENT_OPEN, &attr as *const Attr, 0 as c_int, -1 as c_int, group, 0u64) };
        assert!(fd >= 0, "perf_event_open failed (perf_event_paranoid > 2, or no PMU access in this VM?)");
        fd as c_int
    }

    std::thread_local! {
        /// The group leader (instructions; cycles is its member), opened on first use on this thread.
        static FD: c_int = {
            let lead = open(PERF_COUNT_HW_INSTRUCTIONS, -1);
            open(PERF_COUNT_HW_CPU_CYCLES, lead);
            lead
        };
    }

    pub fn counters() -> (u64, u64) {
        FD.with(|&fd| {
            let mut v = [0u64; 3];
            // SAFETY: a PERF_FORMAT_GROUP read of a two-event group is { nr, instructions, cycles }: 24 bytes into `v`.
            let n = unsafe { read(fd, v.as_mut_ptr().cast(), size_of_val(&v)) };
            assert_eq!(n, 24);
            (v[1], v[2])
        })
    }

    /// Monotonic nanoseconds.
    #[inline(always)]
    pub fn ticks() -> u64 {
        let mut ts = [0i64; 2];
        // SAFETY: clock_gettime(CLOCK_MONOTONIC = 1) writes one timespec.
        unsafe { clock_gettime(1, &mut ts) };
        ts[0] as u64 * 1_000_000_000 + ts[1] as u64
    }

    pub fn ticks_to_ns(t: u64) -> f64 {
        t as f64
    }

    /// No core classes to prefer on the hosts this runs on (not hybrid); placement is the caller's (`taskset`).
    pub fn prefer_p_cores() {}
}

/// (instructions, cycles) retired so far (macOS: this task; Linux: this thread, user space).
#[inline(never)]
pub fn counters() -> (u64, u64) {
    os::counters()
}

pub use os::{prefer_p_cores, ticks, ticks_to_ns};

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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    /// The counter must see work: 10⁶ dependent adds retire at least 10⁶ instructions, and the pair overhead is
    /// small and stable.
    #[test]
    fn counts_user_instructions() {
        let ov = super::overhead();
        let a = super::counters().0;
        let mut x = 0u64;
        for i in 0..1_000_000u64 {
            x = std::hint::black_box(x.wrapping_add(i));
        }
        let b = super::counters().0;
        std::hint::black_box(x);
        assert!(b - a >= 1_000_000, "{} instructions for 10^6 adds", b - a);
        assert!(ov < 2_000, "counter pair overhead {ov}");
    }
}
