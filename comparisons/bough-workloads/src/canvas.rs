//! A design tool: toolbar (44 pt), layers panel (240 pt, a scrolling list of 505 rows), the canvas, inspector
//! (260 pt, twelve labelled numbers of the selected node).
//!
//! The canvas holds 20 artboard frames in a 5 x 4 grid (each a white rounded rect with a drop shadow and a title,
//! 144 x 152 pt, so the whole grid is on screen at once and nothing is culled). Each frame clips 250 layered
//! nodes, 5,000 in all (`SCENE_SCALE` 1): rounded rects with one or two box shadows (blur 2 to 40 px; the blur of
//! a shadow is the only blur the API has), linear and radial gradients (10 recipes, half with a shadow),
//! bordered translucent boxes, text labels, vector icons (`svg_image`, 8 tiny inline SVGs), 4 groups per frame
//! whose children spill out of the group's own clip (nested clips), and 1,000 instances of 5 components (50 per
//! frame: a shadow beneath a rounded bordered box whose fill reads the theme and the hover highlight). The
//! kinds are shuffled together, so layers interleave and overlap heavily; later mounts draw on top. Three nodes
//! per frame are movable (60), 50 are animated, and frame 1 holds two overlaid states of 30 boxes that cross-fade.
//!
//! The fan-out: `look` packs the theme bit and the hovered component. Hovering a component's row in the layers
//! panel (rows 1 to 5, or any instance row of it in a frame's listing) highlights its 200 instances; each of the
//! 1,000 instances' fills reads `look` through `sig!(self.look).map(COMP_FILL[c])`, so one hover wakes 1,000
//! instance wires (200 repaint), plus the ~1,010 row wires and the chrome's. The theme switch recolours the
//! canvas background, the panel chrome, every row and text colour and the 1,000 component fills.
//!
//! Phases. steady: one drag gesture spread over steps (`pointer_down` on a movable node starts a capture, each
//! step is a `pointer_move` that runs the node's `on_drag` route, which writes the node's `translate` operand,
//! the inspector's x and y and the selection outline); every 8th step the gesture ends and the next node in view
//! is pressed (its `on_press` selects it: the inspector's twelve numbers, its name, the outline's size);
//! every 12th step a wheel detent over the canvas (`App::wheel_points`) moves the wheel's travel and one turn
//! turns that into the pan, which moves every frame's content (`offset`) and clip (`clip_by`). hover: the
//! pointer onto the layers row of component 3 and off it. theme: the theme toggles. animate: smart animate
//! (the 50 animated nodes go to their other layout, position and opacity with `.transition`, retargeting what
//! is still moving) and the cross-fading frame fades to its other state (60 opacity transitions on one signal).
//! These keep running through the resize sweep.
//!
//! How the canvas is built (what this API needed): the canvas content is raw elements at window coordinates
//! (`deco_box`, `quad`, `svg_image`, raw `text`, `clip`) with flow boxes (`div().abs(..)`, the only things with a
//! `translate` or `opacity` operand) mounted among them in the canvas box. Per frame: a `clip_by` on the pan
//! around an `offset` on the pan around the mixed layer; each group its own `clip_by` + `offset`; a second
//! `clip_by` + `offset` for the top layer (animated and movable nodes). 20 frames x (2 + 4 groups) = 120 clips and
//! 120 spaces, all wired to `pan`. A pan is ~240 wire runs, 120 clip writes and 120 record-range translations.
//!
//! Does `translate` on a box move raw children? No: `LBox::translate` moves the box's own quad only (children in
//! flow, `deco_box`es, quads stay). Panning a raw subtree is `offset(sig!(pair), children)`: it moves raw
//! children and flow boxes mounted inside it (tested), but takes the `Signal<(f32, f32)>` itself (not a
//! projection, not a transition) and does not move `clip`s.
//!
//! Wanted and not expressible today (results, not workarounds):
//! * zoom: no scale operand on anything (no `scale` on a subtree, no zoom of coordinates); only a pan exists.
//! * transforms of a subtree: rotation and scale of a group or frame have no spelling. `Xf` (a CSS transform
//!   about the box centre) exists only in `ltemplate` `Deco` tables; `svg_image(..).angle(..)` rotates one image.
//! * opacity of a group: `opacity` multiplies one box's own paint only, never its descendants. The frame
//!   cross-fade is 60 separate per-box opacity transitions, and a half-transparent overlapping stack shows its
//!   overlaps through each other.
//! * layer blur, backdrop blur, blend modes, masks (alpha or clip-path shapes): none. The only blur is the
//!   Gaussian of a box shadow (blur 2 to 40 here); clips are rectangles.
//! * shadows on flow boxes: `LBox` has no `shadow`/`shadows` (only `ltemplate` decls attach one), so every
//!   shadowed node is a `deco_box` and a node that moves or fades cannot have a shadow. A `deco_box` cannot be
//!   shadow-only: it always emits its fill instance (transparent for the components).
//! * moving a `deco_box`: it has no operands at all (position, size, colour, opacity are constants), nor has
//!   raw `text` (colour is constant: the frame titles and labels do not follow the theme). `quad` has `bg`,
//!   `width` and `span` (y, height) operands but no x and no opacity. Everything that changes is a flow box
//!   (`translate` + `opacity`) or sits inside an `offset`.
//! * `offset` takes a `Wired<(f32, f32)>` only: no `.map`, no `.transition`, so a raw subtree cannot be animated.
//! * a wheel writes one `Signal<f32>` (`scrolls`), `offset` needs a pair: the app copies one into the other in a
//!   turn after the wheel (a second turn per detent), and a 2-D pan is not a wheel gesture.
//! * wired clips: static `clip` does not follow an `offset` (it is absolute), so every clip of a panned frame is
//!   a `clip_by` (one wire each). Nested `clip_by`s are wrong after the first write: a patch reads the parent's
//!   rect, wires run newest first (`add_wire` prepends), so the child is cut against the parent's old rect (a
//!   throwaway nested case rendered blank after a pan; a fresh mount at the same pan did not). Groups are
//!   therefore siblings under the viewport clip with rects the author already knows are inside the frame's. The
//!   same order makes a `clip_by` inside an `offset` read the space origin before the offset's own wire moved it.
//! * `clip_by` inside an `anchor` mounts with the anchor's origin missing (the clip is cut at window
//!   coordinates until its first write): the first frame is wrong and pointer hits on the content fail until a
//!   pan. The scene mounts raw content at window coordinates in the canvas box instead of an `anchor` (so it
//!   hard-codes the panels' widths as the canvas origin); `anchor` is also fixed in size (it does not follow a
//!   flexible parent, so the viewport clip is fixed too).
//! * `clip_overflow` on a flow box is a layout rule, not a paint clip: nothing clips flow boxes to a rect that
//!   layout owns except a `scroll` box.
//! * heterogeneous children: `Element` is not object safe (generic `seal`), there is no `Either`, `AnyElement`
//!   or `Vec<Box<dyn Element>>`, and `each` needs one item type: a design tool's mixed list of layers needs the
//!   `El` wrapper below (a boxed `FnOnce(&mut Mounter)` that mounts the node).
//! * a fill or colour from two signals (theme and hover) is packed into one `u32` by hand (`look`), and
//!   `[Signal<T>; N]` arrays (`pos`, `anim_pos`, `anim_op`, `insp`) stand in for per-node state: a `Vec` cannot
//!   be wired. `.map` closures are at most 16 bytes of captures (12 for `.transition`): a clip rect with its
//!   frame's origin does not fit, so rects are world coordinates.
//! * the harness has no hook after the mount and none at the end of a phase: the steady phase's last gesture
//!   leaves the pointer pressed (a captured drag makes `pointer_move` run the drag route, so the hover phase's
//!   first step would not hover), and `hover(k = 0)` has to release it.
//! * `App::presented_pixels` / `reference_pixels` are BGRA (red and blue swapped in a PNG written as RGBA).
use crate::{scaled, Rng, Scene};
use bough::prelude::*;
use bough::raw::{clip, clip_by, deco_box, no_hit, offset, quad, svg_image, text as rtext, App, Button, Caps, Ev, Grad, Mounter, NumFmt, Shadow};

// ------------------------------------------------------------------------------------------------ geometry

const WIN_W: f32 = 1280.0;
const WIN_H: f32 = 800.0;
const TOOLBAR: f32 = 44.0;
const LEFT: f32 = 240.0;
const RIGHT: f32 = 260.0;
/// The viewport of the canvas (the window less the panels), points.
const CW: f32 = WIN_W - LEFT - RIGHT;
const CH: f32 = WIN_H - TOOLBAR;
pub const FRAMES: usize = 20;
const GRID_COLS: usize = 5;
const FW: f32 = 144.0;
const FH: f32 = 152.0;
const CELL_W: f32 = 156.0;
const CELL_H: f32 = 176.0;
const ROW_H: f32 = 20.0;
const LAYERS_HEAD: f32 = 28.0;
const MAX_DRAG: usize = FRAMES * 3;
const MAX_ANIM: usize = 50;
const NINSP: usize = 12;
const NO_COMP: u32 = 0xff;
const DARK: u32 = 0x100;
const NO_SH: u8 = 255;
/// Wheel travel of the canvas (points) and the detent the steady phase turns by.
const WHEEL_MAX: f32 = 288.0;
const DETENT: f32 = 36.0;

const MOVE: Curve = Curve::out(0.3);
const FADE: Curve = Curve::in_out(0.5);

// ------------------------------------------------------------------------------------------------ look

/// An RGBA8 word (`Shadow::color`, gradient stops: red in the low byte) from `0xRRGGBB` and an alpha.
const fn px(hex: u32, a: u32) -> u32 {
    ((hex >> 16) & 0xff) | (hex & 0xff00) | ((hex & 0xff) << 16) | (a << 24)
}

macro_rules! sh {
    ($dy:expr, $blur:expr, $spread:expr, $hex:expr, $a:expr) => {
        Shadow { dx: 0.0, dy: $dy, blur: $blur, spread: $spread, color: px($hex, $a) }
    };
}

/// Shadow recipes: one and two shadows, blur 2 to 40 (the blur of a shadow is the only blur the API has).
static SHADOWS: [&[Shadow]; 14] = [
    &[sh!(1.0, 2.0, 0.0, 0x000000, 0x50)],
    &[sh!(1.0, 3.0, 0.0, 0x000000, 0x40)],
    &[sh!(2.0, 4.0, 0.0, 0x000000, 0x40)],
    &[sh!(2.0, 6.0, 0.0, 0x1a237e, 0x50)],
    &[sh!(3.0, 8.0, 0.0, 0x000000, 0x40)],
    &[sh!(4.0, 12.0, 0.0, 0x000000, 0x38)],
    &[sh!(6.0, 16.0, 0.0, 0x4a148c, 0x40)],
    &[sh!(8.0, 24.0, 0.0, 0x000000, 0x38)],
    &[sh!(10.0, 32.0, -4.0, 0x000000, 0x40)],
    &[sh!(12.0, 40.0, 0.0, 0x000000, 0x48)],
    &[sh!(1.0, 2.0, 0.0, 0x000000, 0x50), sh!(8.0, 24.0, 0.0, 0x000000, 0x30)],
    &[sh!(2.0, 4.0, 0.0, 0x000000, 0x40), sh!(12.0, 32.0, -2.0, 0x0d47a1, 0x40)],
    &[sh!(0.0, 6.0, 1.0, 0x000000, 0x40), sh!(16.0, 40.0, 0.0, 0x000000, 0x40)],
    &[sh!(3.0, 8.0, 0.0, 0x000000, 0x40), sh!(6.0, 20.0, 2.0, 0xb71c1c, 0x30)],
];
static COMP_SH: [&[Shadow]; 5] = [
    &[sh!(2.0, 6.0, 0.0, 0x1d4ed8, 0x60)],
    &[sh!(1.0, 3.0, 0.0, 0x000000, 0x50)],
    &[sh!(4.0, 14.0, 0.0, 0x000000, 0x40)],
    &[sh!(1.0, 2.0, 0.0, 0x000000, 0x40), sh!(3.0, 10.0, 0.0, 0x6b21a8, 0x40)],
    &[sh!(2.0, 8.0, 0.0, 0x000000, 0x50)],
];
static FRAME_SH: [Shadow; 1] = [sh!(6.0, 24.0, 0.0, 0x000000, 0x50)];

const STOPS: [&[(f32, u32)]; 8] = [
    &[(0.0, px(0x4f8bff, 255)), (1.0, px(0xa05bff, 255))],
    &[(0.0, px(0xff7a3d, 255)), (0.5, px(0xff3d7f, 255)), (1.0, px(0x8e24aa, 255))],
    &[(0.0, px(0x00c9a7, 255)), (1.0, px(0x0077b6, 255))],
    &[(0.0, px(0xfff3b0, 255)), (1.0, px(0xf59e0b, 255))],
    &[(0.0, px(0xffffff, 255)), (1.0, px(0x1e293b, 255))],
    &[(0.0, px(0xffffff, 0xf0)), (0.6, px(0x93c5fd, 0xc0)), (1.0, px(0x1d4ed8, 0x80))],
    &[(0.0, px(0xfef08a, 255)), (0.4, px(0xf97316, 255)), (1.0, px(0x7f1d1d, 255))],
    &[(0.0, px(0xe0f2fe, 255)), (1.0, px(0x4ade80, 255))],
];
static GRADS: [Grad; 10] = [
    Grad { radial: false, angle_deg: 0.0, at: [0.5, 0.5], stops: STOPS[0] },
    Grad { radial: false, angle_deg: 45.0, at: [0.5, 0.5], stops: STOPS[1] },
    Grad { radial: false, angle_deg: 90.0, at: [0.5, 0.5], stops: STOPS[2] },
    Grad { radial: false, angle_deg: 135.0, at: [0.5, 0.5], stops: STOPS[3] },
    Grad { radial: false, angle_deg: 180.0, at: [0.5, 0.5], stops: STOPS[7] },
    Grad { radial: true, angle_deg: 0.0, at: [0.3, 0.3], stops: STOPS[4] },
    Grad { radial: true, angle_deg: 0.0, at: [0.5, 0.5], stops: STOPS[5] },
    Grad { radial: true, angle_deg: 0.0, at: [0.7, 0.2], stops: STOPS[6] },
    Grad { radial: true, angle_deg: 0.0, at: [0.5, 0.9], stops: STOPS[1] },
    Grad { radial: true, angle_deg: 0.0, at: [0.2, 0.5], stops: STOPS[2] },
];

/// Tiny vector icons (24 x 24 viewBox), rasterised once per (document, size).
const ICONS: [&str; 8] = [
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="#3b82f6"/><path d="M7 12.5l3.2 3.2L17 8.5" stroke="#fff" stroke-width="2.5" fill="none"/></svg>"##,
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><rect x="3" y="3" width="18" height="18" rx="4" fill="#f59e0b"/><circle cx="12" cy="12" r="4" fill="#fff"/></svg>"##,
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M12 2l10 18H2z" fill="#ef4444"/></svg>"##,
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M12 2l2.9 6.3 6.9.8-5.1 4.7 1.4 6.8L12 17.3 5.9 20.6l1.4-6.8L2.2 9.1l6.9-.8z" fill="#eab308"/></svg>"##,
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M10 3h4v7h7v4h-7v7h-4v-7H3v-4h7z" fill="#22c55e"/></svg>"##,
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M12 21s-8-5.2-8-11a4.5 4.5 0 018-2.8A4.5 4.5 0 0120 10c0 5.8-8 11-8 11z" fill="#ec4899"/></svg>"##,
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M4 12h13M12 5l7 7-7 7" stroke="#6366f1" stroke-width="3" fill="none" stroke-linecap="round"/></svg>"##,
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="#14b8a6"/><path d="M10 8l6 4-6 4z" fill="#fff"/></svg>"##,
];

const PALETTE: [u32; 12] = [0xff6b6b, 0xffa94d, 0xffd43b, 0x69db7c, 0x38d9a9, 0x4dabf7, 0x748ffc, 0xb197fc, 0xf783ac, 0xe9ecef, 0xced4da, 0x495057];

/// The five components: size and corner radius of every instance, and the fill per theme.
const COMPS: [(f32, f32, f32); 5] = [(34.0, 14.0, 7.0), (16.0, 16.0, 8.0), (38.0, 24.0, 4.0), (26.0, 12.0, 3.0), (18.0, 18.0, 9.0)];
const COMP_LIGHT: [u32; 5] = [0x3b82f6, 0xf59e0b, 0x14b8a6, 0xa855f7, 0x22c55e];
const COMP_DARK: [u32; 5] = [0x1d4ed8, 0xb45309, 0x0f766e, 0x7e22ce, 0x15803d];
const COMP_NAMES: [&str; 5] = ["Button / Primary", "Badge / Count", "Card / Product", "Chip / Filter", "Avatar / Round"];
const NAMES: [&str; 12] = ["Button", "Label", "Title", "Subtitle", "Avatar", "Badge", "Divider", "Input", "Price", "Tag", "Caption", "Menu"];
const FIELDS: [&str; NINSP] = ["X", "Y", "Width", "Height", "Radius", "Opacity", "Rotation", "Blur", "Shadow X", "Shadow Y", "Spread", "Layer"];

fn dark(l: u32) -> bool {
    l & DARK != 0
}
fn chrome(l: u32) -> Color {
    if dark(l) { rgb(0x1f2229) } else { rgb(0xf4f5f7) }
}
fn chrome2(l: u32) -> Color {
    if dark(l) { rgb(0x2a2d35) } else { rgb(0xffffff) }
}
fn canvas_bg(l: u32) -> Color {
    if dark(l) { rgb(0x101217) } else { rgb(0xdcdfe5) }
}
fn ink(l: u32) -> Color {
    if dark(l) { rgb(0xe6e9ef) } else { rgb(0x1d2129) }
}
fn dim(l: u32) -> Color {
    if dark(l) { rgb(0x8b93a3) } else { rgb(0x6b7280) }
}
/// A component instance's fill: its theme colour, or the highlight while the pointer is on its layers row.
fn comp_fill(l: u32, c: u32) -> Color {
    match (dark(l), l & 0xff == c) {
        (false, false) => rgb(COMP_LIGHT[c as usize]),
        (true, false) => rgb(COMP_DARK[c as usize]),
        (false, true) => rgb(0xff3b6b),
        (true, true) => rgb(0xffe14d),
    }
}
fn row_bg(l: u32, c: u32) -> Color {
    if c < 5 && l & 0xff == c { comp_fill(l, c) } else { chrome(l) }
}
// one projection per component: a `fn` pointer is `Copy` and nameable, so the instances share one type
fn comp_fill_0(l: &u32) -> Color {
    comp_fill(*l, 0)
}
fn comp_fill_1(l: &u32) -> Color {
    comp_fill(*l, 1)
}
fn comp_fill_2(l: &u32) -> Color {
    comp_fill(*l, 2)
}
fn comp_fill_3(l: &u32) -> Color {
    comp_fill(*l, 3)
}
fn comp_fill_4(l: &u32) -> Color {
    comp_fill(*l, 4)
}
const COMP_FILL: [fn(&u32) -> Color; 5] = [comp_fill_0, comp_fill_1, comp_fill_2, comp_fill_3, comp_fill_4];

// ------------------------------------------------------------------------------------------------ the document model

#[derive(Clone, Copy)]
struct Kid {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fill: u32,
    sh: u8,
}

#[derive(Clone, Copy)]
enum Kind {
    Card { fill: u32, sh: u8 },
    Grad { g: u8, sh: u8 },
    Border { bw: f32, line: u32, fill: u32 },
    Label { s: u8, n: u8, size: f32 },
    Icon { i: u8 },
    Group { kids: [Kid; 3] },
    Comp(u8),
    FadeA { fill: u32 },
    FadeB { fill: u32 },
    Anim { i: u8, fill: u32 },
    Drag { i: u8, fill: u32 },
}

/// One node of a frame, in the frame's coordinates.
#[derive(Clone, Copy)]
struct Spec {
    kind: Kind,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
}

impl Spec {
    /// (instances, glyphs) the node mounts, for sizing the region.
    fn cost(&self) -> (usize, usize) {
        let nsh = |sh: u8| if sh == NO_SH { 0 } else if (sh as usize) < 10 { 1 } else { 2 };
        match self.kind {
            Kind::Card { sh, .. } => (1 + nsh(sh), 0),
            Kind::Grad { sh, .. } => (1 + nsh(sh), 0),
            Kind::Border { .. } | Kind::Icon { .. } | Kind::FadeA { .. } | Kind::FadeB { .. } | Kind::Anim { .. } | Kind::Drag { .. } => (1, 0),
            Kind::Label { .. } => (0, 12),
            Kind::Group { kids } => (kids.iter().map(|k| 1 + nsh(k.sh)).sum(), 0),
            Kind::Comp(c) => (2 + COMP_SH[c as usize].len() - 1, 0),
        }
    }
    fn layer_name(&self) -> String {
        match self.kind {
            Kind::Card { .. } => "Rectangle".into(),
            Kind::Grad { .. } => "Gradient".into(),
            Kind::Border { .. } => "Frame".into(),
            Kind::Label { s, .. } => format!("Text \"{}\"", NAMES[s as usize]),
            Kind::Icon { .. } => "Icon".into(),
            Kind::Group { .. } => "Group (clipped)".into(),
            Kind::Comp(c) => COMP_NAMES[c as usize].into(),
            Kind::FadeA { .. } => "Overlay A".into(),
            Kind::FadeB { .. } => "Overlay B".into(),
            Kind::Anim { .. } => "Animated".into(),
            Kind::Drag { .. } => "Movable".into(),
        }
    }
}

/// A frame: its nodes in three layers (mixed, then clipped groups, then the animated and movable ones on top).
struct Frame {
    x: f32,
    y: f32,
    title: String,
    mixed: Vec<Spec>,
    groups: Vec<Spec>,
    top: Vec<Spec>,
}

/// A node the user can drag (its position is `pos[i]`, an offset from where it was placed).
struct Drag {
    base: (f32, f32),
    local: (f32, f32),
    w: f32,
    h: f32,
    r: f32,
    name: String,
}

struct Row {
    name: String,
    /// The component this row names (hovering it highlights the component's instances), or `NO_COMP`.
    comp: u32,
    depth: u8,
}

#[derive(Clone, Copy)]
struct Cnt {
    n: usize,
    comps_per: usize,
    grads: usize,
    borders: usize,
    labels: usize,
    icons: usize,
    groups: usize,
    pub fade: usize,
    anim_total: usize,
    layers: usize,
}

impl Cnt {
    fn new(scale: f32) -> Cnt {
        Cnt {
            n: scaled(250, scale, 30),
            comps_per: scaled(10, scale, 1),
            grads: scaled(30, scale, 2),
            borders: scaled(25, scale, 2),
            labels: scaled(25, scale, 2),
            icons: scaled(25, scale, 2),
            groups: scaled(4, scale, 1),
            fade: scaled(30, scale, 2),
            anim_total: scaled(MAX_ANIM, scale, 5),
            layers: scaled(24, scale, 4),
        }
    }
}

pub struct Canvas {
    frames: Vec<Frame>,
    drags: Vec<Drag>,
    rows: Vec<Row>,
    anim_b: Vec<(f32, f32)>,
    nodes: usize,
    comps: usize,
    instances: usize,
    glyphs: usize,
    /// Theme bit (`DARK`) | the component whose layers row is hovered (`NO_COMP`: none).
    pub look: Signal<u32>,
    /// The canvas's pan (points): every frame's content and clip follows it.
    pub pan: Signal<(f32, f32)>,
    /// The wheel's travel on the canvas; `pan` is derived from it.
    pub wheel: Signal<f32>,
    pub layers_scroll: Signal<f32>,
    pub sel_name: Signal<String>,
    /// The inspector's numbers for the selected node.
    pub insp: [Signal<f32>; NINSP],
    /// The selection outline: x, y, w, h in the canvas's coordinates.
    pub ring: Signal<[f32; 4]>,
    pub pos: [Signal<(f32, f32)>; MAX_DRAG],
    pub anim_pos: [Signal<(f32, f32)>; MAX_ANIM],
    pub anim_op: [Signal<f32>; MAX_ANIM],
    pub fade: Signal<f32>,
    // the harness's bookkeeping of one drag gesture (plain fields, never wired)
    pub sel: usize,
    pub drag_from: (f32, f32),
    pub pan0: (f32, f32),
    pub press: Option<(f32, f32)>,
    pub press_k: usize,
}

fn gen_frame(f: usize, c: &Cnt, rng: &mut Rng, drag_id: &mut usize, anim_id: &mut usize, anim_here: usize, drags: &mut Vec<Drag>, anim_b: &mut Vec<(f32, f32)>) -> Frame {
    let (col, row) = (f % GRID_COLS, f / GRID_COLS);
    let (fx, fy) = (6.0 + col as f32 * CELL_W, 24.0 + row as f32 * CELL_H);
    let pal = |rng: &mut Rng| PALETTE[rng.below(PALETTE.len())];
    let alpha = |rng: &mut Rng| [0xff, 0xff, 0xd8, 0xa0][rng.below(4)];
    let place = |rng: &mut Rng, w: f32, h: f32| ((rng.unit() * (FW - w)).floor(), (rng.unit() * (FH - h)).floor());
    let sz = |rng: &mut Rng, lo: usize, n: usize| (lo + rng.below(n)) as f32;
    let mut mixed: Vec<Spec> = Vec::new();
    let mut groups = Vec::new();
    let mut top = Vec::new();
    let others = 5 * c.comps_per + c.grads + c.borders + c.labels + c.icons + 4 * c.groups + if f == 0 { 2 * c.fade } else { 0 } + anim_here + 3;
    let cards = c.n.saturating_sub(others).max(1);
    for _ in 0..cards {
        let (w, h) = (sz(rng, 14, 56), sz(rng, 10, 34));
        let (x, y) = place(rng, w, h);
        let fill = px(pal(rng), alpha(rng));
        mixed.push(Spec { kind: Kind::Card { fill, sh: rng.below(SHADOWS.len()) as u8 }, x, y, w, h, r: sz(rng, 2, 9) });
    }
    for _ in 0..c.grads {
        let (w, h) = (sz(rng, 20, 60), sz(rng, 14, 36));
        let (x, y) = place(rng, w, h);
        let sh = if rng.below(2) == 0 { NO_SH } else { rng.below(SHADOWS.len()) as u8 };
        mixed.push(Spec { kind: Kind::Grad { g: rng.below(GRADS.len()) as u8, sh }, x, y, w, h, r: sz(rng, 3, 10) });
    }
    for _ in 0..c.borders {
        let (w, h) = (sz(rng, 16, 44), sz(rng, 12, 28));
        let (x, y) = place(rng, w, h);
        let (line, fill) = (px(pal(rng), 0xff), px(pal(rng), 0x30));
        mixed.push(Spec { kind: Kind::Border { bw: (1 + rng.below(2)) as f32, line, fill }, x, y, w, h, r: sz(rng, 0, 9) });
    }
    for _ in 0..c.labels {
        let size = sz(rng, 7, 4);
        let (w, h) = (44.0, size * 1.3);
        let (x, y) = place(rng, w, h);
        mixed.push(Spec { kind: Kind::Label { s: rng.below(NAMES.len()) as u8, n: rng.below(100) as u8, size }, x, y, w, h, r: 0.0 });
    }
    for _ in 0..c.icons {
        let s = if rng.below(2) == 0 { 12.0 } else { 16.0 };
        let (x, y) = place(rng, s, s);
        mixed.push(Spec { kind: Kind::Icon { i: rng.below(ICONS.len()) as u8 }, x, y, w: s, h: s, r: 0.0 });
    }
    for ci in 0..5 {
        for _ in 0..c.comps_per {
            let (w, h, r) = COMPS[ci];
            let (x, y) = place(rng, w, h);
            mixed.push(Spec { kind: Kind::Comp(ci as u8), x, y, w, h, r });
        }
    }
    if f == 0 {
        // two states of one layout: state A and state B overlay each other and cross-fade
        for _ in 0..c.fade {
            let (w, h) = (sz(rng, 18, 24), sz(rng, 14, 16));
            let (x, y) = place(rng, w, h);
            let (a, b) = (px(0xff7a3d, 0xff), px(0x3b82f6, 0xff));
            mixed.push(Spec { kind: Kind::FadeA { fill: a }, x, y, w, h, r: 4.0 });
            mixed.push(Spec { kind: Kind::FadeB { fill: b }, x: x + 3.0, y: y + 2.0, w, h, r: 4.0 });
        }
    }
    // a deterministic shuffle: the layers interleave by kind
    for i in (1..mixed.len()).rev() {
        mixed.swap(i, rng.below(i + 1));
    }
    for _ in 0..c.groups {
        let (w, h) = (sz(rng, 40, 30), sz(rng, 30, 20));
        let (x, y) = place(rng, w, h);
        let kid = |rng: &mut Rng| Kid { x: sz(rng, 0, 30) - 14.0, y: sz(rng, 0, 20) - 10.0, w: sz(rng, 24, 28), h: sz(rng, 18, 20), fill: px(pal(rng), 0xff), sh: rng.below(SHADOWS.len()) as u8 };
        let kids = [kid(rng), kid(rng), kid(rng)];
        groups.push(Spec { kind: Kind::Group { kids }, x, y, w, h, r: 0.0 });
    }
    for _ in 0..anim_here {
        let (w, h) = (24.0, 16.0);
        let (x, y) = place(rng, w, h);
        let i = *anim_id;
        *anim_id += 1;
        // the second layout: another place in the same frame
        let (x2, y2) = place(rng, w, h);
        anim_b.push((x2 - x, y2 - y));
        top.push(Spec { kind: Kind::Anim { i: i as u8, fill: px(pal(rng), 0xff) }, x, y, w, h, r: 4.0 });
    }
    for _ in 0..3 {
        let (w, h) = (44.0, 30.0);
        let (x, y) = place(rng, w, h);
        let i = *drag_id;
        *drag_id += 1;
        drags.push(Drag { base: (fx + x, fy + y), local: (x, y), w, h, r: 6.0, name: format!("Movable {:02} / frame {:02}", i, f + 1) });
        top.push(Spec { kind: Kind::Drag { i: i as u8, fill: px(pal(rng), 0xff) }, x, y, w, h, r: 6.0 });
    }
    Frame { x: fx, y: fy, title: format!("Artboard {:02}  {}x{}", f + 1, FW as u32, FH as u32), mixed, groups, top }
}

impl Canvas {
    /// Select node `i`: the inspector and the outline show it; the gesture remembers where it began.
    fn select(&mut self, i: usize) {
        self.sel = i;
        let (t, pan) = (*self.pos[i], *self.pan);
        let d = &self.drags[i];
        let vals = [d.local.0 + t.0, d.local.1 + t.1, d.w, d.h, d.r, 1.0, 0.0, 8.0, 0.0, 3.0, 0.0, i as f32];
        let name = d.name.clone();
        let ring = [d.base.0 + t.0 - 2.0, d.base.1 + t.1 - 2.0, d.w + 4.0, d.h + 4.0];
        for (k, v) in vals.iter().enumerate() {
            self.insp[k].set(*v);
        }
        self.sel_name.set(name);
        self.ring.set(ring);
        self.drag_from = t;
        self.pan0 = pan;
    }

    /// The drag route: pointer travel since the press, less what the canvas panned meanwhile; the node stays in its frame.
    fn drag(&mut self, i: usize, e: &Ev) {
        let pan = *self.pan;
        let d = &self.drags[i];
        let tx = (self.drag_from.0 + e.dx - (pan.0 - self.pan0.0)).clamp(-d.local.0, FW - d.w - d.local.0);
        let ty = (self.drag_from.1 + e.dy - (pan.1 - self.pan0.1)).clamp(-d.local.1, FH - d.h - d.local.1);
        self.insp[0].set(d.local.0 + tx);
        self.insp[1].set(d.local.1 + ty);
        self.ring.set([d.base.0 + tx - 2.0, d.base.1 + ty - 2.0, d.w + 4.0, d.h + 4.0]);
        self.pos[i].set((tx, ty));
    }

    fn hover_component(&mut self, c: u32, on: bool) {
        let l = *self.look;
        let next = if on { (l & !0xff) | c } else if l & 0xff == c { (l & !0xff) | NO_COMP } else { l };
        self.look.set(next);
    }

    /// The canvas's pan follows the wheel's travel (a wheel writes one `f32`; the offset operand takes a pair).
    fn pan_to_wheel(&mut self) {
        let w = *self.wheel;
        self.pan.set((-0.5 * w, -w));
    }

    /// Window position (points) of the centre of draggable node `i`.
    fn screen_of(&self, i: usize) -> (f32, f32) {
        let (d, t, p) = (&self.drags[i], *self.pos[i], *self.pan);
        (LEFT + p.0 + d.base.0 + t.0 + d.w * 0.5, TOOLBAR + p.1 + d.base.1 + t.1 + d.h * 0.5)
    }
    fn on_screen(&self, i: usize) -> bool {
        let (x, y) = self.screen_of(i);
        x > LEFT + 10.0 && x < LEFT + CW - 10.0 && y > TOOLBAR + 10.0 && y < TOOLBAR + CH - 10.0
    }
}

// ------------------------------------------------------------------------------------------------ the description

/// A node of a frame as an element. The nodes are of many types (shadowed cards, gradient boxes, labels, icons,
/// components, flow boxes), and `Element` is not object safe (its `seal` is generic), so a design tool's list
/// of heterogeneous layers needs this forwarding wrapper: the node is mounted by a boxed closure.
struct El<'a>(Box<dyn FnOnce(&mut Mounter<'_, Canvas>) + 'a>);

fn el<'a>(e: impl Element<Canvas> + 'a) -> El<'a> {
    El(Box::new(move |m| e.mount(m)))
}

impl Element<Canvas> for El<'_> {
    fn mount(self, m: &mut Mounter<'_, Canvas>) {
        (self.0)(m)
    }
    // only ever inside the canvas's anchor, which a re-description steps over
    fn update(self, _: &mut Mounter<'_, Canvas>) {}
}

const NUM: NumFmt = NumFmt::new("", Some(1), "");

impl Canvas {
    fn spec_el(&self, s: &Spec, fx: f32, fy: f32) -> El<'_> {
        // flow boxes sit in the canvas box (`abs` is from its corner); raw elements are window coordinates
        let (x, y, w, h) = (fx + s.x, fy + s.y, s.w, s.h);
        let (rx, ry) = (LEFT + x, TOOLBAR + y);
        let shadows = |sh: u8| -> &'static [Shadow] { if sh == NO_SH { &[] } else { SHADOWS[sh as usize] } };
        match s.kind {
            Kind::Card { fill, sh } => el(deco_box(rx, ry, w, h, s.r).bg(Color(fill)).shadows(shadows(sh))),
            Kind::Grad { g, sh } => el(deco_box(rx, ry, w, h, s.r).gradient(GRADS[g as usize]).shadows(shadows(sh))),
            Kind::Border { bw, line, fill } => el(quad(rx, ry, w, h).bg(Color(fill)).rounded(s.r).border(bw, Color(line))),
            Kind::Label { s: name, n, size } => el(rtext(format!("{} {}", NAMES[name as usize], n)).at(rx, ry).size(size).line_height(size * 1.3).color(rgb(0x2a2f3a))),
            Kind::Icon { i } => el(svg_image(rx, ry, w, h, ICONS[i as usize])),
            Kind::Comp(c) => el((deco_box(rx, ry, w, h, s.r).shadows(COMP_SH[c as usize]), quad(rx, ry, w, h).bg(sig!(self.look).map(COMP_FILL[c as usize])).rounded(s.r).border(1.0, Color(px(0x000000, 0x59))))),
            Kind::FadeA { fill } => el(div().abs(x, y).w(w).h(h).bg(Color(fill)).rounded(s.r).opacity(sig!(self.fade).map(|f: &f32| *f).transition(FADE))),
            Kind::FadeB { fill } => el(div().abs(x, y).w(w).h(h).bg(Color(fill)).rounded(s.r).opacity(sig!(self.fade).map(|f: &f32| 1.0 - *f).transition(FADE))),
            Kind::Anim { i, fill } => el(div().abs(x, y).w(w).h(h).bg(Color(fill)).rounded(s.r).translate(sig!(self.anim_pos).at(i as usize).transition(MOVE)).opacity(sig!(self.anim_op).at(i as usize).transition(MOVE))),
            Kind::Drag { i, fill } => {
                let i = i as usize;
                el(div().abs(x, y).w(w).h(h).bg(Color(fill)).rounded(s.r).translate(sig!(self.pos).at(i)).on_press(move |c: &mut Canvas, _: &Ev| c.select(i)).on_drag(move |c: &mut Canvas, e: &Ev| c.drag(i, e)))
            }
            Kind::Group { .. } => unreachable!("groups mount through group_el"),
        }
    }

    /// A clipped group: a clip rect that follows the pan, around a space that follows it too. (Nested wired
    /// clips read their parent's rect when they patch, and wires run newest first, so a group's clip inside
    /// its frame's would read a stale parent: the group is its own clip under the viewport instead, and its
    /// rect is already inside the frame's.)
    fn group_el(&self, f: usize, s: &Spec) -> El<'_> {
        let Kind::Group { kids } = s.kind else { unreachable!() };
        let fr = &self.frames[f];
        let (fx, fy) = (fr.x, fr.y);
        let r = [LEFT + fx + s.x, TOOLBAR + fy + s.y, s.w, s.h];
        let kid = move |k: &Kid| deco_box(r[0] + k.x, r[1] + k.y, k.w, k.h, 4.0).bg(Color(k.fill)).shadows(SHADOWS[k.sh as usize]);
        el(clip_by(sig!(self.pan), move |p: &(f32, f32)| [r[0] + p.0, r[1] + p.1, r[2], r[3]], offset(sig!(self.pan), (kid(&kids[0]), kid(&kids[1]), kid(&kids[2])))))
    }

    fn frame_el(&self, f: usize) -> impl Element<Self> + '_ {
        let fr = &self.frames[f];
        let (fx, fy) = (fr.x, fr.y);
        let rect = move |p: &(f32, f32)| [LEFT + fx + p.0, TOOLBAR + fy + p.1, FW, FH];
        (
            clip_by(sig!(self.pan), rect, offset(sig!(self.pan), each(fr.mixed.iter().map(move |s| self.spec_el(s, fx, fy))))),
            each(fr.groups.iter().map(move |s| self.group_el(f, s))),
            clip_by(sig!(self.pan), rect, offset(sig!(self.pan), each(fr.top.iter().map(move |s| self.spec_el(s, fx, fy))))),
        )
    }

    fn layer_row(&self, i: usize) -> impl Element<Self> + '_ {
        let r = &self.rows[i];
        let c = r.comp;
        div()
            .h(ROW_H)
            .pad(8.0 + 10.0 * r.depth as f32, 0.0, 8.0, 0.0)
            .justify(Justify::Center)
            .bg(sig!(self.look).map(move |l: &u32| row_bg(*l, c)))
            .child(text(r.name.clone()).font(11.0, 14.0).color(sig!(self.look).map(|l: &u32| ink(*l))))
            .hover_bg(rgb(0xbcd4ff))
            .on_hover(move |s: &mut Canvas, e: &Ev| s.hover_component(c, e.on))
    }

    fn field(&self, i: usize) -> impl Element<Self> + '_ {
        row((
            text(FIELDS[i]).font(11.0, 16.0).w(90.0).color(sig!(self.look).map(|l: &u32| dim(*l))),
            text(sig!(self.insp).at(i).num(|v: &f32| *v, NUM)).font(11.0, 16.0).color(sig!(self.look).map(|l: &u32| ink(*l))),
        ))
        .h(22.0)
        .items(Align::Center)
    }
}

impl View for Canvas {
    fn view(&self) -> impl Element<Self> + '_ {
        let toolbar = row((
            text("bough design").font(14.0, 20.0).color(sig!(self.look).map(|l: &u32| ink(*l))),
            button("Move"),
            button("Frame"),
            button("Rectangle"),
            button("Text"),
            div().grow(1.0),
            text(sig!(self.wheel).num(|v: &f32| *v, NumFmt::new("pan ", Some(0), " pt"))).font(12.0, 16.0).color(sig!(self.look).map(|l: &u32| dim(*l))),
            text("100%").font(12.0, 16.0).color(sig!(self.look).map(|l: &u32| ink(*l))),
        ))
        .h(TOOLBAR)
        .shrink(0.0)
        .px(14.0)
        .gap(10.0)
        .items(Align::Center)
        .bg(sig!(self.look).map(|l: &u32| chrome2(*l)));

        let layers = column((
            row((text("Layers").font(12.0, 16.0).color(sig!(self.look).map(|l: &u32| ink(*l))), text(format!("{} rows", self.rows.len())).font(11.0, 16.0).color(sig!(self.look).map(|l: &u32| dim(*l)))))
                .h(LAYERS_HEAD)
                .shrink(0.0)
                .px(10.0)
                .gap(8.0)
                .items(Align::Center)
                .bg(sig!(self.look).map(|l: &u32| chrome2(*l))),
            column(each((0..self.rows.len()).map(|i| self.layer_row(i)))).grow(1.0).min_h(0.0).scroll(sig!(self.layers_scroll)),
        ))
        .w(LEFT)
        .shrink(0.0)
        .bg(sig!(self.look).map(|l: &u32| chrome(*l)));

        // the canvas: backdrops (frames, their shadows and titles), then every frame's content, then the outline
        let backdrop = each(self.frames.iter().map(|fr| (deco_box(LEFT + fr.x, TOOLBAR + fr.y, FW, FH, 6.0).bg(rgb(0xffffff)).shadows(&FRAME_SH), rtext(fr.title.clone()).at(LEFT + fr.x, TOOLBAR + fr.y - 13.0).size(9.0).line_height(12.0).color(rgb(0x8a90a0)))));
        let ring = div()
            .abs(0.0, 0.0)
            .w_op(sig!(self.ring).map(|r: &[f32; 4]| r[2]))
            .h_op(sig!(self.ring).map(|r: &[f32; 4]| r[3]))
            .border(2.0, rgb(0x2f6bff))
            .translate(sig!(self.ring).map(|r: &[f32; 4]| (r[0], r[1])));
        let world = (offset(sig!(self.pan), backdrop), each((0..self.frames.len()).map(|f| self.frame_el(f))), offset(sig!(self.pan), no_hit(ring)));
        let canvas = div()
            .grow(1.0)
            .min_w(0.0)
            .bg(sig!(self.look).map(|l: &u32| canvas_bg(*l)))
            .relative()
            .child(clip(LEFT, TOOLBAR, CW, CH, world))
            .scrolls(sig!(self.wheel), 1.0, WHEEL_MAX);

        let inspector = column((
            text("Design").font(12.0, 16.0).color(sig!(self.look).map(|l: &u32| dim(*l))),
            text(sig!(self.sel_name)).font(13.0, 18.0).color(sig!(self.look).map(|l: &u32| ink(*l))),
            column(each((0..NINSP).map(|i| self.field(i)))).gap(4.0),
            row((block(24.0, 24.0).bg(rgb(0x3b82f6)).rounded(4.0), block(24.0, 24.0).bg(rgb(0xf59e0b)).rounded(4.0), block(24.0, 24.0).bg(rgb(0x22c55e)).rounded(12.0))).gap(8.0),
        ))
        .w(RIGHT)
        .shrink(0.0)
        .p(14.0)
        .gap(10.0)
        .bg(sig!(self.look).map(|l: &u32| chrome(*l)));

        column((toolbar, row((layers, canvas, inspector)).grow(1.0).min_h(0.0))).w_pct(1.0).h_pct(1.0).bg(sig!(self.look).map(|l: &u32| chrome(*l)))
    }
}

// ------------------------------------------------------------------------------------------------ the scene

impl Scene for Canvas {
    const NAME: &'static str = "canvas";

    fn build(scale: f32) -> Canvas {
        let cnt = Cnt::new(scale);
        let mut rng = Rng::new(0xca17_a5);
        let (mut drags, mut anim_b) = (Vec::new(), Vec::new());
        let (mut drag_id, mut anim_id) = (0usize, 0usize);
        let mut frames = Vec::new();
        for f in 0..FRAMES {
            let anim_here = cnt.anim_total / FRAMES + usize::from(f < cnt.anim_total % FRAMES);
            frames.push(gen_frame(f, &cnt, &mut rng, &mut drag_id, &mut anim_id, anim_here, &mut drags, &mut anim_b));
        }
        let (mut nodes, mut comps, mut instances, mut glyphs) = (0, 0, 0, 0);
        for fr in &frames {
            instances += 2;
            glyphs += fr.title.len();
            for s in fr.mixed.iter().chain(&fr.top) {
                nodes += 1;
                comps += usize::from(matches!(s.kind, Kind::Comp(_)));
                let (i, g) = s.cost();
                instances += i;
                glyphs += g;
            }
            for s in &fr.groups {
                nodes += 4;
                instances += s.cost().0;
            }
        }
        let mut rows: Vec<Row> = (0..5).map(|c| Row { name: format!("{} (main)", COMP_NAMES[c]), comp: c as u32, depth: 0 }).collect();
        for fr in &frames {
            rows.push(Row { name: fr.title.clone(), comp: NO_COMP, depth: 0 });
            for (j, s) in fr.mixed.iter().chain(&fr.groups).chain(&fr.top).take(cnt.layers).enumerate() {
                let comp = if let Kind::Comp(c) = s.kind { c as u32 } else { NO_COMP };
                rows.push(Row { name: format!("{} {}", s.layer_name(), j + 1), comp, depth: 1 });
            }
        }
        let mut c = Canvas {
            frames,
            drags,
            rows,
            anim_b,
            nodes,
            comps,
            instances,
            glyphs,
            look: Signal::new(NO_COMP),
            pan: Signal::new((0.0, 0.0)),
            wheel: Signal::new(0.0),
            layers_scroll: Signal::new(0.0),
            sel_name: Signal::new(String::new()),
            insp: std::array::from_fn(|_| Signal::new(0.0)),
            ring: Signal::new([0.0; 4]),
            pos: std::array::from_fn(|_| Signal::new((0.0, 0.0))),
            anim_pos: std::array::from_fn(|_| Signal::new((0.0, 0.0))),
            anim_op: std::array::from_fn(|_| Signal::new(1.0)),
            fade: Signal::new(1.0),
            sel: 0,
            drag_from: (0.0, 0.0),
            pan0: (0.0, 0.0),
            press: None,
            press_k: 0,
        };
        c.select(0);
        c
    }

    fn caps(&self) -> Caps {
        let rows = self.rows.len();
        let instances = (self.instances * 5 / 4 + rows * 20 + 4096).next_power_of_two();
        let records = self.nodes * 3 + rows * 4 + 4096;
        Caps {
            instances: instances.min(Caps::RESERVE.instances),
            pages: (records / 32 + 1024).next_power_of_two().min(Caps::RESERVE.pages),
            glyphs: ((self.glyphs + rows * 16) * 2 + 65536).next_power_of_two().min(Caps::RESERVE.glyphs),
            runs: (self.nodes * 3 + rows * 3 + 8192).next_power_of_two().min(Caps::RESERVE.runs),
            spaces: 1024,
            clips: 1024,
            ..Caps::DEFAULT
        }
    }

    fn describe(&self) -> String {
        let anim = self.anim_b.len();
        format!(
            "{} frames, {} nodes ({} component instances, {} movable, {} animated, {} cross-fading), {} layer rows, ~{} instances",
            self.frames.len(),
            self.nodes,
            self.comps,
            self.drags.len(),
            anim,
            self.frames[0].mixed.iter().filter(|s| matches!(s.kind, Kind::FadeA { .. } | Kind::FadeB { .. })).count(),
            self.rows.len(),
            self.instances
        )
    }

    fn fanout(&self) -> usize {
        // every component instance reads `look` (one wire each); the hovered component's repaint
        self.comps
    }

    fn steady(app: &mut App<Self>, k: usize) {
        if k % 12 == 11 {
            // a wheel detent over the canvas, alternating down and up; the pan follows the wheel's travel
            let dir = if (k / 12) % 16 < 8 { 1.0 } else { -1.0 };
            app.wheel_points(LEFT + CW * 0.5, TOOLBAR + CH * 0.5, DETENT * dir);
            app.turn(|s| s.pan_to_wheel());
        }
        if k % 8 == 7 || k == 0 {
            // the selection changes: the last gesture ends, the pointer goes to the next node that is in view and presses it
            if let Some((x, y)) = app.owner.press.take() {
                app.pointer_up(x, y, Button::Primary);
            }
            let n = app.owner.drags.len();
            let mut j = (app.owner.sel + 7) % n;
            for _ in 0..n {
                if app.owner.on_screen(j) {
                    break;
                }
                j = (j + 7) % n;
            }
            let (x, y) = app.owner.screen_of(j);
            app.pointer_move(x, y);
            app.pointer_down(x, y, Button::Primary);
            app.owner.press = Some((x, y));
            app.owner.press_k = k;
        } else if let Some((x0, y0)) = app.owner.press {
            // the pointer travels on while pressed (a small loop), the drag route moves the node and the inspector
            let t = (k - app.owner.press_k) as f32;
            app.pointer_move(x0 + 22.0 * (0.45 * t).sin(), y0 + 14.0 * (0.31 * t).cos() - 14.0);
        } else {
            app.turn(|_| {});
        }
    }

    fn hover(app: &mut App<Self>, k: usize) {
        if let Some((x, y)) = app.owner.press.take() {
            app.pointer_up(x, y, Button::Primary); // the steady phase's last gesture ends
        }
        // onto the layers row of component 3 (index 2), then up into the toolbar
        let y = TOOLBAR + LAYERS_HEAD + 2.0 * ROW_H + ROW_H * 0.5;
        if k % 2 == 0 {
            app.pointer_move(LEFT * 0.5, y);
        } else {
            app.pointer_move(LEFT * 0.5, TOOLBAR * 0.5);
        }
    }

    fn theme(app: &mut App<Self>, _k: usize) {
        app.turn(|s| {
            let l = *s.look ^ DARK;
            s.look.set(l);
        });
    }

    fn animate(app: &mut App<Self>, k: usize) {
        // smart animate: every animated node goes to its other layout (position and opacity), and the cross-fading
        // frame fades to its other state; retargeting what is still moving
        app.turn(|s| {
            let b = k % 2 == 0;
            for i in 0..s.anim_b.len() {
                let (bx, by) = s.anim_b[i];
                s.anim_pos[i].set(if b { (bx, by) } else { (0.0, 0.0) });
                s.anim_op[i].set(if b { 0.35 } else { 1.0 });
            }
            s.fade.set(if b { 0.0 } else { 1.0 });
        });
    }
}
