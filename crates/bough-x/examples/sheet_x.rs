//! The sheet's phases below pixels, counted like `comparisons/bough-workloads` counts the façade (`RENDER=0`):
//! instructions and cycles of the calling thread per step, bracket cost subtracted, median of `REPS` steps.
//! A step is the handler and the settle: from the input to the damage bitmap.
//!
//!   ROWS=10000 REPS=32 cargo run --profile dist -p bough-x --example sheet_x
#[path = "../scenes/sheet.rs"]
mod sheet;
use bough_mem::pmu;
use bough_x::prelude::*;
use bough_x::soft::Soft;
use sheet::*;
use std::hint::black_box;

struct Meter {
    bracket: [u64; pmu::MAX],
    n: usize,
}
impl Meter {
    fn new() -> Meter {
        let n = pmu::sample_events().len();
        let mut v: Vec<[u64; pmu::MAX]> = (0..64)
            .map(|_| {
                let a = pmu::sample();
                pmu::sample().since(&a).v
            })
            .collect();
        let mut bracket = [0u64; pmu::MAX];
        for (i, b) in bracket.iter_mut().enumerate().take(n) {
            v.sort_by_key(|s| s[i]);
            *b = v[v.len() / 2][i];
        }
        Meter { bracket, n }
    }
    /// Run `step(k)` `reps` times; print the median of each counter and of the damage it left.
    fn phase(&self, name: &str, reps: usize, world: &mut World<Soft>, sheet: &mut Sheet, mut step: impl FnMut(&mut World<Soft>, &mut Sheet, usize)) {
        let mut rows: Vec<([u64; pmu::MAX], u32)> = Vec::with_capacity(reps);
        for k in 0..reps {
            let a = pmu::sample();
            step(world, sheet, k);
            let d = pmu::sample().since(&a);
            // the damage is the product; reading it out belongs to whoever repairs the tiles
            let tiles = world.tiles.count();
            world.tiles.clear();
            let mut v = d.v;
            for i in 0..self.n {
                v[i] = v[i].saturating_sub(self.bracket[i]);
            }
            rows.push((v, black_box(tiles)));
        }
        let med = |f: &dyn Fn(&([u64; pmu::MAX], u32)) -> u64| {
            let mut x: Vec<u64> = rows.iter().map(f).collect();
            x.sort();
            (x[x.len() / 2], x[(x.len() * 95 / 100).min(x.len() - 1)])
        };
        let (i50, i95) = med(&|r| r.0[0]);
        let (c50, _) = med(&|r| r.0[1]);
        let (t50, _) = med(&|r| r.1 as u64);
        let mut extra = String::new();
        for i in 2..self.n {
            extra += &format!("\t{}", med(&|r| r.0[i]).0);
        }
        println!("sheet_x\t{name}\t{reps}\t{i50}\t{i95}\t{c50}\t{t50}{extra}");
    }
}

fn env(name: &str, d: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn main() {
    let (rows, reps) = (env("ROWS", 10_000), env("REPS", 32));
    let m = Meter::new();
    let evs: Vec<&str> = pmu::sample_events().iter().skip(2).map(|e| e.name()).collect();
    println!("# rows={rows} cells={} reps={reps} window={W}x{H} device px", rows * COLS);
    println!("scene\tphase\tsteps\tinstr_p50\tinstr_p95\tcycles_p50\ttiles_p50{}", evs.iter().map(|e| format!("\t{e}")).collect::<String>());

    let t = std::time::Instant::now();
    let a = pmu::sample();
    let mut sheet = Sheet::new(rows);
    let build = pmu::sample().since(&a);
    let build_ms = t.elapsed().as_secs_f64() * 1e3;
    let mut world = World::<Soft>::new(W, H, NWORDS);
    let t = std::time::Instant::now();
    let a = pmu::sample();
    world.mount(&sheet);
    let d = pmu::sample().since(&a);
    let mount_us = t.elapsed().as_secs_f64() * 1e6;
    let (records, pages, bytes) = world.root().stats();
    println!("# model built in {build_ms:.2} ms ({} instructions): the data, not the UI", build.v[0]);
    println!("# mounted: {records} records in {pages} pages, {bytes} bytes of columns, {mount_us:.1} us wall");
    println!("sheet_x\tmount\t1\t{}\t{}\t{}\t{}", d.v[0] - m.bracket[0], d.v[0] - m.bracket[0], d.v[1].saturating_sub(m.bracket[1]), world.tiles.count());
    world.tiles.clear();

    m.phase("floor", reps, &mut world, &mut sheet, |_, _, _| {});
    m.phase("idle", reps, &mut world, &mut sheet, |w, s, _| w.settle(s));
    m.phase("hover", reps, &mut world, &mut sheet, |w, s, k| {
        // onto column D's header, then up into the toolbar
        if k % 2 == 0 {
            w.pointer(s, GUTTER + 3 * CELL_W + CELL_W / 2, TOOLBAR + HEADER / 2);
        } else {
            w.pointer(s, GUTTER + 3 * CELL_W + CELL_W / 2, TOOLBAR / 2);
        }
        w.settle(s);
    });
    m.phase("hover_move", reps, &mut world, &mut sheet, |w, s, k| {
        // from one header to the next: two columns change
        w.pointer(s, GUTTER + (k % COLS) as i32 * CELL_W + CELL_W / 2, TOOLBAR + HEADER / 2);
        w.settle(s);
    });
    m.phase("theme", reps, &mut world, &mut sheet, |w, s, _| {
        s.toggle_theme();
        w.settle(s);
    });
    m.phase("edit_exposed", reps, &mut world, &mut sheet, |w, s, k| {
        let top = (s.u.get::<Scroll>() / CELL_H) as usize;
        s.edit((top + (k * 7) % 30).min(rows - 1), (k * 3) % COLS, (k * 37 % 9973) as f32 / 100.0);
        w.settle(s);
    });
    m.phase("edit_unexposed", reps, &mut world, &mut sheet, |w, s, k| {
        s.edit(rows - 1 - (k % 8).min(rows - 1), (k * 3) % COLS, (k * 37 % 9973) as f32 / 100.0);
        w.settle(s);
    });
    // the parts of an edit, apart (attribution, not scenarios)
    m.phase("part_model_edit", reps, &mut world, &mut sheet, |_, s, k| {
        s.edit(rows - 1 - (k % 8).min(rows - 1), (k * 3) % COLS, (k * 37 % 9973) as f32 / 100.0);
    });
    world.settle(&sheet);
    world.tiles.clear();
    m.phase("part_total_word", reps, &mut world, &mut sheet, |w, s, k| {
        s.u.set_at::<Sum>(k % COLS, k as i32);
        w.settle(s);
    });
    m.phase("part_delta_total", reps, &mut world, &mut sheet, |w, s, k| {
        let mut u2 = s.u;
        u2.set_at::<Sum>(k % COLS, k as i32 + 77);
        w.tree_delta(&s.u, &u2, 1 << (4 + k % COLS), None);
    });
    m.phase("part_diff_only", reps, &mut world, &mut sheet, |w, s, k| {
        let mut u2 = s.u;
        u2.set_at::<Sum>(k % COLS, k as i32 + 77);
        std::hint::black_box(w.shown.diff(&u2, NWORDS));
    });
    m.phase("part_journal_only", reps, &mut world, &mut sheet, |w, s, k| {
        let r = rows - 1 - (k % 8).min(rows - 1);
        let v = s.rows.get(r)[0];
        s.rows.edit(r)[0] = v;
        w.settle(s);
    });
    m.phase("part_mark_cell", reps, &mut world, &mut sheet, |w, _, k| {
        w.tiles.mark(Rect::new(GUTTER + (k % COLS) as i32 * CELL_W, 400, CELL_W, CELL_H));
    });
    m.phase("part_pointer_same", reps, &mut world, &mut sheet, |w, s, _| {
        w.pointer(s, GUTTER + 300, 20);
    });
    m.phase("select", reps, &mut world, &mut sheet, |w, s, k| {
        let top = (s.u.get::<Scroll>() / CELL_H) as u32;
        s.select((top + (k as u32 * 5) % 30) * COLS as u32 + (k as u32 * 3) % COLS as u32, w.now());
        w.settle(s);
    });
    // the outline is now gliding: frames with no turn
    let mut t = world.now();
    m.phase("frame_in_motion", 8, &mut world, &mut sheet, |w, _, _| {
        t += 16;
        w.tick(t);
    });
    world.tick(t + 1000);
    world.tiles.clear();
    m.phase("frame_at_rest", reps, &mut world, &mut sheet, |w, _, _| {
        t += 16;
        w.tick(t);
    });
    m.phase("scroll_3_rows", reps, &mut world, &mut sheet, |w, s, k| {
        s.scroll_by(if (k / 4) % 2 == 0 { 3 * CELL_H } else { -3 * CELL_H });
        w.settle(s);
    });
    m.phase("scroll_6px", reps, &mut world, &mut sheet, |w, s, k| {
        s.scroll_by(if (k / 4) % 2 == 0 { 6 } else { -6 });
        w.settle(s);
    });
    m.phase("scroll_page", reps, &mut world, &mut sheet, |w, s, k| {
        s.scroll_by(if (k / 4) % 2 == 0 { 1500 } else { -1500 });
        w.settle(s);
    });
    m.phase("insert_row", reps, &mut world, &mut sheet, |w, s, k| {
        let top = (s.u.get::<Scroll>() / CELL_H) as usize;
        s.insert_row((top + 5 + k % 20).min(s.rows.len()), [k as f32; COLS], false);
        w.settle(s);
    });
    m.phase("remove_row", reps, &mut world, &mut sheet, |w, s, k| {
        let top = (s.u.get::<Scroll>() / CELL_H) as usize;
        s.remove_row((top + 5 + k % 20).min(s.rows.len() - 1), false);
        w.settle(s);
    });
    m.phase("redescribe_idle", reps, &mut world, &mut sheet, |w, s, _| w.redescribe(s));
    m.phase("redescribe_edit", reps, &mut world, &mut sheet, |w, s, k| {
        let top = (s.u.get::<Scroll>() / CELL_H) as usize;
        s.edit_opaque((top + (k * 7) % 30).min(rows - 1), (k * 3) % COLS, (k * 37 % 9973) as f32 / 100.0);
        w.redescribe(s);
    });
    let (records, pages, bytes) = world.root().stats();
    println!("# at the end: {records} records in {pages} pages, {bytes} bytes of columns");
}
