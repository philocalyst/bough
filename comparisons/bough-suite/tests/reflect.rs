//! Edits through reflection are ordinary writes: for every ring age 1..=4 and the windowed ring, the
//! presented surface equals a full redraw after every step, equals the same edits written directly, and
//! equals a fresh mount at the end.
mod common;
use bough::layout::prelude::*;
use bough::prelude::*;
use common::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Theme {
    pad: f32,
    color: u32,
}
struct Item {
    label: Signal<String>,
    n: Signal<u32>,
}
impl View for Item {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        row((label(sig!(self.label)).font(12.0, 16.0), label(sig!(self.n).map(|n| format!("#{n}"))).font(10.0, 14.0))).gap(4.0)
    }
}
struct Root {
    title: Signal<String>,
    w: Signal<f32>,
    theme: Signal<Theme>,
    items: List<Item>,
}
impl View for Root {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            label(sig!(self.title)).font(14.0, 18.0),
            block(10.0, 12.0).bg(rgb(0x3366ff)).w_op(sig!(self.w)),
            block(30.0, 8.0).bg(sig!(self.theme).map(|t| rgb(t.color))).h_op(sig!(self.theme).map(|t| 4.0 + t.pad)),
            list(sig!(self.items)).gap(2.0),
        ))
        .gap(3.0)
        .w(260.0)
    }
}
fn root() -> Root {
    Root {
        title: Signal::new("title".into()),
        w: Signal::new(10.0),
        theme: Signal::new(Theme { pad: 2.0, color: 0x00ff00 }),
        items: List::from_iter_in((0..6).map(|i| Item { label: Signal::new(format!("row {i}")), n: Signal::new(i) })),
    }
}
fn edit(t: usize) -> (u64, String, String) {
    let mut x = (t as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 29;
    let i = (x >> 8) as usize % 6;
    let (p, v) = match x % 6 {
        0 => ("title".to_string(), format!("t{}", x % 1000)),
        1 => ("w".to_string(), format!("{}", 4 + (x >> 12) % 80)),
        2 => ("theme.pad".to_string(), format!("{}", (x >> 12) % 20)),
        3 => ("theme.color".to_string(), format!("{}", (x >> 12) as u32 & 0xffffff)),
        4 => (format!("items[{i}].label"), format!("L{}", (x >> 20) % 10000)),
        _ => (format!("items[{i}].n"), format!("{}", (x >> 20) % 500)),
    };
    (x, p, v)
}

#[test]
fn reflection_edits_are_pixel_exact_at_every_ring_age() {
    let cfg = || Config::new(300.0, 240.0, 2.0, FONT.to_vec());
    for ring in RINGS {
        // the same edits written directly: the reference surface for each step
        let mut direct = app_ring(root(), cfg(), ring);
        direct.frame();
        let mut want: Vec<Vec<u8>> = Vec::new();
        for t in 1..=40 {
            let (x, _, v) = edit(t);
            let i = (x >> 8) as usize % 6;
            direct.turn(|o| match x % 6 {
                0 => *o.title = v.clone(),
                1 => *o.w = v.parse().unwrap(),
                2 => o.theme.pad = v.parse().unwrap(),
                3 => o.theme.color = v.parse().unwrap(),
                4 => *o.items.edit(i).label = v.clone(),
                _ => *o.items.edit(i).n = v.parse().unwrap(),
            });
            direct.frame();
            want.push(direct.presented_pixels());
        }
        let app = app_ring(root(), cfg(), ring);
        let mut steps = 0;
        let px = check(
            "reflect",
            app,
            40,
            1,
            |a, t| {
                let (_, p, v) = edit(t);
                assert!(a.reflect_set(&p, &v), "{p} = {v}");
                steps += 1;
            },
            || {
                let mut r = root();
                for t in 1..=40 {
                    let (_, p, v) = edit(t);
                    assert!(bough::reflect::set_in(&mut r, &p, &v));
                }
                App::new(r, with_ring(cfg(), ring), true)
            },
        );
        assert_eq!(steps, 40);
        assert!(px == *want.last().unwrap(), "ring {ring}: reflection edits != direct writes");
    }
}

#[test]
fn direct_edits_are_pixel_exact_at_every_ring_age() {
    let cfg = || Config::new(300.0, 240.0, 2.0, FONT.to_vec());
    for ring in RINGS {
        let app = app_ring(root(), cfg(), ring);
        check(
            "direct",
            app,
            40,
            1,
            |a, t| {
                let (x, _, v) = edit(t);
                let i = (x >> 8) as usize % 6;
                a.turn(|o| match x % 6 {
                    0 => *o.title = v.clone(),
                    1 => *o.w = v.parse().unwrap(),
                    2 => o.theme.pad = v.parse().unwrap(),
                    3 => o.theme.color = v.parse().unwrap(),
                    4 => *o.items.edit(i).label = v.clone(),
                    _ => *o.items.edit(i).n = v.parse().unwrap(),
                });
            },
            || {
                let mut r = root();
                for t in 1..=40 {
                    let (_, p, v) = edit(t);
                    assert!(bough::reflect::set_in(&mut r, &p, &v));
                }
                App::new(r, with_ring(cfg(), ring), true)
            },
        );
    }
}

