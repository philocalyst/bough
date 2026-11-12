//! Transformed subtrees (round 3c): a box with a CSS transform is mounted with its subtree in a space of its
//! own whose affine (about the box's centre) the kernels apply to every record in it (quads, decorations,
//! glyph runs and instances); each record's ink is the transformed bounding box, so damage stays exact.
//!
//! Gates: a quarter turn of a laid-out card with text equals the unturned frame turned on the CPU (a pixel
//! permutation: the pivot is chosen so pixel centres map to pixel centres); a 30 degree turn covers exactly
//! the turned rounded rect (pixels clearly inside are the card's colour, clearly outside untouched); and the
//! turned and translated cards scroll and relayout at ring ages 1..=4 and windowed with presented == full
//! redraw after every frame.
use bough::deco::Xf;
use bough::raw::*;
use bough_zgui::*;

const W: usize = 600;
const H: usize = 400;

const fn rot(deg: f32) -> [f32; 4] {
    // const-friendly: the three angles the gates use
    if deg == 90.0 {
        [0.0, -1.0, 1.0, 0.0]
    } else if deg == 30.0 {
        [0.866_025_4, -0.5, 0.5, 0.866_025_4]
    } else {
        [1.0, 0.0, 0.0, 1.0]
    }
}

static NODES: &[LNode] = &[
    // root: a column with padding; the card is 200 x 120 at (40, 40): its centre (140, 100) is integral
    LNode { depth: 0, style: 0, size: [600.0, 400.0], bg: 0xffff_ffff, border_w: 0.0, border: 0, radius: 0.0, hover: 0, text: "", font: 0.0, line_h: 0.0, color: 0, face: 0, flags: 0, tag: 0, deco: 0 },
    LNode { depth: 1, style: 1, size: [200.0, 120.0], bg: 0xffd0_6020, border_w: 0.0, border: 0, radius: 12.0, hover: 0, text: "", font: 0.0, line_h: 0.0, color: 0, face: 0, flags: 0, tag: 0, deco: 1 },
    LNode { depth: 2, style: 2, size: [0.0, 0.0], bg: 0, border_w: 0.0, border: 0, radius: 0.0, hover: 0, text: "", font: 0.0, line_h: 0.0, color: 0, face: 0, flags: 0, tag: 0, deco: 0 },
    LNode { depth: 3, style: 0, size: [0.0, 0.0], bg: 0, border_w: 0.0, border: 0, radius: 0.0, hover: 0, text: "Hi Ag", font: 30.0, line_h: 40.0, color: 0xff10_1010, face: 1, flags: 8, tag: 0, deco: 0 },
    LNode { depth: 2, style: 3, size: [40.0, 20.0], bg: 0xff20_40f0, border_w: 0.0, border: 0, radius: 0.0, hover: 0, text: "", font: 0.0, line_h: 0.0, color: 0, face: 0, flags: 0, tag: 0, deco: 0 },
    // a translated badge (translateX(-50%) of its own width) below the card
    LNode { depth: 1, style: 4, size: [80.0, 24.0], bg: 0xff30_a030, border_w: 0.0, border: 0, radius: 6.0, hover: 0, text: "", font: 0.0, line_h: 0.0, color: 0, face: 0, flags: 0, tag: 0, deco: 2 },
];
static DECLS: &[Decls] = &[
    &[(P::Display, "flex"), (P::FlexDirection, "column"), (P::PadL, "40px"), (P::PadT, "40px"), (P::RowGap, "40px")],
    &[(P::Display, "flex"), (P::FlexDirection, "column"), (P::Width, "200px"), (P::Height, "120px"), (P::PadL, "16px"), (P::PadT, "16px"), (P::RowGap, "8px"), (P::FlexShrink, "0")],
    &[(P::Display, "block")],
    &[(P::Width, "40px"), (P::Height, "20px")],
    &[(P::Width, "80px"), (P::Height, "24px"), (P::MarL, "100px")],
];
const fn decos(deg: f32) -> [Deco; 3] {
    [
        Deco::NONE,
        Deco { grad: None, shadows: &[], svg: "", xform: Some(Xf { m: rot(deg), t: [0.0, 0.0], pct: [false, false] }) },
        Deco { grad: None, shadows: &[], svg: "", xform: Some(Xf { m: [1.0, 0.0, 0.0, 1.0], t: [-0.5, 0.0], pct: [true, false] }) },
    ]
}
static D0: [Deco; 3] = decos(0.0);
static D90: [Deco; 3] = decos(90.0);
static D30: [Deco; 3] = decos(30.0);

struct Card {
    deg: u32,
    scroll: Signal<f32>,
}
impl View for Card {
    fn view(&self) -> impl Element<Self> + '_ {
        let t: &'static [Deco] = match self.deg {
            90 => &D90,
            30 => &D30,
            _ => &D0,
        };
        scroll_y(sig!(self.scroll), [0.0, 0.0, W as f32, H as f32], 400.0, ltemplate(NODES, DECLS, [-1.0, f32::NAN], |_: &Card, _, _| LHooked::Default).decos(t)).bg(rgb(0xffffff)).line(20.0)
    }
}

fn app(deg: u32, ring: usize) -> App<Card> {
    let mut c = config(W as f32, H as f32);
    c.ring = if ring == 0 { 4 } else { ring };
    let mut a = App::new(Card { deg, scroll: Signal::new(0.0) }, c, true);
    a.windowed = ring == 0;
    a
}

fn px(p: &[u8], x: usize, y: usize) -> [u8; 4] {
    let i = (y * W + x) * 4;
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

#[test]
fn a_quarter_turn_is_the_cpu_turned_frame() {
    let mut a0 = app(0, 4);
    a0.frame();
    let p0 = a0.presented_pixels();
    let mut a9 = app(90, 4);
    a9.frame();
    let p9 = a9.presented_pixels();
    // pivot K = (140, 100): the turned pixel (x, y) shows the unturned pixel (K.x + (y - K.y), K.y - (x - K.x) - 1)
    let (kx, ky) = (140i32, 100i32);
    let mut worst = 0i32;
    let mut card = 0;
    for y in (ky - 100)..(ky + 100) {
        for x in (kx - 64)..(kx + 64) {
            let (ux, uy) = (kx + (y - ky), ky - (x - kx) - 1);
            if !(0..W as i32).contains(&ux) || !(0..H as i32).contains(&uy) || !(0..W as i32).contains(&x) || !(0..H as i32).contains(&y) {
                continue;
            }
            let (a, b) = (px(&p9, x as usize, y as usize), px(&p0, ux as usize, uy as usize));
            if b != [255, 255, 255, 255] {
                card += 1;
            }
            for k in 0..3 {
                let d = (a[k] as i32 - b[k] as i32).abs();
                worst = worst.max(d);
                assert!(d <= 2, "turned ({x},{y}) channel {k}: {} vs unturned ({ux},{uy}) {}", a[k], b[k]);
            }
        }
    }
    assert!(card > 20_000, "the card, its text and its box were compared ({card} non-background pixels)");
    eprintln!("quarter turn: worst {worst} levels over {card} card pixels");
}

#[test]
fn a_thirty_degree_turn_covers_the_turned_rect() {
    let mut a = app(30, 4);
    a.frame();
    let p = a.presented_pixels();
    let (kx, ky) = (140.0f32, 100.0f32);
    let (c, s) = (30f32.to_radians().cos(), 30f32.to_radians().sin());
    let (mut inside, mut outside) = (0, 0);
    for y in 0..H {
        for x in 0..W {
            // the pixel centre in the card's unturned frame (M^-1 = rotation by -30 degrees)
            let (dx, dy) = (x as f32 + 0.5 - kx, y as f32 + 0.5 - ky);
            let (ux, uy) = (c * dx + s * dy, -s * dx + c * dy);
            // signed distance to the 200 x 120 rounded rect (radius 12) about the pivot
            let (qx, qy) = (ux.abs() - (100.0 - 12.0), uy.abs() - (60.0 - 12.0));
            let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - 12.0;
            let got = px(&p, x, y);
            if d < -1.5 && d > -12.0 {
                // inside, in the band no child reaches (the children sit 16 px in)
                inside += 1;
                assert_eq!([got[2], got[1], got[0]], [0x20, 0x60, 0xd0], "({x},{y}) {d:.2} px inside the turned card");
            } else if d > 1.5 && y < 196 {
                outside += 1;
                assert_eq!(got, [255, 255, 255, 255], "({x},{y}) {d:.2} px outside the turned card was written");
            }
        }
    }
    assert!(inside > 2_000 && outside > 50_000, "{inside} / {outside}");
}

#[test]
fn turned_and_translated_subtrees_scroll_and_relayout_exactly() {
    for (deg, ring) in [(30, 1), (30, 2), (30, 3), (30, 4), (30, 0), (90, 4)] {
        let mut a = app(deg, ring);
        let name = format!("turn {deg} (windowed {}, ring {})", a.windowed, a.surfaces());
        a.frame();
        for t in 0..36 {
            match t {
                0..=11 => {
                    a.wheel(300.0, 200.0, 0.0, if t % 4 < 2 { 1.0 } else { -1.0 });
                }
                12..=23 => {
                    let _ = a.tick(0.05);
                }
                24 => a.resize(520.0, 360.0),
                28 => a.resize(600.0, 400.0),
                _ => {
                    a.turn(|v: &mut Card| {
                        v.scroll.set((t as f32 * 7.0) % 60.0);
                    });
                }
            }
            a.frame();
            let (p, r) = (a.presented_pixels(), a.reference_pixels());
            let w = a.surface_width();
            let (cw, ch) = (a.rt.win_w as usize, a.rt.win_h as usize);
            for y in 0..ch {
                let (s0, s1) = (&p[y * w * 4..y * w * 4 + cw * 4], &r[y * w * 4..y * w * 4 + cw * 4]);
                assert!(s0 == s1, "{name} step {t}: presented != full redraw in row {y}");
            }
        }
    }
}
