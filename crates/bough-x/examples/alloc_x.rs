//! Allocations and page faults of the new core's hot paths: the same cells as `hot_x`, under a counting global
//! allocator (so this binary's instruction counts are not release numbers and none are printed).
//!
//!   ROWS=10000 cargo run --profile dist -p bough-x --example alloc_x
//!
//! First the start of a process, each step once and cold: building the model, making a world (the arena's
//! reservation), the first mount, the first publish, the first frame through the reference raster. Then every
//! cell: heap allocations, bytes allocated and page faults per repetition (median and max of 64), after 16
//! unmeasured repetitions. A turn is expected to allocate nothing and fault nothing.
#[path = "../scenes/cells.rs"]
mod cells;
use bough_mem::count;
use bough_x::prelude::*;
use bough_x::soft::Soft;
use cells::sheet::*;
use cells::{Bed, CELLS};

#[global_allocator]
static A: count::Counting = count::Counting;

/// Minor and major page faults of the process so far.
fn faults() -> (i64, i64) {
    unsafe extern "C" {
        fn getrusage(who: i32, usage: *mut [i64; 18]) -> i32;
    }
    let mut r = [0i64; 18];
    // SAFETY: `struct rusage` is two timevals (four longs) and fourteen longs on both hosts; the buffer is
    // that size and plain integers.
    unsafe { getrusage(0, &mut r) };
    (r[8], r[9])
}

struct Mark(u64, i64);
fn mark() -> Mark {
    Mark(count::allocs() as u64, faults().0)
}
impl Mark {
    fn since(&self) -> (u64, i64) {
        (count::allocs() as u64 - self.0, faults().0 - self.1)
    }
}

fn main() {
    let rows: usize = std::env::var("ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000);
    println!("# rows={rows} window={W}x{H}; page size {} bytes", page_size());
    println!("step\tallocations\tpage_faults");
    let m = mark();
    let sheet = Sheet::new(rows);
    let (a, f) = m.since();
    println!("model built\t{a}\t{f}");
    let m = mark();
    let mut world = World::<Soft>::new(W, H, NWORDS);
    let (a, f) = m.since();
    println!("world made (arena reserved)\t{a}\t{f}");
    let m = mark();
    world.mount(&sheet);
    let (a, f) = m.since();
    println!("first mount\t{a}\t{f}");
    let m = mark();
    world.publish();
    let (a, f) = m.since();
    println!("first publish\t{a}\t{f}");
    let m = mark();
    let mut soft = Soft::new(W, H);
    let (a, f) = m.since();
    println!("frame buffer made (16 MB, zeroed)\t{a}\t{f}");
    let m = mark();
    world.paint(&mut soft);
    let (a, f) = m.since();
    println!("first frame (reference raster)\t{a}\t{f}");
    let m = mark();
    world.paint(&mut soft);
    let (a, f) = m.since();
    println!("second frame\t{a}\t{f}");
    drop((world, sheet, soft));

    println!("cell\tallocs_p50\tallocs_max\tfaults_p50\tfaults_max");
    let mut bed = Bed::new(rows);
    for &(name, _, f) in CELLS {
        for k in 0..16 {
            f(&mut bed, k);
            bed.world.tiles.clear();
        }
        let (mut al, mut fl) = (Vec::new(), Vec::new());
        for k in 0..64 {
            let m = mark();
            f(&mut bed, k);
            let (a, p) = m.since();
            bed.world.tiles.clear();
            al.push(a);
            fl.push(p);
        }
        al.sort();
        fl.sort();
        println!("{name}\t{}\t{}\t{}\t{}", al[32], al[63], fl[32], fl[63]);
    }
    std::fs::remove_file(&bed.image).ok();
}

fn page_size() -> i64 {
    unsafe extern "C" {
        fn sysconf(name: i32) -> i64;
    }
    // SAFETY: a plain query. `_SC_PAGESIZE` is 29 on macOS and 30 on Linux.
    unsafe { sysconf(if cfg!(target_os = "macos") { 29 } else { 30 }) }
}
