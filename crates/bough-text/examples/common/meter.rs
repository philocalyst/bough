//! Retired instructions (macOS `proc_pid_rusage`, RUSAGE_INFO_V4 `ri_instructions`; elsewhere the calling thread's
//! counters from `bough_mem::pmu`, user mode: round 7, bench lane) and time.
#![allow(dead_code)]

use std::time::Instant;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut core::ffi::c_void) -> i32;
}

#[cfg(target_os = "macos")]
pub fn instructions() -> u64 {
    // SAFETY: plain syscall wrapper writing a rusage_info_v4 (RUSAGE_INFO_V4 = 4).
    unsafe {
        let mut info: libc::rusage_info_v4 = core::mem::zeroed();
        let r = proc_pid_rusage(libc::getpid(), 4, (&mut info as *mut libc::rusage_info_v4).cast());
        assert_eq!(r, 0, "proc_pid_rusage failed");
        info.ri_instructions
    }
}
#[cfg(not(target_os = "macos"))]
pub fn instructions() -> u64 {
    bough_mem::pmu::instructions_cycles().0
}

pub fn overhead() -> u64 {
    let mut best = u64::MAX;
    for _ in 0..50 {
        let a = instructions();
        let b = instructions();
        best = best.min(b - a);
    }
    best
}

#[derive(Clone, Copy, Debug)]
pub struct Count {
    pub instr: f64,
    pub ns: f64,
}

impl Count {
    pub fn worst() -> Self {
        Count { instr: f64::MAX, ns: f64::MAX }
    }
    pub fn min(self, o: Count) -> Count {
        Count { instr: self.instr.min(o.instr), ns: self.ns.min(o.ns) }
    }
}

/// Run `f` `reps` times; best instructions (overhead subtracted) and best ns, per call of `f`.
pub fn count(reps: usize, mut f: impl FnMut()) -> Count {
    let ov = overhead();
    let mut best = Count::worst();
    for _ in 0..reps.max(1) {
        let a = instructions();
        f();
        let b = instructions();
        let i = (b - a).saturating_sub(ov) as f64;
        let t = Instant::now();
        f();
        let ns = t.elapsed().as_nanos() as f64;
        best = best.min(Count { instr: i, ns });
    }
    best
}

/// Like `count` but `f` is stateful: `setup` restores the state before every measured call
/// (setup is not counted). Time is taken on the same call.
pub fn count_stateful<S>(reps: usize, mut setup: impl FnMut() -> S, mut f: impl FnMut(&mut S)) -> Count {
    let ov = overhead();
    let mut best = Count::worst();
    for _ in 0..reps.max(1) {
        let mut s = setup();
        let t = Instant::now();
        let a = instructions();
        f(&mut s);
        let b = instructions();
        let ns = t.elapsed().as_nanos() as f64;
        best = best.min(Count { instr: (b - a).saturating_sub(ov) as f64, ns });
    }
    best
}
