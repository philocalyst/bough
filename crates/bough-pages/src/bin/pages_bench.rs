//! Cells of the page-pipeline model (see the crate docs and `docs/lanes/core/JOURNAL.md`).
//!
//!     cargo run --release --bin pages_bench -- [cell ...]      (no argument: every cell)
//!
//! Each line: `cell,params,per,instructions,cycles,cache_misses,branch_misses,ns`, all per unit of `per`,
//! the minimum of five samples. Counters are the calling process's own, user space only: on Linux
//! `perf_event_open` (inherited by worker threads; cache misses are the PMU's generic last-level event), on
//! macOS `thread_selfcounts` (instructions and cycles of the calling thread only: cells that use workers
//! report wall time and leave the counters to Linux). Zero means "not available on this host".
use bough_pages::*;
use std::hint::black_box;
use std::time::Instant;

#[derive(Clone, Copy, Default)]
struct C {
    instr: u64,
    cycles: u64,
    cmiss: u64,
    bmiss: u64,
    ns: u64,
}

#[cfg(target_os = "macos")]
mod ctr {
    unsafe extern "C" {
        fn thread_selfcounts(kind: i32, buf: *mut u64, n: usize) -> i32;
    }
    pub struct Ctr;
    impl Ctr {
        pub fn new() -> Ctr {
            Ctr
        }
        pub fn read(&self) -> [u64; 4] {
            let mut b = [0u64; 2];
            // SAFETY: the kernel writes two u64 (instructions, cycles) into a 16-byte buffer.
            unsafe { thread_selfcounts(1, b.as_mut_ptr(), 16) };
            [b[0], b[1], 0, 0]
        }
    }
}
#[cfg(target_os = "linux")]
mod ctr {
    unsafe extern "C" {
        fn syscall(num: i64, ...) -> i64;
        fn read(fd: i32, buf: *mut u8, n: usize) -> isize;
    }
    pub struct Ctr([i32; 4]);
    impl Ctr {
        pub fn new() -> Ctr {
            // PERF_TYPE_HARDWARE: cycles 0, instructions 1, cache misses 3, branch misses 5
            let mut fds = [-1i32; 4];
            for (k, cfg) in [1u64, 0, 3, 5].into_iter().enumerate() {
                let mut attr = [0u64; 16];
                attr[0] = 112u64 << 32; // type 0 (hardware), size 112 (PERF_ATTR_SIZE_VER5)
                attr[1] = cfg;
                // inherit (bit 1), exclude_kernel (bit 5), exclude_hv (bit 6)
                attr[5] = (1 << 1) | (1 << 5) | (1 << 6);
                // SAFETY: perf_event_open(attr, pid 0 = self, cpu -1, group -1, flags 0); attr is 128 bytes.
                #[cfg(target_arch = "x86_64")]
                let nr = 298i64;
                #[cfg(target_arch = "aarch64")]
                let nr = 241i64;
                fds[k] = unsafe { syscall(nr, attr.as_ptr(), 0i32, -1i32, -1i32, 0u64) } as i32;
            }
            Ctr(fds)
        }
        pub fn read(&self) -> [u64; 4] {
            let mut o = [0u64; 4];
            for k in 0..4 {
                if self.0[k] >= 0 {
                    let mut v = 0u64;
                    // SAFETY: an open perf counter reads as one u64.
                    unsafe { read(self.0[k], &mut v as *mut u64 as *mut u8, 8) };
                    o[k] = v;
                }
            }
            o
        }
    }
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod ctr {
    pub struct Ctr;
    impl Ctr {
        pub fn new() -> Ctr {
            Ctr
        }
        pub fn read(&self) -> [u64; 4] {
            [0; 4]
        }
    }
}

struct Bench {
    ctr: ctr::Ctr,
    only: Vec<String>,
}
impl Bench {
    fn want(&self, cell: &str) -> bool {
        self.only.is_empty() || self.only.iter().any(|o| cell.starts_with(o.as_str()))
    }
    /// Run `f` `reps` times per sample, five samples; print the minimum per `units` units of work per run.
    fn cell(&self, cell: &str, params: &str, per: &str, units: u64, reps: u32, mut f: impl FnMut()) {
        f();
        let mut best = C { instr: u64::MAX, cycles: u64::MAX, cmiss: u64::MAX, bmiss: u64::MAX, ns: u64::MAX };
        for _ in 0..5 {
            let (a, t) = (self.ctr.read(), Instant::now());
            for _ in 0..reps {
                f();
            }
            let (ns, b) = (t.elapsed().as_nanos() as u64, self.ctr.read());
            best.instr = best.instr.min(b[0] - a[0]);
            best.cycles = best.cycles.min(b[1] - a[1]);
            best.cmiss = best.cmiss.min(b[2] - a[2]);
            best.bmiss = best.bmiss.min(b[3] - a[3]);
            best.ns = best.ns.min(ns);
        }
        let d = (units * reps as u64) as f64;
        println!("{cell},{params},{per},{:.2},{:.2},{:.4},{:.4},{:.2}", best.instr as f64 / d, best.cycles as f64 / d, best.cmiss as f64 / d, best.bmiss as f64 / d, best.ns as f64 / d);
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize
    }
}

fn main() {
    let b = Bench { ctr: ctr::Ctr::new(), only: std::env::args().skip(1).collect() };
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!("cell,params,per,instructions,cycles,cache_misses,branch_misses,ns");

    // ---------------------------------------------------------------- the write
    if b.want("write") {
        for n in [64usize, 256, 4096, 100_000, 1_000_000] {
            let mut s = Sheet::grid(n, 1, 960, 32, 1280, 800);
            let mut r = Rng(7);
            let idx: Vec<usize> = (0..1024).map(|_| r.next() % n).collect();
            // the planes saturate after a few passes: the cell then measures the write to an already dirty
            // page (no summary update), which is the common case inside a turn; the first-touch case is the
            // same four instructions plus the summary's three
            b.cell("write", &format!("n={n}"), "write", 1024, 64, || {
                for &i in &idx {
                    s.mark(black_box(i));
                }
            });
        }
    }

    // ---------------------------------------------------------------- stage 1
    if b.want("classify") {
        for n in [64usize, 256, 4096, 100_000, 1_000_000] {
            let mut s = Sheet::grid(n, 1, 960, 32, 1280, 800);
            let mut work = Vec::with_capacity(s.pages());
            for dirty in [0usize, 1] {
                if dirty == 1 {
                    s.mark(n / 2);
                }
                b.cell("classify-scan", &format!("n={n} dirty={dirty}"), "frame", 1, 200, || {
                    work.clear();
                    s.classify_scan(0..s.pages(), &mut work);
                    black_box(&work);
                });
                b.cell("classify-sum", &format!("n={n} dirty={dirty}"), "frame", 1, 200, || {
                    work.clear();
                    s.classify_sum(&mut work);
                    black_box(&work);
                });
            }
        }
    }

    // ---------------------------------------------------------------- stage 2
    if b.want("project") {
        // a list of rows in a 1280 x 800 window: 1, 8 and every visible row dirty
        for n in [64usize, 256, 100_000] {
            for k in [1usize, 8, usize::MAX] {
                let mut s = Sheet::grid(n, 1, 960, 32, 1280, 800);
                let vis = n.min(25);
                let k = k.min(vis);
                for j in 0..k {
                    s.mark(j * vis / k);
                }
                let mut work = Vec::new();
                s.classify_sum(&mut work);
                let mut t = Tiles::new(1280, 800);
                for dense_at in [0u32, 65] {
                    b.cell("project-list", &format!("n={n} dirty={k} dense_at={dense_at}"), "record", k as u64, 2000, || {
                        s.project(black_box(&work), &mut t, dense_at);
                    });
                }
            }
        }
        // a table of 10 columns: one column of 10,000 cells highlighted (hover over its header), and a theme
        // switch (every cell); the window shows all of it (5120 x 2880, rows of 2 px: a model of density, not
        // of a real table)
        for (name, cols, rows) in [("column", 10usize, 10_000usize), ("theme", 10, 10_000), ("theme-1e6", 1000, 1000)] {
            let n = cols * rows;
            let row_h = (2880 / rows as i32).max(1);
            let mut s = Sheet::grid(n, cols, 5120, row_h, 5120, 2880);
            for p in 0..s.pages() {
                s.vis[p] = s.live[p];
            }
            let mut dirty = 0u64;
            for i in 0..n {
                if name != "column" || i % cols == 3 {
                    s.mark(i);
                    dirty += 1;
                }
            }
            let mut work = Vec::new();
            s.classify_sum(&mut work);
            let mut t = Tiles::new(5120, 2880);
            for dense_at in [0u32, 4, 16, 65] {
                b.cell(&format!("project-{name}"), &format!("n={n} dirty={dirty} dense_at={dense_at}"), "record", dirty, 20, || {
                    s.project(black_box(&work), &mut t, dense_at);
                });
            }
            if b.want("project-par") || b.only.is_empty() {
                let mut th = 1;
                while th <= cores {
                    b.cell(&format!("project-par-{name}"), &format!("n={n} dirty={dirty} threads={th}"), "record", dirty, 20, || {
                        s.project_par(black_box(&work), &mut t, 4, th);
                    });
                    th *= 2;
                }
            }
        }
        // the cost of starting scoped workers that do nothing (what a pool would save)
        let s = Sheet::grid(64 * 64, 1, 960, 32, 1280, 800);
        let work: Vec<u32> = (0..64).collect();
        let mut t = Tiles::new(1280, 800);
        let mut th = 2;
        while th <= cores {
            b.cell("par-overhead", &format!("threads={th} (64 pages, nothing dirty)"), "call", 1, 200, || {
                s.project_par(black_box(&work), &mut t, 4, th);
            });
            th *= 2;
        }
    }

    // ---------------------------------------------------------------- scroll: translate and cull as lanes
    if b.want("scroll") {
        for n in [256usize, 4096, 100_000, 1_000_000] {
            let mut s = Sheet::grid(n, 1, 960, 32, 1280, 800);
            let np = s.pages();
            let mut dy = 3i16;
            b.cell("scroll-translate", &format!("n={n}"), "page", np as u64, 50, || {
                s.translate(0..np, dy);
                dy = -dy;
            });
            b.cell("scroll-cull", &format!("n={n}"), "page", np as u64, 50, || {
                s.cull(0..np);
                black_box(&s.vis);
            });
        }
    }

    // ---------------------------------------------------------------- pages under edits
    if b.want("insert") {
        for fill in [16usize, 32, 56] {
            let mut r = Rng(3);
            let ops: Vec<usize> = (0..512).map(|_| r.next()).collect();
            let rec = |i: usize| Rec { payload: i as u32, handle: (i % 4096) as u32, rect: [0, 0, 10, 10], depth: 1, kind: 1, clip: 0 };
            let mut a = ShiftPage::new();
            let mut htab = vec![0u32; 4096];
            for i in 0..fill {
                a.insert(i, rec(i), &mut htab);
            }
            b.cell("insert-remove-shift", &format!("fill={fill}"), "pair", 512, 200, || {
                for &o in &ops {
                    let k = o % (a.len + 1);
                    a.insert(k, rec(o), &mut htab);
                    a.remove((o >> 8) % a.len, &mut htab);
                }
            });
            b.cell("walk-shift", &format!("fill={fill}"), "record", fill as u64, 2000, || {
                black_box(a.walk());
            });
            let mut s = SelPage::new();
            for i in 0..fill {
                s.insert(i, rec(i));
            }
            b.cell("walk-sel-identity", &format!("fill={fill}"), "record", fill as u64, 2000, || {
                black_box(s.walk());
            });
            b.cell("insert-remove-sel", &format!("fill={fill}"), "pair", 512, 200, || {
                for &o in &ops {
                    let k = o % (s.len + 1);
                    s.insert(k, rec(o));
                    s.remove((o >> 8) % s.len);
                }
            });
            b.cell("walk-sel-permuted", &format!("fill={fill}"), "record", fill as u64, 2000, || {
                black_box(s.walk());
            });
        }
    }

    // ---------------------------------------------------------------- mount: a run of identical-shape rows
    if b.want("stamp") {
        let rects = [[0i16, 0, 960, 32], [8, 8, 72, 24], [80, 8, 300, 24]];
        for rows in [1usize, 64, 256, 100_000] {
            let reps = if rows >= 100_000 { 5 } else { 2000 };
            b.cell("mount-per-record", &format!("rows={rows} k=3"), "row", rows as u64, reps, || {
                let (mut pages, mut htab) = (Vec::new(), Vec::new());
                mount_per_record(&mut pages, &mut htab, black_box(&rects), rows, 32);
                black_box((&pages, &htab));
            });
            let (images, _) = tile_template(&rects, 32);
            let n_img = images.len();
            b.cell("mount-stamp-copy", &format!("rows={rows} k=3"), "row", rows as u64, reps, || {
                let mut d = Dir { images: images.clone(), ..Dir::default() };
                d.stamp_copy(0, n_img, 3, black_box(rows), 32, 0);
                black_box(&d);
            });
            b.cell("mount-stamp-shared", &format!("rows={rows} k=3"), "row", rows as u64, reps, || {
                let mut d = Dir { images: images.clone(), ..Dir::default() };
                d.stamp_shared(0, n_img, 3, black_box(rows), 32, 0);
                black_box(&d);
            });
        }
    }

    // ---------------------------------------------------------------- the directory: piece table against shifting
    if b.want("dir") {
        use bough_pages::pieces::{Pieces, Shift};
        let rec = |i: usize| Rec { payload: i as u32, handle: 0, rect: [0, 0, 10, 10], depth: 1, kind: 1, clip: 0 };
        macro_rules! dir_cells {
            ($name:literal, $T:ty) => {
                for n in [1_000usize, 10_000, 100_000, 1_000_000] {
                    let eight: Vec<Rec> = (0..8).map(rec).collect();
                    let mut ids = Vec::with_capacity(1 << 16);
                    // build by appends of 8
                    let reps = if n >= 100_000 { 1 } else { 20 };
                    b.cell(concat!("dir-build-append8-", $name), &format!("n={n}"), "record", n as u64, reps, || {
                        let mut d = <$T>::new();
                        for _ in 0..n / 8 {
                            ids.clear();
                            d.insert(d.len, &eight, &mut ids);
                        }
                        black_box(&d);
                    });
                    let mut d = <$T>::new();
                    for _ in 0..n / 8 {
                        ids.clear();
                        d.insert(d.len, &eight, &mut ids);
                    }
                    let mut r = Rng(11);
                    let at: Vec<usize> = (0..1024).map(|_| r.next() % (n - 16)).collect();
                    b.cell(concat!("dir-pair8-front-", $name), &format!("n={n}"), "pair", 256, 8, || {
                        for _ in 0..256 {
                            ids.clear();
                            d.insert(0, &eight, &mut ids);
                            d.remove(0, 8);
                        }
                    });
                    b.cell(concat!("dir-pair8-random-", $name), &format!("n={n}"), "pair", 1024, 4, || {
                        for &p in &at {
                            ids.clear();
                            d.insert(p, &eight, &mut ids);
                            d.remove(at[(p * 7 + 3) % 1024], 8);
                        }
                    });
                    b.cell(concat!("dir-walk-fresh-", $name), &format!("n={n}"), "record", n as u64, if n >= 100_000 { 5 } else { 200 }, || {
                        black_box(d.walk());
                    });
                    // churn: every record replaced four times over, at random places
                    let mut d = <$T>::new();
                    for _ in 0..n / 8 {
                        ids.clear();
                        d.insert(d.len, &eight, &mut ids);
                    }
                    let fresh = d.shape();
                    let fresh_bytes = d.bytes();
                    let mut r = Rng(5);
                    for _ in 0..n / 2 {
                        let p = r.next() % (d.len - 8);
                        d.remove(p, 8);
                        ids.clear();
                        let q = r.next() % (d.len + 1);
                        d.insert(q, &eight, &mut ids);
                    }
                    let (pieces, pages, permuted, len) = d.shape();
                    println!(
                        "# shape,{},n={n},fresh entries {} pages {} bytes/record {:.1}; churned entries {pieces} pages {pages} permuted {permuted} records/entry {:.1} fill {:.2} bytes/record {:.1}",
                        $name,
                        fresh.0,
                        fresh.1,
                        fresh_bytes as f64 / fresh.3 as f64,
                        len as f64 / pieces as f64,
                        len as f64 / (pages * 64) as f64,
                        d.bytes() as f64 / len as f64
                    );
                    b.cell(concat!("dir-walk-churned-", $name), &format!("n={n}"), "record", n as u64, if n >= 100_000 { 5 } else { 200 }, || {
                        black_box(d.walk());
                    });
                    let at: Vec<usize> = (0..1024).map(|_| r.next() % (n - 16)).collect();
                    b.cell(concat!("dir-pair8-random-churned-", $name), &format!("n={n}"), "pair", 1024, 4, || {
                        for &p in &at {
                            ids.clear();
                            d.insert(p, &eight, &mut ids);
                            d.remove(at[(p * 7 + 3) % 1024], 8);
                        }
                    });
                }
            };
        }
        dir_cells!("pieces", Pieces);
        dir_cells!("shift", Shift);
        // the workloads a view produces: rows of 8 records, pages built with slack (48 of 64), a row replaced
        // in place, a list scrolled (rows leave at one end and arrive at the other), a row inserted between
        // two rows and a row removed elsewhere
        macro_rules! row_cells {
            ($name:literal, $T:ty) => {
                for n in [1_000usize, 100_000, 1_000_000] {
                    let eight: Vec<Rec> = (0..8).map(rec).collect();
                    let mut ids = Vec::with_capacity(1 << 16);
                    let mut d = <$T>::new();
                    for r in 0..n / 8 {
                        ids.clear();
                        d.insert(d.len, &eight, &mut ids);
                        if r % 6 == 5 {
                            d.break_page();
                        }
                    }
                    let rows = n / 8;
                    let mut r = Rng(21);
                    let at: Vec<usize> = (0..1024).map(|_| r.next() % (rows - 2)).collect();
                    b.cell(concat!("rows-replace-", $name), &format!("n={n}"), "row", 1024, 4, || {
                        for &p in &at {
                            d.remove(p * 8, 8);
                            ids.clear();
                            d.insert(p * 8, &eight, &mut ids);
                        }
                    });
                    b.cell(concat!("rows-scroll-", $name), &format!("n={n}"), "row", 1024, 4, || {
                        for _ in 0..1024 {
                            d.remove(0, 8);
                            ids.clear();
                            d.insert(d.len, &eight, &mut ids);
                        }
                    });
                    b.cell(concat!("rows-insert-remove-", $name), &format!("n={n}"), "pair", 1024, 4, || {
                        for &p in &at {
                            ids.clear();
                            d.insert(p * 8, &eight, &mut ids);
                            d.remove(at[(p * 7 + 3) % 1024] * 8, 8);
                        }
                    });
                    // long run of the last workload, then the shape and an ordered walk
                    for _ in 0..n / 2 {
                        let p = (r.next() % (d.len / 8 - 1)) * 8;
                        ids.clear();
                        d.insert(p, &eight, &mut ids);
                        let q = (r.next() % (d.len / 8 - 1)) * 8;
                        d.remove(q, 8);
                    }
                    let (pieces, pages, permuted, len) = d.shape();
                    println!("# rows-shape,{},n={n},entries {pieces} pages {pages} permuted {permuted} records/entry {:.1} fill {:.2} bytes/record {:.1}", $name, len as f64 / pieces as f64, len as f64 / (pages * 64) as f64, d.bytes() as f64 / len as f64);
                    b.cell(concat!("rows-walk-after-", $name), &format!("n={n}"), "record", n as u64, if n >= 100_000 { 5 } else { 200 }, || {
                        black_box(d.walk());
                    });
                    b.cell(concat!("rows-insert-remove-after-", $name), &format!("n={n}"), "pair", 1024, 4, || {
                        for &p in &at {
                            ids.clear();
                            d.insert(p * 8, &eight, &mut ids);
                            d.remove(at[(p * 7 + 3) % 1024] * 8, 8);
                        }
                    });
                }
            };
        }
        row_cells!("pieces", Pieces);
        row_cells!("shift", Shift);
    }

    // ---------------------------------------------------------------- idle re-description over operand lanes
    if b.want("redescribe") {
        for rows in [1usize, 64, 256, 1000, 100_000] {
            let mut lane = vec![[0u32; P]; rows.div_ceil(P)];
            let ids: Vec<u32> = (0..rows as u32).collect();
            let selected = black_box(u32::MAX);
            let tint = |i: usize| if ids[i] == selected { 0x2f6bffu32 } else if i % 2 == 0 { 0xf5f5f5 } else { 0xe6e6e6 };
            redescribe_ops(&mut lane, rows, tint);
            b.cell("redescribe-operands-idle", &format!("rows={rows}"), "row", rows as u64, if rows >= 100_000 { 20 } else { 2000 }, || {
                black_box(redescribe_ops(&mut lane, black_box(rows), tint));
            });
        }
    }
}
