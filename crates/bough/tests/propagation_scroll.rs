//! Scroll containers reconcile their content (round 6, defect 1): after every operation the retained pixels
//! equal a fresh mount of the same state (not only presented == full redraw), at every ring size 1..4.
//!
//! The mutant is `--features mutant_scroll_skip` (the content is opaque to a re-description again): every
//! test here that changes a plain operand under a scroll port must fail with it.
use bough::app::{App, Config, View};
use bough::prelude::*;

const W: usize = 240;
const H: usize = 180;

fn config_sized(ring: usize, w: f32, h: f32) -> Config {
    let mut c = Config::new(w, h, 1.0, zgx_workload::FONT_BYTES.to_vec());
    c.ring = ring;
    c
}
fn config(ring: usize) -> Config {
    config_sized(ring, W as f32, H as f32)
}
fn compare(a: &[u8], b: &[u8], context: &str) {
    assert_eq!(a.len(), b.len(), "{context}: dimensions");
    let n = a.chunks(4).zip(b.chunks(4)).filter(|(a, b)| a != b).count();
    if n != 0 {
        let first = a.chunks(4).zip(b.chunks(4)).position(|(a, b)| a != b).unwrap();
        panic!("{context}: {n} unequal pixels, the first at ({}, {})", first % W, first / W);
    }
}
/// The two gates after a frame: presented == a full redraw of the retained state, and that == `fresh`.
fn gate<V: View>(a: &mut App<V>, fresh: App<V>, context: &str) {
    let mut fresh = fresh;
    a.frame();
    let retained = a.reference_pixels();
    compare(&a.presented_pixels(), &retained, &format!("{context}: presented/full"));
    fresh.frame();
    compare(&retained, &fresh.reference_pixels(), &format!("{context}: retained/fresh"));
}
/// The BGRA pixel at (x, y) as 0xRRGGBB.
fn px(p: &[u8], w: usize, x: usize, y: usize) -> u32 {
    let i = (y * w + x) * 4;
    (p[i + 2] as u32) << 16 | (p[i + 1] as u32) << 8 | p[i] as u32
}

// =============================================================================== plain rows, nested ports

struct Document {
    rows: Vec<(f32, u32)>,
    y: Signal<f32>,
    inner_y: Signal<f32>,
    leading: f32,
    width: f32,
    title: String,
}
impl Document {
    fn new() -> Self {
        Self { rows: (0..10).map(|i| (70.0 + i as f32, 0x102030 + i * 0x101010)).collect(), y: Signal::new(0.0), inner_y: Signal::new(0.0), leading: 8.0, width: 160.0, title: "rows".into() }
    }
    fn snapshot(&self) -> Self {
        Self { rows: self.rows.clone(), y: Signal::new(*self.y), inner_y: Signal::new(*self.inner_y), leading: self.leading, width: self.width, title: self.title.clone() }
    }
}
impl View for Document {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            block(10.0, self.leading),
            column((
                column((block(90.0, 24.0).bg(rgb(self.rows.first().map_or(0, |r| r.1))), block(90.0, 24.0).bg(rgb(0x00aa88)))).w(100.0).h(32.0).scroll(sig!(self.inner_y)),
                text(self.title.clone()),
                each(self.rows.iter().map(|&(w, c)| block(w, 24.0).bg(rgb(c)))),
            ))
            .w(self.width)
            .h(120.0)
            .scroll(sig!(self.y)),
            block(30.0, 12.0).bg(rgb(0xffaa00)),
        ))
        .w(220.0)
    }
}

#[test]
fn scroll_children_equal_fresh_after_each_operation_at_every_ring_age() {
    for ring in 1..=4 {
        let mut a = App::new(Document::new(), config(ring), true);
        a.frame();
        for op in 0..18 {
            a.turn(|d| match op {
                0 => d.rows[0].1 = 0xff0000,
                1 => d.rows[1].0 = 38.0,
                2 => {
                    d.y.set(29.0);
                    d.inner_y.set(7.0);
                }
                3 => d.rows.insert(0, (120.0, 0x00ff00)),
                4 => {
                    d.rows.remove(1);
                }
                5 => d.rows.swap(0, 3),
                6 => d.leading = 19.0,
                7 => d.width = 130.0,
                // a scroll and a plain change in one turn (scroll by copy plus a repaint inside the moved content)
                8 => {
                    d.y.set(52.0);
                    d.rows[3].1 = 0x4444ff;
                    d.title = "moved".into();
                }
                // a change below the port, shown only by the next scroll
                9 => d.rows[9].1 = 0xff00ff,
                10 => {
                    d.y.set(140.0);
                }
                11 => d.rows.truncate(2),
                12 => {
                    d.y.set(0.0);
                }
                13 => d.rows.clear(),
                14 => d.rows.push((60.0, 0x8833ff)),
                15 => d.title = "a longer title that is wider".into(),
                16 => d.rows.extend((0..12).map(|i| (50.0 + 3.0 * i as f32, 0x203040 + i * 0x070503))),
                17 => {
                    d.inner_y.set(16.0);
                    d.y.set(60.0);
                }
                _ => unreachable!(),
            });
            let fresh = App::new(a.owner.snapshot(), config(ring), true);
            gate(&mut a, fresh, &format!("ring {ring} op {op}"));
        }
    }
}

// =============================================================================== the nested port's clip

/// The port of a nested scroll box is its box cut by the outer port, wherever the outer port and offset put it
/// (an expected-pixel check: retained and fresh agreed on a clip cut with the box's stale absolute rect).
#[test]
fn nested_port_clip_is_the_port() {
    for y in [0.0f32, 13.0] {
        let mut d = Document::new();
        d.y = Signal::new(y);
        d.inner_y = Signal::new(16.0);
        let mut a = App::new(d, config(1), true);
        a.frame();
        let p = a.reference_pixels();
        // the inner port: 100 x 32 at the top of the outer content, below the 8 pt leading; its content at offset
        // 16 shows the last 8 rows of the first block (0x102030) and the whole second one (0x00aa88)
        let bottom = 8 + 32 - y as usize;
        assert_eq!(px(&p, W, 50, 8), if y == 0.0 { 0x102030 } else { 0x00aa88 }, "offset {y}: the nested port's first visible row");
        assert_eq!(px(&p, W, 50, bottom - 1), 0x00aa88, "offset {y}: the nested port's last row");
        assert_ne!(px(&p, W, 50, bottom), 0x00aa88, "offset {y}: below the nested port");
    }
}

// =============================================================================== scroll boxes that come and go

struct Panels {
    a: Signal<f32>,
    b: Signal<f32>,
    outer: Signal<f32>,
    show_a: bool,
    show_b: bool,
    tint: u32,
    clipped: bool,
}
impl Panels {
    fn new() -> Self {
        Self { a: Signal::new(0.0), b: Signal::new(0.0), outer: Signal::new(0.0), show_a: true, show_b: false, tint: 0x3355aa, clipped: false }
    }
    fn snapshot(&self) -> Self {
        Self { a: Signal::new(*self.a), b: Signal::new(*self.b), outer: Signal::new(*self.outer), show_a: self.show_a, show_b: self.show_b, tint: self.tint, clipped: self.clipped }
    }
    fn stripes(&self, base: u32) -> impl Element<Self> + '_ {
        each((0..6u32).map(move |i| block(80.0, 14.0).bg(rgb(if i == 2 { self.tint } else { base + i * 0x0a0a0a }))))
    }
}
impl View for Panels {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            block(200.0, 6.0).bg(rgb(0x222222)),
            self.show_a.then(|| column(self.stripes(0x400000)).w(120.0).h(40.0).bg(rgb(0x101010)).scroll(sig!(self.a))),
            block(150.0, 10.0).bg(rgb(self.tint)),
            self.show_b.then(|| column((self.stripes(0x004000), self.clipped.then(|| column(block(300.0, 9.0).bg(rgb(0xffffff))).w(60.0).clip_overflow()))).w(140.0).h(30.0).scroll(sig!(self.b))),
            each((0..8u32).map(|i| block(100.0 + 4.0 * i as f32, 16.0).bg(rgb(0x000040 + i * 0x101000)))),
        ))
        .w(220.0)
        .h(150.0)
        .scroll(sig!(self.outer))
    }
}

/// Scroll boxes inside a scroll box's content are removed and inserted by re-descriptions: an outer scroll
/// after a removal moves no stale view, a reinserted box starts from its signal, and the cycle is sustainable
/// (a view's space, clip and placement hook return when it is released).
#[test]
fn nested_scroll_boxes_removed_and_reinserted() {
    for ring in 1..=4 {
        let mut a = App::new(Panels::new(), config(ring), true);
        a.frame();
        let ops: [fn(&mut Panels); 14] = [
            |d| { d.a.set(20.0); },
            |d| d.show_b = true,
            |d| { d.b.set(11.0); d.outer.set(9.0); },
            |d| d.show_a = false,
            |d| { d.outer.set(31.0); },
            |d| d.tint = 0xee2211,
            |d| d.show_a = true,
            |d| { d.outer.set(4.0); d.show_b = false; },
            |d| d.clipped = true,
            |d| d.show_b = true,
            |d| { d.b.set(50.0); },
            |d| d.clipped = false,
            |d| { d.show_a = false; d.show_b = false; d.outer.set(0.0); },
            |d| { d.show_a = true; d.show_b = true; d.clipped = true; d.tint = 0x00ccff; },
        ];
        for (i, op) in ops.iter().enumerate() {
            a.turn(op);
            let fresh = App::new(a.owner.snapshot(), config(ring), true);
            gate(&mut a, fresh, &format!("ring {ring} op {i}"));
        }
        // far more cycles than the default columns hold spaces (2,048) or clips (512)
        for i in 0..3000 {
            a.turn(|d| {
                d.show_a = i % 2 == 0;
                d.show_b = i % 3 != 0;
                d.clipped = i % 5 != 0;
            });
            if i % 64 == 0 {
                a.frame();
            }
        }
        a.turn(|d| { d.show_a = true; d.show_b = true; d.clipped = true; d.outer.set(22.0); d.a.set(7.0); });
        let fresh = App::new(a.owner.snapshot(), config(ring), true);
        gate(&mut a, fresh, &format!("ring {ring} after 3000 cycles"));
    }
}

// =============================================================================== the window and the wheel

/// A window resize re-lays the groups out; a re-description at the new size still agrees with a fresh mount.
#[test]
fn window_resize_then_redescribe() {
    for ring in 1..=4 {
        let mut a = App::new(Document::new(), config(ring), true);
        a.frame();
        a.turn(|d| { d.y.set(35.0); });
        a.frame();
        for (i, (w, h)) in [(200.0f32, 140.0f32), (240.0, 180.0), (176.0, 96.0)].into_iter().enumerate() {
            a.resize(w, h);
            a.frame();
            a.turn(|d| {
                d.rows[i].1 = 0xffee00 + i as u32;
                d.width = 150.0 - 10.0 * i as f32;
            });
            a.frame();
            let retained = a.reference_pixels();
            compare(&a.presented_pixels(), &retained, &format!("ring {ring} size {i}: presented/full"));
            let mut fresh = App::new(a.owner.snapshot(), config_sized(ring, w, h), true);
            fresh.frame();
            let f = fresh.reference_pixels();
            assert_eq!(retained.len(), f.len());
            assert!(retained == f, "ring {ring} size {i}: retained/fresh");
        }
    }
}

/// The scroller is the box's identity: a glide in flight survives re-descriptions (it is not reinstalled),
/// and the content it moves is the re-described one.
#[test]
fn glide_survives_redescription() {
    for ring in 1..=4 {
        let mut a = App::new(Document::new(), config(ring), true);
        a.frame();
        // one detent down over the outer port (below the nested one)
        a.wheel(120.0, 100.0, 0.0, 1.0);
        let mut steps = 0;
        while a.animating() {
            a.tick(1.0 / 60.0);
            // every tick is followed by a re-description (ordinary views); every third one changes the content
            if steps % 3 == 0 {
                a.turn(|d| d.rows[4].1 = 0x00ffff - steps as u32 * 0x000101);
            }
            a.frame();
            steps += 1;
            assert!(steps < 100, "the glide ends");
        }
        assert!(steps > 5, "a glide ran over several frames ({steps})");
        assert_eq!(*a.owner.y, 48.0, "one detent is 48 points");
        let fresh = App::new(a.owner.snapshot(), config(ring), true);
        gate(&mut a, fresh, &format!("ring {ring} after the glide"));
    }
}

// =============================================================================== a list under a port

struct Item {
    label: Signal<String>,
    c: u32,
}
impl View for Item {
    fn view(&self) -> impl Element<Self> + '_ {
        row((block(12.0, 12.0).bg(rgb(self.c)), text(sig!(self.label)))).gap(4.0)
    }
}
struct Mixed {
    items: List<Item>,
    y: Signal<f32>,
    head: u32,
    extra: Vec<u32>,
}
impl Mixed {
    fn new() -> Self {
        Self { items: List::from_iter_in((0..30).map(|i| Item { label: Signal::new(format!("item {i}")), c: 0x202020 + i * 0x050301 })), y: Signal::new(0.0), head: 0x777777, extra: vec![0x111111, 0x222222] }
    }
    fn snapshot(&self) -> Self {
        Self { items: List::from_iter_in(self.items.iter().map(|i| Item { label: Signal::new((*i.label).clone()), c: i.c })), y: Signal::new(*self.y), head: self.head, extra: self.extra.clone() }
    }
}
impl View for Mixed {
    fn view(&self) -> impl Element<Self> + '_ {
        column((block(180.0, 10.0).bg(rgb(self.head)), each(self.extra.iter().map(|&c| block(60.0, 8.0).bg(rgb(c)))), list(sig!(self.items)).w(200.0), block(90.0, 10.0).bg(rgb(self.head ^ 0xffffff))))
            .w(220.0)
            .h(160.0)
            .scroll(sig!(self.y))
    }
}

/// A keyed list and plain siblings under one port: the siblings re-describe around the list's rows (whose run
/// the walk steps over), list edits replay, and both survive scrolling. The rows are there from the first
/// frame (an expected-pixel check: the region windowed against the port's clip before the port was placed, and
/// mounted no row until the first scroll; retained and fresh agreed on the empty list).
#[test]
fn list_and_plain_siblings_under_a_port() {
    for ring in 1..=4 {
        let mut a = App::new(Mixed::new(), config(ring), true);
        a.frame();
        assert!(a.rt.list_window(0).len() >= 6, "rows are mounted at the first frame: {:?}", a.rt.list_window(0));
        let p = a.reference_pixels();
        // 10 (head) + 2 x 8 (extra) above the list; row 0's 12 x 12 swatch is 0x202020
        assert_eq!(px(&p, W, 5, 10 + 16 + 5), 0x202020, "the first row's swatch");
        let ops: [fn(&mut Mixed); 10] = [
            |d| d.head = 0xaa0000,
            |d| { d.y.set(40.0); },
            |d| d.extra.push(0x00aa00),
            |d| { d.items.remove(2); },
            |d| { d.items.edit(4).label.set("edited".into()); },
            |d| { d.extra.clear(); d.y.set(120.0); },
            |d| { d.items.insert(0, Item { label: Signal::new("new".into()), c: 0xffffff }); d.head = 0x0000aa; },
            |d| d.items.swap(1, 9),
            |d| { d.y.set(0.0); d.extra = vec![1, 2, 3, 4, 5].into_iter().map(|i| i * 0x302010).collect(); },
            |d| { d.items.truncate(3); },
        ];
        for (i, op) in ops.iter().enumerate() {
            a.turn(op);
            let fresh = App::new(a.owner.snapshot(), config(ring), true);
            gate(&mut a, fresh, &format!("ring {ring} op {i}"));
        }
    }
}

// =============================================================================== the raw scroll view

mod raw {
    use super::{compare, config, gate};
    use bough::raw::*;

    pub struct Sheet {
        pub y: Signal<f32>,
        pub inner: Signal<f32>,
        pub cells: Vec<u32>,
        pub shift: f32,
        pub bg: u32,
    }
    impl Sheet {
        pub fn snapshot(&self) -> Sheet {
            Sheet { y: Signal::new(*self.y), inner: Signal::new(*self.inner), cells: self.cells.clone(), shift: self.shift, bg: self.bg }
        }
    }
    impl View for Sheet {
        fn view(&self) -> impl Element<Self> + '_ {
            (
                quad(0.0, 0.0, 240.0, 180.0).bg(rgb(0x050505)),
                scroll_y(
                    sig!(self.y),
                    [10.0, 20.0, 200.0, 120.0],
                    400.0,
                    (
                        each(self.cells.iter().enumerate().map(|(i, &c)| quad(4.0 + self.shift, 4.0 + 22.0 * i as f32, 150.0, 18.0).bg(rgb(c)).children(text(format!("cell {i}")).at(4.0, 2.0)))),
                        scroll_y(sig!(self.inner), [160.0, 30.0, 36.0, 60.0], 100.0, each((0..8u32).map(|i| quad(2.0, 2.0 + 20.0 * i as f32, 30.0, 16.0).bg(rgb(self.bg + i * 0x111111))))).bg(rgb(0x202020)),
                        clip(0.0, 300.0, 100.0, 12.0, quad(0.0, 296.0, 180.0, 20.0).bg(rgb(self.bg))),
                    ),
                )
                .bg(rgb(0x181818)),
            )
        }
    }

    /// `scroll_y` (the scene surface): its content re-describes too, with nested views and clips following.
    #[test]
    fn raw_scroll_view_content_redescribes() {
        for ring in 1..=4 {
            let mut a = App::new(Sheet { y: Signal::new(0.0), inner: Signal::new(0.0), cells: (0..14).map(|i| 0x300000 + i * 0x001108).collect(), shift: 0.0, bg: 0x004488 }, config(ring), true);
            a.frame();
            let ops: [fn(&mut Sheet); 9] = [
                |d| d.cells[1] = 0xffffff,
                |d| { d.y.set(33.0); },
                |d| d.shift = 9.0,
                |d| { d.inner.set(41.0); d.bg = 0x884400; },
                |d| { d.cells.remove(0); },
                |d| { d.y.set(190.0); d.cells.push(0x00ff00); },
                |d| d.cells.truncate(5),
                |d| { d.y.set(0.0); d.inner.set(0.0); },
                |d| { d.cells.extend([0x123456, 0x654321]); d.shift = 0.0; },
            ];
            for (i, op) in ops.iter().enumerate() {
                a.turn(op);
                let fresh = App::new(a.owner.snapshot(), config(ring), true);
                gate(&mut a, fresh, &format!("raw ring {ring} op {i}"));
            }
            let _ = compare;
        }
    }
}
