//! Per-thread CPU time via Mach (diagnostic: who pays for a frame, not just the main thread).
use std::collections::HashMap;

#[repr(C)]
struct ThreadExtendedInfo {
    pth_user_time: u64,
    pth_system_time: u64,
    pth_cpu_usage: i32,
    pth_policy: i32,
    pth_run_state: i32,
    pth_flags: i32,
    pth_sleep_time: i32,
    pth_curpri: i32,
    pth_priority: i32,
    pth_maxpriority: i32,
    pth_name: [u8; 64],
}

unsafe extern "C" {
    fn mach_task_self() -> u32;
    fn task_threads(task: u32, list: *mut *mut u32, count: *mut u32) -> i32;
    fn thread_info(thread: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
    fn mach_port_deallocate(task: u32, name: u32) -> i32;
    fn vm_deallocate(task: u32, addr: usize, size: usize) -> i32;
}

/// port -> (name, cpu ns)
pub fn snapshot() -> HashMap<u32, (String, u64)> {
    let mut out = HashMap::new();
    unsafe {
        let task = mach_task_self();
        let mut list: *mut u32 = std::ptr::null_mut();
        let mut n = 0u32;
        if task_threads(task, &mut list, &mut n) != 0 {
            return out;
        }
        for i in 0..n as usize {
            let t = *list.add(i);
            let mut info: ThreadExtendedInfo = std::mem::zeroed();
            let mut cnt = (std::mem::size_of::<ThreadExtendedInfo>() / 4) as u32;
            if thread_info(t, 5, &mut info as *mut _ as *mut i32, &mut cnt) == 0 {
                let name = String::from_utf8_lossy(&info.pth_name).trim_end_matches('\0').to_string();
                out.insert(t, (name, info.pth_user_time + info.pth_system_time));
            }
            mach_port_deallocate(task, t);
        }
        vm_deallocate(task, list as usize, n as usize * 4);
    }
    out
}

/// Top threads by CPU delta, formatted as "name: us/tick".
pub fn diff_top(a: &HashMap<u32, (String, u64)>, b: &HashMap<u32, (String, u64)>, ticks: u64, k: usize) -> String {
    let mut v: Vec<(String, f64)> = b
        .iter()
        .map(|(p, (n, t))| (format!("{}#{}", if n.is_empty() { "?" } else { n }, p), (*t - a.get(p).map(|x| x.1).unwrap_or(0)) as f64 / 1e3 / ticks.max(1) as f64))
        .filter(|x| x.1 > 0.05)
        .collect();
    v.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap());
    v.iter().take(k).map(|(n, us)| format!("{n} {us:.1}us")).collect::<Vec<_>>().join(", ")
}

/// Per-thread retired instructions of this process: `proc_pidinfo(PROC_PIDTHREADCOUNTS = 34)` (xnu's
/// `struct proc_threadcounts`: per performance level `{instructions, cycles, user, system, energy}`),
/// summed over levels, keyed by thread id, with the thread's name. Diagnostic: who pays for a frame.
pub fn instr_snapshot() -> HashMap<u64, (String, u64)> {
    unsafe extern "C" {
        fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buf: *mut u8, size: i32) -> i32;
    }
    let mut out = HashMap::new();
    let pid = std::process::id() as i32;
    let mut ids = vec![0u64; 512];
    // SAFETY: PROC_PIDLISTTHREADIDS (28) fills at most `size` bytes of u64 thread ids.
    let n = unsafe { proc_pidinfo(pid, 28, 0, ids.as_mut_ptr() as *mut u8, (ids.len() * 8) as i32) };
    if n <= 0 {
        return out;
    }
    for &tid in &ids[..n as usize / 8] {
        let mut info = [0u8; 112];
        // SAFETY: PROC_PIDTHREADID64INFO (15) fills a `struct proc_threadinfo` (name at byte 48, 64 bytes).
        let name = if unsafe { proc_pidinfo(pid, 15, tid, info.as_mut_ptr(), 112) } > 0 {
            String::from_utf8_lossy(&info[48..112]).trim_end_matches('\0').to_string()
        } else {
            String::new()
        };
        let mut buf = [0u64; 1 + 5 * 4];
        // SAFETY: PROC_PIDTHREADCOUNTS fills the 8-byte header and up to 4 levels of 5 u64s.
        let r = unsafe { proc_pidinfo(pid, 34, tid, buf.as_mut_ptr() as *mut u8, (buf.len() * 8) as i32) };
        if r <= 0 {
            continue;
        }
        let levels = (buf[0] & 0xFFFF) as usize;
        let instrs: u64 = (0..levels.min(4)).map(|l| buf[1 + 5 * l]).sum();
        out.insert(tid, (name, instrs));
    }
    out
}

/// Per-thread instruction deltas per `frames`, largest first: "name#tid instr" (threads under `min`
/// instructions per frame are summed into "rest").
pub fn instr_diff(a: &HashMap<u64, (String, u64)>, b: &HashMap<u64, (String, u64)>, frames: u64, min: f64) -> String {
    let mut v: Vec<(String, f64)> = b
        .iter()
        .map(|(t, (n, i))| (format!("{}#{}", if n.is_empty() { "?" } else { n }, t % 100_000), (*i - a.get(t).map(|x| x.1).unwrap_or(0)) as f64 / frames.max(1) as f64))
        .collect();
    v.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap());
    let rest: f64 = v.iter().filter(|x| x.1 < min).map(|x| x.1).sum();
    let mut s: Vec<String> = v.iter().filter(|x| x.1 >= min).map(|(n, i)| format!("{n} {:.1}K", i / 1e3)).collect();
    s.push(format!("rest {:.1}K", rest / 1e3));
    s.join(", ")
}
