//! What the first buffer costs, and what a pixel section mapped from a file would cost instead (threads lane,
//! for the e2e lane's question). One 2560 x 1600 XRGB frame, 16,384,000 bytes, three ways:
//!
//!   anon    an anonymous shared mapping, every page written once (what a client's first wl_shm buffer is)
//!   file    a file whose pages are in the page cache, mapped shared and read-only, every page read once
//!           (what the compositor does to a pool made from an image's file; the client touches nothing)
//!   filepop the same with MAP_POPULATE (Linux): the mapping call maps every page, no fault afterwards
//!
//! Prints wall time and minor page faults for each. `FILE=<path>` chooses where the file is written (it must be
//! on a real filesystem to say anything about a real one); it is written, read back once, and removed.
use std::io::Write;
use std::os::fd::AsRawFd;
use std::time::Instant;

unsafe extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
    fn getrusage(who: i32, usage: *mut [i64; 18]) -> i32;
}
fn faults() -> i64 {
    let mut r = [0i64; 18];
    // SAFETY: `struct rusage` is eighteen longs on both hosts.
    unsafe { getrusage(0, &mut r) };
    r[8]
}
const LEN: usize = 2560 * 1600 * 4;
const SHARED: i32 = 1;
#[cfg(target_os = "macos")]
const ANON: i32 = 0x1000;
#[cfg(not(target_os = "macos"))]
const ANON: i32 = 0x20;
#[cfg(target_os = "linux")]
const POPULATE: i32 = 0x8000;

fn run(name: &str, f: impl FnOnce() -> (*mut u8, bool)) {
    let (f0, t) = (faults(), Instant::now());
    let (p, write) = f();
    assert!(p as isize != -1, "{name}: mmap failed");
    let mapped = t.elapsed();
    let mut sum = 0u64;
    let mut i = 0;
    while i < LEN {
        // SAFETY: inside the mapping.
        unsafe {
            if write {
                p.add(i).write_volatile(1);
            } else {
                sum += p.add(i).read_volatile() as u64;
            }
        }
        i += 4096;
    }
    let (all, f1) = (t.elapsed(), faults());
    std::hint::black_box(sum);
    // SAFETY: the mapping made above.
    unsafe { munmap(p, LEN) };
    println!("{name}\tmap_us\t{:.1}\ttouch_every_page_us\t{:.1}\tminor_faults\t{}", mapped.as_secs_f64() * 1e6, (all - mapped).as_secs_f64() * 1e6, f1 - f0);
}

fn main() {
    let path = std::env::var("FILE").unwrap_or_else(|_| std::env::temp_dir().join(format!("pixels_x-{}", std::process::id())).to_string_lossy().into_owned());
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&vec![0x5au8; LEN]).unwrap();
    }
    // in the page cache: just written, and read once more
    std::hint::black_box(std::fs::read(&path).unwrap().len());
    for round in 0..3 {
        println!("# round {round}");
        // SAFETY (all three): fresh mappings at addresses of the kernel's choosing; results checked in `run`.
        run("anon", || (unsafe { mmap(core::ptr::null_mut(), LEN, 3, SHARED | ANON, -1, 0) }, true));
        let f = std::fs::File::open(&path).unwrap();
        run("file", || (unsafe { mmap(core::ptr::null_mut(), LEN, 1, SHARED, f.as_raw_fd(), 0) }, false));
        #[cfg(target_os = "linux")]
        run("filepop", || (unsafe { mmap(core::ptr::null_mut(), LEN, 1, SHARED | POPULATE, f.as_raw_fd(), 0) }, false));
    }
    std::fs::remove_file(&path).ok();
}
