//! Process facts the harness reports, per platform (the crate's only platform code besides the GPU readback).

/// Resident memory now and its peak, MiB. macOS: `phys_footprint` (what the system charges; Activity Monitor's
/// "Memory") and `ru_maxrss`; Linux: `VmRSS` and `VmHWM` from `/proc/self/status`.
pub fn memory_mib() -> (f64, f64) {
    const MIB: f64 = 1024.0 * 1024.0;
    #[cfg(target_os = "macos")]
    {
        let s = bough_metal::perf::snap();
        (s.footprint as f64 / MIB, peak_rss_bytes() as f64 / MIB)
    }
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        let kb = |key: &str| {
            status.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0)
        };
        (kb("VmRSS:") * 1024.0 / MIB, kb("VmHWM:") * 1024.0 / MIB)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        (0.0, peak_rss_bytes() as f64 / MIB)
    }
}

/// `getrusage(RUSAGE_SELF).ru_maxrss` in bytes (macOS reports bytes, Linux KiB).
pub fn peak_rss_bytes() -> u64 {
    // SAFETY: getrusage fills the struct it is given.
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    let v = u.ru_maxrss as u64;
    if cfg!(target_os = "macos") { v } else { v * 1024 }
}

/// Milliseconds since this process was exec'd (cold-start wall time up to the call).
pub fn since_exec_ms() -> f64 {
    #[cfg(target_os = "macos")]
    {
        bough_metal::perf::since_exec_ns() as f64 / 1e6
    }
    #[cfg(target_os = "linux")]
    {
        // /proc/self/stat field 22: start time in clock ticks since boot; /proc/uptime: seconds since boot.
        let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
        let after = stat.rsplit_once(')').map(|(_, r)| r).unwrap_or("");
        let start: f64 = after.split_whitespace().nth(19).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        // SAFETY: plain sysconf query.
        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
        let up: f64 = std::fs::read_to_string("/proc/uptime").ok().and_then(|s| s.split_whitespace().next().and_then(|v| v.parse().ok())).unwrap_or(0.0);
        (up - start / hz) * 1e3
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0.0
    }
}
