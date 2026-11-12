//! bough-sys: the operating-system services bough's engine and its measurement tools use that are not a window
//! or a GPU: retired-instruction counters (process and calling thread), process CPU time and memory, the time
//! since exec, and a machine identity string. One API, one implementation per OS (`perf`): macOS uses
//! `proc_pid_rusage` and `thread_selfcounts`; Linux uses `perf_event_open` (user-space instructions and cycles,
//! the calling thread or the process from first use) and procfs.
//!
//! Moved from bough-metal's `perf` module (round 7, platform lane); bough-metal re-exports it.
pub mod perf;

/// A string naming this machine's CPU model and OS build: a cache key for anything whose result depends on the
/// hardware and the OS (the CPU repair backend's self-check verdict).
pub fn machine_key() -> String {
    #[cfg(target_os = "macos")]
    {
        format!("{}|{}", sysctl_string("machdep.cpu.brand_string"), sysctl_string("kern.osversion"))
    }
    #[cfg(target_os = "linux")]
    {
        let cpu = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|s| s.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|v| v.trim().to_string()))
            .unwrap_or_default();
        let os = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
        format!("{}|{}", cpu, os.trim())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        String::from(std::env::consts::OS)
    }
}

#[cfg(target_os = "macos")]
fn sysctl_string(name: &str) -> String {
    let c = std::ffi::CString::new(name).unwrap();
    let mut buf = [0u8; 256];
    let mut len = buf.len();
    // SAFETY: a NUL-terminated name, a 256-byte buffer and its length.
    if unsafe { libc::sysctlbyname(c.as_ptr(), buf.as_mut_ptr() as *mut libc::c_void, &mut len, std::ptr::null_mut(), 0) } != 0 {
        return String::new();
    }
    String::from_utf8_lossy(&buf[..len.saturating_sub(1)]).into_owned()
}
