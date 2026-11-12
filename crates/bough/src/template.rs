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
    /// The hook mounted the node; its children are clipped to this clip id (a `wired_clip`).
    NodeClip(u16),
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
    /// A paint-less box record at the current depth (a template node a hook mounts as structure only).
    pub fn push_box(&mut self) {
        self.recs.push(RecordInit { depth: self.depth, ink: NRect::EMPTY, kind: REC_BOX, payload: 0, handle: 0, clip: self.clip });
    }
}

impl<V: 'static> Element<V> for Template<V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_opaque(m, |m| self.mount_in(m));
    }
    /// State-driven content: a re-description steps over it.
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}
impl<V: 'static> Template<V> {
    fn mount_in(self, m: &mut Mounter<'_, V>) {
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
                Hooked::NodeClip(id) => {
                    clips.push((n.depth, id));
                    continue;
                }
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
    /// Views nested in this one's content (their origins follow this one's moves) and tooltips inside it.
    pub nested: Vec<u32>,
    pub tips: Vec<u32>,
    /// The content's layout group root (NONE: not laid out), and the width the window leaves it (the port
    /// follows the window when `follows`).
    pub root: u32,
    pub follows: bool,
    pub inset_w: f32,
}

/// The content origin of view `v`.
#[inline]
fn view_origin(rt: &Rt, v: usize) -> (f32, f32) {
    let view = &rt.input.views[v];
    (view.vx, view.vy - view.px)
}

/// Re-derive view `v`'s clips from its content origin (parents first: registration order is preorder).
fn view_clips(rt: &mut Rt, v: usize) {
    let (ox, oy) = view_origin(rt, v);
    for k in 0..rt.input.views[v].clips.len() {
        let (id, parent, r) = rt.input.views[v].clips[k];
        let p = rt.bough.clip_rect(parent).to_xyxy();
        let a = [r[0] + ox, r[1] + oy, r[2] + ox, r[3] + oy];
        let c = NRect::new(a[0].max(p[0] as f32).floor() as i32, a[1].max(p[1] as f32).floor() as i32, a[2].min(p[2] as f32).ceil() as i32, a[3].min(p[3] as f32).ceil() as i32);
        rt.set_clip(id, c);
    }
}

/// Views nested in `v` (and their tooltips) follow a move of `v`'s content by (dx, dy): their records were
/// moved with `v`'s, so only their spaces, clips and tooltip rects change.
fn shift_nested(rt: &mut Rt, v: usize, dx: i32, dy: i32) {
    for k in 0..rt.input.views[v].nested.len() {
        let c = rt.input.views[v].nested[k] as usize;
        {
            let view = &mut rt.input.views[c];
            view.vx += dx as f32;
            view.vy += dy as f32;
        }
        let (ox, oy) = view_origin(rt, c);
        let space = rt.input.views[c].space;
        rt.set_space(space, [ox, oy]);
        let port = rt.input.views[c].clip;
        let _ = port;
        view_clips(rt, c);
        shift_nested(rt, c, dx, dy);
    }
}

/// Move view `v`'s port origin to (vx, vy) device px (an anchor following its laid-out box): its space,
/// its content's clips and records, and every view nested in it.
pub(crate) fn move_view(rt: &mut Rt, v: u32, vx: f32, vy: f32) {
    let v = v as usize;
    let (vx, vy) = (vx.round(), vy.round());
    let (ox, oy) = (rt.input.views[v].vx, rt.input.views[v].vy);
    if (vx, vy) == (ox, oy) {
        return;
    }
    let (dx, dy) = ((vx - ox) as i32, (vy - oy) as i32);
    {
        let view = &mut rt.input.views[v];
        view.vx = vx;
        view.vy = vy;
    }
    let (cx, cy) = view_origin(rt, v);
    let space = rt.input.views[v].space;
    rt.set_space(space, [cx, cy]);
    view_clips(rt, v);
    let h = rt.input.views[v].handle;
    if let Some(pos) = rt.bough.pos_of(h) {
        let end = rt.bough.subtree_end(pos);
        rt.bough.translate_run(pos + 1, end - pos - 1, dx, dy);
    }
    shift_nested(rt, v, dx, dy);
}

impl Rt {
    /// A space for a transformed subtree mounted in `parent`: the same translation, registered as a view
    /// nested in the view being mounted so outer scrolls move it.
    pub(crate) fn xform_space(&mut self, parent: u16) -> u16 {
        let s = self.alloc_space();
        let o = self.space(parent);
        self.set_space(s, o);
        let hd = self.bough.new_handle();
        let v = self.input.views.len() as u32;
        self.input.views.push(ScrollView { handle: hd, space: s, clip: 0, vx: o[0], vy: o[1], px: 0.0, clips: Vec::new(), nested: Vec::new(), tips: Vec::new(), root: NONE, follows: false, inset_w: 0.0 });
        if let Some(outer) = self.input.mounting_view {
            self.input.views[outer as usize].nested.push(v);
        }
        s
    }

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
    // absolute (no drift); `set_space` carries the delta to nested spaces and list regions
    rt.set_space(space, [vx, vy - new_px]);
    // the content's clips move with it (parents first: they were registered in preorder)
    view_clips(rt, v);
    if let Some(pos) = rt.bough.pos_of(h) {
        let end = rt.bough.subtree_end(pos);
        let port = rt.bough.clip_rect(clip);
        rt.bough.scroll_run(pos + 1, end - pos - 1, 0, -dy, port);
    }
    shift_nested(rt, v, 0, -dy);
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
        crate::describe::mount_opaque(m, |m| self.mount_in(m));
    }
    /// State-driven content: a re-description steps over it.
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}
impl<V: 'static, C: Element<V>> ScrollY<'_, V, C> {
    fn mount_in(self, m: &mut Mounter<'_, V>) {
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
        let space = m.rt.alloc_space_in(m.space);
        let px = (*self.sig.get() * s).round();
        m.rt.set_space(space, [vx, vy - px]);
        let v = m.rt.input.views.len() as u32;
        let follows = vx == 0.0 && vy == 0.0 && (w * s).round() as i32 == m.rt.win_w && (h * s).round() as i32 == m.rt.win_h;
        m.rt.input.views.push(ScrollView { handle, space, clip, vx, vy, px, clips: Vec::new(), nested: Vec::new(), tips: Vec::new(), root: NONE, follows, inset_w: 0.0 });
        if let Some(outer) = m.rt.input.mounting_view {
            m.rt.input.views[outer as usize].nested.push(v);
        }
        m.wire(&self.sig, patch_view, v, [0; 16]);
        let saved = (m.space, m.ox, m.oy, m.clip, m.depth, m.rt.input.mounting_view);
        m.space = space;
        m.ox = 0.0;
        m.oy = 0.0;
        m.clip = clip;
        m.depth += 1;
        m.rt.input.mounting_view = Some(v);
        let roots_before = m.scope.lroots.len();
        let lay_cur = m.rt.lay.cur;
        m.rt.lay.cur = NONE; // the content is a layout group of its own
        self.content.mount(m);
        m.rt.lay.cur = lay_cur;
        if let Some(&root) = m.scope.lroots.get(roots_before) {
            // laid-out content: the wheel's limit is the content's laid-out height
            let avail = m.rt.lay.info[root as usize].avail_w;
            let view = &mut m.rt.input.views[v as usize];
            view.root = root;
            view.inset_w = w * s - avail;
            let a = m.rt.input.act_of_handle(handle);
            let sc = m.rt.input.acts[a as usize].scroll;
            m.rt.input.scrollers[sc as usize].root = root;
            m.rt.input.scrollers[sc as usize].port_h = h;
        }
        (m.space, m.ox, m.oy, m.clip, m.depth, m.rt.input.mounting_view) = saved;
    }
}

/// A laid-out scroll box (`LBox::scroll`, the design's `.scroll(&self.scroll)`). The port is a box in its
/// parent's flow: it is the wheel target, painted with the box's background. The content is a layout group
/// of its own, in a scroll view whose clip and origin follow the port wherever layout puts it. `sig` is the
/// offset (points); the wheel's limit is the content's laid-out height less the port's.
pub struct ScrollBox<'a, V, Bg, Ch> {
    port: crate::layout::LBox<Bg, ()>,
    content: crate::layout::LBox<Color, Ch>,
    sig: Wired<'a, f32, V>,
    line: f32,
}
impl<'a, V, Bg, Ch> ScrollBox<'a, V, Bg, Ch> {
    pub(crate) fn new(port: crate::layout::LBox<Bg, ()>, content: crate::layout::LBox<Color, Ch>, sig: Wired<'a, f32, V>) -> Self {
        ScrollBox { port, content, sig, line: 48.0 }
    }
    /// Points per wheel line.
    pub fn line(mut self, l: f32) -> Self {
        self.line = l;
        self
    }
    /// One more child of the scrolled content.
    pub fn child<X>(self, x: X) -> ScrollBox<'a, V, Bg, (Ch, X)> {
        ScrollBox { port: self.port, content: self.content.child(x), sig: self.sig, line: self.line }
    }
}

impl<'a, V: 'static, Bg: crate::elem::IntoProp<Color, V>, Ch: Element<V>> Element<V> for ScrollBox<'a, V, Bg, Ch> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        // state lives in signals, the atlas or the region: a re-description steps over it
        crate::describe::mount_opaque(m, move |m: &mut Mounter<'_, V>| {
        let s = m.rt.scale;
        let before = m.recs.len();
        // the port: a box in the parent's flow, the wheel target (its limit comes from the laid-out content)
        self.port.scrolls(self.sig, self.line, 0.0).mount(m);
        let lid = m.rt.lay.last_box;
        let handle = m.rt.bough.handle_gen(m.recs[before].handle);
        let clip = m.rt.layout_clip(lid, m.clip);
        let space = m.rt.alloc_space();
        let px = (*self.sig.get() * s).round();
        m.rt.set_space(space, [0.0, -px]);
        let v = m.rt.input.views.len() as u32;
        m.rt.input.views.push(ScrollView { handle, space, clip, vx: 0.0, vy: 0.0, px, clips: Vec::new(), nested: Vec::new(), tips: Vec::new(), root: NONE, follows: false, inset_w: 0.0 });
        if let Some(outer) = m.rt.input.mounting_view {
            m.rt.input.views[outer as usize].nested.push(v);
        }
        let a = m.rt.input.act_of_handle(handle);
        let sc = m.rt.input.acts[a as usize].scroll;
        m.rt.add_place_hook(lid, crate::ltemplate::PlaceHook::ScrollPort { view: v, scroller: sc });
        m.wire(&self.sig, patch_view, v, [0; 16]);
        let saved = (m.space, m.ox, m.oy, m.clip, m.depth, m.rt.input.mounting_view);
        m.space = space;
        m.ox = 0.0;
        m.oy = 0.0;
        m.clip = clip;
        m.depth += 1;
        m.rt.input.mounting_view = Some(v);
        let roots_before = m.scope.lroots.len();
        let lay_cur = m.rt.lay.cur;
        m.rt.lay.cur = NONE; // the content is a layout group of its own
        self.content.mount(m);
        m.rt.lay.cur = lay_cur;
        if let Some(&root) = m.scope.lroots.get(roots_before) {
            m.rt.input.views[v as usize].root = root;
            m.rt.input.scrollers[sc as usize].root = root;
        }
        (m.space, m.ox, m.oy, m.clip, m.depth, m.rt.input.mounting_view) = saved;
    })
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}

// =============================================================================== wired clips and port lists

/// Children clipped to a rect that a signal moves: `f(&value)` gives (x, y, w, h) points in the current
/// space. A new value is one clip write (damage: the old and new clip's xor).
pub struct ClipBy<'a, T, V, F, C> {
    sig: Wired<'a, T, V>,
    f: F,
    ch: C,
}
pub fn clip_by<'a, T, V, F: Fn(&T) -> [f32; 4] + Copy + 'static, C>(sig: Wired<'a, T, V>, f: F, ch: C) -> ClipBy<'a, T, V, F, C> {
    ClipBy { sig, f, ch }
}

/// Derived geometry. A clip entry's absolute rect is a function of three inputs: `local` (edges in device
/// px from the origin of `space`), the origin of `space`, and the rect of `parent`. Nothing writes the rect directly: a wire stores
/// `local`, a tooltip sets `hidden`, a move of the space or a change of the parent raises the entry's level,
/// and `Rt::clips_settle` recomputes entries level by level. **Level** is the static schedule: 0 when the
/// parent clip is not derived, else the parent's level + 1; spaces are eager (a space's origin is final as
/// soon as it is written, nested ones included), so an entry's inputs are final once every lower level is,
/// whatever order the writes of a turn came in.
#[derive(Clone, Copy)]
pub(crate) struct WClip {
    pub id: u16,
    pub space: u16,
    pub parent: u16,
    pub local: [f32; 4],
    pub level: u16,
    /// Shows nothing while set (a tooltip not showing).
    pub hidden: bool,
    pub live: bool,
    /// Index in its level's list.
    at: u32,
}

/// # Safety
/// Wire contract: `sig` is a `Signal<T>`; the recipe holds an `F`; `w.node` is the clip's entry in `Input::wclips`.
unsafe fn patch_clip_by<T: 'static, F: Fn(&T) -> [f32; 4] + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { &*(sig as *const Signal<T>) };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    // an input, not the rect: the rect is derived when every wire of the cell has run (`clips_settle`)
    let local = edges(f(s), rt.scale);
    let e = &mut rt.input.wclips[w.node as usize];
    e.local = local;
    let l = e.level as u32;
    rt.clips_raise(l);
}

/// The absolute rect of local edges `e` (device px `[x0, y0, x1, y1]` from the space's origin), rounded
/// outward and cut to `parent`.
fn clip_px(rt: &Rt, space: u16, e: [f32; 4], parent: u16) -> NRect {
    let o = rt.space(space);
    let a = NRect::new((o[0] + e[0]).floor() as i32, (o[1] + e[1]).floor() as i32, (o[0] + e[2]).ceil() as i32, (o[1] + e[3]).ceil() as i32);
    a.intersect(rt.bough.clip_rect(parent)).canon()
}

/// Local edges (device px) of a rect `(x, y, w, h)` in points.
#[inline]
pub(crate) fn edges(r: [f32; 4], s: f32) -> [f32; 4] {
    [r[0] * s, r[1] * s, (r[0] + r[2]) * s, (r[1] + r[3]) * s]
}

fn lower(v: &mut Vec<u32>, i: usize, x: u32) {
    if v.len() <= i {
        v.resize(i + 1, u32::MAX);
    }
    v[i] = v[i].min(x);
}

impl Rt {
    /// Derived entries of level `from` and above may have a changed input.
    #[inline(always)]
    pub(crate) fn clips_raise(&mut self, from: u32) {
        self.input.wclip_from = self.input.wclip_from.min(from);
    }
    /// Space `s` moved: the derived entries that read its origin (or a nested space's) are stale.
    #[inline(always)]
    pub(crate) fn clips_space_moved(&mut self, s: u16) {
        if let Some(&r) = self.input.space_rank.get(s as usize) {
            self.input.wclip_from = self.input.wclip_from.min(r);
        }
    }
    /// Set clip `id`'s rect (`Bough::set_clip_rect`) and raise the derived entries cut to it.
    #[inline]
    pub(crate) fn set_clip(&mut self, id: u16, r: NRect) {
        self.bough.set_clip_rect(id, r);
        if let Some(&k) = self.input.clip_rank.get(id as usize) {
            self.input.wclip_from = self.input.wclip_from.min(k);
        }
    }
    /// Recompute the derived entries whose inputs changed, level by level from the lowest raised one (an
    /// entry recomputed here is read by higher levels only). One compare when nothing was raised. Called when
    /// the wires of a cell have run, and after a layout flush has placed its boxes.
    #[inline(always)]
    pub(crate) fn clips_settle(&mut self) {
        if self.input.wclip_from != u32::MAX {
            self.clips_settle_from();
        }
    }
    #[inline(never)]
    fn clips_settle_from(&mut self) {
        let from = std::mem::replace(&mut self.input.wclip_from, u32::MAX) as usize;
        for l in from..self.input.wclip_levels.len() {
            let list = std::mem::take(&mut self.input.wclip_levels[l]);
            for &k in &list {
                let c = self.input.wclips[k as usize];
                let r = if c.hidden { NRect::EMPTY } else { clip_px(self, c.space, c.local, c.parent) };
                if r != self.bough.clip_rect(c.id) {
                    self.bough.set_clip_rect(c.id, r);
                }
            }
            self.input.wclip_levels[l] = list;
        }
        // a recompute raises nothing below its own level, and everything above was just recomputed
        self.input.wclip_from = u32::MAX;
    }
    /// Is clip `id` a derived one?
    #[inline]
    pub(crate) fn clip_is_derived(&self, id: u16) -> bool {
        self.input.clip_entry.get(id as usize).is_some_and(|&k| k != u32::MAX)
    }
    fn clip_entry_new(&mut self, e: WClip) -> u32 {
        let l = e.level as usize;
        if self.input.wclip_levels.len() <= l {
            self.input.wclip_levels.resize_with(l + 1, Vec::new);
        }
        let k = match self.input.wclip_free.pop() {
            Some(k) => k,
            None => {
                self.input.wclips.push(e);
                (self.input.wclips.len() - 1) as u32
            }
        };
        self.input.wclips[k as usize] = WClip { at: self.input.wclip_levels[l].len() as u32, live: true, ..e };
        self.input.wclip_levels[l].push(k);
        lower(&mut self.input.clip_rank, e.parent as usize, l as u32);
        // the space and the spaces it is nested in: a move of any of them moves this entry
        let mut s = e.space;
        loop {
            lower(&mut self.input.space_rank, s as usize, l as u32);
            match self.space_parent_of(s) {
                Some(p) => s = p,
                None => break,
            }
        }
        k
    }
    fn clip_level_under(&self, parent: u16) -> u16 {
        match self.input.clip_entry.get(parent as usize) {
            Some(&k) if k != u32::MAX => self.input.wclips[k as usize].level + 1,
            _ => 0,
        }
    }
    /// A new derived clip: `local` (edges, device px) in `space`, cut to `parent`; returns (clip id, entry).
    pub(crate) fn clip_derived(&mut self, space: u16, local: [f32; 4], parent: u16, hidden: bool) -> (u16, u32) {
        let r = if hidden { NRect::EMPTY } else { clip_px(self, space, local, parent) };
        let id = match self.input.clip_free.pop() {
            Some(id) => {
                self.bough.set_clip_rect(id, r);
                id
            }
            None => self.bough.new_clip(r),
        };
        let level = self.clip_level_under(parent);
        let k = self.clip_entry_new(WClip { id, space, parent, local, level, hidden, live: true, at: 0 });
        if self.input.clip_entry.len() <= id as usize {
            self.input.clip_entry.resize(id as usize + 1, u32::MAX);
        }
        self.input.clip_entry[id as usize] = k;
        (id, k)
    }
    /// Show or hide a derived clip (a tooltip's).
    pub(crate) fn clip_show(&mut self, k: u32, on: bool) {
        let e = &mut self.input.wclips[k as usize];
        e.hidden = !on;
        let l = e.level as u32;
        self.clips_raise(l);
        self.clips_settle();
    }
    /// Release derived entry `k` (its scope unmounted): it leaves its level, and a clip's id is kept for the
    /// next derived clip.
    pub(crate) fn clip_release(&mut self, k: u32) {
        let e = self.input.wclips[k as usize];
        if !e.live {
            return;
        }
        let list = &mut self.input.wclip_levels[e.level as usize];
        list.swap_remove(e.at as usize);
        if let Some(&moved) = list.get(e.at as usize) {
            self.input.wclips[moved as usize].at = e.at;
        }
        self.input.wclips[k as usize].live = false;
        self.input.wclip_free.push(k);
        self.bough.set_clip_rect(e.id, NRect::EMPTY);
        self.input.clip_entry[e.id as usize] = u32::MAX;
        if let Some(r) = self.input.clip_rank.get_mut(e.id as usize) {
            *r = u32::MAX; // its dependents were mounted under it and are released with it
        }
        self.input.clip_free.push(e.id);
    }
}

/// A clip id whose rect `f(&value)` (points, current space, inside the current clip) follows `sig`.
pub fn wired_clip<T: 'static, V, F: Fn(&T) -> [f32; 4] + Copy + 'static>(m: &mut Mounter<'_, V>, sig: Wired<'_, T, V>, f: F) -> u16 {
    let prev = m.clip;
    let (id, k) = m.rt.clip_derived(m.space, edges(f(sig.get()), m.rt.scale), prev, false);
    m.scope.clips.push(k);
    let mut recipe = [0u8; 16];
    const { assert!(size_of::<F>() <= 16, "clip closure: at most 16 bytes of captures") };
    // SAFETY: size_of::<F>() <= 16 (checked above); unaligned write into a byte array.
    unsafe { std::ptr::write_unaligned(recipe.as_mut_ptr() as *mut F, f) };
    m.wire(&sig, patch_clip_by::<T, F>, k, recipe);
    id
}

impl<'a, T: 'static, V, F: Fn(&T) -> [f32; 4] + Copy + 'static, C: Element<V>> Element<V> for ClipBy<'a, T, V, F, C> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_opaque(m, |m| self.mount_in(m));
    }
    /// State-driven content: a re-description steps over it.
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}
impl<'a, T: 'static, V, F: Fn(&T) -> [f32; 4] + Copy + 'static, C: Element<V>> ClipBy<'a, T, V, F, C> {
    fn mount_in(self, m: &mut Mounter<'_, V>) {
        let prev = m.clip;
        m.clip = wired_clip(m, self.sig, self.f);
        self.ch.mount(m);
        m.clip = prev;
    }
}

/// A fixed-row virtual list whose port's top edge a signal moves (a resizable panel's content): rows
/// `count`, `row_h` points each, the port from `top` to `bottom` (points) at `x`, `w` wide. A new top
/// edge moves the mounted rows (one space write per row, `scroll_run` over their records), moves the port
/// clip, and mounts or unmounts the rows that entered or left the port. Scrolling is `scroll`.
pub struct PortList<'a, V> {
    scroll: Wired<'a, f32, V>,
    top: Wired<'a, f32, V>,
    x: f32,
    w: f32,
    bottom: f32,
    row_h: f32,
    count: usize,
    row: fn(&V, usize, &mut Mounter<'_, V>),
}

#[allow(clippy::too_many_arguments)]
pub fn port_list<'a, V>(scroll: Wired<'a, f32, V>, top: Wired<'a, f32, V>, x: f32, w: f32, bottom: f32, row_h: f32, count: usize, row: fn(&V, usize, &mut Mounter<'_, V>)) -> PortList<'a, V> {
    PortList { scroll, top, x, w, bottom, row_h, count, row }
}

/// # Safety
/// Wire contract: `sig` is a `Signal<f32>` (the port's top edge, points); `owner` is the region's owner.
unsafe fn patch_port(rt: &mut Rt, w: &Wire, sig: *const u8, owner: *const u8) {
    // SAFETY: see the function contract.
    let top = **unsafe { &*(sig as *const Signal<f32>) };
    let reg = w.node as usize;
    let s = rt.scale;
    let (x, bottom_px, wpx, oy, space) = {
        let r = &rt.regions[reg];
        (r.vp_x, r.vp_y + r.port_h * s, r.port_w, r.oy, r.space)
    };
    let o = rt.space(space);
    let y = (o[1] + oy + top * s).round();
    // SAFETY: see the function contract.
    unsafe { rt.region_set_port(reg, owner, [o[0] + x, y, wpx, o[1] + bottom_px - y]) };
}

impl Rt {
    /// Move and resize list region `reg`'s port to `r` (device px: x, y, w, h): the mounted rows follow its
    /// top edge (one space write per row, one `scroll_run`), the port clip moves, and rows that entered or
    /// left the port are mounted or unmounted.
    ///
    /// # Safety
    /// `owner` points at the live owner the region's row function was monomorphised for.
    pub(crate) unsafe fn region_set_port(&mut self, reg: usize, owner: *const u8, r: [f32; 4]) {
        let s = self.scale;
        let (x, y, w, h) = (r[0].round(), r[1].round(), r[2].round(), r[3].round());
        let (old_x, old_y, clip, ck, scroll_px, space) = {
            let g = &self.regions[reg];
            (g.vp_x, g.vp_y, g.clip, g.ck, g.scroll_px, g.space)
        };
        // the port's place in its space: the clip is derived from it (and follows the space and the clip above
        // it from then on)
        let o = self.space(space);
        let (lx, ly) = (x - o[0], y - o[1]);
        let old_vp = self.bough.clip_rect(clip);
        let e = &mut self.input.wclips[ck as usize];
        e.local = [lx, ly, lx + w, ly + h];
        let l = e.level as u32;
        self.clips_raise(l);
        self.clips_settle();
        let vp = self.bough.clip_rect(clip);
        if vp == old_vp && lx == old_x && ly == old_y {
            return;
        }
        let (dx, dy) = ((lx - old_x) as i32, (ly - old_y) as i32);
        {
            let g = &mut self.regions[reg];
            g.vp_x = lx;
            g.vp_y = ly;
            g.port_w = w;
            g.port_h = h / s;
        }
        if dx != 0 || dy != 0 {
            let (base, nrec) = {
                let g = &self.regions[reg];
                (g.base_pos, g.rows.iter().map(|x| x.nrec as usize).sum::<usize>())
            };
            for i in 0..self.regions[reg].rows.len() {
                let sp = self.regions[reg].rows[i].space;
                self.shift_space(sp, dx, dy);
            }
            self.bough.scroll_run(base, nrec, dx, dy, old_vp.union(vp));
        }
        // rows that entered or left the port
        // SAFETY: see the function contract.
        unsafe { self.region_scroll(reg, owner, scroll_px / s) };
    }
}

impl<'a, V: 'static> Element<V> for PortList<'a, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_opaque(m, |m| self.mount_in(m));
    }
    /// State-driven content: a re-description steps over it.
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}
impl<'a, V: 'static> PortList<'a, V> {
    fn mount_in(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let o = m.rt.space(m.space);
        let top = *self.top.get();
        let (vx, vy) = ((o[0] + (m.ox + self.x) * s).round(), (o[1] + (m.oy + top) * s).round());
        let vb = (o[1] + (m.oy + self.bottom) * s).round();
        // a clip of its own, derived from the port's place in its space (it moves with the port, with the
        // space and with the clip above it)
        let (lx, ly) = (vx - o[0], vy - o[1]);
        let pw = (self.w * s).round();
        let (clip, ck) = m.rt.clip_derived(m.space, [lx, ly, lx + pw, vb - o[1]], m.clip, false);
        m.scope.clips.push(ck);
        let reg = m.rt.regions.len();
        fn none(_: f32) -> std::ops::Range<usize> {
            0..0
        }
        m.rt.regions.push(VRegion {
            row_h: self.row_h * s,
            vp_x: lx,
            vp_y: ly,
            port_w: pw,
            oy: m.oy * s,
            space: m.space,
            clip,
            ck,
            depth: m.depth,
            base_pos: m.recs.len(),
            range: 0..0,
            rows: Default::default(),
            scroll_px: 0.0,
            visible: none,
            count: self.count,
            port_h: (vb - vy) / s,
            scale_pt: s,
            mount_row: crate::elem::mount_row::<V>,
            row_fn: self.row as usize,
        });
        m.wire(&self.scroll, crate::elem::patch_scroll, reg as u32, [0; 16]);
        m.wire(&self.top, patch_port, reg as u32, [0; 16]);
        // SAFETY: the owner pointer is the mounted owner (`App::new` / a row mount).
        unsafe { m.rt.region_initial(reg, m.owner as *const u8, *self.scroll.get(), m.recs) };
    }
}

// =============================================================================== offsets (drag previews, popovers)

/// Children translated by a signal's (dx, dy) points: they live in a space of their own (the GPU adds its
/// translation to their instances and glyph runs), under a paint-less record whose subtree they are; a new
/// value is one space write plus one `translate_run` over the subtree's records (damage: their old and new
/// bounds). A drag preview following the pointer is one such write per move.
pub struct Offset<'a, V, C> {
    sig: Wired<'a, (f32, f32), V>,
    ch: C,
}
pub fn offset<V, C>(sig: Wired<'_, (f32, f32), V>, ch: C) -> Offset<'_, V, C> {
    Offset { sig, ch }
}

/// (record handle, space, base origin x, y in device px, current offset in device px).
pub(crate) type OffsetState = (bough_core::Handle, u16, f32, f32, [i32; 2]);

/// # Safety
/// Wire contract: `sig` is a `Signal<(f32, f32)>`; `w.node` indexes `Input::offsets`.
unsafe fn patch_offset(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let v = **unsafe { &*(sig as *const Signal<(f32, f32)>) };
    let (h, space, bx, by, cur) = rt.input.offsets[w.node as usize];
    let s = rt.scale;
    let new = [(v.0 * s).round() as i32, (v.1 * s).round() as i32];
    if new == cur {
        return;
    }
    rt.input.offsets[w.node as usize].4 = new;
    let _ = (bx, by);
    // relative: the space may have moved with a parent since mount
    rt.shift_space(space, new[0] - cur[0], new[1] - cur[1]);
    let Some(pos) = rt.bough.pos_of(h) else { return };
    let end = rt.bough.subtree_end(pos);
    rt.bough.translate_run(pos + 1, end - pos - 1, new[0] - cur[0], new[1] - cur[1]);
}

impl<V: 'static, C: Element<V>> Element<V> for Offset<'_, V, C> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_opaque(m, |m| self.mount_in(m));
    }
    /// State-driven content: a re-description steps over it.
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}
impl<V: 'static, C: Element<V>> Offset<'_, V, C> {
    fn mount_in(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let o = m.rt.space(m.space);
        let (bx, by) = (o[0] + m.ox * s, o[1] + m.oy * s);
        let v = *self.sig.get();
        let cur = [(v.0 * s).round() as i32, (v.1 * s).round() as i32];
        let space = m.rt.alloc_space_in(m.space);
        m.rt.set_space(space, [bx + cur[0] as f32, by + cur[1] as f32]);
        let h = m.rt.bough.new_handle();
        m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_BOX, payload: 0, handle: h.idx, clip: m.clip });
        let k = m.rt.input.offsets.len() as u32;
        m.rt.input.offsets.push((h, space, bx, by, cur));
        m.wire(&self.sig, patch_offset, k, [0; 16]);
        let saved = (m.space, m.ox, m.oy, m.depth);
        m.space = space;
        m.ox = 0.0;
        m.oy = 0.0;
        m.depth += 1;
        self.ch.mount(m);
        (m.space, m.ox, m.oy, m.depth) = saved;
    }
}

// =============================================================================== anchors

/// Non-layout content anchored to a layout box: a `w` x `h` points box in the flow (a layout node) whose
/// subtree is `content`, mounted in a space of its own at the box's top-left. When the layout places the
/// box, the space, the content's records and clips (and any view nested in it) move with it: a text field,
/// a tooltip trigger or any absolutely drawn widget inside a laid-out document.
pub struct Anchor<C> {
    w: f32,
    h: f32,
    content: C,
}
pub fn anchor<C>(w: f32, h: f32, content: C) -> Anchor<C> {
    Anchor { w, h, content }
}

impl<V: 'static, C: Element<V>> Element<V> for Anchor<C> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        // state lives in signals, the atlas or the region: a re-description steps over it
        crate::describe::mount_opaque(m, move |m: &mut Mounter<'_, V>| {
        use crate::layout::{attach, Dim, Spec};
        let s = m.rt.scale;
        let hd = m.rt.bough.new_handle();
        let mut spec = Spec::new(false, false);
        spec.w = Dim::Pt(self.w);
        spec.h = Dim::Pt(self.h);
        spec.shrink = 0.0;
        let lid = m.rt.lay.eng.add(hd, &spec, s, None);
        let parent = attach(m, lid);
        m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_BOX, payload: 0, handle: hd.idx, clip: m.clip });
        let space = m.rt.alloc_space();
        m.rt.set_space(space, [0.0, 0.0]);
        let v = m.rt.input.views.len() as u32;
        m.rt.input.views.push(ScrollView { handle: hd, space, clip: m.clip, vx: 0.0, vy: 0.0, px: 0.0, clips: Vec::new(), nested: Vec::new(), tips: Vec::new(), root: NONE, follows: false, inset_w: 0.0 });
        if let Some(outer) = m.rt.input.mounting_view {
            m.rt.input.views[outer as usize].nested.push(v);
        }
        m.rt.add_place_hook(lid, crate::ltemplate::PlaceHook::Anchor { view: v });
        let saved = (m.space, m.ox, m.oy, m.depth, m.rt.input.mounting_view, m.rt.lay.cur);
        m.space = space;
        m.ox = 0.0;
        m.oy = 0.0;
        m.depth += 1;
        m.rt.input.mounting_view = Some(v);
        m.rt.lay.cur = lid; // a box with no layout children: the content is not in the flow
        self.content.mount(m);
        (m.space, m.ox, m.oy, m.depth, m.rt.input.mounting_view, m.rt.lay.cur) = saved;
        m.rt.lay.cur = parent;
    })
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}

/// A fixed-row virtual list whose port is the laid-out box it is mounted in (a panel): `count` rows of
/// `row_h` points, scrolled by `scroll`. When the layout places the box, the port moves and resizes with it
/// (`Rt::region_set_port`: the rows follow, the rows that enter or leave are mounted or unmounted).
pub struct PortListIn<'a, V> {
    scroll: Wired<'a, f32, V>,
    row_h: f32,
    count: usize,
    row: fn(&V, usize, &mut Mounter<'_, V>),
}
pub fn port_list_in<'a, V>(scroll: Wired<'a, f32, V>, row_h: f32, count: usize, row: fn(&V, usize, &mut Mounter<'_, V>)) -> PortListIn<'a, V> {
    PortListIn { scroll, row_h, count, row }
}

impl<'a, V: 'static> Element<V> for PortListIn<'a, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        // state lives in signals, the atlas or the region: a re-description steps over it
        crate::describe::mount_opaque(m, move |m: &mut Mounter<'_, V>| {
        let s = m.rt.scale;
        let lid = m.rt.lay.cur;
        assert!(lid != NONE, "port_list_in is mounted inside a laid-out box");
        // no port until the layout places it: an empty derived clip in the current space
        let (clip, ck) = m.rt.clip_derived(m.space, [0.0; 4], m.clip, false);
        m.scope.clips.push(ck);
        let reg = m.rt.regions.len();
        fn none(_: f32) -> std::ops::Range<usize> {
            0..0
        }
        m.rt.regions.push(VRegion {
            row_h: self.row_h * s,
            vp_x: 0.0,
            vp_y: 0.0,
            port_w: 0.0,
            oy: 0.0,
            space: m.space,
            clip,
            ck,
            depth: m.depth,
            base_pos: m.recs.len(),
            range: 0..0,
            rows: Default::default(),
            scroll_px: 0.0,
            visible: none,
            count: self.count,
            port_h: 0.0,
            scale_pt: s,
            mount_row: crate::elem::mount_row::<V>,
            row_fn: self.row as usize,
        });
        m.wire(&self.scroll, crate::elem::patch_scroll, reg as u32, [0; 16]);
        m.rt.add_place_hook(lid, crate::ltemplate::PlaceHook::Port { reg: reg as u32 });
        // no rows until the layout gives the port a size
    })
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}
