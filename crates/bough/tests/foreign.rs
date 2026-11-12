//! Foreign elements (`foreign.rs`): a record bough lays out, clips, scrolls, orders and hit-tests while the
//! host composites its content. Gates: the retained frame equals a full redraw at every ring age (inside a
//! scroll view, scroll by copy included); the hole of an `Under` element is transparent, rounded, and what is
//! painted after it lies over it; the placement handed to the host is the layout box, the clip and the paint
//! order; `Over` paints nothing; `Sampled` shows the host's pixels and repaints only on its content signal;
//! hit policies; unmount. Negative controls: `the_hole_gate_fails_without_the_punch` (below) and the
//! `sabotage_foreign` feature (placements ignore scroll: `placement_follows_scroll_*` must fail).
use bough::input::Button;
use bough::prelude::*;
use bough::raw::{App, Config};
use std::cell::RefCell;
use std::rc::Rc;

const S: f32 = 2.0;
const W: f32 = 400.0;
const H: f32 = 300.0;

#[derive(Default)]
struct Log {
    places: Vec<Placement>,
    removed: Vec<u64>,
    pointers: Vec<(u64, Pointer)>,
    /// Sampled content: the colour (premultiplied RGBA) the host paints.
    fill: [u8; 4],
    asked: u32,
}
/// The checker's dark cell: half the colour, opaque.
fn dark(c: [u8; 4]) -> [u8; 4] {
    [c[0] / 2, c[1] / 2, c[2] / 2, c[3]]
}
struct Host(Rc<RefCell<Log>>);
impl ForeignHost for Host {
    fn place(&mut self, p: &Placement) {
        self.0.borrow_mut().places.push(*p);
    }
    fn remove(&mut self, id: u64) {
        self.0.borrow_mut().removed.push(id);
    }
    fn pixels(&mut self, _id: u64, w: u32, h: u32, out: &mut [u8]) -> bool {
        let mut l = self.0.borrow_mut();
        l.asked += 1;
        for y in 0..h {
            for x in 0..w {
                // a checker of the fill and its half: any shift of the content shows
                let c = if (x / 8 + y / 8) % 2 == 0 { l.fill } else { dark(l.fill) };
                out[((y * w + x) * 4) as usize..][..4].copy_from_slice(&c);
            }
        }
        true
    }
    fn pointer(&mut self, id: u64, what: Pointer, _ev: &bough::input::Ev) {
        self.0.borrow_mut().pointers.push((id, what));
    }
}

/// A web view between two panels inside a scroll view, a translucent rounded badge painted over its corner.
struct Page {
    y: Signal<f32>,
    mode: Mode,
    hit: Hit,
    clicks: Signal<u32>,
    under: Signal<u32>,
}

const PORT: Color = rgb(0xffffff);
const BADGE: Color = Color::rgba(0, 0, 255, 128);

impl View for Page {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0x202830)).p(20.0).child(
            div()
                .w(360.0)
                .h(260.0)
                .bg(PORT)
                .scroll(sig!(self.y))
                .child(div().w(340.0).h(80.0).bg(rgb(0xcc3333)))
                .child(
                    div()
                        .relative()
                        .w(340.0)
                        .h(120.0)
                        // a bough panel under the web view: a click passes to it with `Hit::Pass`
                        .child(div().abs(0.0, 0.0).w(340.0).h(120.0).bg(rgb(0x888888)).on_click(|p: &mut Page| *p.under += 1))
                        .child(foreign(7).w(340.0).h(120.0).abs(0.0, 0.0).rounded(12.0).mode(self.mode).hit(self.hit).on_click(|p: &mut Page| *p.clicks += 1))
                        .child(div().abs(20.0, 20.0).w(60.0).h(30.0).bg(BADGE).rounded(8.0)),
                )
                .child(div().w(340.0).h(200.0).bg(rgb(0x33cc33))),
        )
    }
}

fn page(mode: Mode, hit: Hit) -> (App<Page>, Rc<RefCell<Log>>) {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut app = App::new(Page { y: Signal::new(0.0), mode, hit, clicks: Signal::new(0), under: Signal::new(0) }, Config::new(W, H, S, font), true);
    let log = Rc::new(RefCell::new(Log { fill: [200, 100, 50, 255], ..Log::default() }));
    app.set_foreign_host(Box::new(Host(log.clone())));
    app.frame();
    (app, log)
}

/// BGRA of device pixel (x, y) of the presented surface.
fn px(app: &mut App<Page>, x: i32, y: i32) -> [u8; 4] {
    let p = app.presented_pixels();
    let i = ((y * (W * S) as i32 + x) * 4) as usize;
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

fn exact(app: &mut App<Page>, what: &str) {
    let (a, b) = (app.presented_pixels(), app.reference_pixels());
    let n = a.iter().zip(&b).filter(|(x, y)| x != y).count();
    assert_eq!(n, 0, "{what}: presented differs from a full redraw in {n} bytes");
}

/// The foreign box in device px, from layout: the web view's row starts 80 pt into the content, at the port's
/// origin (20 pt, 20 pt) less the scroll.
fn expect_rect(scroll_pt: f32) -> [f32; 4] {
    let (x, y) = (20.0 * S, (20.0 + 80.0 - scroll_pt) * S);
    [x, y, x + 340.0 * S, y + 120.0 * S]
}

#[test]
fn under_the_hole_is_transparent_rounded_and_overlaid() {
    let (mut app, log) = page(Mode::Under, Hit::Bough);
    exact(&mut app, "first frame");
    let r = expect_rect(0.0);
    let (cx, cy) = (((r[0] + r[2]) / 2.0) as i32, ((r[1] + r[3]) / 2.0) as i32);
    // inside the hole, away from the badge: nothing of bough's (the panel under it is removed too)
    assert_eq!(px(&mut app, cx, cy), [0, 0, 0, 0], "the hole is transparent");
    // the square corner the 12 pt radius rounds off: what was painted before the hole is still there
    assert_eq!(px(&mut app, r[0] as i32 + 2, r[1] as i32 + 2), [0x88, 0x88, 0x88, 255], "outside the rounded hole");
    // the badge is painted after the hole: premultiplied blue at half alpha over nothing
    let b = px(&mut app, r[0] as i32 + (50.0 * S) as i32, r[1] as i32 + (35.0 * S) as i32);
    assert!(b[0].abs_diff(128) <= 1 && b[1] == 0 && b[2] == 0 && b[3].abs_diff(128) <= 1, "the badge lies over the hole with its own alpha: {b:?}");
    // panel A above the web view is untouched
    assert_eq!(px(&mut app, cx, r[1] as i32 - 40), [0x33, 0x33, 0xcc, 255]);
    let l = log.borrow();
    assert_eq!(l.places.len(), 1, "placed once");
    let p = l.places[0];
    assert_eq!((p.id, p.plane, p.order, p.rect), (7, Plane::Below, 0, r));
    assert_eq!(p.clip, [(20.0 * S) as i32, (20.0 * S) as i32, (380.0 * S) as i32, (280.0 * S) as i32], "the scroll port");
    assert_eq!(p.radius, 12.0 * S);
    assert!(p.covered, "the badge is painted over it");
}

/// The negative control of the hole gate: the same scene with the punch absent (`Over` paints nothing) has
/// bough's own pixels where the hole must be.
#[test]
fn the_hole_gate_fails_without_the_punch() {
    let (mut app, log) = page(Mode::Over, Hit::Bough);
    exact(&mut app, "first frame");
    let r = expect_rect(0.0);
    let (cx, cy) = (((r[0] + r[2]) / 2.0) as i32, ((r[1] + r[3]) / 2.0) as i32);
    assert_eq!(px(&mut app, cx, cy), [0x88, 0x88, 0x88, 255], "Over: the surface keeps what is painted under the element");
    let p = log.borrow().places[0];
    assert_eq!((p.plane, p.covered), (Plane::Above, true), "the host is told a layer above would hide the badge");
}

#[test]
fn placement_follows_scroll_and_frames_stay_exact_at_every_ring_age() {
    let (mut app, log) = page(Mode::Under, Hit::Bough);
    // 12 scroll steps: every repair age of the 4-surface ring, scroll by copy on each
    let mut y = 0.0f32;
    for step in 0..12 {
        y += if step % 3 == 2 { 31.5 } else { 7.0 };
        app.turn(|p| *p.y = y);
        assert!(app.frame());
        exact(&mut app, &format!("scroll step {step} (y = {y})"));
        let p = *log.borrow().places.last().unwrap();
        let px_y = (y * S).round() / S;
        assert_eq!(p.rect, expect_rect(px_y), "step {step}: the placement is the scrolled layout box");
    }
    assert_eq!(log.borrow().places.len(), 13, "one placement per frame that moved it");
    // scrolled until the port's top edge cuts the web view: the visible part shrinks, the hole with it
    app.turn(|p| *p.y = 140.0);
    app.frame();
    exact(&mut app, "clipped by the port");
    let p = *log.borrow().places.last().unwrap();
    let v = p.visible().unwrap();
    assert_eq!(v, [(20.0 * S) as i32, (20.0 * S) as i32, (360.0 * S) as i32, ((20.0 + 60.0) * S) as i32], "rect ∩ port");
    // above the port (the window's padding) the hole does not reach
    assert_eq!(px(&mut app, 200, (20.0 * S) as i32 - 3), [0x30, 0x28, 0x20, 255]);
    assert_eq!(px(&mut app, 400, (20.0 * S) as i32 + 40), [0, 0, 0, 0], "the hole inside the port");
    // scrolled away entirely: nothing visible
    app.turn(|p| *p.y = 100000.0);
    app.frame();
    exact(&mut app, "scrolled to the end");
}

#[test]
fn an_idle_frame_places_nothing_and_draws_nothing() {
    let (mut app, log) = page(Mode::Under, Hit::Bough);
    assert!(!app.frame(), "no damage: no frame");
    assert_eq!(log.borrow().places.len(), 1);
    // a content change in a layer mode is the compositor's business: no bough pixels depend on it
    app.foreign_signal(7).unwrap().content_changed();
    assert!(!app.frame(), "Under: a content change repaints nothing");
    assert_eq!(log.borrow().places.last().unwrap().serial, 1, "the host and the inspector still see the serial");
}

#[test]
fn sampled_content_is_painted_and_repainted_on_its_signal_only() {
    let (mut app, log) = page(Mode::Sampled, Hit::Bough);
    exact(&mut app, "first frame");
    let r = expect_rect(0.0);
    // BGRA of the fill (RGBA 200,100,50), in a checker cell away from the badge
    let (x, y) = (r[2] as i32 - 20, r[3] as i32 - 20);
    let cell = |x: i32, y: i32, fill: [u8; 4]| -> [u8; 4] {
        let (u, v) = ((x - r[0] as i32) as u32, (y - r[1] as i32) as u32);
        let c = if (u / 8 + v / 8) % 2 == 0 { fill } else { dark(fill) };
        [c[2], c[1], c[0], c[3]]
    };
    assert_eq!(px(&mut app, x, y), cell(x, y, [200, 100, 50, 255]), "the host's pixels, texel for pixel");
    assert_eq!(log.borrow().asked, 1);
    assert_eq!(log.borrow().places[0].plane, Plane::Inline);
    assert!(!app.frame(), "idle");
    // the content changes on its own clock, eight times: the ring's four surfaces are first drawn whole, then
    // each is repaired from the last four epochs' damage, which is these content changes and nothing else
    let sig = app.foreign_signal(7).unwrap();
    for k in 0..8u8 {
        log.borrow_mut().fill = [10 + k, 220, 30, 255];
        let s2 = sig.clone();
        std::thread::spawn(move || s2.content_changed()).join().unwrap();
        assert!(app.frame(), "the signal is damage");
        assert_eq!(log.borrow().asked, 2 + k as u32);
        exact(&mut app, "after a content change");
        assert_eq!(px(&mut app, x, y), cell(x, y, [10 + k, 220, 30, 255]));
    }
    // exactly the tiles of its visible ink were repaired (ink = box + 1 px margin)
    let rows = app.debug_rows().to_vec();
    let (tx0, ty0, tx1, ty1) = ((r[0] as i32 - 1) / 32, (r[1] as i32 - 1) / 32, (r[2] as i32 + 1 + 31) / 32, (r[3] as i32 + 1 + 31) / 32);
    for (ty, row) in rows.iter().enumerate() {
        let want = if (ty as i32) >= ty0 && (ty as i32) < ty1 { (u128::MAX >> (128 - (tx1 - tx0))) << tx0 } else { 0 };
        assert_eq!(*row, want, "tile row {ty}");
    }
    // every ring age holds the new content
    for k in 0..4 {
        app.turn(|p| *p.y = 3.0 + k as f32);
        app.frame();
        exact(&mut app, "scrolled sampled content");
    }
}

fn centre_pt() -> (f32, f32) {
    let r = expect_rect(0.0);
    ((r[0] + r[2]) / 2.0 / S, (r[1] + r[3]) / 2.0 / S)
}

fn click(app: &mut App<Page>) -> (u32, u32) {
    let (x, y) = centre_pt();
    app.pointer_move(x, y);
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
    app.frame();
    let mut r = (0, 0);
    app.turn(|p| r = (*p.clicks, *p.under));
    r
}

#[test]
fn hit_policies() {
    // Bough: the element is a hit target and its own route runs
    let (mut app, log) = page(Mode::Under, Hit::Bough);
    assert_eq!(app.foreign_at(centre_pt().0, centre_pt().1), Some((7, Hit::Bough)));
    assert_eq!(click(&mut app), (1, 0));
    assert!(log.borrow().pointers.is_empty());
    // Pass: pointer-transparent, the panel painted under it answers
    let (mut app, log) = page(Mode::Under, Hit::Pass);
    assert_eq!(app.foreign_at(centre_pt().0, centre_pt().1), None);
    assert_eq!(click(&mut app), (0, 1));
    assert!(log.borrow().pointers.is_empty());
    // Capture: the host gets the events, no route runs
    let (mut app, log) = page(Mode::Under, Hit::Capture);
    assert_eq!(click(&mut app), (0, 0));
    assert_eq!(log.borrow().pointers, vec![(7, Pointer::Move), (7, Pointer::Down), (7, Pointer::Up)]);
    // over the badge (painted after the element) bough answers, whatever the policy
    let r = expect_rect(0.0);
    assert_eq!(app.foreign_at(r[0] / S + 50.0, r[1] / S + 35.0), None);
}

/// Two foreign elements and a dynamic one at the root: order is paint order; unmounting tells the host.
struct Two {
    show: bool,
}
impl View for Two {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0x101010)).p(10.0).gap(10.0).child(foreign(1).w(100.0).h(50.0).mode(Mode::Over)).child(if self.show { Some(foreign(2).w(100.0).h(50.0)) } else { None }).child(foreign(3).w(100.0).h(50.0))
    }
}

#[test]
fn order_is_paint_order_and_unmount_removes() {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut app = App::new(Two { show: true }, Config::new(W, H, S, font), true);
    let log = Rc::new(RefCell::new(Log::default()));
    app.set_foreign_host(Box::new(Host(log.clone())));
    app.frame();
    {
        let l = log.borrow();
        let got: Vec<(u64, u32, Plane, bool)> = l.places.iter().map(|p| (p.id, p.order, p.plane, p.covered)).collect();
        assert_eq!(got, vec![(1, 0, Plane::Above, false), (2, 1, Plane::Below, false), (3, 2, Plane::Below, false)]);
        assert_eq!(l.places[1].rect, [20.0, 140.0, 220.0, 240.0]);
    }
    let views = app.reflect_foreign();
    assert_eq!(views.iter().map(|v| v.id).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert!(views[0].source.contains("tests/foreign.rs:"), "provenance: {}", views[0].source);
    // a plain-field handler: the view re-describes, the Option part unmounts element 2
    log.borrow_mut().places.clear();
    app.turn(|v| v.show = false);
    assert!(app.frame());
    let (a, b) = (app.presented_pixels(), app.reference_pixels());
    assert!(a == b, "after the unmount the frame equals a full redraw");
    let l = log.borrow();
    assert_eq!(l.removed, vec![2]);
    let got: Vec<(u64, u32, [f32; 4])> = l.places.iter().map(|p| (p.id, p.order, p.rect)).collect();
    assert_eq!(got, vec![(3, 1, [20.0, 140.0, 220.0, 240.0])], "element 3 moved up and is now second");
}

/// A virtualized list (rows mounted by exposure, scrolled by copy) with a web view in two of its rows.
struct Row {
    n: u64,
    web: bool,
}
impl View for Row {
    fn view(&self) -> impl Element<Self> + '_ {
        let c = if self.n % 2 == 0 { rgb(0xf0f0f0) } else { rgb(0xd8d8d8) };
        row((block(30.0, 30.0).bg(rgb(0x4060c0)), self.web.then(|| foreign(self.n).w(150.0).h(30.0).rounded(6.0)))).w(300.0).h(40.0).p(5.0).gap(5.0).bg(c)
    }
}
struct Feed {
    rows: List<Row>,
    y: Signal<f32>,
}
impl View for Feed {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0x202830)).p(10.0).child(list(sig!(self.rows)).w(300.0).h(200.0).scroll(sig!(self.y)).estimate(40.0).bg(rgb(0xffffff)))
    }
}

#[test]
fn rows_of_a_virtual_list_mount_place_scroll_by_copy_and_remove() {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut rows = List::new();
    for n in 0..80u64 {
        rows.push(Row { n, web: n == 3 || n == 30 });
    }
    let mut app = App::new(Feed { rows, y: Signal::new(0.0) }, Config::new(W, H, S, font), true);
    let log = Rc::new(RefCell::new(Log::default()));
    app.set_foreign_host(Box::new(Host(log.clone())));
    app.frame();
    assert_eq!(log.borrow().places.iter().map(|p| p.id).collect::<Vec<_>>(), vec![3], "only the exposed row's web view exists");
    let r0 = log.borrow().places[0].rect;
    assert_eq!(r0, [(10.0 + 5.0 + 30.0 + 5.0) * S, (10.0 + 3.0 * 40.0 + 5.0) * S, (10.0 + 5.0 + 30.0 + 5.0 + 150.0) * S, (10.0 + 3.0 * 40.0 + 5.0 + 30.0) * S]);
    let (mut copies, mut y) = (0, 0.0f32);
    for step in 0..59 {
        y += 19.0;
        app.turn(|f| *f.y = y);
        assert!(app.frame());
        copies += app.rt.bough.scroll_rec(1).is_some() as u32;
        let (a, b) = (app.presented_pixels(), app.reference_pixels());
        let n = a.iter().zip(&b).filter(|(x, y)| x != y).count();
        assert_eq!(n, 0, "step {step}: presented differs from a full redraw in {n} bytes");
        // every placement the host holds is the element's current box
        let l = log.borrow();
        for id in [3u64, 30] {
            let gone = l.removed.contains(&id);
            if let Some(p) = l.places.iter().rev().find(|p| p.id == id).filter(|_| !gone) {
                let want_y = (10.0 + id as f32 * 40.0 + 5.0 - y) * S;
                assert_eq!(p.rect[1], want_y, "step {step}: web view {id}");
            }
        }
    }
    assert!(copies >= 50, "the list scrolled by copy ({copies} of 59 frames)");
    let l = log.borrow();
    assert_eq!(l.removed, vec![3], "row 3 left the mounted window: its content is dropped");
    assert!(l.places.iter().any(|p| p.id == 30), "row 30 was mounted on exposure and placed");
    // the hole of row 30's web view, where it is now
    let p = *l.places.iter().rev().find(|p| p.id == 30).unwrap();
    drop(l);
    if let Some(v) = p.visible() {
        let (cx, cy) = ((v[0] + v[2]) / 2, (v[1] + v[3]) / 2);
        let px = app.presented_pixels();
        let i = ((cy * (W * S) as i32 + cx) * 4) as usize;
        assert_eq!(&px[i..i + 4], &[0, 0, 0, 0], "the hole moved with its row");
    } else {
        panic!("row 30 should be in the port at y = {y}");
    }
}
