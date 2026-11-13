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
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};

/// Every allocation (and reallocation) the process makes, counted.
struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
// SAFETY: forwards to the system allocator; the counter is the only addition.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static GA: Counting = Counting;

/// Allocations made by `f`, the most over `reps` runs.
fn allocs(reps: usize, mut f: impl FnMut(usize)) -> u64 {
    (0..reps)
        .map(|k| {
            let a = ALLOCS.load(Ordering::Relaxed);
            f(k);
            ALLOCS.load(Ordering::Relaxed) - a
        })
        .max()
        .unwrap_or(0)
}

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
            // the handler allocates nothing either, so what is counted is the event path's
            (Fill(Kc::<0xffffffff>).on_text(|a: &mut App, t: &str| {
                let (mut b, mut n) = a.draft.get(0).bytes();
                if n + t.len() > 12 {
                    n = 0;
                }
                b[n..n + t.len()].copy_from_slice(t.as_bytes());
                let mut w = [0u32; 3];
                for (i, &c) in b[..n + t.len()].iter().enumerate() {
                    w[i / 4] |= (c as u32) << (8 * (i % 4));
                }
                *a.draft.edit(0) = Txt(w);
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
    bracket: [u64; pmu::MAX],
    n: usize,
    sweep: Vec<u64>,
}
impl Meter {
    fn new() -> Meter {
        let n = pmu::sample_events().len();
        let v: Vec<[u64; pmu::MAX]> = (0..64)
            .map(|_| {
                let a = pmu::sample();
                pmu::sample().since(&a).v
            })
            .collect();
        let mut bracket = [0u64; pmu::MAX];
        for (i, b) in bracket.iter_mut().enumerate().take(n) {
            let mut c: Vec<u64> = v.iter().map(|x| x[i]).collect();
            c.sort();
            *b = c[32];
        }
        Meter { bracket, n, sweep: vec![0; 8 << 20] }
    }
    fn evict(&mut self) {
        for (i, x) in self.sweep.iter_mut().enumerate() {
            *x = x.wrapping_add(i as u64);
        }
        black_box(&self.sweep);
    }
    /// Every counter of `f`, median of `reps`, warm or after an eviction.
    fn run(&mut self, reps: usize, cold: bool, mut f: impl FnMut(usize)) -> [u64; pmu::MAX] {
        let mut v = Vec::with_capacity(reps);
        for k in 0..reps {
            if cold {
                self.evict();
            }
            let a = pmu::sample();
            f(k);
            let d = pmu::sample().since(&a);
            let mut x = d.v;
            for i in 0..self.n {
                x[i] = x[i].saturating_sub(self.bracket[i]);
            }
            v.push(x);
        }
        let mut out = [0u64; pmu::MAX];
        for (i, o) in out.iter_mut().enumerate().take(self.n) {
            let mut c: Vec<u64> = v.iter().map(|x| x[i]).collect();
            c.sort();
            *o = c[reps / 2];
        }
        out
    }
}

fn main() {
    let reps: usize = std::env::var("REPS").ok().and_then(|s| s.parse().ok()).unwrap_or(31);
    let sizes: Vec<usize> = match std::env::var("ROWS").ok().and_then(|s| s.parse().ok()) {
        Some(n) => vec![n],
        None => vec![100, 10_000, 1_000_000],
    };
    let mut m = Meter::new();
    let extra: Vec<&str> = pmu::sample_events().iter().skip(2).map(|e| e.name()).collect();
    print!("rows\tstep\tdispatch_allocs\tdispatch_instr\tstep_instr\tstep_cycles_warm\tstep_cycles_cold\tcold_minus_warm");
    for e in &extra {
        print!("\twarm:{e}\tcold:{e}");
    }
    println!();
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
            let da = allocs(reps, |k| step(w, a, k, false));
            let di = m.run(reps, false, |k| step(w, a, k, false))[0];
            w.settle(a);
            w.tiles.clear();
            let warm = m.run(reps, false, |k| {
                step(w, a, k, true);
                w.tiles.clear();
            });
            let cold = m.run(reps, true, |k| {
                step(w, a, k, true);
                w.tiles.clear();
            });
            print!("{n}\t{name}\t{da}\t{di}\t{}\t{}\t{}\t{}", warm[0], warm[1], cold[1], cold[1].saturating_sub(warm[1]));
            for i in 2..m.n {
                print!("\t{}\t{}", warm[i], cold[i]);
            }
            println!();
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
