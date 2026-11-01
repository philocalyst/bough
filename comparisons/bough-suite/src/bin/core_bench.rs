//! zgx `core_bench`, native: the same CPU-only invalidation microbenchmarks, on bough's equivalents (the
//! iteration count is argv 1, 100,000). Each "flush" is `App::turn` + `App::frame`, headless.
//!
//! * idle: a frame with no damage over 1,002 retained records (`App::frame` returns false);
//! * fixed text update + flush: a text wire toggling "stream A" / "stream B";
//! * leaf translation + flush: a one-lane `lanes` quad whose x toggles (bough has no x operand on a quad
//!   or on text, so a moved leaf is a one-lane `lanes` element);
//! * 1000 cards translating + flush: 30 x 16 quads on a 40-column grid, x + 1 or 2 each frame. As one
//!   `lanes(1000)` element, and as 1000 one-lane elements ("wires"). Bare quads: zgx's cards carry a text
//!   child, which bough's text could not follow (no x operand);
//! * hit test among 1000 cards: `Bough::hit(x, y)` on the wires scene (1000 records; the lanes scene's
//!   single record covers the whole grid);
//! * rotation: not supported (no transforms);
//! * million-row virtual range: zgx's `VirtualList` is fixed-height, so its equivalent is the fixed list's
//!   closed form (`virtual_list`'s `visible(scroll)`, as `bough-scenes::micro_rows`); the variable-height
//!   index (`HeightIndex`, a Fenwick tree) is measured beside it.
use bough::mlist::HeightIndex;
use bough::prelude::*;
use bough_suite::*;
use std::{hint::black_box, time::Instant};

fn measure(name: &str, iterations: usize, mut operation: impl FnMut(usize)) {
    let start = Instant::now();
    let i0 = instructions();
    for index in 0..iterations {
        operation(index);
    }
    let (elapsed, spent) = (start.elapsed(), instructions() - i0);
    println!(
        "{name}: {iterations} iterations, {:.1} ns/iteration, {:.0} instructions/iteration",
        elapsed.as_nanos() as f64 / iterations as f64,
        spent as f64 / iterations as f64
    );
}

/// 1,000 retained quads and the text `stream`; `x` moves the lane `leaf`.
struct Scene {
    stream: Signal<String>,
    x: Signal<f32>,
}
impl View for Scene {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        (
            text(sig!(self.stream)).at(0.0, 0.0).size(16.0).line_height(normal(16.0)),
            lanes(1, sig!(self.x), |x: &f32, _| Lane { x: *x, y: 120.0, w: 400.0, h: 100.0, color: c(0x20, 0x30, 0x40) }),
            each((0..1000).map(|i| quad((i % 40 * 25) as f32, 300.0 + (i / 40) as f32 * 14.0, 10.0, 10.0).bg(c(50, 50, 50)))),
        )
    }
}

#[inline(always)]
fn card(i: usize, x: f32) -> Lane {
    Lane { x: (i % 40) as f32 * 25.0 + x, y: (i / 40) as f32 * 28.0, w: 30.0, h: 16.0, color: c(0x44, 0x66, 0x88) }
}
struct CardLanes {
    x: Signal<f32>,
}
impl View for CardLanes {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        lanes(1000, sig!(self.x), |x: &f32, i| card(i, *x))
    }
}
struct CardWires {
    x: Signal<f32>,
}
impl View for CardWires {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        each((0..1000u32).map(|i| lanes(1, sig!(self.x), move |x: &f32, _| card(i as usize, *x))))
    }
}

fn main() {
    let iterations = std::env::args().nth(1).map(|s| s.parse::<usize>().expect("positive iteration count")).unwrap_or(100_000).max(1);
    let mut a = app(Scene { stream: Signal::new("ready".into()), x: Signal::new(0.0) }, 1000.0, 700.0, false);
    a.frame();
    assert_eq!(a.rt.bough.len(), 1002);
    measure("idle (1002 retained nodes)", iterations, |_| {
        assert!(!black_box(a.frame()));
    });
    measure("fixed text update + flush", iterations, |i| {
        a.turn(|v| _ = v.stream.set(if i % 2 == 0 { "stream A" } else { "stream B" }.to_string()));
        assert!(black_box(a.frame()));
    });
    measure("leaf translation + flush", iterations, |i| {
        a.turn(|v| _ = v.x.set((i % 2 + 1) as f32));
        assert!(black_box(a.frame()));
    });
    let frames = (iterations / 100).max(10);
    let mut g = app(CardLanes { x: Signal::new(0.0) }, 1000.0, 700.0, false);
    g.frame();
    measure("1000 cards translating + flush (lanes)", frames, |i| {
        g.turn(|v| _ = v.x.set((i % 2 + 1) as f32));
        assert!(black_box(g.frame()));
    });
    let mut w = app(CardWires { x: Signal::new(0.0) }, 1000.0, 700.0, false);
    w.frame();
    assert_eq!(w.rt.bough.len(), 1000);
    measure("1000 cards translating + flush (wires)", frames, |i| {
        w.turn(|v| _ = v.x.set((i % 2 + 1) as f32));
        assert!(black_box(w.frame()));
    });
    measure("hit test among 1000 cards", iterations, |i| {
        black_box(w.rt.bough.hit((i % 1000) as i32, (i % 700) as i32));
    });
    println!("1000 cards rotating + flush: not supported (no transforms)");
    println!("hit test among 1000 rotated cards: not supported (no transforms)");
    let mut maximum = 0;
    // the fixed list's range: first visible row, last, overscan 3 (f64 as zgx's VirtualList)
    let (count, row_h, view, over) = (1_000_000usize, 24.0f64, 480.0f64, 3usize);
    measure("million-row virtual range", iterations, |i| {
        let pos = black_box((i as f32 * 11.0) % 24_000_000.0) as f64;
        let first = (pos / row_h) as usize;
        let end = ((pos + view) / row_h).ceil() as usize;
        let range = first.saturating_sub(over)..(end + over).min(count);
        maximum = maximum.max(range.len());
        assert!(range.len() <= 27);
        black_box(range);
    });
    println!("  maximum_mounted_rows={maximum}; expected <=27; range computation retains no row data");
    let list = HeightIndex::new(1_000_000, 24);
    let mut maximum = 0;
    measure("million-row virtual range (variable-height index)", iterations, |i| {
        let pos = ((i as i64 * 11) % 24_000_000).min(list.total() - 480);
        let first = list.row_at(black_box(pos));
        let end = list.row_at(pos + 479) + 1;
        let range = first.saturating_sub(3)..(end + 3).min(list.len());
        maximum = maximum.max(range.len());
        assert!(range.len() <= 27);
        black_box(range);
    });
    println!("  maximum_mounted_rows={maximum}; expected <=27; range computation retains no row data");
}
