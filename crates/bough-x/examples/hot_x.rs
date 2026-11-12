//! The hot paths of the new core as cells, in the form `bench/r8/perf/hot.sh` drives (threads lane, round 8).
//!
//! The convention, for any binary measured by that script:
//!
//!   OP=<cell>      run that one cell (default: every cell, one table row each)
//!   REPS=<n>       repetitions of the cell after set-up; two values give a slope that subtracts set-up
//!   EVICT_KB=<k>   between repetitions, walk a buffer of `k` KiB, so the cell starts with every cache level
//!                  smaller than that cold (0: warm). The walk is outside the counted bracket.
//!   ROWS=<n>       scene size
//!   BOUGH_PMU=..   up to three more events for the in-process counters (Linux)
//!   CELLS=1        print the cell names and exit
//!
//! Output: `scene  cell  evict_kb  reps  <event>_p50 ...  <event>_max ...  tiles_p50  wall_ns_p50  wall_ns_max`, counted on the calling
//! thread with the bracket's own cost subtracted. Under `perf stat` or `perf record` the same loop runs and the
//! table is still printed; perf's counts then include set-up and the eviction walk, which the script removes
//! with the `floor` cell and a slope.
//!
//! The frame cells paint through `soft::Soft`, the expr lane's reference raster: an oracle, not a product.
//! They are here so the method covers "a full frame"; the raster lane's kernels are measured by the same
//! script from its own binary.
#[path = "../scenes/sheet.rs"]
mod sheet;
use bough_mem::pmu;
use bough_x::prelude::*;
use bough_x::soft::Soft;
use sheet::*;
use std::hint::black_box;

fn env(name: &str, d: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

/// What a cell runs on.
struct Bed {
    world: World<Soft>,
    sheet: Sheet,
    soft: Soft,
    rows: usize,
    /// An image of the sheet as first mounted, for the start-up cells.
    image: std::path::PathBuf,
}

type Cell = (&'static str, &'static str, fn(&mut Bed, usize));

/// Every cell: name, which hot path of ROUND8 section 8 it is, and one repetition. A repetition leaves the bed
/// in a state from which the next one does the same amount of work.
const CELLS: &[Cell] = &[
    ("floor", "nothing: the bracket and the loop", |_, _| {}),
    ("idle", "a turn in which nothing changed", |b, _| b.world.settle(&b.sheet)),
    ("hover", "state write to damaged tiles: pointer onto / off a header (hit, handler, settle)", |b, k| {
        let y = if k % 2 == 0 { TOOLBAR + HEADER / 2 } else { TOOLBAR / 2 };
        b.world.pointer(&mut b.sheet, GUTTER + 3 * CELL_W + CELL_W / 2, y);
        b.world.settle(&b.sheet);
    }),
    ("hover_word", "state write to damaged tiles: the hover word alone (no hit test)", |b, k| {
        b.sheet.u.set::<Hover>(if k % 2 == 0 { 3 } else { NO_HOVER });
        b.world.settle(&b.sheet);
    }),
    ("theme", "state write to damaged tiles: every exposed record changes", |b, _| {
        b.sheet.toggle_theme();
        b.world.settle(&b.sheet);
    }),
    ("edit_exposed", "relation edit: one exposed cell and its total", |b, k| {
        let top = (b.sheet.u.get::<Scroll>() / CELL_H) as usize;
        b.sheet.edit((top + (k * 7) % 30).min(b.rows - 1), (k * 3) % COLS, (k * 37 % 9973) as f32 / 100.0);
        b.world.settle(&b.sheet);
    }),
    ("edit_unexposed", "relation edit: a cell nobody sees and its total", |b, k| {
        b.sheet.edit(b.rows - 1 - (k % 8).min(b.rows - 1), (k * 3) % COLS, (k * 37 % 9973) as f32 / 100.0);
        b.world.settle(&b.sheet);
    }),
    ("insert_remove", "relation edit: a row inserted in the window, then removed", |b, k| {
        let top = (b.sheet.u.get::<Scroll>() / CELL_H) as usize;
        let at = (top + 5 + (k / 2) % 20).min(b.sheet.rows.len() - 1);
        if k % 2 == 0 { b.sheet.insert_row(at, [k as f32; COLS], false) } else { b.sheet.remove_row(at, false) }
        b.world.settle(&b.sheet);
    }),
    ("scroll_6px", "exposure: the space moves, no row enters", |b, k| {
        b.sheet.scroll_by(if (k / 4) % 2 == 0 { 6 } else { -6 });
        b.world.settle(&b.sheet);
    }),
    ("scroll_3_rows", "exposure: three rows enter", |b, k| {
        b.sheet.scroll_by(if (k / 4) % 2 == 0 { 3 * CELL_H } else { -3 * CELL_H });
        b.world.settle(&b.sheet);
    }),
    ("scroll_page", "exposure materialising a page: 34 rows enter", |b, k| {
        b.sheet.scroll_by(if (k / 4) % 2 == 0 { 1500 } else { -1500 });
        b.world.settle(&b.sheet);
    }),
    ("scroll_far", "exposure: a jump of 100 windows, every row is new", |b, k| {
        let max = (b.rows as i32 * CELL_H - (BODY.y1 - BODY.y0)).max(0);
        let y = ((k as i64 * 150_000) % (max as i64 + 1)) as i32;
        let d = y - b.sheet.u.get::<Scroll>();
        b.sheet.scroll_by(d);
        b.world.settle(&b.sheet);
    }),
    ("redescribe_idle", "re-describing mode: the exposed view described again and compared", |b, _| b.world.redescribe(&b.sheet)),
    ("world_new", "a world made and dropped: the arena's reservation and its release, nothing mounted", |_, _| {
        let w = World::<Soft>::new(W, H, NWORDS);
        black_box(&w);
    }),
    ("mount", "first mount into a fresh world (allocation included; the previous world is dropped)", |b, _| {
        let mut w = World::<Soft>::new(W, H, NWORDS);
        w.mount(&b.sheet);
        black_box(&w);
        b.world = w;
    }),
    ("image_open", "start-up: map the image of the mounted scene; it can be painted (the view has not run)", |b, _| {
        let w = World::<Soft>::open(&b.image).expect("the image this run saved");
        black_box(w.shown.w[0]);
        drop(w);
    }),
    ("image_adopt", "start-up: map the image, then describe the view against it (exact; the world can take turns)", |b, _| {
        let mut w = World::<Soft>::open(&b.image).expect("the image this run saved");
        w.adopt(&b.sheet, NWORDS).expect("its own view");
        black_box(&w);
        b.world = w;
    }),
    ("frame_full", "a full frame through the reference raster (oracle; 2560 x 1600)", |b, _| {
        b.soft.records = 0;
        b.world.paint(&mut b.soft);
        black_box(b.soft.px[0]);
    }),
];

/// Walk `buf` a line at a time, writing: afterwards the caches smaller than it hold it and not the scene.
#[inline(never)]
fn evict(buf: &mut [u8]) {
    let mut i = 0;
    while i < buf.len() {
        buf[i] = buf[i].wrapping_add(1);
        i += 64;
    }
    black_box(&mut *buf);
}

fn main() {
    if std::env::var("CELLS").is_ok() {
        for c in CELLS {
            println!("{}\t{}", c.0, c.1);
        }
        return;
    }
    let (rows, reps, evict_kb) = (env("ROWS", 10_000), env("REPS", 64), env("EVICT_KB", 0));
    let only = std::env::var("OP").ok();
    let evs = pmu::sample_events();
    let n = evs.len();
    // the bracket's own cost: median of 64 empty brackets
    let mut br: Vec<[u64; pmu::MAX]> = (0..64)
        .map(|_| {
            let a = pmu::sample();
            pmu::sample().since(&a).v
        })
        .collect();
    let mut bracket = [0u64; pmu::MAX];
    for (i, b) in bracket.iter_mut().enumerate().take(n) {
        br.sort_by_key(|s| s[i]);
        *b = br[br.len() / 2][i];
    }
    let names: Vec<&str> = evs.iter().map(|e| e.name()).collect();
    println!("# rows={rows} cells={} window={W}x{H} evict_kb={evict_kb} reps={reps}", rows * COLS);
    println!(
        "scene\tcell\tevict_kb\treps{}{}\ttiles_p50\twall_ns_p50\twall_ns_max",
        names.iter().map(|e| format!("\t{e}_p50")).collect::<String>(),
        names.iter().map(|e| format!("\t{e}_max")).collect::<String>()
    );
    let mut bed = {
        let sheet = Sheet::new(rows);
        let mut world = World::<Soft>::new(W, H, NWORDS);
        world.mount(&sheet);
        world.tiles.clear();
        let image = std::env::temp_dir().join(format!("hot_x-{}.bx", std::process::id()));
        world.save(&image).expect("writing the image");
        Bed { world, sheet, soft: Soft::new(W, H), rows, image }
    };
    let mut buf = vec![0u8; evict_kb * 1024];
    for &(name, _, f) in CELLS {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        // reach the cell's steady state: every state it alternates between has been visited
        for k in 0..16 {
            f(&mut bed, k);
            bed.world.tiles.clear();
        }
        let mut got: Vec<([u64; pmu::MAX], u32)> = Vec::with_capacity(reps);
        let mut wall: Vec<u64> = Vec::with_capacity(reps);
        for k in 0..reps {
            if evict_kb != 0 {
                evict(&mut buf);
            }
            let t0 = std::time::Instant::now();
            let a = pmu::sample();
            f(&mut bed, k);
            let d = pmu::sample().since(&a);
            // wall time includes the two counter reads (a system call each on Linux): a floor of its own
            wall.push(t0.elapsed().as_nanos() as u64);
            let tiles = bed.world.tiles.count();
            bed.world.tiles.clear();
            let mut v = d.v;
            for i in 0..n {
                v[i] = v[i].saturating_sub(bracket[i]);
            }
            got.push((v, tiles));
        }
        let mut line = format!("sheet\t{name}\t{evict_kb}\t{reps}");
        let col = |i: usize| {
            let mut x: Vec<u64> = got.iter().map(|g| g.0[i]).collect();
            x.sort();
            (x[x.len() / 2], x[x.len() - 1])
        };
        for i in 0..n {
            line += &format!("\t{}", col(i).0);
        }
        for i in 0..n {
            line += &format!("\t{}", col(i).1);
        }
        let mut t: Vec<u32> = got.iter().map(|g| g.1).collect();
        t.sort();
        wall.sort();
        println!("{line}\t{}\t{}\t{}", t[t.len() / 2], wall[wall.len() / 2], wall[wall.len() - 1]);
    }
    std::fs::remove_file(&bed.image).ok();
}
