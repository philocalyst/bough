//! The CPU-instruction counter of the calling thread.

#[cfg(target_os = "linux")]
mod imp {
    use std::cell::Cell;

    #[repr(C)]
    #[derive(Default)]
    struct Attr {
        type_: u32,
        size: u32,
        config: u64,
        sample_period: u64,
        sample_type: u64,
        read_format: u64,
        flags: u64,
        wakeup: u32,
        bp_type: u32,
        bp_addr: u64,
        bp_len: u64,
        branch_sample_type: u64,
        sample_regs_user: u64,
        sample_stack_user: u32,
        clockid: i32,
        sample_regs_intr: u64,
        aux_watermark: u32,
        sample_max_stack: u16,
        reserved: u16,
    }

    thread_local! { static FD: Cell<i32> = const { Cell::new(-2) }; }

    fn open() -> i32 {
        let mut a = Attr::default();
        a.type_ = 0; // PERF_TYPE_HARDWARE
        a.size = std::mem::size_of::<Attr>() as u32;
        a.config = 1; // PERF_COUNT_HW_INSTRUCTIONS
        a.flags = (1 << 5) | (1 << 6); // exclude_kernel | exclude_hv; enabled at open (disabled bit 0 clear)
        // SAFETY: plain syscall with a valid attr; pid 0 = calling thread, cpu -1 = any.
        let fd = unsafe { libc::syscall(libc::SYS_perf_event_open, &a as *const Attr, 0, -1, -1, 0u64) } as i32;
        if fd < 0 {
            panic!("perf_event_open failed: {} (perf_event_paranoid? PMU exposed?)", std::io::Error::last_os_error());
        }
        fd
    }

    pub fn instr() -> u64 {
        let fd = FD.with(|f| {
            if f.get() == -2 {
                f.set(open());
            }
            f.get()
        });
        let mut v = 0u64;
        // SAFETY: reading 8 bytes into a u64 from a perf fd.
        let r = unsafe { libc::read(fd, &mut v as *mut u64 as *mut libc::c_void, 8) };
        assert_eq!(r, 8);
        v
    }

    pub fn rss_bytes() -> u64 {
        let s = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
        s.split_whitespace().nth(1).and_then(|p| p.parse::<u64>().ok()).unwrap_or(0) * 4096
    }
}

#[cfg(target_os = "macos")]
mod imp {
    unsafe extern "C" {
        fn thread_selfcounts(kind: libc::c_int, buf: *mut u64, nbytes: usize) -> libc::c_int;
    }
    pub fn instr() -> u64 {
        let mut b = [0u64; 2];
        // SAFETY: fills 16 bytes (instructions, cycles).
        unsafe { thread_selfcounts(1, b.as_mut_ptr(), 16) };
        b[0]
    }
    pub fn rss_bytes() -> u64 {
        let mut ru: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
        // SAFETY: proc_pid_rusage fills one rusage_info_v4.
        unsafe { libc::proc_pid_rusage(libc::getpid(), 4, &mut ru as *mut _ as *mut _) };
        ru.ri_resident_size
    }
}

pub use imp::{instr, rss_bytes};

/// Median cost of one `instr()` call, in instructions (for reporting; the slope removes it).
pub fn instr_overhead() -> u64 {
    let _ = instr();
    let mut v: Vec<u64> = (0..200).map(|_| { let a = instr(); let b = instr(); b - a }).collect();
    v.sort();
    v[v.len() / 2]
}
