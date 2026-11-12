//! The CPU executor as the only executor (`App::target`; where no GPU presenter exists, every platform but
//! macOS): retained frames equal a full redraw at every ring age, through a scroll (copy and repair), hover
//! (paint-only frames: surfaces held), and structural changes; and on x86-64 the AVX2 + F16C tile loop writes the
//! same bytes as the scalar one (`BOUGH_CPU_SCALAR`).
use bough::backend::MemSurfaces;
use bough::prelude::*;
use bough::raw::{App, Config};

struct Scene {
    y: Signal<f32>,
    n: Signal<u32>,
}

impl View for Scene {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).scroll(sig!(self.y)).child(each((0..60 + *self.n as usize).map(|i| {
            row((text(format!("Row {i:05}")).color(rgb(0x1f2328)), text(format!("{}", i * 7919 % 10000)).color(rgb(0x6e7781))))
                .w_pct(1.0)
                .h(32.0)
                .shrink(0.0)
                .px(12.0)
                .items(Align::Center)
                .justify(Justify::SpaceBetween)
                .bg(rgb(if i % 2 == 0 { 0xffffff } else { 0xf6f8fa }))
                .rounded(if i % 3 == 0 { 4.0 } else { 0.0 })
                .hover_bg(rgb(0xcfe3ff))
        })))
    }
}

fn font() -> Vec<u8> {
    bough::ui::default_font()
}

fn app(ring: usize) -> App<Scene> {
    let mut c = Config::new(640.0, 480.0, 1.0, font());
    c.ring = ring;
    let mut a = App::new(Scene { y: Signal::new(0.0), n: Signal::new(0) }, c, false);
    // the CPU executor on offscreen surfaces, as a window host's would be
    a.target = Some(Box::new(MemSurfaces::new(a.rt.win_w as u32, a.rt.win_h as u32, ring)));
    a
}

fn run(ring: usize) -> Vec<u8> {
    let mut a = app(ring);
    assert!(a.frame());
    let check = |a: &mut App<Scene>, what: &str| {
        let (px, r) = (a.presented_pixels(), a.reference_pixels());
        let bad = px.iter().zip(&r).filter(|(x, y)| x != y).count();
        if bad != 0 {
            let w = a.rt.win_w as usize;
            let i = px.iter().zip(&r).position(|(x, y)| x != y).unwrap() / 4;
            let last = px.iter().zip(&r).rposition(|(x, y)| x != y).unwrap() / 4;
            panic!("ring {ring}, {what}: {bad} bytes differ from a full redraw; first ({}, {}) {:?} vs {:?}; last ({}, {})", i % w, i / w, &px[i * 4..i * 4 + 4], &r[i * 4..i * 4 + 4], last % w, last / w);
        }
    };
    for step in 0..12 {
        a.wheel_points(320.0, 200.0, if step % 4 == 3 { -40.0 } else { 17.0 });
        a.frame();
        check(&mut a, &format!("scroll {step}"));
        a.pointer_move(300.0, 40.0 + 13.0 * step as f32);
        a.frame();
        check(&mut a, &format!("hover {step}"));
    }
    a.turn(|s| {
        s.n.set(7);
    });
    a.frame();
    check(&mut a, "rows added");
    // the x86-64-v3 twin (AVX2 + F16C spans and store) against the scalar loop: the same bytes
    let fast = a.reference_pixels();
    a.cpu.scalar = true;
    let slow = a.reference_pixels();
    a.cpu.scalar = false;
    assert!(fast == slow, "ring {ring}: the vector tile loop differs from the scalar one");
    a.presented_pixels()
}

#[test]
fn retained_equals_full_redraw_at_every_age() {
    let last: Vec<Vec<u8>> = [2, 3, 4, 1].into_iter().map(run).collect();
    // every ring size ends on the same state, so on the same pixels
    for (k, p) in last.iter().enumerate().skip(1) {
        assert!(*p == last[0], "ring {} ends on other pixels than ring 1", k + 1);
    }
}
