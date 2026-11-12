//! The event path's cost, from the input to the damage: a pointer onto another row (hover leave and enter), a
//! pointer within the same row, a click on a row (its handler writes the selection word), a key with nothing
//! focused, text typed into a focused field (its handler edits a relation of one row). Each step is the entry
//! point and the settle after it; `dispatch` is the entry point alone. Warm: the median of `REPS` steps in a
//! row. Cold: the same step after sweeping 64 MiB through the cache (the cycles above warm are what the lines
//! it touches cost from memory). Rows: 100, 10,000, 1,000,000 (`ROWS` to pick one).
//!
//!   cargo run --profile dist -p bough-x --example events_x
use bough_mem::pmu;
use bough_x::event::{Interact, Key, Mods};
use bough_x::prelude::*;
use bough_x::soft::Soft;
use std::hint::black_box;

lens!(pub Sel: u32 = 0);
lens!(pub Hot: u32 = 1);
lens!(pub Keys: u32 = 2);
lens!(pub Scroll: i32 = 3);
const W: i32 = 2560;
const H: i32 = 1600;
const TOP: i32 = 160;
const ROW: i32 = 44;
const SHAPE: [Rect; 2] = [Rect::new(0, 0, W, ROW), Rect::new(20, 8, 400, 28)];
const FIELD: [Rect; 2] = [Rect::new(0, 0, 600, 60), Rect::new(0, 0, 600, 60)];

struct App {
    u: U,
    items: Rel<u32>,
    draft: Rel<Txt>,
}
fn fill(((sel, hot), r): ((u32, u32), u32)) -> Color {
    Color(if sel == r { 0xffdbe8ff } else if hot == r { 0xffeeeeee } else { 0xffffffff })
}
impl App {
    fn hover(&mut self, r: usize, on: bool) {
        if on {
            self.u.set::<Hot>(r as u32)
        } else if self.u.get::<Hot>() == r as u32 {
            self.u.set::<Hot>(u32::MAX)
        }
    }
}
impl View for App {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.row(|| ([Rect::new(0, 0, W, H)], Fill(Kc::<0xfff6f7f9>).on_key(|a: &mut App, _: &Ev| {
            let k = a.u.get::<Keys>() + 1;
            a.u.set::<Keys>(k)
        })));
        s.list(&List { n: 1, row_h: 60, x: 1200, y: 40, shape: &FIELD, edits: &self.draft.journal }, |_| {
            (Fill(Kc::<0xffffffff>).on_text(|a: &mut App, t: &str| {
                let (b, n) = a.draft.get(0).bytes();
                let mut s = String::from_utf8_lossy(&b[..n]).into_owned();
                if s.len() >= 12 {
                    s.clear();
                }
                s.push_str(t);
                *a.draft.edit(0) = Txt::new(&s);
            }), Label(*self.draft.get(0), Kc::<0xff000000>))
        });
        s.scope(Space { clip: Rect { x0: 0, y0: TOP, x1: W, y1: H }, base: (0, TOP), scroll: Scroll::SLOT as u8 }, |s| {
            s.list(&List { n: self.items.len() as u32, row_h: ROW, x: 0, y: 0, shape: &SHAPE, edits: &self.items.journal }, |r| {
                (
                    Fill(((un::<Sel>(), un::<Hot>()), rec(r as u32)).map(fill))
                        .on_click(move |a: &mut App| a.u.set::<Sel>(r as u32))
                        .on_hover(move |a: &mut App, e: &Ev| a.hover(r, e.on)),
                    Num::<0, _, _>(rec(*self.items.get(r) as i32), Kc::<0xff1d2129>),
                )
            });
        });
    }
}

struct Meter {
    bracket: [u64; 2],
    sweep: Vec<u64>,
}
impl Meter {
    fn new() -> Meter {
        let mut v: Vec<(u64, u64)> = (0..64)
            .map(|_| {
                let a = pmu::sample();
                let d = pmu::sample().since(&a);
                (d.v[0], d.v[1])
            })
            .collect();
        v.sort();
        let i = v[32].0;
        v.sort_by_key(|x| x.1);
        Meter { bracket: [i, v[32].1], sweep: vec![0; 8 << 20] }
    }
    fn evict(&mut self) {
        for (i, x) in self.sweep.iter_mut().enumerate() {
            *x = x.wrapping_add(i as u64);
        }
        black_box(&self.sweep);
    }
    /// (instructions, cycles) of `f`, median of `reps`, warm or after an eviction.
    fn run(&mut self, reps: usize, cold: bool, mut f: impl FnMut(usize)) -> (u64, u64) {
        let mut v = Vec::with_capacity(reps);
        for k in 0..reps {
            if cold {
                self.evict();
            }
            let a = pmu::sample();
            f(k);
            let d = pmu::sample().since(&a);
            v.push((d.v[0].saturating_sub(self.bracket[0]), d.v[1].saturating_sub(self.bracket[1])));
        }
        let mut i: Vec<u64> = v.iter().map(|x| x.0).collect();
        let mut c: Vec<u64> = v.iter().map(|x| x.1).collect();
        i.sort();
        c.sort();
        (i[reps / 2], c[reps / 2])
    }
}

fn main() {
    let reps: usize = std::env::var("REPS").ok().and_then(|s| s.parse().ok()).unwrap_or(31);
    let sizes: Vec<usize> = match std::env::var("ROWS").ok().and_then(|s| s.parse().ok()) {
        Some(n) => vec![n],
        None => vec![100, 10_000, 1_000_000],
    };
    let mut m = Meter::new();
    println!("rows\tstep\tdispatch_instr\tstep_instr\tstep_cycles_warm\tstep_cycles_cold\tcold_minus_warm");
    for n in sizes {
        let mut a = App { u: U::default(), items: Rel::new((0..n as u32).collect()), draft: Rel::new(vec![Txt::default()]) };
        a.u.set::<Sel>(u32::MAX);
        a.u.set::<Hot>(u32::MAX);
        let mut w = World::<Soft>::new(W, H, 4);
        w.mount(&a);
        w.tiles.clear();
        let row_y = |r: i32| TOP + r * ROW + ROW / 2;
        let cell = |name: &str, w: &mut World<Soft>, a: &mut App, m: &mut Meter, step: &mut dyn FnMut(&mut World<Soft>, &mut App, usize, bool)| {
            // dispatch alone, then dispatch and settle, warm and cold
            let (di, _) = m.run(reps, false, |k| step(w, a, k, false));
            w.settle(a);
            w.tiles.clear();
            let (si, sc) = m.run(reps, false, |k| {
                step(w, a, k, true);
                w.tiles.clear();
            });
            let (_, cc) = m.run(reps, true, |k| {
                step(w, a, k, true);
                w.tiles.clear();
            });
            println!("{n}\t{name}\t{di}\t{si}\t{sc}\t{cc}\t{}", cc.saturating_sub(sc));
        };
        cell("pointer_onto_row", &mut w, &mut a, &mut m, &mut |w, a, k, s| {
            w.pointer(a, 300, row_y(3 + (k & 1) as i32));
            if s {
                w.settle(a);
            }
        });
        cell("pointer_same_row", &mut w, &mut a, &mut m, &mut |w, a, k, s| {
            w.pointer(a, 300 + (k & 1) as i32 * 7, row_y(3));
            if s {
                w.settle(a);
            }
        });
        cell("click_row", &mut w, &mut a, &mut m, &mut |w, a, k, s| {
            w.click(a, 300, row_y(5 + (k & 3) as i32));
            if s {
                w.settle(a);
            }
        });
        cell("key_unfocused", &mut w, &mut a, &mut m, &mut |w, a, _, s| {
            w.key_down(a, Key::Char('k'), Mods::default());
            if s {
                w.settle(a);
            }
        });
        w.click(&mut a, 1210, 50);
        w.settle(&a);
        w.tiles.clear();
        cell("text_focused_field", &mut w, &mut a, &mut m, &mut |w, a, _, s| {
            w.text(a, "a");
            if s {
                w.settle(a);
            }
        });
        let (rec, _, bytes) = w.root.stats();
        println!("# rows={n}: {rec} records, {bytes} bytes of columns");
    }
}
