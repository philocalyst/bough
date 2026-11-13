//! Text on the new core below pixels (text lane): the rows and search scenes' phases, counted as `sheet_x`
//! counts the sheet. Instructions and cycles of the calling thread per step, bracket cost subtracted, median of
//! `REPS` steps; a step is the handler and the settle, from the input to the damage bitmap.
//!
//!   ROWS=3000 REPS=64 cargo run --profile dist -p bough-x --example text_x
//!
//! `COLD=1` walks a 64 MiB buffer before every step, so the step starts with nothing of its own in any cache:
//! with `BOUGH_PMU=l1d-misses,cache-misses,branch-misses` (Linux) the L1 column is then close to the number of
//! distinct lines the step touches.
#[path = "../scenes/rows.rs"]
mod rows;
#[path = "../scenes/search.rs"]
mod search;
use bough_mem::pmu;
use bough_x::prelude::*;
use bough_x::soft::Soft;
use std::hint::black_box;

struct Meter {
    bracket: [u64; pmu::MAX],
    n: usize,
    cold: Option<Vec<u8>>,
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
        let cold = std::env::var("COLD").is_ok().then(|| vec![1u8; 64 << 20]);
        Meter { bracket, n, cold }
    }
    /// Run `step(k)` `reps` times; print the median of each counter and of the damage it left.
    fn phase<V>(&mut self, scene: &str, name: &str, reps: usize, world: &mut World<Soft>, v: &mut V, mut step: impl FnMut(&mut World<Soft>, &mut V, usize)) {
        let mut rows: Vec<([u64; pmu::MAX], u32)> = Vec::with_capacity(reps);
        for k in 0..reps {
            if let Some(c) = &mut self.cold {
                let mut s = 0u64;
                for b in c.iter_mut().step_by(64) {
                    *b = b.wrapping_add(1);
                    s += *b as u64;
                }
                black_box(s);
            }
            let a = pmu::sample();
            step(world, v, k);
            let d = pmu::sample().since(&a);
            let tiles = world.tiles.count();
            world.tiles.clear();
            let mut x = d.v;
            for i in 0..self.n {
                x[i] = x[i].saturating_sub(self.bracket[i]);
            }
            rows.push((x, black_box(tiles)));
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
        println!("{scene}\t{name}\t{reps}\t{i50}\t{i95}\t{c50}\t{t50}{extra}");
    }
}

fn env(name: &str, d: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn main() {
    let (n, reps) = (env("ROWS", 3_000), env("REPS", 64));
    let mut m = Meter::new();
    let evs: Vec<&str> = pmu::sample_events().iter().skip(2).map(|e| e.name()).collect();
    println!("# rows={n} reps={reps} cold={} window=1280x800 device px, DejaVu Sans 14 px from the prepared face", m.cold.is_some());
    println!("scene\tphase\tsteps\tinstr_p50\tinstr_p95\tcycles_p50\ttiles_p50{}", evs.iter().map(|e| format!("\t{e}")).collect::<String>());

    // ---- rows ----
    {
        use rows::*;
        // mount: a fresh world and a fresh view each time (the glyph memo starts empty: nothing is shaped yet)
        let mut scratch = World::<Soft>::new(W, H, NWORDS);
        let mut holder: Vec<(Rows, World<Soft>)> = Vec::new();
        let mut views: Vec<Rows> = (0..reps.min(16)).map(|_| Rows::new(n)).collect();
        let mut unit = ();
        m.phase("rows", "mount", reps.min(16), &mut scratch, &mut unit, |_, _, _| {
            let v = views.pop().unwrap();
            let mut w = World::<Soft>::new(W, H, NWORDS);
            w.mount(&v);
            holder.push((v, w));
        });
        let (mut v, mut world) = holder.pop().unwrap();
        println!("# rows mount: {} lines shaped, {} found again, {} bytes of glyph state", v.glyphs.borrow().shaped, v.glyphs.borrow().found, v.glyphs.borrow().bytes());
        world.tiles.clear();
        m.phase("rows", "idle", reps, &mut world, &mut v, |w, v, _| w.settle(v));
        m.phase("rows", "hover", reps, &mut world, &mut v, |w, v, k| {
            v.hover((k % 20) as u32);
            w.settle(v);
        });
        m.phase("rows", "tint (text colour alone, every line)", reps, &mut world, &mut v, |w, v, _| {
            v.toggle_tint();
            w.settle(v);
        });
        m.phase("rows", "scroll 3 rows", reps, &mut world, &mut v, |w, v, k| {
            v.scroll_by(if (k / 8) % 2 == 0 { 3 * ROW_H } else { -3 * ROW_H });
            w.settle(v);
        });
        m.phase("rows", "scroll a page (24 rows enter)", reps, &mut world, &mut v, |w, v, k| {
            v.scroll_by(if (k / 4) % 2 == 0 { 24 * ROW_H } else { -24 * ROW_H });
            w.settle(v);
        });
        v.scroll_by(-(1 << 30));
        world.settle(&v);
        world.tiles.clear();
        m.phase("rows", "label edit (toggle a suffix on an exposed row)", reps, &mut world, &mut v, |w, v, k| {
            v.toggle_suffix(k % 16);
            w.settle(v);
        });
        m.phase("rows", "value edit (an exposed row's number)", reps, &mut world, &mut v, |w, v, k| {
            v.set_value(k % 16, k as i32 * 31);
            w.settle(v);
        });
        m.phase("rows", "remove a row then insert one", reps, &mut world, &mut v, |w, v, k| {
            if k % 2 == 0 { v.remove(5) } else { v.insert(5, 900_000 + k as u32) }
            w.settle(v);
        });
        m.phase("rows", "collect the glyph memo (clear, re-describe what is exposed)", reps, &mut world, &mut v, |w, v, _| {
            v.glyphs.borrow_mut().clear();
            w.redescribe(v);
        });
        println!("# rows end: {} lines shaped, {} found again, {} bytes of glyph state", v.glyphs.borrow().shaped, v.glyphs.borrow().found, v.glyphs.borrow().bytes());
    }

    // ---- search ----
    {
        use search::*;
        for filter in [false, true] {
            let tag = if filter { "search" } else { "typing" };
            let mut v = Search::new(n);
            v.filter = filter;
            let mut world = World::<Soft>::new(W, H, NWORDS);
            world.mount(&v);
            world.click(&mut v, FIELD.x0 + 20, FIELD.y0 + 10);
            world.settle(&v);
            world.tiles.clear();
            // a line of 12 characters is kept in the field: type one, delete one
            for t in ["quick ", "river"] {
                world.text(&mut v, t);
                world.settle(&v);
            }
            world.tiles.clear();
            m.phase(tag, "type a character (event, handler, settle)", reps, &mut world, &mut v, |w, v, k| {
                if k % 2 == 0 {
                    w.text(v, ["s", "e", "a", "o"][(k / 2) % 4]);
                } else {
                    w.key_down(v, Key::Backspace, Mods::default());
                }
                w.settle(v);
            });
            m.phase(tag, "caret move", reps, &mut world, &mut v, |w, v, k| {
                w.key_down(v, if (k / 4) % 2 == 0 { Key::Left } else { Key::Right }, Mods::default());
                w.settle(v);
            });
            if filter {
                // empty the field, then narrow to nothing and widen again: the rows that come back were shaped
                for _ in 0..40 {
                    world.key_down(&mut v, Key::Backspace, Mods::default());
                }
                world.settle(&v);
                world.tiles.clear();
                let before = v.glyphs.borrow().shaped;
                m.phase(tag, "narrow by a key, widen by a backspace (24 rows re-enter)", reps, &mut world, &mut v, |w, v, k| {
                    if k % 2 == 0 {
                        w.text(v, "z");
                    } else {
                        w.key_down(v, Key::Backspace, Mods::default());
                    }
                    w.settle(v);
                });
                println!("# search re-entry: {} lines shaped during the phase", v.glyphs.borrow().shaped - before);
            }
            if !filter {
                // how often a budget is reached by typing alone: keys per 256 KiB of memo
                let (b0, k) = (v.glyphs.borrow().bytes(), 4_000);
                for i in 0..k {
                    if i % 40 < 20 {
                        world.text(&mut v, ["s", "e", "a", "o"][i % 4]);
                    } else {
                        world.key_down(&mut v, Key::Backspace, Mods::default());
                    }
                    world.settle(&v);
                }
                world.tiles.clear();
                let grown = v.glyphs.borrow().bytes() - b0;
                println!("# typing: {k} keys grew the memo by {grown} bytes: a 256 KiB budget is reached every {} keys", (256usize << 10) * k / grown.max(1));
            }
            println!("# {tag} end: {} lines shaped, {} found again, {} bytes of glyph state", v.glyphs.borrow().shaped, v.glyphs.borrow().found, v.glyphs.borrow().bytes());
        }
    }
}
