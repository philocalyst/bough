//! The two phase-1 scenes, each described twice on purpose.
//!
//! * As archetypes: paint types whose operands are expressions over the block. This is what is drawn.
//! * As a `Model`: a plain list of nodes that `Model::oracle` turns into the round-7 kernel's commands for a
//!   given block with ordinary arithmetic and the kernel's float stager (`bough_kernel::stage`), using none of
//!   this crate's expression types, leaves or integer staging. The gates compare the two, so a wrong
//!   expression, a wrong `mk_*` or a wrong generated function shows as a differing word.
//!
//! **canvas**: the layered design canvas of the round-7 workloads (`comparisons/bough-workloads/src/canvas.rs`)
//! at 2560 x 1600 px: 20 clipped artboards of 250 nodes each (cards with one or two blurred shadows,
//! gradients, bordered translucent boxes, labels, icons, clipped groups, 1,000 instances of 5 components
//! whose fill reads the pick and the theme, 60 movable nodes, 50 animated ones, 60 that cross-fade), chrome
//! with a 505-row layers list. Radial and multi-stop gradients and SVG icons are not modelled (linear
//! two-stop gradients and bordered boxes stand in), nor is real text (a synthetic glyph atlas).
//!
//! **anim**: N rounded boxes, each moving, resizing, rounding and recolouring there and back on its own
//! phase of one clock word; half of them translucent over a blurred shadow that moves with them.
use crate::expr::*;
use crate::paint::*;
use crate::plan::{Plan, plan};
use crate::scene::{ColId, Desc, Scene};
use bough_kernel::k;
use bough_kernel::scenes::{self as ks, Rng};
use bough_kernel::stage;

lens!(pub PanX: i32 = 0);
lens!(pub PanY: i32 = 1);
lens!(pub Theme: u32 = 2);
lens!(pub Sel: u32 = 3);
lens!(pub DragX: i32 = 4);
lens!(pub DragY: i32 = 5);
pub type Pick = Uw<U_PICK>;
pub type Time = Uw<U_TIME>;

/// Q8 from pixels.
pub const fn q(px: i32) -> i32 {
    px * 256
}
/// A packed RGBA8 word (red in the low byte) from 0xRRGGBB and an alpha.
pub const fn px(hex: u32, a: u32) -> u32 {
    ((hex >> 16) & 0xff) | (hex & 0xff00) | ((hex & 0xff) << 16) | (a << 24)
}
pub const fn rgb(hex: u32) -> u32 {
    px(hex, 255)
}

// ---------------------------------------------------------------------------------------------- the model

#[derive(Clone, Copy, Debug)]
pub enum Fill {
    /// As packed.
    Any(u32),
    /// Alpha forced full.
    Opq(u32),
    /// Light and dark, mixed by the theme word (Q15).
    Theme(u32, u32),
    /// A highlight while the pick equals `id`, else light and dark by the theme.
    Hover { id: u32, l: u32, d: u32 },
    /// The colour scaled by a looping progress.
    Fade { c: u32, phase: u32, period: u32 },
    /// Two opaque colours mixed by a looping progress.
    Mix { a: u32, b: u32, phase: u32, period: u32 },
}
#[derive(Clone, Copy, Debug)]
pub enum Rad {
    Fixed(i32),
    Tri(i32, i32, u32, u32),
}
#[derive(Clone, Copy, Debug)]
pub enum Motion {
    None,
    /// Towards `to` and back by a smoothed looping progress.
    Tri { to: [i32; 4], phase: u32, period: u32 },
    /// Moved by the drag words while selected.
    Drag { id: u32 },
}
#[derive(Clone, Copy, Debug)]
pub struct Sh {
    pub spread: i32,
    pub dy: i32,
    pub radius: i32,
    pub blur: i32,
    pub color: u32,
}
#[derive(Clone, Copy, Debug)]
pub enum Body {
    Solid(Fill),
    RRect(Rad, Fill),
    Grad { r: i32, angle: f32, c0: Fill, c1: Fill },
    Border { r: i32, w: i32, line: u32, fill: Fill },
    Glyph { texel: u32, color: Fill },
}
#[derive(Clone, Copy, Debug)]
pub struct Node {
    /// Q8.
    pub rect: [i32; 4],
    /// Follows the pan words.
    pub pan: bool,
    /// A clip that follows the pan with the content (Q8).
    pub clip: Option<[i32; 4]>,
    /// A clip that does not move (Q8).
    pub view: Option<[i32; 4]>,
    pub motion: Motion,
    pub sh: [Option<Sh>; 2],
    pub body: Body,
}
impl Node {
    pub const fn new(rect: [i32; 4], body: Body) -> Node {
        Node { rect, pan: false, clip: None, view: None, motion: Motion::None, sh: [None, None], body }
    }
}

pub const HI_L: u32 = rgb(0xff3b6b);
pub const HI_D: u32 = rgb(0xffe14d);

/// The scene as plain data.
#[derive(Default)]
pub struct Model {
    pub nodes: Vec<Node>,
}

fn tri(now: u32, phase: u32, period: u32) -> u32 {
    let p = period.clamp(1, 65535);
    let m = now.wrapping_add(phase) % (2 * p);
    let e = if m < p { m } else { 2 * p - m };
    e * k::ONE / p
}
fn smooth(t: u32) -> u32 {
    let t = t.min(k::ONE);
    k::mul15(k::mul15(t, t), 3 * k::ONE - 2 * t)
}
fn fill(f: Fill, u: &U) -> wgsl_rs::std::Vec4u {
    let (theme, now) = (u.w[2].min(k::ONE), u.w[U_TIME]);
    let o = |c: u32| k::premul(c | 0xff00_0000);
    match f {
        Fill::Any(c) => k::premul(c),
        Fill::Opq(c) => o(c),
        Fill::Theme(l, d) => k::mix4u(o(l), o(d), theme),
        Fill::Hover { id, l, d } => {
            if u.w[U_PICK] == id {
                k::mix4u(k::premul(HI_L), k::premul(HI_D), theme)
            } else {
                k::mix4u(o(l), o(d), theme)
            }
        }
        Fill::Fade { c, phase, period } => k::scale(k::premul(c), tri(now, phase, period).min(k::ONE)),
        Fill::Mix { a, b, phase, period } => k::mix4u(o(a), o(b), tri(now, phase, period).min(k::ONE)),
    }
}
fn f(v: i32) -> f32 {
    v as f32 / 256.0
}

impl Model {
    /// The scene under `u` as the generic kernel's commands: plain arithmetic and the float stager.
    pub fn oracle(&self, u: &U) -> stage::Scene {
        let mut out = stage::Scene::default();
        let (pan, now) = ([u.w[0] as i32, u.w[1] as i32], u.w[U_TIME]);
        for n in &self.nodes {
            let off = |r: [i32; 4], d: [i32; 2]| [r[0] + d[0], r[1] + d[1], r[2] + d[0], r[3] + d[1]];
            let p = if n.pan { pan } else { [0, 0] };
            let mut b = off(n.rect, p);
            match n.motion {
                Motion::None => {}
                Motion::Tri { to, phase, period } => {
                    let t = smooth(tri(now, phase, period));
                    let to = off(to, p);
                    let m = |a: i32, c: i32| k::mix_i(a, c, t);
                    b = [m(b[0], to[0]), m(b[1], to[1]), m(b[2], to[2]), m(b[3], to[3])];
                }
                Motion::Drag { id } => {
                    if u.w[3] == id {
                        b = off(b, [u.w[4] as i32, u.w[5] as i32]);
                    }
                }
            }
            // the clip: the panned one cut by the fixed one; an inverted clip shows nothing
            let mut c = [-4_096_000, -4_096_000, 4_096_000, 4_096_000];
            for k2 in [n.clip.map(|r| off(r, p)), n.view].into_iter().flatten() {
                c = [c[0].max(k2[0]), c[1].max(k2[1]), c[2].min(k2[2]), c[3].min(k2[3])];
            }
            if c[0] > c[2] || c[1] > c[3] {
                continue;
            }
            let clip = c.map(f);
            let (x, y, w, h) = (f(b[0]), f(b[1]), f(b[2] - b[0]), f(b[3] - b[1]));
            for s in n.sh.iter().flatten() {
                let (sx, sy, sw, sh) = (f(b[0] - s.spread), f(b[1] - s.spread + s.dy), f(b[2] - b[0] + 2 * s.spread), f(b[3] - b[1] + 2 * s.spread));
                out.add(stage::shadow(sx, sy, sw, sh, f(s.radius), s.blur as f32 / 512.0, s.color, &clip));
            }
            let with = |c: Option<k::Cmd>, pm: wgsl_rs::std::Vec4u| {
                c.map(|mut c| {
                    c.pm = pm;
                    c
                })
            };
            match n.body {
                Body::Solid(fl) => out.add(with(stage::solid(x, y, w, h, 0, &clip), fill(fl, u))),
                Body::RRect(r, fl) => {
                    let r = match r {
                        Rad::Fixed(r) => r,
                        Rad::Tri(a, c, phase, period) => k::mix_i(a, c, tri(now, phase, period).min(k::ONE)),
                    };
                    out.add(with(stage::rrect(x, y, w, h, f(r), 0, &clip), fill(fl, u)))
                }
                Body::Grad { r, angle, c0, c1 } => out.add(stage::grad(x, y, w, h, f(r), angle, 0, 0, &clip).map(|mut c| {
                    (c.pm, c.pm2) = (fill(c0, u), fill(c1, u));
                    c
                })),
                Body::Border { r, w: bw, line, fill: fl } => out.add(stage::border(x, y, w, h, f(r), f(bw), line, 0, &clip).map(|mut c| {
                    c.pm2 = fill(fl, u);
                    c
                })),
                Body::Glyph { texel, color } => out.add(with(stage::glyph(x, y, ks::CELL_W, ks::CELL_H, texel, 0, &clip), fill(color, u))),
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------- shared paint types

type Pan = Off<Lane, Un<PanX>, Un<PanY>>;
type Opq = Pmo<Rec>;
type Any = Pm<Rec>;
type ThemeC = MixV<Opq, Opq, Un<Theme>>;
type HoverC = SelV<Eq<Pick, Rec>, MixV<Pm<K<HI_L>>, Pm<K<HI_D>>, Un<Theme>>, ThemeC>;
type TriT = Tri<Time, Rec, Rec>;
/// A shadow of box `B`: spread, vertical offset, radius, blur, colour (five words).
type ShP<B> = Shadow<Off<Grow<B, I<Rec>>, Ki<0>, I<Rec>>, I<Rec>, I<Rec>, Any>;

fn i(v: i32) -> I<Rec> {
    I { a: Rec(v as u32) }
}
fn any(c: u32) -> Any {
    Pm { c: Rec(c) }
}
fn opq(c: u32) -> Opq {
    Pmo { c: Rec(c) }
}
fn pan() -> Pan {
    Off { b: Lane, dx: un(), dy: un() }
}
fn theme(l: u32, d: u32) -> ThemeC {
    MixV { a: opq(l), b: opq(d), t: un() }
}
fn hover(id: u32, l: u32, d: u32) -> HoverC {
    SelV { c: Eq { a: Uw, b: Rec(id) }, a: MixV { a: Pm { c: K }, b: Pm { c: K }, t: un() }, b: theme(l, d) }
}
fn tri_t(phase: u32, period: u32) -> TriT {
    Tri { now: Uw, phase: Rec(phase), period: Rec(period) }
}
fn sh<B>(b: B, s: Sh) -> ShP<B> {
    Shadow { b: Off { b: Grow { b, d: i(s.spread) }, dx: Ki, dy: i(s.dy) }, r: i(s.radius), blur: i(s.blur), c: any(s.color) }
}
/// A gradient's direction as the kernel's stager scales it (the same float operations).
fn grad_dir(rect: [i32; 4], angle: f32) -> (i32, i32) {
    let (w, h) = (f(rect[2] - rect[0]), f(rect[3] - rect[1]));
    let (dx, dy) = (angle.sin(), -angle.cos());
    let len = ((w * dx).abs() + (h * dy).abs()).max(1.0);
    let s = (1u32 << 26) as f32 / len;
    ((dx * s).round() as i32, (dy * s).round() as i32)
}

// ---------------------------------------------------------------------------------------------- canvas

pub mod canvas {
    use super::*;

    pub const W: u32 = 2560;
    pub const H: u32 = 1600;
    const TOOLBAR: i32 = 88;
    const LEFT: i32 = 480;
    const RIGHT: i32 = 520;
    /// The canvas viewport, Q8.
    pub const VIEW: [i32; 4] = [q(LEFT), q(TOOLBAR), q(W as i32 - RIGHT), q(H as i32)];
    const PANEL: [i32; 4] = [0, q(TOOLBAR), q(LEFT), q(H as i32)];
    pub const FRAMES: usize = 20;
    const FW: i32 = 288;
    const FH: i32 = 304;
    const ROW_H: i32 = 40;
    const ROWS: usize = 505;
    const ROWS_Y: i32 = TOOLBAR + 56;
    /// Pick ids: components 0..5; a layers row that is not a component's; a movable node.
    pub const ROW_ID: u32 = 0x1000;
    pub const DRAG_ID: u32 = 0x2000;

    type View = RectW<K<{ VIEW[0] as u32 }>, K<{ VIEW[1] as u32 }>, K<{ VIEW[2] as u32 }>, K<{ VIEW[3] as u32 }>>;
    type Panel = RectW<K<{ PANEL[0] as u32 }>, K<{ PANEL[1] as u32 }>, K<{ PANEL[2] as u32 }>, K<{ PANEL[3] as u32 }>>;
    /// `P` inside its artboard (a clip that pans with the content) inside the viewport.
    type InFrame<P> = Clip<Clip<P, Off<RecRect, Un<PanX>, Un<PanY>>>, View>;

    pub type Chrome = Solid<Lane, ThemeC>;
    pub type Board = Clip<Over<ShP<Pan>, RRect<Pan, Ki<{ q(8) }>, Opq>>, View>;
    pub type Card1 = InFrame<Over<ShP<Pan>, RRect<Pan, I<Rec>, Opq>>>;
    pub type Card2 = InFrame<Over<Over<ShP<Pan>, ShP<Pan>>, RRect<Pan, I<Rec>, Opq>>>;
    pub type GradS = InFrame<Over<ShP<Pan>, Grad<Pan, I<Rec>, I<Rec>, I<Rec>, Opq, Opq>>>;
    pub type GradP = InFrame<Grad<Pan, I<Rec>, I<Rec>, I<Rec>, Any, Any>>;
    pub type Bord = InFrame<Border<Pan, I<Rec>, I<Rec>, Any, Any>>;
    pub type Label = InFrame<Glyph<Pan, Rec, Any>>;
    /// A component instance: a shadow under a bordered box whose fill reads the pick and the theme.
    pub type Comp = Hit<InFrame<Over<ShP<Pan>, Border<Pan, I<Rec>, Ki<256>, Any, HoverC>>>, Rec>;
    pub type FadeP = InFrame<RRect<Pan, I<Rec>, Fade<Any, TriT>>>;
    type Moved = MixR<Pan, Off<RecRect, Un<PanX>, Un<PanY>>, Smooth<TriT>>;
    pub type AnimP = InFrame<RRect<Moved, I<Rec>, Fade<Any, TriT>>>;
    type Dragged = Off<Pan, SelI<Eq<Un<Sel>, Rec>, Un<DragX>, Ki<0>>, SelI<Eq<Un<Sel>, Rec>, Un<DragY>, Ki<0>>>;
    pub type DragP = Hit<InFrame<RRect<Dragged, I<Rec>, Opq>>, Rec>;
    pub type Row = Hit<Clip<Solid<Lane, HoverC>, Panel>, Rec>;
    pub type Text = Glyph<Lane, Rec, ThemeC>;

    /// The plan the compiler chose, from the archetypes' constants.
    pub const PLAN: Plan<14> = plan([
        Desc::of::<Chrome>(),
        Desc::of::<Board>(),
        Desc::of::<Card1>(),
        Desc::of::<Card2>(),
        Desc::of::<GradS>(),
        Desc::of::<GradP>(),
        Desc::of::<Bord>(),
        Desc::of::<Label>(),
        Desc::of::<Comp>(),
        Desc::of::<FadeP>(),
        Desc::of::<AnimP>(),
        Desc::of::<DragP>(),
        Desc::of::<Row>(),
        Desc::of::<Text>(),
    ]);
    // evaluated when this crate is compiled: the canvas needs the GPU, rebins only on pan, drag and time
    const _: () = assert!(PLAN.whole.gpu && PLAN.whole.cut && PLAN.whole.pick && PLAN.whole.rebin_on == 0b11_1011 | 1 << U_TIME);

    struct Cols {
        chrome: ColId<Chrome>,
        board: ColId<Board>,
        card1: ColId<Card1>,
        card2: ColId<Card2>,
        grad_s: ColId<GradS>,
        grad_p: ColId<GradP>,
        bord: ColId<Bord>,
        label: ColId<Label>,
        comp: ColId<Comp>,
        fade: ColId<FadeP>,
        anim: ColId<AnimP>,
        drag: ColId<DragP>,
        row: ColId<Row>,
        text: ColId<Text>,
    }
    struct B {
        s: Scene,
        m: Model,
        c: Cols,
        g: Rng,
    }

    /// (dy, blur, spread, colour) in points, the workload's recipes; a point is two pixels here.
    const SHADOWS: [(i32, i32, i32, u32); 10] = [
        (1, 2, 0, px(0x000000, 0x50)),
        (1, 3, 0, px(0x000000, 0x40)),
        (2, 4, 0, px(0x000000, 0x40)),
        (2, 6, 0, px(0x1a237e, 0x50)),
        (3, 8, 0, px(0x000000, 0x40)),
        (4, 12, 0, px(0x000000, 0x38)),
        (6, 16, 0, px(0x4a148c, 0x40)),
        (8, 24, 0, px(0x000000, 0x38)),
        (10, 32, -4, px(0x000000, 0x40)),
        (12, 40, 0, px(0x000000, 0x48)),
    ];
    const PALETTE: [u32; 12] = [0xff6b6b, 0xffa94d, 0xffd43b, 0x69db7c, 0x38d9a9, 0x4dabf7, 0x748ffc, 0xb197fc, 0xf783ac, 0xe9ecef, 0xced4da, 0x495057];
    /// The five components: size and radius (pixels), light and dark fill.
    const COMPS: [(i32, i32, i32, u32, u32); 5] = [(68, 28, 14, 0x3b82f6, 0x1d4ed8), (32, 32, 16, 0xf59e0b, 0xb45309), (76, 48, 8, 0x14b8a6, 0x0f766e), (52, 24, 6, 0xa855f7, 0x7e22ce), (36, 36, 18, 0x22c55e, 0x15803d)];
    const CHROME: (u32, u32) = (0xf4f5f7, 0x1f2229);
    const INK: (u32, u32) = (0x1d2129, 0xe6e9ef);

    fn shadow(k: usize, radius: i32) -> Sh {
        let (dy, blur, spread, color) = SHADOWS[k % SHADOWS.len()];
        Sh { spread: q(2 * spread), dy: q(2 * dy), radius, blur: q(2 * blur), color }
    }
    fn in_frame<P>(p: P, clip: [i32; 4]) -> InFrame<P> {
        Clip { p: Clip { p, k: Off { b: rec_rect(clip), dx: un(), dy: un() } }, k: RectW { a: K, b: K, c: K, d: K } }
    }

    impl B {
        fn node(&mut self, rect: [i32; 4], clip: [i32; 4], body: Body) -> Node {
            Node { rect, pan: true, clip: Some(clip), view: Some(VIEW), motion: Motion::None, sh: [None, None], body }
        }
        fn chrome(&mut self, r: [i32; 4], l: u32, d: u32) {
            self.m.nodes.push(Node::new(r, Body::Solid(Fill::Theme(rgb(l), rgb(d)))));
            self.s.push(self.c.chrome, r, Solid { b: Lane, c: theme(rgb(l), rgb(d)) });
        }
        fn text(&mut self, x: i32, y: i32, n: usize, l: u32, d: u32) {
            for j in 0..n {
                let (gx, texel) = (x + j as i32 * ks::CELL_W as i32, ks::cell(self.g.u(ks::ATLAS_CELLS)));
                let r = [q(gx), q(y), q(gx + ks::CELL_W as i32), q(y + ks::CELL_H as i32)];
                self.m.nodes.push(Node::new(r, Body::Glyph { texel, color: Fill::Theme(rgb(l), rgb(d)) }));
                self.s.push(self.c.text, r, Glyph { b: Lane, texel: Rec(texel), c: theme(rgb(l), rgb(d)) });
            }
        }
        fn label(&mut self, x: i32, y: i32, n: usize, color: u32, clip: [i32; 4]) {
            for j in 0..n {
                let (gx, texel) = (x + q(j as i32 * ks::CELL_W as i32), ks::cell(self.g.u(ks::ATLAS_CELLS)));
                let r = [gx, y, gx + q(ks::CELL_W as i32), y + q(ks::CELL_H as i32)];
                let n = self.node(r, clip, Body::Glyph { texel, color: Fill::Any(color) });
                self.m.nodes.push(n);
                self.s.push(self.c.label, r, in_frame(Glyph { b: pan(), texel: Rec(texel), c: any(color) }, clip));
            }
        }
        fn card(&mut self, r: [i32; 4], clip: [i32; 4], rad: i32, fill: u32, shs: &[Sh]) {
            let mut n = self.node(r, clip, Body::RRect(Rad::Fixed(rad), Fill::Opq(fill)));
            let body = RRect { b: pan(), r: i(rad), c: opq(fill) };
            match shs {
                [a] => {
                    n.sh = [Some(*a), None];
                    self.s.push(self.c.card1, r, in_frame(Over(sh(pan(), *a), body), clip));
                }
                [a, b] => {
                    n.sh = [Some(*a), Some(*b)];
                    self.s.push(self.c.card2, r, in_frame(Over(Over(sh(pan(), *a), sh(pan(), *b)), body), clip));
                }
                _ => unreachable!(),
            }
            self.m.nodes.push(n);
        }
        /// A rect of a random size at a random half-pixel position inside (and a little outside) a frame.
        fn place(&mut self, fx: i32, fy: i32, w: (i32, i32), h: (i32, i32)) -> [i32; 4] {
            let (bw, bh) = (w.0 + self.g.u((w.1 - w.0) as u32) as i32, h.0 + self.g.u((h.1 - h.0) as u32) as i32);
            let x = q(fx - 12) + self.g.u(((FW - bw / 2) * 2) as u32) as i32 * 128;
            let y = q(fy - 12) + self.g.u(((FH - bh / 2) * 2) as u32) as i32 * 128;
            [x, y, x + q(bw), y + q(bh)]
        }
        fn frame(&mut self, fi: usize) {
            let (fx, fy) = (LEFT + 24 + (fi % 5) as i32 * 312, TOOLBAR + 40 + (fi / 5) as i32 * 352);
            let fr = [q(fx), q(fy), q(fx + FW), q(fy + FH)];
            // the artboard: a white rounded rect with a drop shadow, and its title above it
            let s = Sh { spread: 0, dy: q(12), radius: q(8), blur: q(48), color: px(0, 0x50) };
            let mut n = Node { view: Some(VIEW), pan: true, sh: [Some(s), None], ..Node::new(fr, Body::RRect(Rad::Fixed(q(8)), Fill::Opq(rgb(0xffffff)))) };
            n.clip = None;
            self.m.nodes.push(n);
            self.s.push(self.c.board, fr, Clip { p: Over(sh(pan(), s), RRect { b: pan(), r: Ki, c: opq(rgb(0xffffff)) }), k: RectW { a: K, b: K, c: K, d: K } });
            let title_clip = [fr[0], fr[1] - q(30), fr[2], fr[3]];
            self.label(q(fx + 4), q(fy - 24), 10, rgb(0x6b7280), title_clip);

            // the mixed layer: kinds shuffled together, so layers interleave and overlap
            let anims = if fi < 10 { 3 } else { 2 };
            let mixed = 250 - 50 - 4 - 3 - anims;
            let mut kinds: Vec<u8> = Vec::with_capacity(250);
            kinds.extend(std::iter::repeat_n(0u8, 50));
            for j in 0..mixed {
                kinds.push(if fi == 1 && j < 60 {
                    7 + (j % 2) as u8
                } else {
                    match self.g.u(100) {
                        0..28 => 1,
                        28..38 => 2,
                        38..46 => 3,
                        46..54 => 4,
                        54..70 => 5,
                        70..85 => 6,
                        _ => 9,
                    }
                });
            }
            for j in (1..kinds.len()).rev() {
                kinds.swap(j, self.g.u(j as u32 + 1) as usize);
            }
            let mut comp = 0usize;
            for kind in kinds {
                let color = rgb(PALETTE[self.g.u(12) as usize]);
                match kind {
                    0 => {
                        let c = (comp + fi) % 5;
                        comp += 1;
                        let (w, h, rad, l, d) = COMPS[c];
                        let r = self.place(fx, fy, (w, w + 1), (h, h + 1));
                        let s = shadow(c + 2, q(rad));
                        let mut n = self.node(r, fr, Body::Border { r: q(rad), w: 256, line: px(0, 0x30), fill: Fill::Hover { id: c as u32, l: rgb(l), d: rgb(d) } });
                        n.sh = [Some(s), None];
                        self.m.nodes.push(n);
                        self.s.push(self.c.comp, r, Hit { p: in_frame(Over(sh(pan(), s), Border { b: pan(), r: i(q(rad)), w: Ki, c: any(px(0, 0x30)), fill: hover(c as u32, rgb(l), rgb(d)) }), fr), id: Rec(c as u32) });
                    }
                    1 => {
                        let r = self.place(fx, fy, (40, 140), (32, 100));
                        let rad = q(self.g.u(17) as i32);
                        let s = shadow(self.g.u(10) as usize, rad);
                        self.card(r, fr, rad, color, &[s]);
                    }
                    2 => {
                        let r = self.place(fx, fy, (48, 140), (40, 100));
                        let rad = q(self.g.u(17) as i32);
                        let (a, b) = (shadow(self.g.u(4) as usize, rad), shadow(6 + self.g.u(4) as usize, rad));
                        self.card(r, fr, rad, color, &[a, b]);
                    }
                    3 | 4 => {
                        let r = self.place(fx, fy, (48, 130), (40, 96));
                        let rad = q(self.g.u(13) as i32);
                        let angle = [0.0f32, 45.0, 90.0, 135.0, 180.0][self.g.u(5) as usize].to_radians();
                        let (dx, dy) = grad_dir(r, angle);
                        let c2 = rgb(PALETTE[self.g.u(12) as usize]);
                        if kind == 3 {
                            let s = shadow(self.g.u(10) as usize, rad);
                            let mut n = self.node(r, fr, Body::Grad { r: rad, angle, c0: Fill::Opq(color), c1: Fill::Opq(c2) });
                            n.sh = [Some(s), None];
                            self.m.nodes.push(n);
                            self.s.push(self.c.grad_s, r, in_frame(Over(sh(pan(), s), Grad { b: pan(), r: i(rad), dx: i(dx), dy: i(dy), c: opq(color), c2: opq(c2) }), fr));
                        } else {
                            let (ca, cb) = (color & 0x00ff_ffff | 0xf000_0000, c2 & 0x00ff_ffff | 0x8000_0000);
                            let n = self.node(r, fr, Body::Grad { r: rad, angle, c0: Fill::Any(ca), c1: Fill::Any(cb) });
                            self.m.nodes.push(n);
                            self.s.push(self.c.grad_p, r, in_frame(Grad { b: pan(), r: i(rad), dx: i(dx), dy: i(dy), c: any(ca), c2: any(cb) }, fr));
                        }
                    }
                    5 | 9 => {
                        // a bordered translucent box; kind 9 stands in for an icon (small, opaque fill)
                        let r = if kind == 5 { self.place(fx, fy, (40, 130), (32, 96)) } else { self.place(fx, fy, (48, 49), (48, 49)) };
                        let rad = q(if kind == 5 { self.g.u(13) as i32 } else { 10 });
                        let bw = if kind == 5 { 128 + self.g.u(4) as i32 * 128 } else { 512 };
                        let (line, fl) = (rgb(PALETTE[self.g.u(12) as usize]), if kind == 5 { color & 0x00ff_ffff | 0x6000_0000 } else { color });
                        let n = self.node(r, fr, Body::Border { r: rad, w: bw, line, fill: Fill::Any(fl) });
                        self.m.nodes.push(n);
                        self.s.push(self.c.bord, r, in_frame(Border { b: pan(), r: i(rad), w: i(bw), c: any(line), fill: any(fl) }, fr));
                    }
                    6 => {
                        let r = self.place(fx, fy, (144, 145), (18, 19));
                        self.label(r[0] & !255, r[1] & !255, 12, rgb(0x1d2129), fr);
                    }
                    _ => {
                        // the cross-fading artboard: two overlaid states on opposite phases of one period
                        let r = self.place(fx, fy, (60, 120), (40, 80));
                        let (phase, period) = (if kind == 7 { 0 } else { 500 }, 500);
                        let n = self.node(r, fr, Body::RRect(Rad::Fixed(q(6)), Fill::Fade { c: color, phase, period }));
                        self.m.nodes.push(n);
                        self.s.push(self.c.fade, r, in_frame(RRect { b: pan(), r: i(q(6)), c: Fade { a: any(color), k: tri_t(phase, period) } }, fr));
                    }
                }
            }
            // four groups whose children spill out of the group's own clip (nested clips: the group's cut by
            // the artboard's, known at mount)
            for _ in 0..4 {
                let gr = self.place(fx + 20, fy + 20, (90, 130), (70, 110));
                let gc = [gr[0].max(fr[0]), gr[1].max(fr[1]), gr[2].min(fr[2]), gr[3].min(fr[3])];
                for _ in 0..3 {
                    let (w, h) = (40 + self.g.u(70) as i32, 30 + self.g.u(60) as i32);
                    let (x, y) = (gr[0] - q(20) + self.g.u(120) as i32 * 256, gr[1] - q(20) + self.g.u(100) as i32 * 256);
                    let rad = q(self.g.u(10) as i32);
                    let s = shadow(self.g.u(10) as usize, rad);
                    let color = rgb(PALETTE[self.g.u(12) as usize]);
                    self.card([x, y, x + q(w), y + q(h)], gc, rad, color, &[s]);
                }
            }
            // the top layer: animated nodes, then movable ones
            for _ in 0..anims {
                let (r, to) = (self.place(fx, fy, (40, 90), (30, 70)), self.place(fx, fy, (40, 120), (30, 90)));
                let (phase, period) = (self.g.u(2000), 300 + self.g.u(900));
                let color = rgb(PALETTE[self.g.u(12) as usize]);
                let mut n = self.node(r, fr, Body::RRect(Rad::Fixed(q(8)), Fill::Fade { c: color, phase, period }));
                n.motion = Motion::Tri { to, phase, period };
                self.m.nodes.push(n);
                let moved = MixR { a: pan(), b: Off { b: rec_rect(to), dx: un(), dy: un() }, t: Smooth { t: tri_t(phase, period) } };
                self.s.push(self.c.anim, r, in_frame(RRect { b: moved, r: i(q(8)), c: Fade { a: any(color), k: tri_t(phase, period) } }, fr));
            }
            for j in 0..3 {
                let r = self.place(fx, fy, (50, 90), (40, 70));
                let id = DRAG_ID + (fi * 3 + j) as u32;
                let color = rgb(PALETTE[self.g.u(12) as usize]);
                let mut n = self.node(r, fr, Body::RRect(Rad::Fixed(q(6)), Fill::Opq(color)));
                n.motion = Motion::Drag { id };
                self.m.nodes.push(n);
                let d = Off { b: pan(), dx: SelI { c: Eq { a: un(), b: Rec(id) }, a: un(), b: Ki }, dy: SelI { c: Eq { a: un(), b: Rec(id) }, a: un(), b: Ki } };
                self.s.push(self.c.drag, r, Hit { p: in_frame(RRect { b: d, r: i(q(6)), c: opq(color) }, fr), id: Rec(id) });
            }
        }
    }

    /// The canvas with `frames` artboards (20 is the workload's size).
    pub fn build(frames: usize) -> (Scene, Model) {
        let mut s = Scene::new(W, H, 1 << 22);
        s.atlas = ks::atlas();
        s.aw = ks::ATLAS_W;
        let n = frames * 260;
        let c = Cols {
            chrome: s.column(16),
            board: s.column(frames),
            card1: s.column(n),
            card2: s.column(n),
            grad_s: s.column(n),
            grad_p: s.column(n),
            bord: s.column(n),
            label: s.column(n * 4),
            comp: s.column(frames * 50),
            fade: s.column(64),
            anim: s.column(64),
            drag: s.column(frames * 3),
            row: s.column(ROWS),
            text: s.column(ROWS * 14 + 512),
        };
        let mut b = B { s, m: Model::default(), c, g: Rng(0x5eed_cafe_f00d_1234) };
        b.chrome(VIEW, 0xdcdfe5, 0x101217);
        for fi in 0..frames {
            b.frame(fi);
        }
        // the chrome is mounted last: it paints over content panned beneath it, and its opaque panels cut
        // that content out of their tiles
        b.chrome([0, 0, q(W as i32), q(TOOLBAR)], CHROME.0, CHROME.1);
        b.chrome(PANEL, CHROME.0, CHROME.1);
        b.chrome([q(W as i32 - RIGHT), q(TOOLBAR), q(W as i32), q(H as i32)], CHROME.0, CHROME.1);
        b.text(24, 34, 24, INK.0, INK.1);
        for r in 0..ROWS {
            let y = ROWS_Y + r as i32 * ROW_H;
            let rect = [0, q(y), q(LEFT), q(y + ROW_H)];
            let id = if r < 5 { r as u32 } else { ROW_ID + r as u32 };
            let mut n = Node::new(rect, Body::Solid(Fill::Hover { id, l: rgb(CHROME.0), d: rgb(CHROME.1) }));
            n.view = Some(PANEL);
            b.m.nodes.push(n);
            b.s.push(b.c.row, rect, Hit { p: Clip { p: Solid { b: Lane, c: hover(id, rgb(CHROME.0), rgb(CHROME.1)) }, k: RectW { a: K, b: K, c: K, d: K } }, id: Rec(id) });
            b.text(24 + (r % 3) as i32 * 16, y + 11, 14, INK.0, INK.1);
        }
        for r in 0..12 {
            b.text(W as i32 - RIGHT + 24, TOOLBAR + 40 + r * 56, 8, 0x6b7280, 0x8b93a3);
            b.text(W as i32 - RIGHT + 200, TOOLBAR + 40 + r * 56, 6, INK.0, INK.1);
        }
        (b.s, b.m)
    }

    /// A pointer position on the layers row of component `c`, and one on no record with an id.
    pub fn row_point(c: usize) -> (i32, i32) {
        (200, ROWS_Y + c as i32 * ROW_H + ROW_H / 2)
    }
    pub const NOWHERE: (i32, i32) = (1200, 40);
}

// ---------------------------------------------------------------------------------------------- anim

pub mod anim {
    use super::*;

    pub const W: u32 = 1920;
    pub const H: u32 = 1080;

    type Breathe = MixR<Lane, RecRect, Smooth<TriT>>;
    /// An opaque box moving, resizing, rounding and recolouring.
    pub type Mover = RRect<Breathe, MixI<I<Rec>, I<Rec>, TriT>, MixV<Opq, Opq, TriT>>;
    /// A translucent box fading over a blurred shadow that moves with it.
    pub type Glow = Over<Shadow<Off<Breathe, Ki<0>, Ki<{ q(6) }>>, Ki<{ q(10) }>, I<Rec>, Any>, RRect<Breathe, Ki<{ q(10) }>, Fade<Any, TriT>>>;
    pub type Back = Solid<Lane, ThemeC>;
    pub type Text = Glyph<Lane, Rec, ThemeC>;

    pub const PLAN: Plan<4> = plan([Desc::of::<Back>(), Desc::of::<Mover>(), Desc::of::<Glow>(), Desc::of::<Text>()]);
    const _: () = assert!(PLAN.whole.gpu && PLAN.whole.rebin_on == 1 << U_TIME && !PLAN.whole.pick);

    /// `n` animating elements on a grid.
    pub fn build(n: usize) -> (Scene, Model) {
        let mut s = Scene::new(W, H, (n * 40 + 4096).next_power_of_two());
        s.atlas = ks::atlas();
        s.aw = ks::ATLAS_W;
        let (back, mover, glow, text) = (s.column::<Back>(4), s.column::<Mover>(n), s.column::<Glow>(n), s.column::<Text>(64));
        let mut m = Model::default();
        let mut g = Rng(0x00a1_1ce5_b0b5_7777);
        let r = [0, 0, q(W as i32), q(H as i32)];
        m.nodes.push(Node::new(r, Body::Solid(Fill::Theme(rgb(0xf4f5f7), rgb(0x16181d)))));
        s.push(back, r, Solid { b: Lane, c: theme(rgb(0xf4f5f7), rgb(0x16181d)) });
        let cols = ((n as f32 * W as f32 / (H as f32 - 80.0)).sqrt().ceil() as usize).max(1);
        let rows = n.div_ceil(cols);
        let (cw, ch) = (W as f32 / cols as f32, (H as f32 - 80.0) / rows as f32);
        for e in 0..n {
            let (cx, cy) = ((e % cols) as f32 * cw, 80.0 + (e / cols) as f32 * ch);
            let qf = |v: f32| (v * 8.0).round() as i32 * 32;
            let a = [qf(cx + cw * 0.2), qf(cy + ch * 0.2), qf(cx + cw * 0.8), qf(cy + ch * 0.8)];
            let (dx, dy, gw, gh) = (g.f(-0.5, 0.5) * cw, g.f(-0.5, 0.5) * ch, g.f(-0.15, 0.3) * cw, g.f(-0.15, 0.3) * ch);
            let b = [a[0] + qf(dx - gw), a[1] + qf(dy - gh), a[2] + qf(dx + gw), a[3] + qf(dy + gh)];
            let (phase, period) = (g.u(4000), 400 + g.u(1600));
            let (c0, c1) = (g.color(true), g.color(true));
            let breathe = MixR { a: Lane, b: rec_rect(b), t: Smooth { t: tri_t(phase, period) } };
            let motion = Motion::Tri { to: b, phase, period };
            if e % 2 == 0 {
                let (r0, r1) = (qf(g.f(0.0, 4.0)), qf(g.f(4.0, cw.min(ch) * 0.4)));
                let mut nd = Node::new(a, Body::RRect(Rad::Tri(r0, r1, phase, period), Fill::Mix { a: c0, b: c1, phase, period }));
                nd.motion = motion;
                m.nodes.push(nd);
                s.push(mover, a, RRect { b: breathe, r: MixI { a: i(r0), b: i(r1), t: tri_t(phase, period) }, c: MixV { a: opq(c0), b: opq(c1), t: tri_t(phase, period) } });
            } else {
                let blur = q(4 + g.u(20) as i32);
                let (cs, cb) = (px(0, 0x60), c0 & 0x00ff_ffff | 0xd000_0000);
                let mut nd = Node::new(a, Body::RRect(Rad::Fixed(q(10)), Fill::Fade { c: cb, phase, period }));
                nd.motion = motion;
                nd.sh = [Some(Sh { spread: 0, dy: q(6), radius: q(10), blur, color: cs }), None];
                m.nodes.push(nd);
                s.push(glow, a, Over(Shadow { b: Off { b: breathe, dx: Ki, dy: Ki }, r: Ki, blur: i(blur), c: any(cs) }, RRect { b: breathe, r: Ki, c: Fade { a: any(cb), k: tri_t(phase, period) } }));
            }
        }
        for j in 0..40 {
            let (gx, texel) = (24 + j * ks::CELL_W as i32, ks::cell(g.u(ks::ATLAS_CELLS)));
            let r = [q(gx), q(30), q(gx + ks::CELL_W as i32), q(30 + ks::CELL_H as i32)];
            m.nodes.push(Node::new(r, Body::Glyph { texel, color: Fill::Theme(rgb(0x1d2129), rgb(0xe6e9ef)) }));
            s.push(text, r, Glyph { b: Lane, texel: Rec(texel), c: theme(rgb(0x1d2129), rgb(0xe6e9ef)) });
        }
        (s, m)
    }
}
