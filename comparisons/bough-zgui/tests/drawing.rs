//! Pixel gates for what zgui draws beyond flat boxes (round 3b): gradients against the CSS formula, box
//! shadows against a numerically integrated Gaussian, vector icons against resvg, a rotated image against
//! the rotated raster, bold / mono faces and display-size text against their own font's raster. Each also
//! animates (moves or turns) under every repair age 1..=4 and the windowed ring with presented == full
//! redraw after every frame, which holds only if every new ink kind's damage covers what the kernel draws.
use bough::deco::Shadow;
use bough::prelude::*;
use bough::text::TShape;
use bough_zgui::*;
use std::f64::consts::PI;

const RINGS: [usize; 5] = [1, 2, 3, 4, 0];
const W: usize = 800;
const H: usize = 600;

fn app_ring<V: View>(v: V, ring: usize) -> App<V> {
    let mut c = config(W as f32, H as f32);
    c.ring = if ring == 0 { 4 } else { ring };
    let mut a = App::new(v, c, true);
    a.windowed = ring == 0;
    a
}

fn first_diff(a: &[u8], b: &[u8]) -> Option<(usize, usize, u8, u8)> {
    a.iter().zip(b).position(|(x, y)| x != y).map(|i| ((i / 4) % W, (i / 4) / W, a[i], b[i]))
}

/// Frame after every step; presented == full redraw each time.
fn run<V: View>(name: &str, mut app: App<V>, steps: usize, mut step: impl FnMut(&mut App<V>, usize)) -> App<V> {
    let name = &format!("{name} (windowed {}, ring {})", app.windowed, app.surfaces());
    app.frame();
    for t in 0..steps {
        step(&mut app, t);
        app.frame();
        let (p, r) = (app.presented_pixels(), app.reference_pixels());
        assert!(p == r, "{name} step {t}: presented != full redraw at {:?}", first_diff(&p, &r));
    }
    app
}

/// BGRA at (x, y) of the last presented frame.
fn px(p: &[u8], x: usize, y: usize) -> [u8; 4] {
    let i = (y * W + x) * 4;
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

const fn c(r: u8, g: u8, b: u8, a: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16 | (a as u32) << 24
}

// ================================================================================== gradients

static LIN: Grad = Grad { radial: false, angle_deg: 100.0, at: [0.5, 0.5], stops: &[(0.0, c(126, 227, 255, 255)), (0.55, c(125, 139, 255, 255)), (1.0, c(209, 139, 255, 255))] };
static RAD: Grad = Grad { radial: true, angle_deg: 180.0, at: [0.3, 0.3], stops: &[(0.0, c(36, 211, 165, 255)), (0.6, c(47, 107, 255, 255)), (1.0, c(18, 22, 30, 255))] };

struct Grads {
    pos: Signal<(f32, f32)>,
}
impl View for Grads {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        (quad(0.0, 0.0, W as f32, H as f32).bg(rgb(0xffffff)), offset(sig!(self.pos), (deco_box(40.0, 40.0, 300.0, 160.0, 8.0).gradient(LIN), deco_box(400.0, 40.0, 300.0, 160.0, 8.0).gradient(RAD))))
    }
}

/// CSS: the colour at gradient position t (opaque stops: straight sRGB interpolation).
fn css_at(stops: &[(f32, u32)], t: f64) -> [f64; 3] {
    let t = t.clamp(0.0, 1.0);
    let ch = |c: u32, k: u32| ((c >> (8 * k)) & 255) as f64;
    let j = stops.iter().position(|s| s.0 as f64 >= t).unwrap_or(stops.len() - 1);
    if j == 0 {
        return [0, 1, 2].map(|k| ch(stops[0].1, k));
    }
    let (a, b) = (stops[j - 1], stops[j]);
    let f = (t - a.0 as f64) / (b.0 - a.0) as f64;
    [0, 1, 2].map(|k| ch(a.1, k) + (ch(b.1, k) - ch(a.1, k)) * f)
}

#[test]
fn gradients_follow_the_css_formula_and_move_exactly() {
    let mut app = app_ring(Grads { pos: Signal::new((0.0, 0.0)) }, 4);
    app.frame();
    let p = app.presented_pixels();
    let mut worst = 0.0f64;
    for (bx, g) in [(40.0f64, &LIN), (400.0, &RAD)] {
        let (by, w, h) = (40.0f64, 300.0f64, 160.0f64);
        let (cx, cy) = (bx + w / 2.0, by + h / 2.0);
        for y in (by as usize + 10..(by + h) as usize - 10).step_by(3) {
            for x in (bx as usize + 10..(bx + w) as usize - 10).step_by(3) {
                let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                let t = if g.radial {
                    let (gx, gy) = ((g.at[0] as f64 - 0.5) * w, (g.at[1] as f64 - 0.5) * h);
                    ((dx - gx).hypot(dy - gy)) / (gx.abs() + w / 2.0).hypot(gy.abs() + h / 2.0)
                } else {
                    let a = (g.angle_deg as f64).to_radians();
                    let (sx, sy) = (a.sin(), -a.cos());
                    (dx * sx + dy * sy) / ((w * sx).abs() + (h * sy).abs()) + 0.5
                };
                let want = css_at(g.stops, t);
                let got = px(&p, x, y);
                for k in 0..3 {
                    let d = (got[2 - k] as f64 - want[k]).abs();
                    worst = worst.max(d);
                    assert!(d <= 2.0, "gradient at ({x},{y}) t={t:.4}: channel {k} {} vs CSS {:.2}", got[2 - k], want[k]);
                }
            }
        }
    }
    eprintln!("gradients: worst channel error {worst:.2} levels");
    for ring in RINGS {
        run("gradients moving", app_ring(Grads { pos: Signal::new((0.0, 0.0)) }, ring), 24, |a, t| {
            a.turn(|v: &mut Grads| {
                v.pos.set((t as f32 * 3.7, (t % 7) as f32 * 11.3));
            });
        });
    }
}

// ================================================================================== shadows

static SH: &[Shadow] = &[Shadow { dx: 0.0, dy: 20.0, blur: 25.0, spread: -5.0, color: c(0, 0, 0, 26) }, Shadow { dx: 0.0, dy: 8.0, blur: 10.0, spread: -6.0, color: c(0, 0, 0, 26) }];
static ONE: &[Shadow] = &[Shadow { dx: 6.0, dy: 10.0, blur: 16.0, spread: 2.0, color: c(0, 0, 0, 128) }];

struct Shadows {
    pos: Signal<(f32, f32)>,
    alone: bool,
}
impl View for Shadows {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        let b = if self.alone { deco_box(200.0, 150.0, 300.0, 180.0, 12.0).shadows(ONE) } else { deco_box(200.0, 150.0, 300.0, 180.0, 12.0).bg(rgb(0xffffff)).shadows(SH) };
        (quad(0.0, 0.0, W as f32, H as f32).bg(rgb(0xffffff)), offset(sig!(self.pos), b))
    }
}

/// The Gaussian (sigma) blur of a rounded box (centre c, half size hs, corner r) at p, by summing the
/// box's coverage on a 1/4 px grid against the kernel (an independent reference for the kernel's closed
/// form).
fn blurred_rbox(p: (f64, f64), c: (f64, f64), hs: (f64, f64), r: f64, sigma: f64) -> f64 {
    let st = 0.25;
    let reach = 4.0 * sigma;
    let inside = |x: f64, y: f64| {
        let (qx, qy) = ((x - c.0).abs() - (hs.0 - r), (y - c.1).abs() - (hs.1 - r));
        qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r <= 0.0
    };
    let (x0, x1) = ((p.0 - reach).max(c.0 - hs.0), (p.0 + reach).min(c.0 + hs.0));
    let (y0, y1) = ((p.1 - reach).max(c.1 - hs.1), (p.1 + reach).min(c.1 + hs.1));
    let mut v = 0.0;
    let mut y = y0 + st / 2.0;
    while y < y1 {
        let mut x = x0 + st / 2.0;
        while x < x1 {
            if inside(x, y) {
                let d2 = (x - p.0).powi(2) + (y - p.1).powi(2);
                v += (-d2 / (2.0 * sigma * sigma)).exp();
            }
            x += st;
        }
        y += st;
    }
    v * st * st / (2.0 * PI * sigma * sigma)
}

#[test]
fn box_shadows_are_gaussian_and_their_ink_is_exact() {
    let mut app = app_ring(Shadows { pos: Signal::new((0.0, 0.0)), alone: true }, 4);
    app.frame();
    let p = app.presented_pixels();
    // the shadow box: offset (6, 10), spread 2, corner 12 + 2, sigma 8
    let (c, hs, r, sigma) = ((350.0 + 6.0, 240.0 + 10.0), (152.0, 92.0), 14.0, 8.0);
    let mut worst = 0.0f64;
    for (x, y) in (0..60).map(|i| (150 + i * 6, 250)).chain((0..40).map(|i| (356, 120 + i * 7))).chain((0..30).map(|i| (470 + i, 330 + i))) {
        let v = blurred_rbox((x as f64 + 0.5, y as f64 + 0.5), c, hs, r, sigma);
        let want = 255.0 * (1.0 - 128.0 / 255.0 * v);
        let got = px(&p, x, y)[1] as f64;
        worst = worst.max((got - want).abs());
        assert!((got - want).abs() <= 2.0, "shadow at ({x},{y}): {got} vs Gaussian {want:.2} (coverage {v:.4})");
    }
    eprintln!("shadow: worst error {worst:.2} levels");
    // nothing outside the record's ink: the whole frame is white there
    let ink = (c.0 - hs.0 - (3.0 * sigma).ceil() - 1.0, c.1 - hs.1 - (3.0 * sigma).ceil() - 1.0, c.0 + hs.0 + (3.0 * sigma).ceil() + 1.0, c.1 + hs.1 + (3.0 * sigma).ceil() + 1.0);
    for y in 0..H {
        for x in 0..W {
            let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
            if fx < ink.0 || fy < ink.1 || fx > ink.2 || fy > ink.3 {
                assert_eq!(px(&p, x, y), [255, 255, 255, 255], "pixel ({x},{y}) outside the shadow's ink was written");
            }
        }
    }
    for ring in RINGS {
        run("shadow moving", app_ring(Shadows { pos: Signal::new((0.0, 0.0)), alone: false }, ring), 30, |a, t| {
            a.turn(|v: &mut Shadows| {
                v.pos.set(((t as f32 * 7.3) % 90.0 - 40.0, (t % 5) as f32 * 9.7 - 20.0));
            });
        });
    }
}

// ================================================================================== icons and rotation

const ARROW: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><path d="M4 14 L20 14 L20 7 L30 16 L20 25 L20 18 L4 18 Z" fill="#c02020"/><rect x="2" y="2" width="6" height="6" fill="#2050c0"/></svg>"##;

struct Spin {
    a: Signal<f32>,
}
impl View for Spin {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        (quad(0.0, 0.0, W as f32, H as f32).bg(rgb(0xffffff)), svg_image(100.0, 100.0, 32.0, 32.0, ARROW), svg_image(300.0, 100.0, 32.0, 32.0, ARROW).angle(sig!(self.a)))
    }
}

/// resvg's raster of ARROW at 32 x 32 (premultiplied RGBA).
fn arrow_raster() -> Vec<u8> {
    let tree = resvg::usvg::Tree::from_str(ARROW, &resvg::usvg::Options::default()).unwrap();
    let mut pm = resvg::tiny_skia::Pixmap::new(32, 32).unwrap();
    resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pm.as_mut());
    pm.data().to_vec()
}

/// Premultiplied RGBA over white, as BGR.
fn over_white(s: &[u8]) -> [f64; 3] {
    let a = s[3] as f64 / 255.0;
    [s[2] as f64 + 255.0 * (1.0 - a), s[1] as f64 + 255.0 * (1.0 - a), s[0] as f64 + 255.0 * (1.0 - a)]
}

#[test]
fn icons_match_resvg_and_rotation_turns_the_raster() {
    let ras = arrow_raster();
    let mut app = app_ring(Spin { a: Signal::new(0.0) }, 4);
    app.frame();
    let p = app.presented_pixels();
    for y in 0..32 {
        for x in 0..32 {
            let want = over_white(&ras[(y * 32 + x) * 4..]);
            let got = px(&p, 100 + x, 100 + y);
            for k in 0..3 {
                assert!((got[k] as f64 - want[k]).abs() <= 1.0, "icon ({x},{y}) channel {k}: {} vs resvg {:.1}", got[k], want[k]);
            }
        }
    }
    // a quarter turn: destination (x, y) shows source (y, 31 - x) (clockwise about the centre)
    app.turn(|v: &mut Spin| {
        v.a.set(std::f32::consts::FRAC_PI_2);
    });
    app.frame();
    let p = app.presented_pixels();
    for y in 0..32 {
        for x in 0..32 {
            let want = over_white(&ras[((31 - x) * 32 + y) * 4..]);
            let got = px(&p, 300 + x, 100 + y);
            for k in 0..3 {
                assert!((got[k] as f64 - want[k]).abs() <= 2.0, "quarter turn ({x},{y}) channel {k}: {} vs {:.1}", got[k], want[k]);
            }
        }
    }
    // turning: each frame repaints the circumscribed square only, exactly
    for ring in RINGS {
        let app = run("spinning", app_ring(Spin { a: Signal::new(0.0) }, ring), 40, |a, t| {
            a.turn(|v: &mut Spin| {
                v.a.set(t as f32 * 0.37);
            });
        });
        assert!(app.repaired_tiles <= 9, "a 32 px image turning repairs its circumscribed square's tiles, not {}", app.repaired_tiles);
    }
}

// ================================================================================== faces and display text

static FACE_NODES: &[LNode] = &[
    LNode { depth: 0, style: 0, size: [800.0, 600.0], bg: 0xffff_ffff, border_w: 0.0, border: 0, radius: 0.0, hover: 0, text: "", font: 0.0, line_h: 0.0, color: 0, face: 0, flags: 0, tag: 0, deco: 0 },
    LNode { depth: 1, style: 0, size: [0.0, 0.0], bg: 0, border_w: 0.0, border: 0, radius: 0.0, hover: 0, text: "H", font: 40.0, line_h: 56.0, color: 0xff00_0000, face: 1, flags: 8, tag: 0, deco: 0 },
    LNode { depth: 1, style: 0, size: [0.0, 0.0], bg: 0, border_w: 0.0, border: 0, radius: 0.0, hover: 0, text: "M", font: 30.0, line_h: 42.0, color: 0xff00_0000, face: 2, flags: 8, tag: 0, deco: 0 },
    LNode { depth: 1, style: 0, size: [0.0, 0.0], bg: 0, border_w: 0.0, border: 0, radius: 0.0, hover: 0, text: "A", font: 120.0, line_h: 140.0, color: 0xff00_0000, face: 0, flags: 8, tag: 0, deco: 0 },
];
static FACE_DECLS: &[Decls] = &[&[(P::Display, "flex"), (P::FlexDirection, "column"), (P::PadL, "20px"), (P::PadT, "10px")]];

struct Faces;
impl View for Faces {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        ltemplate(FACE_NODES, FACE_DECLS, [-1.0, -1.0], |_: &Faces, _, _| LHooked::Default)
    }
}

fn font_file(var: &str, file: &str) -> Vec<u8> {
    let dir = "/Library/Fonts/Nix Fonts/vl9mbzk97wmh1s1yj3766b4zai8msjq6-dejavu-fonts-2.37/share/fonts/truetype";
    std::fs::read(std::env::var(var).unwrap_or_else(|_| format!("{dir}/{file}"))).expect("DejaVu face")
}

/// swash's alpha raster of `ch` in `font` at `size` px: (left, top, w, h, alpha).
fn raster(font: &[u8], ch: char, size: f32) -> (i32, i32, usize, usize, Vec<u8>) {
    use swash::scale::{image::Content, Render, ScaleContext, Source};
    use swash::zeno::Format;
    let f = swash::FontRef::from_index(font, 0).unwrap();
    let gid = f.charmap().map(ch);
    let mut ctx = ScaleContext::new();
    let mut sc = ctx.builder(f).size(size).hint(false).build();
    let img = Render::new(&[Source::Outline]).format(Format::Alpha).render(&mut sc, gid).unwrap();
    assert_eq!(img.content, Content::Mask);
    (img.placement.left, img.placement.top, img.placement.width as usize, img.placement.height as usize, img.data)
}

#[test]
fn faces_and_display_text_draw_their_own_font() {
    let mut app = app_ring(Faces, 4);
    app.frame();
    let p = app.presented_pixels();
    let regular = font_bytes();
    let bold = font_file("BOUGH_FONT_BOLD", "DejaVuSans-Bold.ttf");
    let mono = font_file("BOUGH_FONT_MONO", "DejaVuSansMono.ttf");
    for (ch, font, size) in [('H', &bold, 40.0f32), ('M', &mono, 30.0), ('A', &regular, 120.0)] {
        let ti = app.rt.texts.iter().position(|t| t.text == ch.to_string()).expect("text node");
        // where the GPU drew the glyph's bitmap: its instance (general path) or its run's first slot
        let (gx, gy) = match &app.rt.texts[ti].shape {
            TShape::Glyphs { run } => {
                let ins = app.rt.inst[app.rt.runs[*run as usize].first as usize];
                let o = app.rt.space(ins.space);
                ((ins.pos[0] + o[0]) as i32, (ins.pos[1] + o[1]) as i32)
            }
            TShape::Lines { first, .. } => {
                let h = app.rt.glyphs.runs[*first as usize];
                let sp = app.rt.glyphs.spaces[h.space as usize];
                let e = app.rt.text.cache.tok(h.strike as usize).entry[ch as usize];
                ((h.origin[0] + 0.5).floor() as i32 + e.geo[0] as i32 + sp[0], h.origin[1] as i32 + e.geo[1] as i32 + sp[1])
            }
            _ => panic!("unexpected text shape"),
        };
        let (_, _, w, h, a) = raster(font, ch, size);
        // the face matters: the regular face's raster differs
        if !std::ptr::eq(font, &regular) {
            let r = raster(&regular, ch, size);
            assert!(r.4 != a, "{ch}: the test font differs from the regular face");
        }
        let mut worst = 0i32;
        for y in 0..h {
            for x in 0..w {
                let want = 255 - a[y * w + x] as i32;
                let got = px(&p, (gx + x as i32) as usize, (gy + y as i32) as usize)[1] as i32;
                worst = worst.max((got - want).abs());
                assert!((got - want).abs() <= 1, "{ch} ({size} px) at ({x},{y}): {got} vs its font's raster {want}");
            }
        }
        eprintln!("{ch} {size} px: {w}x{h} glyph, worst {worst}");
    }
}
