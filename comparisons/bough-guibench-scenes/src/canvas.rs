//! The design canvas to the gui-bench spec (`spec::canvas`), in bough as an application author writes it.
//!
//! * Chrome (toolbar, layers panel, inspector) is flow layout: `row` / `column` / `div`, a `.scroll(..)` box for
//!   the layers panel (all 525 rows are mounted), `.on_hover` on the component rows, `.num(..)` for the inspector's
//!   x, and an `.on_key` route on the root for `t` (theme) and `m` (move the selected node).
//! * The canvas draws raw elements at window coordinates, as the spec's geometry is in window coordinates: the
//!   frames (a transparent `deco_box` for the drop shadow under a `quad` whose fill follows the theme), their
//!   titles, a static `clip` per frame and a nested `clip` per group, and the nodes (`deco_box` with box shadows
//!   and gradients, bordered `quad`s, component instances as `quad`s whose fill reads the `look` signal).
//! * Things that must change with the theme or move are flow boxes mounted among the raw elements in the canvas
//!   box: the labels (a `text` with a colour operand, `.abs`) and the selected node (`div().abs(..)` with a
//!   `translate` operand on `sel_x`, and its outline).
//! * One signal, `look`, packs the theme bit and the hovered component: each instance's fill, each component
//!   row's background, the swatches, the frames' fill, the chrome and the labels read it through `.map`.
//!
//! The view is described by the owner and re-described after every handler (the application mode, no
//! `Config::once`): the nodes are a heterogeneous list, so each is mounted through [`El`].
use crate::spec::{self, canvas as sc};
use bough::prelude::*;
use bough::raw::{clip, deco_box, quad, text as rtext, Caps, Ev, Grad, Mounter, NumFmt, Shadow};

const DARK: u32 = 0x100;
const NO_HOT: u32 = 0xff;
const CANVAS_X: f32 = sc::PANEL_W;
const CANVAS_Y: f32 = sc::TOOLBAR_H;
const INSPECTOR_W: f32 = 1280.0 - sc::INSPECTOR_X;
/// A whole number.
const WHOLE: NumFmt = NumFmt::new("", None, "");

fn theme(l: u32) -> usize {
    (l & DARK != 0) as usize
}
fn hot(l: u32) -> usize {
    (l & 0xff) as usize
}
fn chrome(l: &u32) -> Color {
    rgb(sc::CHROME[theme(*l)])
}
fn ink(l: &u32) -> Color {
    rgb(sc::INK[theme(*l)])
}
fn canvas_bg(l: &u32) -> Color {
    rgb(sc::CANVAS_BG[theme(*l)])
}
fn frame_bg(l: &u32) -> Color {
    rgb(sc::FRAME_BG[theme(*l)])
}
fn inst_fill<const K: usize>(l: &u32) -> Color {
    rgb(sc::comp(theme(*l), K, hot(*l) == K))
}
fn swatch_fill<const K: usize>(l: &u32) -> Color {
    rgb(sc::comp(theme(*l), K, false))
}
fn row_bg<const K: usize>(l: &u32) -> Color {
    if hot(*l) == K { rgb(sc::ROW_HOT[theme(*l)]) } else { Color(0) }
}
// one projection per component: a `fn` pointer is `Copy` and nameable, so the instances share one operand type
const INST: [fn(&u32) -> Color; 5] = [inst_fill::<0>, inst_fill::<1>, inst_fill::<2>, inst_fill::<3>, inst_fill::<4>];
const SWATCH: [fn(&u32) -> Color; 5] = [swatch_fill::<0>, swatch_fill::<1>, swatch_fill::<2>, swatch_fill::<3>, swatch_fill::<4>];
const ROW_BG: [fn(&u32) -> Color; 5] = [row_bg::<0>, row_bg::<1>, row_bg::<2>, row_bg::<3>, row_bg::<4>];

/// `0xaarrggbb` / `0xrrggbb` + alpha as bough's RGBA8 word (red in the low byte).
const fn abgr(rgb: u32, a: u32) -> u32 {
    ((rgb >> 16) & 0xff) | (rgb & 0xff00) | ((rgb & 0xff) << 16) | (a << 24)
}
fn col(hex: u32, a: u32) -> Color {
    Color(abgr(hex, a))
}

/// Box shadows are `&'static [Shadow]`: the spec's blur radius 2..=40 with its offset (blur / 4, rounded), one
/// entry each.
const fn blur_shadows() -> [Shadow; 39] {
    let mut a = [Shadow { dx: 0.0, dy: 0.0, blur: 0.0, spread: 0.0, color: 0 }; 39];
    let mut i = 0;
    while i < 39 {
        let b = 2 + i as u32;
        a[i] = Shadow { dx: 0.0, dy: ((b + 2) / 4) as f32, blur: b as f32, spread: 0.0, color: abgr(0, 0x4d) };
        i += 1;
    }
    a
}
static BLUR_SHADOWS: [Shadow; 39] = blur_shadows();
static FRAME_SHADOW: [Shadow; 1] = [Shadow { dx: 0.0, dy: sc::FRAME_SHADOW.0, blur: sc::FRAME_SHADOW.1, spread: 0.0, color: abgr(0, 0x40) }];

/// Gradient stops are `&'static`: one pair per (top, bottom) of the 8 x 8 palette.
const fn stop_table() -> [[(f32, u32); 2]; 64] {
    let mut t = [[(0.0, 0); 2]; 64];
    let mut i = 0;
    while i < 64 {
        t[i] = [(0.0, abgr(sc::PALETTE[i / 8], 0xff)), (1.0, abgr(sc::PALETTE[i % 8], 0xff))];
        i += 1;
    }
    t
}
static STOPS: [[(f32, u32); 2]; 64] = stop_table();

fn palette_ix(c: u32) -> usize {
    sc::PALETTE.iter().position(|&p| p == c).unwrap_or(0)
}

pub struct Canvas {
    nodes: usize,
    per: usize,
    /// Theme bit (`DARK`) | the component whose layers row is hovered (`NO_HOT`: none).
    look: Signal<u32>,
    layers: Signal<f32>,
    /// The selected node's x within frame 0 (`MOVE_A` or `MOVE_B`).
    sel_x: Signal<f32>,
}

impl Canvas {
    pub fn new(nodes: usize) -> Canvas {
        let per = (nodes / sc::FRAMES).max(5);
        Canvas { nodes: per * sc::FRAMES, per, look: Signal::new(NO_HOT), layers: Signal::new(0.0), sel_x: Signal::new(sc::MOVE_A) }
    }

    pub fn caps(&self) -> Caps {
        let rows = sc::COMPONENTS + sc::FRAMES * (1 + sc::LAYER_ROWS);
        // a node: up to a shadow and its box; labels are text; the rows and the frames' titles are glyphs
        let instances = (self.nodes * 3 + rows * 16 + sc::FRAMES * 16 + 4096).next_power_of_two();
        let records = self.nodes * 3 + rows * 4 + 4096;
        Caps {
            instances: instances.min(Caps::RESERVE.instances),
            pages: (records / 32 + 1024).next_power_of_two().min(Caps::RESERVE.pages),
            glyphs: ((self.nodes / 5 * 6 + rows * 14) * 2 + 65536).next_power_of_two().min(Caps::RESERVE.glyphs),
            runs: (self.nodes * 3 + rows * 3 + 8192).next_power_of_two().min(Caps::RESERVE.runs),
            ..Caps::DEFAULT
        }
    }

    fn hover_comp(&mut self, k: usize, on: bool) {
        let l = *self.look;
        let next = if on { (l & !0xff) | k as u32 } else if hot(l) == k { (l & !0xff) | NO_HOT } else { l };
        self.look.set(next);
    }

    fn key(&mut self, e: &Ev) {
        match e.key {
            Some(Key::Char(c)) if c.to_string() == spec::THEME_KEY => {
                let l = *self.look ^ DARK;
                self.look.set(l);
            }
            Some(Key::Char(c)) if c.to_string() == sc::MOVE_KEY => {
                let x = if *self.sel_x == sc::MOVE_A { sc::MOVE_B } else { sc::MOVE_A };
                self.sel_x.set(x);
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------------------------------------ nodes

/// Object-safe mounting of any element of this view (`Element` itself is not object safe: its `seal` is generic).
trait Mount {
    fn mount_box(self: Box<Self>, m: &mut Mounter<'_, Canvas>);
    fn update_box(self: Box<Self>, m: &mut Mounter<'_, Canvas>);
}
impl<E: Element<Canvas>> Mount for E {
    fn mount_box(self: Box<Self>, m: &mut Mounter<'_, Canvas>) {
        (*self).mount(m)
    }
    fn update_box(self: Box<Self>, m: &mut Mounter<'_, Canvas>) {
        (*self).update(m)
    }
}

/// A node of any kind, as one element type (an `each` takes one item type; the nodes are of many).
struct El<'a>(Box<dyn Mount + 'a>);

fn el<'a>(e: impl Element<Canvas> + 'a) -> El<'a> {
    El(Box::new(e))
}

impl Element<Canvas> for El<'_> {
    fn mount(self, m: &mut Mounter<'_, Canvas>) {
        self.0.mount_box(m)
    }
    fn update(self, m: &mut Mounter<'_, Canvas>) {
        self.0.update_box(m)
    }
}

impl Canvas {
    /// Node `i` of frame `f`: its rect is frame-relative, or group-relative inside a clip of the group's rect.
    fn node_el(&self, f: usize, i: usize, fx: f32, fy: f32) -> El<'_> {
        let n = sc::node(f, i, self.per);
        let (ox, oy, group) = match n.group {
            Some(q) => {
                let (gx, gy, gw, gh) = sc::group_rect(q);
                (fx + gx, fy + gy, Some((fx + gx, fy + gy, gw, gh)))
            }
            None => (fx, fy, None),
        };
        let (x, y, w, h) = (ox + n.rect.0, oy + n.rect.1, n.rect.2, n.rect.3);
        let inner = match n.kind {
            sc::Kind::Shadowed { radius, fill, blur, .. } => {
                let s = &BLUR_SHADOWS[blur as usize - 2..blur as usize - 1];
                el(deco_box(x, y, w, h, radius).bg(rgb(fill)).shadows(s))
            }
            sc::Kind::Gradient { top, bottom } => {
                let g = Grad { radial: false, angle_deg: 180.0, at: [0.5, 0.5], stops: &STOPS[palette_ix(top) * 8 + palette_ix(bottom)] };
                el(deco_box(x, y, w, h, 0.0).gradient(g))
            }
            sc::Kind::Bordered { fill, border } => el(quad(x, y, w, h).bg(col(fill, 0x66)).border(1.0, rgb(border))),
            // a label follows the theme: flow text with a colour operand, placed from the canvas box's corner
            sc::Kind::Label => el(text(format!("L{}", n.g)).font(10.0, 12.0).color(sig!(self.look).map(|l: &u32| ink(l))).abs(x - CANVAS_X, y - CANVAS_Y)),
            sc::Kind::Instance { k } => el(quad(x, y, w, h).bg(sig!(self.look).map(INST[k])).rounded(4.0).border(1.0, col(0, 0x33))),
            sc::Kind::Selected => {
                let (ax, ay) = (x - CANVAS_X, y - CANVAS_Y);
                el((
                    div().abs(ax, ay).w(w).h(h).bg(rgb(sc::SELECT_FILL)).rounded(6.0).translate(sig!(self.sel_x).map(|x: &f32| (*x - sc::MOVE_A, 0.0))),
                    div().abs(ax - 3.0, ay - 3.0).w(w + 6.0).h(h + 6.0).border(1.0, rgb(sc::SELECT_OUTLINE)).rounded(9.0).translate(sig!(self.sel_x).map(|x: &f32| (*x - sc::MOVE_A, 0.0))),
                ))
            }
        };
        match group {
            Some((gx, gy, gw, gh)) => el(clip(gx, gy, gw, gh, inner)),
            None => inner,
        }
    }

    fn frame_el(&self, f: usize) -> impl Element<Self> + '_ {
        let (fx, fy, fw, fh) = sc::frame_rect(f);
        (
            // the shadow is a box shadow of a transparent box (a `deco_box` always emits its fill); the fill follows the theme
            deco_box(fx, fy, fw, fh, sc::FRAME_RADIUS).shadows(&FRAME_SHADOW),
            quad(fx, fy, fw, fh).bg(sig!(self.look).map(|l: &u32| frame_bg(l))).rounded(sc::FRAME_RADIUS),
            rtext(format!("Frame {}", f + 1)).at(fx, fy - 16.0).size(11.0).line_height(14.0).color(rgb(sc::MUTED)),
            // index order, later on top; the selected node (node 0 of frame 0) last, on top of the frame (the spec)
            clip(fx, fy, fw, fh, each((0..self.per).filter(move |&i| !(f == 0 && i == 0)).chain((f == 0).then_some(0)).map(move |i| self.node_el(f, i, fx, fy)))),
        )
    }

    fn comp_row(&self, k: usize) -> impl Element<Self> + '_ {
        row((block(12.0, 12.0).bg(sig!(self.look).map(SWATCH[k])), text(format!("Component {}", k + 1)).font(12.0, 16.0).color(sig!(self.look).map(|l: &u32| ink(l)))))
            .h(sc::ROW_H)
            .shrink(0.0)
            .pad(12.0, 0.0, 0.0, 0.0)
            .gap(8.0)
            .items(Align::Center)
            .bg(sig!(self.look).map(ROW_BG[k]))
            .on_hover(move |c: &mut Canvas, e: &Ev| c.hover_comp(k, e.on))
    }

    fn layer_row(&self, g: usize) -> impl Element<Self> + '_ {
        row((text(format!("Layer {g}")).font(12.0, 16.0).color(sig!(self.look).map(|l: &u32| ink(l))),)).h(sc::ROW_H).shrink(0.0).pad(24.0, 0.0, 0.0, 0.0).items(Align::Center)
    }

    fn frame_rows(&self, f: usize) -> impl Element<Self> + '_ {
        (
            row((text(format!("Frame {}", f + 1)).font(12.0, 16.0).color(sig!(self.look).map(|l: &u32| ink(l))),)).h(sc::ROW_H).shrink(0.0).pad(12.0, 0.0, 0.0, 0.0).items(Align::Center),
            each((0..sc::LAYER_ROWS.min(self.per)).map(move |i| self.layer_row(f * self.per + i))),
        )
    }

    fn field(&self, i: usize) -> impl Element<Self> + '_ {
        let v = sc::inspector(sc::MOVE_A)[i];
        row((
            text(sc::INSPECTOR[i]).font(12.0, 16.0).w(108.0).color(rgb(sc::MUTED)),
            text(format!("{v}")).font(12.0, 16.0).color(sig!(self.look).map(|l: &u32| ink(l))),
        ))
        .h(sc::ROW_H)
        .shrink(0.0)
        .items(Align::Center)
    }
}

impl View for Canvas {
    fn view(&self) -> impl Element<Self> + '_ {
        let toolbar = row((text("Canvas").font(14.0, 20.0).color(sig!(self.look).map(|l: &u32| ink(l))),))
            .h(sc::TOOLBAR_H)
            .shrink(0.0)
            .px(12.0)
            .items(Align::Center)
            .bg(sig!(self.look).map(|l: &u32| chrome(l)));

        let layers = column((column((each((0..sc::COMPONENTS).map(|k| self.comp_row(k))), each((0..sc::FRAMES).map(|f| self.frame_rows(f))))).grow(1.0).min_h(0.0).scroll(sig!(self.layers)),))
            .w(sc::PANEL_W)
            .shrink(0.0)
            .bg(sig!(self.look).map(|l: &u32| chrome(l)));

        // the canvas box: raw frames and nodes at window coordinates, flow labels and the selected node among them
        let canvas = div().grow(1.0).min_w(0.0).relative().bg(sig!(self.look).map(|l: &u32| canvas_bg(l))).child(each((0..sc::FRAMES).map(|f| self.frame_el(f))));

        // the inspector: 12 rows of 24 from y 56; the first value (x) follows the selected node
        let inspector = column((
            row((
                text(sc::INSPECTOR[0]).font(12.0, 16.0).w(108.0).color(rgb(sc::MUTED)),
                text(sig!(self.sel_x).num(|v: &f32| *v, WHOLE)).font(12.0, 16.0).color(sig!(self.look).map(|l: &u32| ink(l))),
            ))
            .h(sc::ROW_H)
            .shrink(0.0)
            .items(Align::Center),
            each((1..sc::INSPECTOR.len()).map(|i| self.field(i))),
        ))
        .w(INSPECTOR_W)
        .shrink(0.0)
        .pad(12.0, 12.0, 0.0, 0.0)
        .bg(sig!(self.look).map(|l: &u32| chrome(l)));

        column((toolbar, row((layers, canvas, inspector)).grow(1.0).min_h(0.0)))
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(sig!(self.look).map(|l: &u32| chrome(l)))
            // the spec's keys (the window host focuses the root)
            .on_key(|c: &mut Canvas, e: &Ev| c.key(e))
    }
}
