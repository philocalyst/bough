//! Built-in transitions (round 4): `sig!(..).transition(curve)` is an operand like any other, so a colour, a
//! quad's width and a laid-out box's flex basis transition with no code of their own. Gates: mid-transition
//! pixels at ring ages 1..=4 and windowed equal both a full redraw and the CPU-evaluated curve (the same f32
//! ops); a retarget is value-continuous (and, smooth, velocity-continuous); a layout property relayouts every
//! frame to the eased value.
use bough::prelude::*;
use bough::transition::{Curve, Ease, Lerp};
use bough_zgui::*;
use cephal::style::Dimension;

const W: usize = 400;
const H: usize = 200;
const RINGS: [usize; 5] = [1, 2, 3, 4, 0];
const DT: f32 = 1.0 / 60.0;
/// Transitions are numbered in lowering order: a quad lowers its colour, then its width.
const WIDTH_ID: u32 = 1;
const C0: Color = Color::rgba(200, 40, 30, 255);
const C1: Color = Color::rgba(20, 90, 230, 255);

struct Tr {
    bg: Signal<Color>,
    w: Signal<f32>,
    smooth: bool,
}
impl View for Tr {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        let curve = if self.smooth { Curve::smooth(0.4) } else { Curve::out(0.3) };
        (
            quad(0.0, 0.0, W as f32, H as f32).bg(rgb(0xffffff)),
            quad(20.0, 20.0, 100.0, 60.0).width(sig!(self.w).transition(curve)).bg(sig!(self.bg).transition(Curve::linear(0.25))),
        )
    }
}

fn app<V: View>(v: V, ring: usize) -> App<V> {
    let mut c = config(W as f32, H as f32);
    c.ring = if ring == 0 { 4 } else { ring };
    let mut a = App::new(v, c, true);
    a.windowed = ring == 0;
    a
}

fn px(p: &[u8], x: usize, y: usize) -> [u8; 4] {
    let i = (y * W + x) * 4;
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

fn exact<V: View>(a: &mut App<V>, what: &str) {
    let (p, r) = (a.presented_pixels(), a.reference_pixels());
    assert!(p == r, "{what}: presented != full redraw");
}

#[test]
fn colour_and_width_mid_transition_equal_the_curve_at_every_ring_age() {
    for ring in RINGS {
        let mut a = app(Tr { bg: Signal::new(C0), w: Signal::new(100.0), smooth: false }, ring);
        a.frame();
        a.turn(|v: &mut Tr| {
            v.bg.set(C1);
            v.w.set(260.0);
        });
        a.frame_paced();
        let t0 = a.rt.transitions.now;
        let mut now = t0;
        for k in 1..=24 {
            a.tick(DT);
            now += DT as f64;
            a.frame_paced();
            exact(&mut a, &format!("ring {ring} frame {k}"));
            let p = a.presented_pixels();
            // the colour: linear over 0.25 s, the same ops as the runtime's
            let pc = ((now - t0) / 0.25f32 as f64) as f32;
            let want = if pc >= 1.0 { C1 } else { Color::lerp(C0, C1, Ease::Linear.at(pc)) };
            let got = px(&p, 25, 50);
            assert_eq!([got[2], got[1], got[0]], [want.0 as u8, (want.0 >> 8) as u8, (want.0 >> 16) as u8], "ring {ring} frame {k}: colour at progress {pc}");
            // the width: ease-out cubic over 0.3 s; the quad covers the pixel centres in [20, 20 + w)
            let pw = ((now - t0) / 0.3f32 as f64) as f32;
            let w = if pw >= 1.0 { 260.0 } else { f32::lerp(100.0, 260.0, Ease::OutCubic.at(pw)) };
            let x1 = bough::rt::snap(20.0) + bough::rt::snap(w);
            let last = (x1 - 0.5).ceil() as usize - 1;
            assert_ne!(px(&p, last, 50), [255, 255, 255, 255], "ring {ring} frame {k}: column {last} inside the width {w}");
            assert_eq!(px(&p, last + 1, 50), [255, 255, 255, 255], "ring {ring} frame {k}: column {} past the width {w}", last + 1);
        }
        assert!(!a.animating(), "the transitions ended");
    }
}

#[test]
fn a_retarget_is_continuous() {
    for smooth in [false, true] {
        let mut a = app(Tr { bg: Signal::new(C0), w: Signal::new(100.0), smooth }, 4);
        a.frame();
        a.turn(|v: &mut Tr| {
            v.w.set(300.0);
        });
        a.frame_paced();
        let mut hist = Vec::new();
        for _ in 0..10 {
            a.tick(DT);
            a.frame_paced();
            hist.push(a.rt.transitions.shown::<f32>(WIDTH_ID).unwrap());
        }
        let before = *hist.last().unwrap();
        let d_before = before - hist[hist.len() - 2];
        // retarget mid-flight: back to 50
        a.turn(|v: &mut Tr| {
            v.w.set(50.0);
        });
        a.frame_paced();
        exact(&mut a, "retarget frame");
        assert_eq!(a.rt.transitions.shown::<f32>(WIDTH_ID).unwrap(), before, "smooth {smooth}: the value right after the retarget is the value before it");
        a.tick(DT);
        a.frame_paced();
        exact(&mut a, "first frame after the retarget");
        let after = a.rt.transitions.shown::<f32>(WIDTH_ID).unwrap();
        let d_after = after - before;
        if smooth {
            // velocity continuity: the step after the retarget continues the step before it
            assert!((d_after - d_before).abs() <= 0.15 * d_before.abs() + 0.05, "smooth: step {d_before} then {d_after}");
        } else {
            // value continuity: no jump larger than a frame's motion toward the new target
            assert!(d_after < 0.0 && d_after.abs() <= (300.0 - 50.0) * 3.0 * DT / 0.3, "out: step {d_before} then {d_after}");
        }
        for _ in 0..40 {
            a.tick(DT);
            a.frame_paced();
        }
        assert_eq!(a.rt.transitions.shown::<f32>(WIDTH_ID).unwrap(), 50.0, "smooth {smooth}: lands on the new target");
    }
}

// ---------------------------------------------------------------------------------- a layout property

static NODES: &[LNode] = &[
    LNode { depth: 0, style: 0, size: [400.0, 200.0], bg: 0xffff_ffff, border_w: 0.0, border: 0, radius: 0.0, hover: 0, text: "", font: 0.0, line_h: 0.0, color: 0, face: 0, flags: 0, tag: 0, deco: 0 },
    LNode { depth: 1, style: 1, size: [0.0, 0.0], bg: 0xff30_8020, border_w: 0.0, border: 0, radius: 0.0, hover: 0, text: "", font: 0.0, line_h: 0.0, color: 0, face: 0, flags: 4, tag: 1, deco: 0 },
    LNode { depth: 1, style: 1, size: [0.0, 0.0], bg: 0xff20_30c0, border_w: 0.0, border: 0, radius: 0.0, hover: 0, text: "", font: 0.0, line_h: 0.0, color: 0, face: 0, flags: 0, tag: 0, deco: 0 },
];
static DECLS: &[Decls] = &[&[(P::Display, "flex"), (P::FlexDirection, "row"), (P::Height, "200px")], &[(P::FlexBasis, "100px"), (P::FlexShrink, "0"), (P::Height, "80px")]];

struct Lay {
    basis: Signal<f32>,
}
impl View for Lay {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        ltemplate(NODES, DECLS, [-1.0, -1.0], |v: &Lay, m, ctx| {
            if ctx.node.tag == 1 {
                let (lid, _) = ctx.mount_default(m);
                wire_restyle(m, sig!(v: Lay => basis).transition(Curve::linear(0.2)), lid, |b: &f32, st: &mut cephal::Style| st.flex_basis = Dimension::length(*b));
            }
            LHooked::Default
        })
    }
}

#[test]
fn a_layout_property_relayouts_every_frame() {
    for ring in RINGS {
        let mut a = app(Lay { basis: Signal::new(100.0) }, ring);
        a.frame();
        a.turn(|v: &mut Lay| {
            v.basis.set(220.0);
        });
        a.frame_paced();
        let t0 = a.rt.transitions.now;
        let mut now = t0;
        for k in 1..=14 {
            a.tick(DT);
            now += DT as f64;
            a.frame_paced();
            exact(&mut a, &format!("layout ring {ring} frame {k}"));
            let p = a.presented_pixels();
            let pb = ((now - t0) / 0.2f32 as f64) as f32;
            let b = if pb >= 1.0 { 220.0 } else { f32::lerp(100.0, 220.0, pb) };
            // the first box spans [0, b): its colour up to b, the second box's after (the row relayouts)
            let edge = (bough::rt::snap(b) - 0.5).ceil() as usize - 1;
            let (l, r) = (px(&p, edge, 40), px(&p, edge + 1, 40));
            assert_eq!([l[2], l[1], l[0]], [0x20, 0x80, 0x30], "ring {ring} frame {k}: column {edge} is the first box (basis {b})");
            assert_eq!([r[2], r[1], r[0]], [0xc0, 0x30, 0x20], "ring {ring} frame {k}: column {} is the second box", edge + 1);
        }
    }
}
