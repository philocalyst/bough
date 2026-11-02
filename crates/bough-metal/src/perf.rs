#![allow(deprecated)]//! Process-level counters: retired instructions (proc_pid_rusage V4), CPU time, memory.
use std::time::Instant;

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
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Snap {
    pub instrs: u64,
    pub cycles: u64,
    pub cpu_ns: u64,
    pub rss: u64,
    pub footprint: u64,
    /// package-idle + interrupt wakeups (process lifetime)
    pub wakeups: u64,
    /// CPU time other processes billed to us (vouchers), ns
    pub billed_ns: u64,
}

pub fn snap() -> Snap {
    let mut r = RusageInfoV4::default();
    let rc = unsafe { proc_pid_rusage(libc::getpid(), 4, &mut r as *mut _ as *mut libc::c_void) };
    assert_eq!(rc, 0, "proc_pid_rusage failed");
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

unsafe extern "C" {
    fn thread_selfcounts(kind: libc::c_int, buf: *mut u64, nbytes: usize) -> libc::c_int;
}

/// Retired instructions of the CALLING THREAD (libsystem_kernel `thread_selfcounts`, ~1.4 K instr per call,
/// 4x cheaper than `proc_pid_rusage`). Used for in-tick and per-thread (completion handler) attribution.
#[inline]
pub fn tinstr() -> u64 {
    let mut b = [0u64; 2];
    unsafe { thread_selfcounts(1, b.as_mut_ptr(), 16) };
    b[0]
}

/// Cost of one `tinstr()` call (subtract once from a bracketed delta).
pub fn tinstr_overhead() -> u64 {
    let _ = tinstr();
    let mut v: Vec<u64> = (0..200).map(|_| { let a = tinstr(); let b = tinstr(); b - a }).collect();
    v.sort();
    v[v.len() / 2]
}

fn timebase() -> (u64, u64) {
    static TB: std::sync::OnceLock<(u64, u64)> = std::sync::OnceLock::new();
    *TB.get_or_init(|| {
        let mut t = libc::mach_timebase_info { numer: 0, denom: 0 };
        unsafe { libc::mach_timebase_info(&mut t) };
        (t.numer as u64, t.denom as u64)
    })
}

/// Cost of one `snap()` call in instructions (subtract from deltas that bracket work).
/// Wall time since this process was exec'd (the kernel's start time; microsecond resolution), ns.
pub fn since_exec_ns() -> u64 {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let sz = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: PROC_PIDTBSDINFO fills one proc_bsdinfo; `info` is that size.
    let rc = unsafe { libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut libc::c_void, sz) };
    assert_eq!(rc, sz, "proc_pidinfo failed");
    let start = std::time::UNIX_EPOCH + std::time::Duration::new(info.pbi_start_tvsec, info.pbi_start_tvusec as u32 * 1000);
    std::time::SystemTime::now().duration_since(start).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

pub fn snap_overhead() -> u64 {
    let _ = snap();
    let mut v: Vec<u64> = (0..200).map(|_| { let a = snap(); let b = snap(); b.instrs - a.instrs }).collect();
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
