//! Layout templates: a document known at compile time as a preorder table of nodes whose styles are CSS
//! declarations (computed values), resolved at mount by bough's own path into cephal `Style`s and laid
//! out by cephal from scratch, with text measured and wrapped (`para`'s leaves).
//!
//! * **Styles.** A template carries its distinct declaration lists once (`&[(P, &str)]`, like compiled
//!   style rules); a node names one by index. The first node that uses a list resolves it (each value
//!   through cephal's own parsers: `parse_dimension`, `parse_grid_template`, …) into a `Style`, interned
//!   once; every other node shares the id. There is no selector matching: bough has no cascade (a
//!   bough author states an element's style where the element is written).
//! * **Records.** A painted element is a quad record with a layout node (its instance is placed by the
//!   write-back), a paint-less element a box record, a text node a measured leaf; `display: contents`
//!   elements are transparent (their children join the parent), `display: none` subtrees are absent.
//! * **Clips follow their boxes.** An element with `overflow` other than `visible` owns a clip id whose
//!   rect the write-back sets from the laid-out box (∩ its parent's clip), and inside a scroll view the
//!   clip is registered with the view so a scroll moves it.
//! * **Hooks** mount the nodes the application owns (wired, interactive) with typed builders.
use crate::color::Color;
pub use crate::deco::{Deco, Grad, Shadow, Xf};
use crate::elem::{quad_instance, Element, Mounter};
use crate::layout::{attach, LeafCtx};
use crate::rt::*;
use crate::text::{TShape, TextNode};
use bough_core::{NRect, RecordInit, KF_FOCUS, KF_INTERACT};
use cephal::style::parse::*;
use cephal::style::{Display, GridContainerStyle, Ident};
use cephal::Style;
use std::borrow::Cow;

/// `v` with every `<number>px` multiplied by `s` (layout runs in device px).
fn scaled(v: &str, s: f32) -> Cow<'_, str> {
    if s == 1.0 || !v.contains("px") {
        return Cow::Borrowed(v);
    }
    let mut out = String::with_capacity(v.len() + 8);
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        if b[i].is_ascii_digit() || b[i] == b'.' || (b[i] == b'-' && i + 1 < b.len() && (b[i + 1].is_ascii_digit() || b[i + 1] == b'.')) {
            i += 1;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            let num = &v[start..i];
            if v[i..].starts_with("px") {
                let x: f32 = num.parse().unwrap_or(0.0);
                out.push_str(&format!("{}", x * s));
            } else {
                out.push_str(num);
            }
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    Cow::Owned(out)
}

/// A layout property a declaration sets (CSS longhands, computed values).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum P {
    Display,
    Position,
    FlexDirection,
    FlexWrap,
    FlexGrow,
    FlexShrink,
    FlexBasis,
    Width,
    Height,
    MinWidth,
    MinHeight,
    MaxWidth,
    MaxHeight,
    PadT,
    PadR,
    PadB,
    PadL,
    MarT,
    MarR,
    MarB,
    MarL,
    BorT,
    BorR,
    BorB,
    BorL,
    RowGap,
    ColGap,
    AlignItems,
    AlignSelf,
    JustifyContent,
    JustifyItems,
    OverflowX,
    OverflowY,
    GridTemplateColumns,
    GridTemplateRows,
    GridAutoRows,
    GridColumnStart,
    GridColumnEnd,
    GridRowStart,
    GridRowEnd,
    Top,
    Right,
    Bottom,
    Left,
    AspectRatio,
    BoxSizing,
}

pub type Decls = &'static [(P, &'static str)];

/// Resolve declarations into a cephal `Style` (cephal's own value parsers; unknown values keep the
/// initial value).
pub fn resolve_style(decls: &[(P, &str)], scale: f32) -> Style {
    let mut st = Style::DEFAULT;
    let mut grid = GridContainerStyle::default();
    let mut is_grid = false;
    let mut name = |_: &str| Ident(0);
    for &(p, v) in decls {
        let v: &str = &scaled(v, scale);
        let lpa = || parse_length_percentage_auto(if v == "none" { "auto" } else { v });
        let lp = || parse_length_percentage(if v == "normal" { "0" } else { v });
        match p {
            P::Display => {
                if let Some(d) = parse_display(v) {
                    st.display = d;
                    is_grid = d == Display::Grid;
                }
            }
            P::Position => st.position = parse_position(v).unwrap_or(st.position),
            P::FlexDirection => st.flex_direction = parse_flex_direction(v).unwrap_or(st.flex_direction),
            P::FlexWrap => st.flex_wrap = parse_flex_wrap(v).unwrap_or(st.flex_wrap),
            P::FlexGrow => st.flex_grow = v.parse().unwrap_or(st.flex_grow),
            P::FlexShrink => st.flex_shrink = v.parse().unwrap_or(st.flex_shrink),
            P::FlexBasis => st.flex_basis = parse_dimension(v).unwrap_or(st.flex_basis),
            P::Width => st.size.width = parse_dimension(v).unwrap_or(st.size.width),
            P::Height => st.size.height = parse_dimension(v).unwrap_or(st.size.height),
            P::MinWidth => st.min_size.width = lpa().unwrap_or(st.min_size.width),
            P::MinHeight => st.min_size.height = lpa().unwrap_or(st.min_size.height),
            P::MaxWidth => st.max_size.width = lpa().unwrap_or(st.max_size.width),
            P::MaxHeight => st.max_size.height = lpa().unwrap_or(st.max_size.height),
            P::PadT => st.padding.top = lp().unwrap_or(st.padding.top),
            P::PadR => st.padding.right = lp().unwrap_or(st.padding.right),
            P::PadB => st.padding.bottom = lp().unwrap_or(st.padding.bottom),
            P::PadL => st.padding.left = lp().unwrap_or(st.padding.left),
            P::MarT => st.margin.top = lpa().unwrap_or(st.margin.top),
            P::MarR => st.margin.right = lpa().unwrap_or(st.margin.right),
            P::MarB => st.margin.bottom = lpa().unwrap_or(st.margin.bottom),
            P::MarL => st.margin.left = lpa().unwrap_or(st.margin.left),
            P::BorT => st.border.top = lp().unwrap_or(st.border.top),
            P::BorR => st.border.right = lp().unwrap_or(st.border.right),
            P::BorB => st.border.bottom = lp().unwrap_or(st.border.bottom),
            P::BorL => st.border.left = lp().unwrap_or(st.border.left),
            P::RowGap => st.gap.height = lp().unwrap_or(st.gap.height),
            P::ColGap => st.gap.width = lp().unwrap_or(st.gap.width),
            P::AlignItems => st.align_items = if v == "normal" { None } else { parse_align_items(v) },
            P::AlignSelf => st.align_self = if v == "auto" { None } else { parse_align_items(v) },
            P::JustifyContent => st.justify_content = if v == "normal" { None } else { parse_align_content(v) },
            P::JustifyItems => st.justify_items = if v == "normal" || v == "legacy" { None } else { parse_align_items(v) },
            P::OverflowX => st.overflow.x = parse_overflow(v).unwrap_or(st.overflow.x),
            P::OverflowY => st.overflow.y = parse_overflow(v).unwrap_or(st.overflow.y),
            P::GridTemplateColumns => {
                if let Some((t, names)) = parse_grid_template(v, &mut name) {
                    grid.template_columns = t;
                    grid.template_column_names = names;
                }
            }
            P::GridTemplateRows => {
                if let Some((t, names)) = parse_grid_template(v, &mut name) {
                    grid.template_rows = t;
                    grid.template_row_names = names;
                }
            }
            P::GridAutoRows => {
                if let Some(t) = parse_grid_auto_tracks(v) {
                    grid.auto_rows = t;
                }
            }
            P::GridColumnStart => st.grid_column.start = parse_grid_placement(v, &mut name).unwrap_or(st.grid_column.start),
            P::GridColumnEnd => st.grid_column.end = parse_grid_placement(v, &mut name).unwrap_or(st.grid_column.end),
            P::GridRowStart => st.grid_row.start = parse_grid_placement(v, &mut name).unwrap_or(st.grid_row.start),
            P::GridRowEnd => st.grid_row.end = parse_grid_placement(v, &mut name).unwrap_or(st.grid_row.end),
            P::Top => st.inset.top = lpa().unwrap_or(st.inset.top),
            P::Right => st.inset.right = lpa().unwrap_or(st.inset.right),
            P::Bottom => st.inset.bottom = lpa().unwrap_or(st.inset.bottom),
            P::Left => st.inset.left = lpa().unwrap_or(st.inset.left),
            P::AspectRatio => {
                let mut it = v.split('/').map(|x| x.trim().parse::<f32>().ok());
                if let (Some(Some(a)), Some(Some(b))) = (it.next(), it.next()) {
                    st.aspect_ratio = Some(a / b);
                } else if let Ok(a) = v.trim().parse::<f32>() {
                    st.aspect_ratio = Some(a);
                }
            }
            P::BoxSizing => st.box_sizing = parse_box_sizing(v).unwrap_or(st.box_sizing),
        }
    }
    if is_grid {
        st.grid = Some(std::sync::Arc::new(grid));
    }
    st
}

/// Node flags.
pub const L_FOCUS: u16 = 1;
pub const L_CLIP: u16 = 2;
pub const L_HOOK: u16 = 4;
pub const L_TEXT: u16 = 8;
pub const L_NOWRAP: u16 = 16;
/// `display: contents`: no box; the children join the parent.
pub const L_CONTENTS: u16 = 32;
/// `display: none`: the subtree is absent.
pub const L_NONE: u16 = 64;
/// `visibility: hidden`: laid out, not painted.
pub const L_HIDDEN: u16 = 128;

#[derive(Clone, Copy, Debug)]
pub struct LNode {
    pub depth: u16,
    /// Index into the template's declaration lists (elements).
    pub style: u16,
    /// The size zgui laid the node out at (points): for hooks that mount fixed-size widgets.
    pub size: [f32; 2],
    pub bg: u32,
    pub border_w: f32,
    pub border: u32,
    pub radius: f32,
    /// `:hover` background (0: none).
    pub hover: u32,
    pub text: &'static str,
    pub font: f32,
    pub line_h: f32,
    pub color: u32,
    /// Font face (0 regular, 1 bold, 2 mono, 3 oblique).
    pub face: u8,
    pub flags: u16,
    pub tag: u16,
    /// Decorations: index into the template's [`Deco`] table (0: none).
    pub deco: u16,
}

/// What a layout-template hook did with a node.
pub enum LHooked {
    /// The template mounts it.
    Default,
    /// The hook mounted the node and its subtree.
    Subtree,
}

/// The hook's view of the template while mounting one node: it can mount the node the template's way and
/// get its act (to install routes), its layout node and its record.
pub struct LCtx<'t> {
    pub node: &'t LNode,
    style_ids: &'t mut [u32],
    decls: &'t [Decls],
    text_style: u32,
    decos: &'t [Deco],
}

impl LCtx<'_> {
    /// Mount the node as the template would; returns (layout node, record index in the mount batch).
    pub fn mount_default<V: 'static>(&mut self, m: &mut Mounter<'_, V>) -> (u32, usize) {
        mount_element(m, self.node, self.style_ids, self.decls, self.text_style, self.decos)
    }
    /// The interned style id of a declaration list (resolved on first use).
    pub fn style_id<V>(&mut self, m: &mut Mounter<'_, V>, index: usize) -> u32 {
        style_of(m, self.style_ids, self.decls, index)
    }
    /// Make the node's record interactive and return its act (install ops with `ActOp::install`).
    pub fn act<V>(&mut self, m: &mut Mounter<'_, V>, rec: usize) -> u32 {
        crate::input::act_at(m, rec)
    }
    /// Mount `e` as content of layout node `lid` (one level below it), outside the flow unless `e` is a
    /// layout element.
    pub fn mount_inside<V, E: Element<V>>(&mut self, m: &mut Mounter<'_, V>, lid: u32, e: E) {
        let (d, cur) = (m.depth, m.rt.lay.cur);
        m.depth += 1;
        m.rt.lay.cur = lid;
        e.mount(m);
        m.depth = d;
        m.rt.lay.cur = cur;
    }
}

/// A wire that restyles layout node `lid` from a signal: `f(&value, &mut style)` edits a copy of the node's
/// current style (a resizable panel's basis); the node's group is laid out again.
pub fn wire_restyle<T: 'static, V, F: Fn(&T, &mut Style) + Copy + 'static, S: Restyled<T, V>>(m: &mut Mounter<'_, V>, src: S, lid: u32, f: F) {
    src.install(m, lid, f);
}

/// What drives a layout restyle (`wire_restyle`): a wired signal (each change restyles once), or a
/// transition of one (`.transition(curve)`: the frame clock restyles with each eased value, so the box
/// relayouts every frame of the transition).
pub trait Restyled<T, V> {
    fn install<F: Fn(&T, &mut Style) + Copy + 'static>(self, m: &mut Mounter<'_, V>, lid: u32, f: F);
}

impl<T: 'static, V> Restyled<T, V> for crate::signal::Wired<'_, T, V> {
    fn install<F: Fn(&T, &mut Style) + Copy + 'static>(self, m: &mut Mounter<'_, V>, lid: u32, f: F) {
        restyle_wired(m, self, lid, f)
    }
}

impl<T: crate::transition::Lerp, V> Restyled<T, V> for crate::transition::Transit<crate::signal::Wired<'_, T, V>> {
    fn install<F: Fn(&T, &mut Style) + Copy + 'static>(self, m: &mut Mounter<'_, V>, lid: u32, f: F) {
        let init = *self.w.get();
        let id = m.rt.transitions.add::<T>(lid, Box::new(move |rt: &mut Rt, lid: u32, v: &T| rt.lay_restyle(lid, |st| f(v, st))), init, self.curve);
        let mut recipe = [0u8; 16];
        recipe[..4].copy_from_slice(&id.to_le_bytes());
        m.wire(&self.w, crate::transition::patch_transit::<T>, lid, recipe);
        let mut st = m.rt.lay.eng.style(lid).clone();
        f(&init, &mut st);
        let sid = m.rt.lay.eng.intern(st);
        m.rt.lay.eng.set_style_fresh(lid, sid);
    }
}

fn restyle_wired<T: 'static, V, F: Fn(&T, &mut Style) + Copy + 'static>(m: &mut Mounter<'_, V>, sig: crate::signal::Wired<'_, T, V>, lid: u32, f: F) {
    const { assert!(size_of::<F>() <= 16, "restyle closure: at most 16 bytes of captures") };
    let mut r = [0u8; 16];
    // SAFETY: size_of::<F>() <= 16 (checked above); unaligned write into a byte array.
    unsafe { std::ptr::write_unaligned(r.as_mut_ptr() as *mut F, f) };
    m.wire(&sig, patch_restyle::<T, F>, lid, r);
    // the initial value applies at mount (the node's style starts as the template's; it is laid out cold)
    let mut st = m.rt.lay.eng.style(lid).clone();
    f(sig.get(), &mut st);
    let id = m.rt.lay.eng.intern(st);
    m.rt.lay.eng.set_style_fresh(lid, id);
}

/// # Safety
/// Wire contract: `sig` is a `Signal<T>`; the recipe holds an `F`; `w.node` is the layout node.
unsafe fn patch_restyle<T: 'static, F: Fn(&T, &mut Style) + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { &*(sig as *const crate::signal::Signal<T>) };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    rt.lay_restyle(w.node, |st| f(s, st));
}

pub type LHook<V> = fn(&V, &mut Mounter<'_, V>, &mut LCtx<'_>) -> LHooked;

pub struct LTemplate<V: 'static> {
    nodes: &'static [LNode],
    decls: &'static [Decls],
    /// Available size (points) of the template's root (its group); `NaN` height: max-content; negative:
    /// the window's (the group then follows the window on resize).
    avail: [f32; 2],
    hook: LHook<V>,
    decos: &'static [Deco],
}

pub fn ltemplate<V>(nodes: &'static [LNode], decls: &'static [Decls], avail: [f32; 2], hook: LHook<V>) -> LTemplate<V> {
    LTemplate { nodes, decls, avail, hook, decos: &[] }
}

impl<V> LTemplate<V> {
    /// The decorations the nodes' `deco` fields index (gradients, box shadows, vector icons; deco.rs).
    pub fn decos(mut self, t: &'static [Deco]) -> Self {
        self.decos = t;
        self
    }
}

fn style_of<V>(m: &mut Mounter<'_, V>, ids: &mut [u32], decls: &[Decls], i: usize) -> u32 {
    if ids[i] == NONE {
        let st = resolve_style(decls[i], m.rt.scale);
        ids[i] = m.rt.lay.eng.intern(st);
    }
    ids[i]
}

fn mount_element<V: 'static>(m: &mut Mounter<'_, V>, n: &LNode, ids: &mut [u32], decls: &[Decls], text_style: u32, decos: &[Deco]) -> (u32, usize) {
    let s = m.rt.scale;
    let hd = m.rt.bough.new_handle();
    let mut rec = m.recs.len();
    let deco = if n.deco != 0 && n.flags & L_TEXT == 0 { decos.get(n.deco as usize).copied().unwrap_or(Deco::NONE) } else { Deco::NONE };
    if n.flags & L_TEXT != 0 {
        // a measured leaf (para / label): shaped once into the pool, wrapped by the layout
        let node = m.rt.node_at(hd);
        let size_px = n.font * s;
        let lh_px = n.line_h * s;
        let face = m.rt.text.face_id(n.face);
        let strike = m.rt.text.strike_in(face, size_px);
        let mut ctx = LeafCtx::new(size_px, lh_px, n.flags & L_NOWRAP == 0).in_face(face);
        {
            let Rt { text, glyphs, .. } = &mut *m.rt;
            ctx.set_text(text, glyphs, n.text);
        }
        let lid = m.rt.lay.eng.add_style(hd, text_style, Some(ctx));
        attach(m, lid);
        let ti = m.rt.alloc_text(TextNode {
            text: n.text.to_string(),
            strike: strike as u32,
            size_px,
            line_h: lh_px,
            color: n.color,
            ox: 0.0,
            oy: 0.0,
            space: m.space,
            handle: hd,
            ink: bough_core::nrect::WRect::EMPTY,
            shape: TShape::None,
            per_glyph: false,
            num: None,
            base_off: 0.0,
            paint_key: 0,
        });
        m.rt.hot[node as usize] = Hot { kind: NK_LTEXT, flags: 0, clip: m.clip, link: ti };
        m.rt.lay.info[lid as usize].kind = 2;
        m.scope.texts.push(ti);
        m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_TEXT, payload: 0, handle: hd.idx, clip: m.clip });
        return (lid, rec);
    }
    let sid = style_of(m, ids, decls, n.style as usize);
    let lid = m.rt.lay.eng.add_style(hd, sid, None);
    attach(m, lid);
    let hidden = n.flags & L_HIDDEN != 0;
    if !hidden {
        // box shadows beneath the box (the first in CSS order on top), placed with it
        for sh in deco.shadows.iter().rev() {
            let (ins, margin) = crate::deco::shadow_instance(sh, s, m.space, n.radius * s);
            let node = crate::deco::mount_deco(m, ins, 0.0, 0.0, 0.0, 0.0, margin);
            m.rt.add_place_hook(lid, PlaceHook::Deco { node, dx: sh.dx * s, dy: sh.dy * s, grow: sh.spread * s, sized: true });
        }
        rec = m.recs.len();
    }
    let bordered = n.border_w > 0.0 && n.border >> 24 != 0;
    let painted = !hidden && (n.bg >> 24 != 0 || bordered || n.hover != 0 || deco.grad.is_some());
    if painted {
        let node = m.rt.node_at(hd);
        let border = bordered.then_some((n.border_w, Color(n.border)));
        let (mut ins, _kind, mut margin) = quad_instance(Color(n.bg), n.radius * s, border, s, m.space);
        if let Some(g) = &deco.grad {
            let ramp = m.rt.images.ramp(g.stops);
            crate::deco::gradient_instance(&mut ins, g, ramp);
            margin = 1.0;
        }
        let inst = m.rt.inst_of(node);
        *m.rt.inst_mut(inst) = ins;
        let _ = margin;
        m.rt.hot[node as usize] = Hot { kind: if bordered { NK_BORDER } else { NK_QUAD }, flags: 0, clip: m.clip, link: 0 };
        m.rt.lay.info[lid as usize].kind = 1;
        m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_QUAD, payload: inst, handle: hd.idx, clip: m.clip });
    } else {
        m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_BOX, payload: 0, handle: hd.idx, clip: m.clip });
    }
    if !hidden && !deco.svg.is_empty() {
        // a vector icon over the box, rasterised at the box's size, following its top-left
        let (w, h) = ((n.size[0] * s).round().max(1.0), (n.size[1] * s).round().max(1.0));
        let uv = m.rt.images.svg(deco.svg, w as u32, h as u32);
        let ins = bough_metal::instance::Instance { color: u32::MAX, uv, kind: crate::deco::KIND_IMAGE, ..Default::default() };
        m.depth += 1;
        let node = crate::deco::mount_deco(m, ins, 0.0, 0.0, w, h, 1.0);
        m.depth -= 1;
        m.rt.add_place_hook(lid, PlaceHook::Deco { node, dx: 0.0, dy: 0.0, grow: 0.0, sized: false });
    }
    (lid, rec)
}

impl<V: 'static> Element<V> for LTemplate<V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        // state lives in signals, the atlas or the region: a re-description steps over it
        crate::describe::mount_opaque(m, move |m: &mut Mounter<'_, V>| {
        let base = m.depth;
        let (clip0, cur0) = (m.clip, m.rt.lay.cur);
        let avail0 = m.rt.lay.mount_avail;
        // a negative extent: the window's (what the app mounts at)
        let pick = |v: f32, w: f32| if v < 0.0 { w } else { v * m.rt.scale };
        m.rt.lay.mount_avail = (pick(self.avail[0], avail0.0), pick(self.avail[1], avail0.1));
        let mut ids = vec![NONE; self.decls.len()];
        let text_style = m.rt.lay.eng.intern(Style::DEFAULT);
        // SAFETY of the owner reference: the mounter's owner is the live owner being mounted.
        let owner: &V = unsafe { &*m.owner };
        // the layout parents: (template depth, layout node, clip of its children)
        let space0 = m.space;
        // (template depth, layout node, clip of its children, space of its children)
        let mut parents: Vec<(u16, u32, u16, u16)> = Vec::new();
        let mut contents: Vec<u16> = Vec::new();
        let mut skip_below: Option<u16> = None;
        for n in self.nodes.iter() {
            if let Some(d) = skip_below {
                if n.depth > d {
                    continue;
                }
                skip_below = None;
            }
            while parents.last().is_some_and(|p| p.0 >= n.depth) {
                parents.pop();
            }
            while contents.last().is_some_and(|&d| d >= n.depth) {
                contents.pop();
            }
            if n.flags & L_NONE != 0 {
                skip_below = Some(n.depth);
                continue;
            }
            if n.flags & L_CONTENTS != 0 {
                contents.push(n.depth);
                continue;
            }
            m.depth = base + n.depth - contents.len() as u16;
            m.rt.lay.cur = parents.last().map_or(cur0, |p| p.1);
            m.clip = parents.last().map_or(clip0, |p| p.2);
            m.space = parents.last().map_or(space0, |p| p.3);
            // a transformed box: it and its subtree are mounted in a space of their own (the kernels'
            // per-instance transform), following outer scrolls as a nested view; its records get the same
            // transform in the core when laid out (PlaceHook::Xform -> Rt::transform_subtree)
            let xform = if n.deco != 0 && n.flags & L_TEXT == 0 { self.decos.get(n.deco as usize).and_then(|d| d.xform) } else { None };
            if xform.is_some() {
                m.space = m.rt.xform_space(m.space);
            }
            let (lid, rec) = if n.flags & L_HOOK != 0 {
                let before = m.recs.len();
                let mut ctx = LCtx { node: n, style_ids: &mut ids, decls: self.decls, text_style, decos: self.decos };
                match (self.hook)(owner, m, &mut ctx) {
                    LHooked::Subtree => {
                        skip_below = Some(n.depth);
                        continue;
                    }
                    LHooked::Default => {
                        if m.recs.len() > before {
                            // the hook mounted the node itself (mount_default): continue with its children
                            let h = m.recs[before].handle;
                            (h, before)
                        } else {
                            mount_element(m, n, &mut ids, self.decls, text_style, self.decos)
                        }
                    }
                }
            } else {
                mount_element(m, n, &mut ids, self.decls, text_style, self.decos)
            };
            if n.flags & L_FOCUS != 0 {
                let r = &mut m.recs[rec];
                r.kind |= KF_FOCUS | KF_INTERACT;
                let h = m.rt.bough.handle_gen(r.handle);
                m.rt.input.ensure_act(h);
            }
            if n.hover != 0 && n.flags & L_HIDDEN == 0 {
                let a = crate::input::act_at(m, rec);
                crate::input::ActOp::<V>::install(crate::input::HoverBg(Color(n.hover)), m, a);
            }
            if n.flags & L_TEXT != 0 {
                continue;
            }
            let mut clip = m.clip;
            if n.flags & L_CLIP != 0 {
                clip = m.rt.layout_clip(lid, m.clip);
            }
            if let Some(xf) = xform {
                let h = m.rt.bough.handle_gen(m.recs[rec].handle);
                m.rt.add_place_hook(lid, PlaceHook::Xform { space: m.space, handle: h, xf });
            }
            parents.push((n.depth, lid, clip, m.space));
        }
        m.depth = base;
        m.clip = clip0;
        m.rt.lay.cur = cur0;
        m.rt.lay.mount_avail = avail0;
        m.space = space0;
    })
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}

// =============================================================================== placement hooks

/// What a layout node's placement drives (`LInfo::hook`).
#[derive(Clone, Copy)]
pub(crate) enum PlaceHook {
    /// A clip that is the box (∩ the parent clip); inside a scroll view also registered with it. `abs` is
    /// the box's last absolute rect (re-cut when the parent clip changes).
    Clip { clip: u16, parent: u16, view: u32, slot: u32, abs: [f32; 4] },
    /// A space whose origin is the box's top-left: non-layout content anchored to a layout box (a view).
    Anchor { view: u32 },
    /// A list region whose port is the box (`port_list` under a laid-out panel).
    Port { reg: u32 },
    /// A transformed box: the subtree of record `handle` (its instances in `space`) takes the affine about the
    /// box's centre whenever the box moves or resizes.
    Xform { space: u16, handle: bough_core::Handle, xf: crate::deco::Xf },
    /// A decoration (deco.rs) at the box offset by (dx, dy): grown by `grow` on every side and sized with
    /// the box (a shadow), or keeping its own size (an icon).
    Deco { node: u32, dx: f32, dy: f32, grow: f32, sized: bool },
    /// A scroll box's port (`LBox::scroll`): the view's origin follows the box, and the wheel limit uses the
    /// box's height.
    ScrollPort { view: u32, scroller: u32 },
}

impl Rt {
    /// Change layout node `lid`'s style (`f` edits a copy of the current one; the result is interned) and
    /// queue its group for layout: a resizable panel's basis, a splitter.
    pub fn lay_restyle(&mut self, lid: u32, f: impl FnOnce(&mut Style)) {
        let mut st = self.lay.eng.style(lid).clone();
        f(&mut st);
        let id = self.lay.eng.intern(st);
        self.lay.eng.set_style(&self.bough, lid, id);
        let root = self.lay.root_of(lid);
        if !self.lay.info[root as usize].queued {
            self.lay.info[root as usize].queued = true;
            self.lay.pending.push(root);
        }
    }

    /// A clip owned by layout node `lid`: its rect follows the laid-out box.
    pub(crate) fn layout_clip(&mut self, lid: u32, parent: u16) -> u16 {
        let id = self.bough.new_clip(NRect::EMPTY);
        let (view, slot) = match self.input.mounting_view {
            Some(v) => {
                let views = &mut self.input.views[v as usize];
                views.clips.push((id, parent, [0.0; 4]));
                (v, (views.clips.len() - 1) as u32)
            }
            None => (NONE, NONE),
        };
        self.add_place_hook(lid, PlaceHook::Clip { clip: id, parent, view, slot, abs: [0.0; 4] });
        let k = self.lay.info[lid as usize].hook;
        self.input.clip_kids.entry(parent).or_default().push(k);
        id
    }

    /// Chain a placement hook onto layout node `lid`.
    pub(crate) fn add_place_hook(&mut self, lid: u32, h: PlaceHook) {
        let next = self.lay.info[lid as usize].hook;
        self.input.lhooks.push((h, next));
        self.lay.info[lid as usize].hook = (self.input.lhooks.len() - 1) as u32;
    }

    /// Layout placed a hooked node at (ax, ay, w, h), group-local px in `space`.
    pub(crate) fn lay_hook(&mut self, mut k: u32, space: u16, ax: f32, ay: f32, w: f32, h: f32) {
        while k != NONE {
            let (hk, next) = self.input.lhooks[k as usize];
            self.place_one(k, hk, space, ax, ay, w, h);
            k = next;
        }
    }

    /// Re-cut clip hook `k` from its box and its parent clip, then every clip below it.
    fn recut(&mut self, k: u32) {
        let PlaceHook::Clip { clip, parent, abs, .. } = self.input.lhooks[k as usize].0 else { return };
        let r = NRect::new(abs[0].floor() as i32, abs[1].floor() as i32, abs[2].ceil() as i32, abs[3].ceil() as i32);
        let r = r.intersect(self.bough.clip_rect(parent)).canon();
        if r == self.bough.clip_rect(clip) {
            return;
        }
        self.bough.set_clip_rect(clip, r);
        let kids = self.input.clip_kids.get(&clip).cloned().unwrap_or_default();
        for c in kids {
            self.recut(c);
        }
    }

    fn place_one(&mut self, k: u32, hk: PlaceHook, space: u16, ax: f32, ay: f32, w: f32, h: f32) {
        match hk {
            PlaceHook::Clip { clip: _, parent, view, slot, .. } => {
                let o = self.space(space);
                if let PlaceHook::Clip { abs, .. } = &mut self.input.lhooks[k as usize].0 {
                    *abs = [o[0] + ax, o[1] + ay, o[0] + ax + w, o[1] + ay + h];
                }
                let _ = parent;
                if view != NONE {
                    self.input.views[view as usize].clips[slot as usize].2 = [ax, ay, ax + w, ay + h];
                }
                self.recut(k);
            }
            PlaceHook::Anchor { view } => {
                let o = self.space(space);
                crate::template::move_view(self, view, o[0] + ax, o[1] + ay);
            }
            PlaceHook::ScrollPort { view, scroller } => {
                let o = self.space(space);
                crate::template::move_view(self, view, o[0] + ax, o[1] + ay);
                if scroller != NONE {
                    self.input.scrollers[scroller as usize].port_h = h / self.scale;
                }
            }
            PlaceHook::Xform { space: s, handle, xf } => {
                let _ = space;
                let mut t = [if xf.pct[0] { xf.t[0] * w } else { xf.t[0] * self.scale }, if xf.pct[1] { xf.t[1] * h } else { xf.t[1] * self.scale }];
                if xf.m == [1.0, 0.0, 0.0, 1.0] {
                    t = [t[0].round(), t[1].round()]; // a pure translation: whole px (sharp glyphs)
                }
                self.transform_subtree(handle, s, xf.m, [ax + 0.5 * w, ay + 0.5 * h], t);
            }
            PlaceHook::Deco { node, dx, dy, grow, sized } => {
                let _ = space;
                let (dw, dh) = if sized { (w + 2.0 * grow, h + 2.0 * grow) } else { { let p = &self.inst[self.inst_of(node) as usize]; (p.size[0], p.size[1]) } };
                self.place_deco(node, ax + dx - grow, ay + dy - grow, dw, dh);
            }
            PlaceHook::Port { reg } => {
                let o = self.space(space);
                let owner = self.owner;
                // SAFETY: `owner` is the mounted owner the region's row function was monomorphised for.
                unsafe { self.region_set_port(reg as usize, owner, [o[0] + ax, o[1] + ay, w, h]) };
            }
        }
    }
}
