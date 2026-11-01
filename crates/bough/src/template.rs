//! Static templates and scroll views.
//!
//! **Templates** (DESIGN.md 7: "mounting memcpys the const template into pages"): a document known at
//! compile time is a `&'static [TNode]` in paint preorder (depth, absolute rect, paint, text, flags), and
//! mounting it is one pass over the table: a record per node (paint-less boxes are depth markers with
//! empty ink), a 48-byte instance per painted box, a label per text. A hook mounts the nodes the
//! application owns (wired, interactive) with typed builders at the node's depth and origin.
//!
//! **Scroll views** (`scroll_y`): a viewport record whose subtree is its content, mounted in a space of its
//! own and clipped to the port. A new offset is one space write (instances and glyph runs follow on the
//! GPU), one `scroll_run` over the content's records (page offsets: the damage is the port ∩ the old and
//! new content bounds), and the clips the content owns moved with it (clip rects are absolute).
use crate::color::Color;
use crate::elem::{quad, text, Element, Mounter};
use crate::input::Interact;
use crate::rt::*;
use crate::signal::{Signal, Wired};
use bough_core::{NRect, RecordInit, KF_FOCUS, KF_INTERACT};

/// Node flags (`TNode::flags`).
pub const T_FOCUS: u16 = 1;
/// The node clips its subtree to its rect (`overflow` other than visible).
pub const T_CLIP: u16 = 2;
/// The hook mounts this node (see [`Hook`]).
pub const T_HOOK: u16 = 4;

#[derive(Clone, Copy, Debug)]
pub struct TNode {
    /// Depth below the template's root (preorder).
    pub depth: u16,
    /// Absolute rect (points) in the template's space.
    pub rect: [f32; 4],
    /// Background (alpha 0 and no border: a paint-less box).
    pub bg: u32,
    pub border_w: f32,
    pub border: u32,
    pub radius: f32,
    /// Text content (a text node when non-empty or `font > 0`).
    pub text: &'static str,
    pub font: f32,
    pub line_h: f32,
    pub color: u32,
    pub flags: u16,
    /// Application tag for the hook.
    pub tag: u16,
}

/// What a hook did with a node.
pub enum Hooked {
    /// Nothing: the template mounts the node itself.
    Default,
    /// The hook mounted the node; the template mounts its children.
    Node,
    /// The hook mounted the node and its whole subtree.
    Subtree,
}

/// Mounts application-owned nodes (`T_HOOK`) with typed builders. The mounter is at the node's depth, in
/// the template's space and clip, origin at the template origin (node rects are absolute in it).
pub type Hook<V> = fn(&V, &mut Mounter<'_, V>, usize, &TNode) -> Hooked;

pub struct Template<V: 'static> {
    nodes: &'static [TNode],
    hook: Hook<V>,
}

pub fn template<V>(nodes: &'static [TNode], hook: Hook<V>) -> Template<V> {
    Template { nodes, hook }
}

impl<V> Mounter<'_, V> {
    /// The current depth (hooks place children below it).
    pub fn depth(&self) -> u16 {
        self.depth
    }
    pub fn set_depth(&mut self, d: u16) {
        self.depth = d;
    }
}

impl<V: 'static> Element<V> for Template<V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let base = m.depth;
        let s = m.rt.scale;
        let (clip0, ox, oy) = (m.clip, m.ox, m.oy);
        // clip stack by depth: (depth the clip applies below, clip id)
        let mut clips: Vec<(u16, u16)> = Vec::new();
        let mut skip_below: Option<u16> = None;
        // SAFETY of the owner reference: the mounter's owner is the live owner being mounted.
        let owner: &V = unsafe { &*m.owner };
        for (i, n) in self.nodes.iter().enumerate() {
            if let Some(d) = skip_below {
                if n.depth > d {
                    continue;
                }
                skip_below = None;
            }
            while clips.last().is_some_and(|&(d, _)| n.depth <= d) {
                clips.pop();
            }
            m.depth = base + n.depth;
            m.clip = clips.last().map_or(clip0, |c| c.1);
            let before = m.recs.len();
            let hooked = if n.flags & T_HOOK != 0 { (self.hook)(owner, m, i, n) } else { Hooked::Default };
            match hooked {
                Hooked::Subtree => {
                    skip_below = Some(n.depth);
                    continue;
                }
                Hooked::Node => {}
                Hooked::Default => mount_node(m, n),
            }
            if n.flags & T_FOCUS != 0 && m.recs.len() > before {
                let r = &mut m.recs[before];
                if r.handle == 0 {
                    r.handle = m.rt.bough.new_handle().idx;
                }
                r.kind |= KF_FOCUS | KF_INTERACT;
                let h = m.rt.bough.handle_gen(r.handle);
                m.rt.input.ensure_act(h);
            }
            if n.flags & T_CLIP != 0 {
                let o = m.rt.space(m.space);
                let [x, y, w, h] = n.rect;
                let r = [o[0] + (ox + x) * s, o[1] + (oy + y) * s, o[0] + (ox + x + w) * s, o[1] + (oy + y + h) * s];
                let id = m.rt.scoped_clip(r, m.clip);
                clips.push((n.depth, id));
            }
        }
        m.depth = base;
        m.clip = clip0;
    }
}

/// The template's own lowering of a node: a text label, a painted quad, or a paint-less box record.
fn mount_node<V: 'static>(m: &mut Mounter<'_, V>, n: &TNode) {
    let [x, y, w, h] = n.rect;
    if !n.text.is_empty() {
        // a one-line label caches its tile rows (at most 4: 128 px); larger display text is set at 60 px
        let size = n.font.min(60.0 / m.rt.scale);
        text(n.text).at(x, y).size(size).line_height(n.line_h.min(size * 1.4)).color(Color(n.color)).mount(m);
        return;
    }
    let painted = n.bg >> 24 != 0 || (n.border_w > 0.0 && n.border >> 24 != 0);
    if painted && w > 0.0 && h > 0.0 {
        let q = quad(x, y, w, h).bg(Color(n.bg)).rounded(n.radius);
        if n.border_w > 0.0 && n.border >> 24 != 0 {
            q.border(n.border_w, Color(n.border)).mount(m);
        } else {
            q.mount(m);
        }
        return;
    }
    m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_BOX, payload: 0, handle: 0, clip: m.clip });
}

// =============================================================================== scroll views

/// A vertical scroll view: one per `scroll_y` (`Input::views`).
pub(crate) struct ScrollView {
    pub handle: bough_core::Handle,
    pub space: u16,
    pub clip: u16,
    /// Port origin (device px, in the parent's space terms: absolute).
    pub vx: f32,
    pub vy: f32,
    /// Current offset (device px).
    pub px: f32,
    /// Clips owned by the content: (id, parent clip id, rect in content px at offset 0).
    pub clips: Vec<(u16, u16, [f32; 4])>,
}

impl Rt {
    /// A clip for an absolute rect inside the current clip `parent`. Inside a scroll view's content the
    /// clip is fresh (it moves with the content) and registered with the view; elsewhere it is shared.
    pub(crate) fn scoped_clip(&mut self, r: [f32; 4], parent: u16) -> u16 {
        let p = self.bough.clip_rect(parent).to_xyxy();
        let c = [r[0].max(p[0] as f32), r[1].max(p[1] as f32), r[2].min(p[2] as f32), r[3].min(p[3] as f32)];
        if let Some(v) = self.input.mounting_view {
            let view = &self.input.views[v as usize];
            // content px at offset 0: absolute minus the port origin plus the offset
            let (dx, dy) = (view.vx, view.vy - view.px);
            let id = self.bough.new_clip(NRect::new(c[0].floor() as i32, c[1].floor() as i32, c[2].ceil() as i32, c[3].ceil() as i32));
            self.input.views[v as usize].clips.push((id, parent, [r[0] - dx, r[1] - dy, r[2] - dx, r[3] - dy]));
            return id;
        }
        self.clip_rect(c)
    }
}

/// # Safety
/// Wire contract: `sig` is a `Signal<f32>`; `w.node` is the view index.
unsafe fn patch_view(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { &*(sig as *const Signal<f32>) };
    let v = w.node as usize;
    let new_px = (**s * rt.scale).round();
    let old_px = rt.input.views[v].px;
    if new_px == old_px {
        return;
    }
    let dy = (new_px - old_px) as i32;
    let (h, space, clip, vx, vy) = {
        let view = &mut rt.input.views[v];
        view.px = new_px;
        (view.handle, view.space, view.clip, view.vx, view.vy)
    };
    rt.set_space(space, [vx, vy - new_px]);
    // the content's clips move with it (parents first: they were registered in preorder)
    let n = rt.input.views[v].clips.len();
    for k in 0..n {
        let (id, parent, r) = rt.input.views[v].clips[k];
        let p = rt.bough.clip_rect(parent).to_xyxy();
        let a = [r[0] + vx, r[1] + vy - new_px, r[2] + vx, r[3] + vy - new_px];
        let c = NRect::new(a[0].max(p[0] as f32).floor() as i32, a[1].max(p[1] as f32).floor() as i32, a[2].min(p[2] as f32).ceil() as i32, a[3].min(p[3] as f32).ceil() as i32);
        rt.bough.set_clip_rect(id, c);
    }
    let Some(pos) = rt.bough.pos_of(h) else { return };
    let end = rt.bough.subtree_end(pos);
    let port = rt.bough.clip_rect(clip);
    rt.bough.scroll_run(pos + 1, end - pos - 1, 0, -dy, port);
}

pub struct ScrollY<'a, V, C> {
    sig: Wired<'a, f32, V>,
    rect: [f32; 4],
    bg: Color,
    line: f32,
    max: f32,
    content: C,
}

/// A vertical scroll view of `rect` (points) over `content` (mounted in its own space, origin at the
/// port's top-left at offset 0), scrolled by `sig` (points); the wheel glides it within `0..=max`.
pub fn scroll_y<V, C>(sig: Wired<'_, f32, V>, rect: [f32; 4], max: f32, content: C) -> ScrollY<'_, V, C> {
    ScrollY { sig, rect, bg: Color(0), line: 48.0, max, content }
}
impl<V, C> ScrollY<'_, V, C> {
    pub fn bg(mut self, c: Color) -> Self {
        self.bg = c;
        self
    }
    /// Points per wheel line.
    pub fn line(mut self, l: f32) -> Self {
        self.line = l;
        self
    }
}

impl<V: 'static, C: Element<V>> Element<V> for ScrollY<'_, V, C> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let [x, y, w, h] = self.rect;
        let s = m.rt.scale;
        let before = m.recs.len();
        // the port: a quad (possibly transparent) with a handle, the wheel target
        quad(x, y, w, h).bg(self.bg).scrolls(self.sig, self.line, self.max).mount(m);
        let hidx = m.recs[before].handle;
        let handle = m.rt.bough.handle_gen(hidx);
        let o = m.rt.space(m.space);
        let (vx, vy) = (o[0] + (m.ox + x) * s, o[1] + (m.oy + y) * s);
        let clip = m.rt.scoped_clip([vx, vy, vx + w * s, vy + h * s], m.clip);
        let space = m.rt.alloc_space();
        let px = (*self.sig.get() * s).round();
        m.rt.set_space(space, [vx, vy - px]);
        let v = m.rt.input.views.len() as u32;
        m.rt.input.views.push(ScrollView { handle, space, clip, vx, vy, px, clips: Vec::new() });
        m.wire(&self.sig, patch_view, v, [0; 16]);
        let saved = (m.space, m.ox, m.oy, m.clip, m.depth, m.rt.input.mounting_view);
        m.space = space;
        m.ox = 0.0;
        m.oy = 0.0;
        m.clip = clip;
        m.depth += 1;
        m.rt.input.mounting_view = Some(v);
        self.content.mount(m);
        (m.space, m.ox, m.oy, m.clip, m.depth, m.rt.input.mounting_view) = saved;
    }
}
