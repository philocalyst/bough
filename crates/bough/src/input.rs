//! Input: pointer, hover, click, focus, keys, text fields, drag, wheel scrolling with glides, tooltips and
//! resize (INPUT.md). One mechanism per job:
//!
//! * **Hit testing is a range query over the paint preorder** (bough-core `hit.rs`): the topmost record
//!   under the pointer is found by the cull's SIMD descent, scanned from the top; its dispatch chain is its
//!   ancestors-or-self flagged [`KF_INTERACT`] in the kind column (read off the depth column). Nothing is
//!   built, so nothing is ever rebuilt.
//! * **The hit cell**: between two pointer events the topmost record is constant inside a rectangle (its
//!   visible ink minus every later record over it). A move inside the cell costs one compare plus a look
//!   at the damage planes since the cell was cut (geometry changes mark them; paint changes do not).
//! * **Hover is a paint write**: a crossing diffs the old and new chains (enter/leave = the symmetric
//!   difference) and writes the entered/left records' colour words (paint ring, content addressed) and
//!   runs their typed hover routes. No restyle, no rebuild.
//! * **Routes are typed**: `on_click(|m: &mut V| ..)` lowers to a monomorphic trampoline plus the closure
//!   bytes (at most 16) in the act table; after a route runs, the turn settles QUEUED signals as any
//!   handler turn does.
//! * **Focus order is preorder**: Tab is a scan of the kind column for the next [`KF_FOCUS`] bit.
//! * **A keystroke is a label patch plus a caret move**: the field's text is the owner's
//!   `Signal<String>`; its label wire patches the glyph slots from the first changed byte and the caret
//!   wire (installed first, so it runs last) moves one quad.
//! * **Wheel and drag write signals**: a wheel aims a glide (zgui's 240 ms ease-out tween, detents
//!   compose) whose ticks write the scroller's `Signal<f32>`; the scroll wire moves one space and the
//!   content's page offsets. A drag route receives the pointer delta since the press (pointer capture: no
//!   hit test while held).
//! * **Tooltips are clips**: a tooltip subtree is mounted once, hidden behind an empty clip; hovering its
//!   trigger sets the clip (one write, damage = the clip's xor).
//! * **Resize** re-lays the layout groups out at the new size through cephal's incremental path.
use crate::app::{App, View};
use crate::color::Color;
use crate::elem::{Element, Mounter};
use crate::rt::*;
use crate::signal::{Signal, Wired};
use bough_core::{Handle, NRect, KF_FOCUS, KF_INTERACT, KF_NOHIT};

// =============================================================================== events

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Primary,
    Secondary,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Tab,
    Backspace,
    Delete,
    Enter,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub cmd: bool,
}

/// What a route receives. Positions in points, window coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Ev {
    pub x: f32,
    pub y: f32,
    /// Drag: pointer travel since the press (points). Wheel: the delta (lines).
    pub dx: f32,
    pub dy: f32,
    pub key: Option<Key>,
    pub mods: Mods,
    pub button: Button,
    /// Hover: entered (true) or left. Drag: still held.
    pub on: bool,
}

impl Ev {
    fn at(x: f32, y: f32) -> Ev {
        Ev { x, y, dx: 0.0, dy: 0.0, key: None, mods: Mods::default(), button: Button::Primary, on: true }
    }
}

// =============================================================================== routes

/// A typed route lowered: a trampoline monomorphised for (owner type, closure type) and the closure's
/// bytes. Created only by `Mounter<V>`, whose `V` is the app's owner type (every mount in one runtime is).
pub(crate) type RouteFn = unsafe fn(*mut u8, &[u8; 16], &Ev);

#[derive(Clone, Copy)]
pub(crate) struct Route {
    f: RouteFn,
    r: [u8; 16],
}

impl Route {
    /// # Safety
    /// `owner` points at the live owner the route was lowered for.
    #[inline]
    unsafe fn call(&self, owner: *mut u8, e: &Ev) {
        // SAFETY: see the function contract; the recipe holds the closure the trampoline reads.
        unsafe { (self.f)(owner, &self.r, e) }
    }
}

fn recipe<F: Copy>(f: F) -> [u8; 16] {
    const { assert!(size_of::<F>() <= 16, "route closure: at most 16 bytes of captures") };
    let mut r = [0u8; 16];
    // SAFETY: size_of::<F>() <= 16 (checked at compile time); unaligned write into a byte array.
    unsafe { std::ptr::write_unaligned(r.as_mut_ptr() as *mut F, f) };
    r
}

/// # Safety
/// `o` is the owner `V`; `r` holds an `F` written by `recipe`.
unsafe fn call_plain<V, F: Fn(&mut V) + Copy>(o: *mut u8, r: &[u8; 16], _: &Ev) {
    // SAFETY: see the function contract.
    let f: F = unsafe { std::ptr::read_unaligned(r.as_ptr() as *const F) };
    f(unsafe { &mut *(o as *mut V) })
}

/// # Safety
/// As `call_plain`.
unsafe fn call_ev<V, F: Fn(&mut V, &Ev) + Copy>(o: *mut u8, r: &[u8; 16], e: &Ev) {
    // SAFETY: see the function contract.
    let f: F = unsafe { std::ptr::read_unaligned(r.as_ptr() as *const F) };
    f(unsafe { &mut *(o as *mut V) }, e)
}

fn route_plain<V, F: Fn(&mut V) + Copy + 'static>(f: F) -> Route {
    Route { f: call_plain::<V, F>, r: recipe(f) }
}
fn route_ev<V, F: Fn(&mut V, &Ev) + Copy + 'static>(f: F) -> Route {
    Route { f: call_ev::<V, F>, r: recipe(f) }
}

// =============================================================================== the act table

/// The routes and interaction styles of one interactive record (indexed by its handle).
#[derive(Clone, Copy)]
pub(crate) struct Act {
    pub handle: Handle,
    /// The record's quad node (`Rt::nodes`), or NONE.
    pub node: u32,
    /// Its single instance (the payload of a quad record), or NONE.
    pub inst: u32,
    pub hover_bg: Option<u32>,
    /// The instance's colour when it has no node (a constant background): restored on leave.
    pub base: u32,
    /// The instance is a border box: its background is `color2`.
    pub bordered: bool,
    /// The pointer is over it (its hover style shows).
    pub hovered: bool,
    pub click: Option<Route>,
    pub press: Option<Route>,
    pub hover: Option<Route>,
    pub key: Option<Route>,
    pub drag: Option<Route>,
    /// `Input::scrollers` index, or NONE.
    pub scroll: u32,
    /// `Input::fields` index, or NONE.
    pub field: u32,
    /// `Input::tips` index, or NONE.
    pub tip: u32,
}

impl Act {
    fn new(handle: Handle) -> Act {
        Act { handle, node: NONE, inst: NONE, hover_bg: None, base: 0, bordered: false, hovered: false, click: None, press: None, hover: None, key: None, drag: None, scroll: NONE, field: NONE, tip: NONE }
    }
}

/// A wheel target: the owner's `Signal<f32>` (points) at `off`, scrolled by glides.
pub(crate) struct Scroller {
    pub off: u32,
    /// Points per wheel line.
    pub line: f32,
    /// Largest offset (points); for laid-out content, the content's height less the port's.
    pub max: f32,
    pub glide: Option<Tween>,
    /// Laid-out content: its layout group root (NONE: `max` as given) and the port height (points).
    pub root: u32,
    pub port_h: f32,
}

/// A text field: the owner's `Signal<String>` at `off`, its label (`Rt::texts`) and caret quad.
pub(crate) struct Field {
    pub off: u32,
    pub ti: u32,
    pub caret_node: u32,
    /// Caret byte index.
    pub caret: usize,
    pub caret_color: u32,
    pub focused: bool,
    /// The caret's blink phase: shown (`true`) or hidden.
    pub lit: bool,
}

/// zgui's smooth scroll (zgui-scroll `Tween`): 240 ms, ease-out cubic; a detent during a glide adds to
/// its destination and restarts the ease from where the content has reached.
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    pub elapsed: f32,
}
impl Tween {
    pub const DURATION: f32 = 0.240;
    /// Advance by `dt` seconds: (position, done).
    pub fn advance(&mut self, dt: f32) -> (f32, bool) {
        self.elapsed = (self.elapsed + dt).min(Self::DURATION);
        if self.elapsed >= Self::DURATION {
            return (self.to, true);
        }
        let p = self.elapsed / Self::DURATION;
        let r = 1.0 - p;
        let e = 1.0 - r * r * r;
        (self.from + (self.to - self.from) * e, false)
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub struct InputStats {
    /// Pointer events answered by the hit cell (one compare).
    pub cell_hits: u64,
    /// Hit tests run (cell missed or invalid).
    pub hits: u64,
    /// Hover chain changes.
    pub crossings: u64,
    /// Routes run.
    pub routes: u64,
    /// Hit index rebuilds: none exists, so always 0 (kept for zgui's counter).
    pub hit_rebuilds: u64,
}

/// The interaction state of a window (lives in `Rt`, so mounts can install acts).
pub struct Input {
    pub(crate) acts: Vec<Act>,
    by_handle: Vec<u32>,
    pub(crate) scrollers: Vec<Scroller>,
    pub(crate) fields: Vec<Field>,
    pub(crate) tips: Vec<Tip>,
    /// Scroll views (`template::scroll_y`) and the one whose content is being mounted.
    pub(crate) views: Vec<crate::template::ScrollView>,
    pub(crate) mounting_view: Option<u32>,
    /// Layout placement hooks (`ltemplate::PlaceHook`, by `LInfo::hook`).
    pub(crate) lhooks: Vec<(crate::ltemplate::PlaceHook, u32)>,
    /// Layout clips by parent clip id (re-cut when the parent changes).
    pub(crate) clip_kids: std::collections::HashMap<u16, Vec<u32>>,
    /// Wired clips (`template::clip_by`): (clip id, space, parent clip).
    pub(crate) wclips: Vec<(u16, u16, u16)>,
    /// Offset groups (`template::offset`).
    pub(crate) offsets: Vec<crate::template::OffsetState>,
    /// Handles of the hover chain (dispatch chain under the pointer), nearest first.
    hover: Vec<Handle>,
    cell: NRect,
    cell_epoch: u32,
    cell_ok: bool,
    /// Pointer position (device px) and whether it is in the window.
    pub pointer: Option<(i32, i32)>,
    /// Pressed act (activation target, drag capture) and where the press was (points).
    pressed: u32,
    press_at: (f32, f32),
    dragging: bool,
    pub focus: Option<Handle>,
    focus_act: u32,
    /// Keyboard focus shows the ring (`:focus-visible`); pointer focus does not.
    focus_visible: bool,
    /// The focus ring: one record at the end of the paint order, moved to the focused element.
    ring: Option<(Handle, u32)>,
    ring_rect: NRect,
    pub ring_color: Color,
    pub ring_width: f32,
    pub ring_offset: f32,
    pub stats: InputStats,
    /// Caret blink: half-period (seconds; 0: no blink) and the time since the last flip.
    pub blink_half: f32,
    blink_t: f32,
    chain: Vec<usize>,
    chain_h: Vec<Handle>,
}

impl Default for Input {
    fn default() -> Self {
        Input {
            acts: vec![Act::new(Handle::default())],
            by_handle: Vec::new(),
            scrollers: Vec::new(),
            fields: Vec::new(),
            tips: Vec::new(),
            views: Vec::new(),
            mounting_view: None,
            lhooks: Vec::new(),
            clip_kids: Default::default(),
            wclips: Vec::new(),
            offsets: Vec::new(),
            hover: Vec::new(),
            cell: NRect::EMPTY,
            cell_epoch: 0,
            cell_ok: false,
            pointer: None,
            pressed: 0,
            press_at: (0.0, 0.0),
            dragging: false,
            focus: None,
            focus_act: 0,
            focus_visible: false,
            ring: None,
            ring_rect: NRect::EMPTY,
            ring_color: Color::rgba(126, 227, 255, 255),
            ring_width: 2.0,
            ring_offset: 2.0,
            stats: InputStats::default(),
            blink_half: 0.53,
            blink_t: 0.0,
            chain: Vec::new(),
            chain_h: Vec::new(),
        }
    }
}

impl Input {
    /// Interaction tables as memory-ledger lines.
    pub fn ledger(&self, l: &mut bough_mem::Ledger) {
        use bough_mem::Scales::Content;
        l.vec("input", "acts", &self.acts, self.acts.capacity(), Content);
        l.vec("input", "act by handle", &self.by_handle, self.by_handle.capacity(), Content);
        let rest = self.scrollers.capacity() * size_of::<Scroller>() + self.fields.capacity() * size_of::<Field>() + self.tips.capacity() * size_of::<Tip>() + self.views.capacity() * size_of::<crate::template::ScrollView>() + self.hover.capacity() * 8 + self.wclips.capacity() * 6 + self.offsets.capacity() * size_of::<crate::template::OffsetState>();
        l.many("input", "scrollers, fields, tips, views, hover chain", Content, rest, rest);
    }
    #[inline]
    fn act_of(&self, h: Handle) -> u32 {
        match self.by_handle.get(h.idx as usize) {
            Some(&a) if a != 0 && self.acts[a as usize].handle == h => a,
            _ => 0,
        }
    }
    /// The act of handle `h`, created if absent.
    fn ensure(&mut self, h: Handle) -> u32 {
        let a = self.act_of(h);
        if a != 0 {
            return a;
        }
        if self.by_handle.len() <= h.idx as usize {
            self.by_handle.resize(h.idx as usize + 1, 0);
        }
        self.acts.push(Act::new(h));
        let a = (self.acts.len() - 1) as u32;
        self.by_handle[h.idx as usize] = a;
        a
    }
    pub(crate) fn ensure_act(&mut self, h: Handle) -> u32 {
        self.ensure(h)
    }
    pub(crate) fn act_of_handle(&self, h: Handle) -> u32 {
        self.act_of(h)
    }
    pub(crate) fn tips_of(&self, v: usize) -> &[u32] {
        &self.views[v].tips
    }
    /// A tooltip inside a moved view: its rect moves too (and its clip, if it is showing).
    pub(crate) fn shift_tip(&mut self, b: &mut bough_core::Bough, t: u32, dx: i32, dy: i32) {
        let tip = &mut self.tips[t as usize];
        tip.rect = tip.rect.translate(dx, dy);
        if b.clip_rect(tip.clip) != NRect::EMPTY {
            b.set_clip_rect(tip.clip, tip.rect);
        }
    }
    /// Is the focused element a text field (zgui's `input::typing`)?
    pub fn focused_field(&self) -> bool {
        self.focus_act != 0 && self.acts[self.focus_act as usize].field != NONE
    }
    pub fn hovered(&self) -> &[Handle] {
        &self.hover
    }
}

// =============================================================================== mounting: the ops

/// Make the first record `m` pushed since `before` interactive: give it a handle (if it has none), flag
/// it, and return its act. A quad's node and instance are found so interaction styles can write it.
fn act_for<V>(m: &mut Mounter<'_, V>, before: usize, nodes_before: usize, flags: u8) -> u32 {
    assert!(m.recs.len() > before, "an interactive element mounts a record");
    let rec = m.recs[before];
    let node = m.scope.nodes[nodes_before..].iter().copied().find(|&n| m.rt.nodes[n as usize].handle.idx == rec.handle && rec.handle != 0).unwrap_or(NONE);
    let h = if node != NONE {
        m.rt.nodes[node as usize].handle
    } else if rec.handle != 0 {
        m.rt.bough.handle_gen(rec.handle)
    } else {
        let h = m.rt.bough.new_handle();
        m.recs[before].handle = h.idx;
        h
    };
    m.recs[before].kind |= KF_INTERACT | flags;
    let a = m.rt.input.ensure(h);
    let act = &mut m.rt.input.acts[a as usize];
    if rec.kind & 0x1F == REC_QUAD && rec.payload & 0xC000_0000 == 0 {
        act.inst = rec.payload;
    }
    if node != NONE {
        act.node = node;
    }
    a
}

/// The act of record `rec` of the mount batch (made interactive: a handle, `KF_INTERACT`); its quad node
/// is the most recent node mounted with that handle.
pub fn act_at<V>(m: &mut Mounter<'_, V>, rec: usize) -> u32 {
    let r = m.recs[rec];
    let node = if r.handle != 0 { m.scope.nodes.iter().rev().take(64).copied().find(|&n| m.rt.nodes[n as usize].handle.idx == r.handle) } else { None };
    let h = match node {
        Some(n) => m.rt.nodes[n as usize].handle,
        None if r.handle != 0 => m.rt.bough.handle_gen(r.handle),
        None => {
            let h = m.rt.bough.new_handle();
            m.recs[rec].handle = h.idx;
            h
        }
    };
    m.recs[rec].kind |= KF_INTERACT;
    let a = m.rt.input.ensure(h);
    let act = &mut m.rt.input.acts[a as usize];
    if r.kind & 0x1F == REC_QUAD && r.payload & 0xC000_0000 == 0 {
        act.inst = r.payload;
    }
    if let Some(n) = node {
        act.node = n;
    }
    a
}

/// Routes, scrollers and text fields run on the app's root owner (`App` dispatches them with it), so they
/// may only be mounted by the root owner's description: in a list row or an entity view (another owner
/// type, `Mounter::cls != 0`) the route's `V` is not the root's, and calling it would be undefined
/// behaviour. Refused at mount (a panic, not UB) until routes carry their owner.
#[track_caller]
fn root_owner_only<V>(m: &Mounter<'_, V>, what: &str) {
    assert!(m.cls == 0, "{what} inside a list row or an entity view: routes run on the app's root owner; put it on an element of the root view");
}

/// One interaction op on an element (`On<E, O>` installs it on E's first record).
pub trait ActOp<V> {
    /// Kind flags the record gets besides [`KF_INTERACT`].
    const FLAGS: u8 = 0;
    fn install(self, m: &mut Mounter<'_, V>, act: u32);
}

/// An element with an interaction op on its first record.
pub struct On<E, O>(pub E, pub O);

impl<V, E: Element<V>, O: ActOp<V>> Element<V> for On<E, O> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let (before, nodes_before) = (m.recs.len(), m.scope.nodes.len());
        self.0.mount(m);
        let a = act_for(m, before, nodes_before, O::FLAGS);
        self.1.install(m, a);
    }
    /// The interaction stays installed on the element's first record; the element re-describes.
    fn update(self, m: &mut Mounter<'_, V>) {
        self.0.update(m);
    }
}

pub struct Click<F>(pub F);
impl<V: 'static, F: Fn(&mut V) + Copy + 'static> ActOp<V> for Click<F> {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        root_owner_only(m, "click");
        m.rt.input.acts[a as usize].click = Some(route_plain::<V, F>(self.0));
    }
}
pub struct Press<F>(pub F);
impl<V: 'static, F: Fn(&mut V, &Ev) + Copy + 'static> ActOp<V> for Press<F> {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        root_owner_only(m, "press");
        m.rt.input.acts[a as usize].press = Some(route_ev::<V, F>(self.0));
    }
}
pub struct Hover<F>(pub F);
impl<V: 'static, F: Fn(&mut V, &Ev) + Copy + 'static> ActOp<V> for Hover<F> {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        root_owner_only(m, "hover");
        m.rt.input.acts[a as usize].hover = Some(route_ev::<V, F>(self.0));
    }
}
pub struct HoverBg(pub Color);
impl<V> ActOp<V> for HoverBg {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        let inst = m.rt.input.acts[a as usize].inst;
        assert!(inst != NONE, "hover_bg needs a painted quad");
        let i = &m.rt.inst[inst as usize];
        let bordered = i.kind == KIND_BORDER;
        let base = if bordered { i.color2 } else { i.color };
        let act = &mut m.rt.input.acts[a as usize];
        act.hover_bg = Some(self.0 .0);
        act.base = base;
        act.bordered = bordered;
    }
}
pub struct Focusable;
impl<V> ActOp<V> for Focusable {
    const FLAGS: u8 = KF_FOCUS;
    fn install(self, _: &mut Mounter<'_, V>, _: u32) {}
}
pub struct OnKey<F>(pub F);
impl<V: 'static, F: Fn(&mut V, &Ev) + Copy + 'static> ActOp<V> for OnKey<F> {
    const FLAGS: u8 = KF_FOCUS;
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        root_owner_only(m, "onkey");
        m.rt.input.acts[a as usize].key = Some(route_ev::<V, F>(self.0));
    }
}
pub struct Drag<F>(pub F);
impl<V: 'static, F: Fn(&mut V, &Ev) + Copy + 'static> ActOp<V> for Drag<F> {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        root_owner_only(m, "drag");
        m.rt.input.acts[a as usize].drag = Some(route_ev::<V, F>(self.0));
    }
}
/// The element scrolls the owner's `Signal<f32>` (points, `0..=max`) with wheel glides.
pub struct Scrolls<'a, V> {
    sig: Wired<'a, f32, V>,
    line: f32,
    max: f32,
}
impl<V> ActOp<V> for Scrolls<'_, V> {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        root_owner_only(m, "scrolls");
        m.rt.input.scrollers.push(Scroller { off: self.sig.off, line: self.line, max: self.max, glide: None, root: NONE, port_h: 0.0 });
        m.rt.input.acts[a as usize].scroll = (m.rt.input.scrollers.len() - 1) as u32;
    }
}

/// The paint state of an instance as one hash word (content addressing: one convention per record).
#[inline]
fn pair(color: u32, color2: u32) -> u64 {
    (color as u64) << 32 | color2 as u64
}

/// `paint_with`: a wire writing both colour words of the element's instance (`color`, `color2`: the
/// border and the fill of a border box) from one signal: one class on one element. While the element is
/// hovered its fill stays the hover colour and the new fill becomes the base it returns to.
pub struct PaintOp<'a, T, V, F> {
    sig: Wired<'a, T, V>,
    f: F,
}
impl<'a, T: 'static, V, F: Fn(&T) -> (Color, Color) + Copy + 'static> ActOp<V> for PaintOp<'a, T, V, F> {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        let inst = m.rt.input.acts[a as usize].inst;
        assert!(inst != NONE, "paint_with needs a painted quad");
        let bordered = m.rt.inst[inst as usize].kind == KIND_BORDER;
        let (c1, c2) = (self.f)(self.sig.get());
        let act = &mut m.rt.input.acts[a as usize];
        act.bordered = bordered;
        let p = &mut m.rt.inst[inst as usize];
        if bordered {
            p.color = c1.0;
            p.color2 = c2.0;
            act.base = c2.0;
        } else {
            p.color = c2.0;
            act.base = c2.0;
        }
        m.wire(&self.sig, patch_paint2::<T, F>, a, recipe(self.f));
    }
}

/// # Safety
/// Wire contract: `sig` is a `Signal<T>`; the recipe holds an `F`; `w.node` is the act.
unsafe fn patch_paint2<T: 'static, F: Fn(&T) -> (Color, Color) + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { &*(sig as *const Signal<T>) };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    let (c1, c2) = f(s);
    let act = rt.input.acts[w.node as usize];
    rt.input.acts[w.node as usize].base = c2.0;
    let p = rt.inst_mut(act.inst);
    let old = pair(p.color, p.color2);
    let fill = if act.hovered && act.hover_bg.is_some() { act.hover_bg.unwrap() } else { c2.0 };
    if act.bordered {
        p.color = c1.0;
        p.color2 = fill;
    } else {
        p.color = fill;
    }
    let now = pair(p.color, p.color2);
    if now != old {
        rt.paint_write(act.handle, old, now);
    }
}

/// Interaction ops on any element (they apply to its first record). The owner type is inferred from
/// where the element is mounted (and from the closures' argument types).
pub trait Interact: Sized {
    /// Activation (a primary press and release on the element, zgui's / the DOM's click).
    fn on_click<V, F: Fn(&mut V) + Copy + 'static>(self, f: F) -> On<Self, Click<F>> {
        On(self, Click(f))
    }
    /// The press itself.
    fn on_press<V, F: Fn(&mut V, &Ev) + Copy + 'static>(self, f: F) -> On<Self, Press<F>> {
        On(self, Press(f))
    }
    /// Pointer enters (`ev.on`) or leaves the element.
    fn on_hover<V, F: Fn(&mut V, &Ev) + Copy + 'static>(self, f: F) -> On<Self, Hover<F>> {
        On(self, Hover(f))
    }
    /// Background while hovered (`:hover { background }`): a paint write on crossing, no route.
    fn hover_bg(self, c: Color) -> On<Self, HoverBg> {
        On(self, HoverBg(c))
    }
    /// Both colour words of the element (border and fill of a border box; the fill of a plain one) from one
    /// signal, hover-aware (see [`PaintOp`]).
    fn paint_with<'a, T: 'static, V, F: Fn(&T) -> (Color, Color) + Copy + 'static>(self, sig: Wired<'a, T, V>, f: F) -> On<Self, PaintOp<'a, T, V, F>> {
        On(self, PaintOp { sig, f })
    }
    /// A tab stop (focus order is preorder).
    fn focusable(self) -> On<Self, Focusable> {
        On(self, Focusable)
    }
    /// Keys while the element (or a descendant) has focus; a tab stop.
    fn on_key<V, F: Fn(&mut V, &Ev) + Copy + 'static>(self, f: F) -> On<Self, OnKey<F>> {
        On(self, OnKey(f))
    }
    /// Pointer travel while pressed (capture: no hit test until release). `ev.dx/dy` since the press.
    fn on_drag<V, F: Fn(&mut V, &Ev) + Copy + 'static>(self, f: F) -> On<Self, Drag<F>> {
        On(self, Drag(f))
    }
    /// The wheel over this element glides `sig` (points) within `0..=max`, `line` points per detent line.
    fn scrolls<V>(self, sig: Wired<'_, f32, V>, line: f32, max: f32) -> On<Self, Scrolls<'_, V>> {
        On(self, Scrolls { sig, line, max })
    }
}
impl<E> Interact for E {}

/// Pointer-transparent subtree (carets, overlays, drag previews): every record it mounts is [`KF_NOHIT`].
pub struct NoHit<E>(pub E);
pub fn no_hit<E>(e: E) -> NoHit<E> {
    NoHit(e)
}
impl<V, E: Element<V>> Element<V> for NoHit<E> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let before = m.recs.len();
        self.0.mount(m);
        for r in &mut m.recs[before..] {
            r.kind |= KF_NOHIT;
        }
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        self.0.update(m);
    }
}

/// A tooltip: a bubble (rounded box + one line of text) mounted once, pointer-transparent, hidden behind an
/// empty clip. While the pointer is over `trigger` (its first record) the clip is the bubble's rect and the
/// bubble fades in (alpha writes on its two records each tick, zgui's surface enter fade); leaving hides it
/// at once (zgui: a tooltip taken down to show the next goes instantly). No mount, no rebuild.
pub struct Tooltip<T> {
    trigger: T,
    text: String,
    rect: [f32; 4],
    style: TipStyle,
}
#[derive(Clone, Copy)]
pub struct TipStyle {
    pub bg: Color,
    pub border: Color,
    pub fg: Color,
    pub size: f32,
    pub radius: f32,
    /// Fade-in seconds (0: instant).
    pub fade: f32,
}
impl Default for TipStyle {
    fn default() -> Self {
        TipStyle { bg: Color::rgba(23, 23, 23, 255), border: Color::rgba(38, 38, 38, 255), fg: Color::rgba(250, 250, 250, 255), size: 12.0, radius: 6.0, fade: 0.150 }
    }
}
pub fn tooltip<T>(trigger: T, text: impl Into<String>, rect: [f32; 4]) -> Tooltip<T> {
    Tooltip { trigger, text: text.into(), rect, style: TipStyle::default() }
}
impl<T> Tooltip<T> {
    pub fn style(mut self, s: TipStyle) -> Self {
        self.style = s;
        self
    }
}

/// A mounted tooltip (`Input::tips`).
pub(crate) struct Tip {
    clip: u16,
    pub(crate) rect: NRect,
    bubble: Handle,
    inst: u32,
    label: Handle,
    ti: u32,
    style: TipStyle,
    /// Current alpha (0..=1) and whether it is fading in.
    alpha: f32,
    fading: bool,
}

fn with_alpha(c: u32, a: f32) -> u32 {
    (c & 0x00ff_ffff) | (((c >> 24) as f32 * a).round() as u32) << 24
}

/// Write tooltip `k`'s alpha into its bubble instance and label run header (paint writes).
fn tip_alpha(rt: &mut Rt, k: usize, a: f32) {
    let (inst, bh, lh, ti, st) = {
        let t = &mut rt.input.tips[k];
        t.alpha = a;
        (t.inst, t.bubble, t.label, t.ti, t.style)
    };
    let p = rt.inst_mut(inst);
    let old = pair(p.color, p.color2);
    p.color = with_alpha(st.border.0, a);
    p.color2 = with_alpha(st.bg.0, a);
    let now = pair(p.color, p.color2);
    if old != now {
        rt.paint_write(bh, old, now);
    }
    if let crate::text::TShape::Label(lb) = &rt.texts[ti as usize].shape {
        let run = lb.run as usize;
        let old = rt.glyphs.runs[run].color;
        let now = with_alpha(st.fg.0, a);
        if old != now {
            rt.glyphs.runs[run].color = now;
            rt.paint_write(lh, old as u64, now as u64);
        }
    }
}

impl<V: 'static, T: Element<V>> Element<V> for Tooltip<T> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_opaque(m, |m| self.mount_in(m));
    }
    /// The bubble's state is the tooltip's own (its fade): a re-description steps over it.
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}
impl<T> Tooltip<T> {
    fn mount_in<V: 'static>(self, m: &mut Mounter<'_, V>)
    where
        T: Element<V>,
    {
        use crate::elem::{quad, text};
        let (before, nodes_before) = (m.recs.len(), m.scope.nodes.len());
        self.trigger.mount(m);
        let a = act_for(m, before, nodes_before, 0);
        let s = m.rt.scale;
        let id = m.rt.bough.new_clip(NRect::EMPTY);
        let [x, y, w, h] = self.rect;
        let o = m.rt.space(m.space);
        let r = NRect::new((o[0] + (m.ox + x) * s).floor() as i32, (o[1] + (m.oy + y) * s).floor() as i32, (o[0] + (m.ox + x + w) * s).ceil() as i32, (o[1] + (m.oy + y + h) * s).ceil() as i32);
        let prev = m.clip;
        m.clip = id;
        let st = self.style;
        // the bubble and its label, transparent until shown; both get handles (their paint is written)
        let b = m.recs.len();
        quad(x, y, w, h).bg(Color(with_alpha(st.bg.0, 0.0))).rounded(st.radius).border(1.0, Color(with_alpha(st.border.0, 0.0))).mount(m);
        let bubble = m.rt.bough.new_handle();
        m.recs[b].handle = bubble.idx;
        let inst = m.recs[b].payload;
        let t = m.recs.len();
        let tn = m.scope.nodes.len();
        let lh = (st.size * 1.4).ceil();
        text(self.text).at(x + 8.0, y + (h - lh) * 0.5).size(st.size).line_height(lh).color(Color(with_alpha(st.fg.0, 0.0))).mount(m);
        let label = m.rt.bough.new_handle();
        m.recs[t].handle = label.idx;
        let ti = m.scope.nodes[tn..].iter().map(|&n| m.rt.nodes[n as usize].text).find(|&t| t != NONE).expect("tooltip label");
        m.rt.texts[ti as usize].handle = label;
        for rec in &mut m.recs[b..] {
            rec.kind |= KF_NOHIT;
        }
        m.clip = prev;
        let k = m.rt.input.tips.len() as u32;
        m.rt.input.tips.push(Tip { clip: id, rect: r, bubble, inst, label, ti, style: st, alpha: 0.0, fading: false });
        m.rt.input.acts[a as usize].tip = k;
        if let Some(v) = m.rt.input.mounting_view {
            m.rt.input.views[v as usize].tips.push(k);
        }
    }
}

// =============================================================================== text field

/// A one-line text field editing the owner's `Signal<String>`: a bordered box (a tab stop), the text as a
/// label wired to the signal, and a caret quad. A keystroke edits the string (QUEUED), and the settle runs
/// the label's patch (glyph slots from the first changed byte) then the caret's (one quad).
pub struct TextField<'a, V> {
    sig: Wired<'a, String, V>,
    rect: [f32; 4],
    size: f32,
    pad: f32,
    bg: Color,
    border: Color,
    color: Color,
    radius: f32,
}
pub fn text_field<V>(sig: Wired<'_, String, V>) -> TextField<'_, V> {
    TextField {
        sig,
        rect: [0.0, 0.0, 240.0, 32.0],
        size: 14.0,
        pad: 8.0,
        bg: Color::rgba(20, 24, 32, 255),
        border: Color::rgba(60, 70, 90, 255),
        color: Color::rgba(229, 237, 247, 255),
        radius: 6.0,
    }
}
impl<V> TextField<'_, V> {
    pub fn rect(mut self, x: f32, y: f32, w: f32, h: f32) -> Self {
        self.rect = [x, y, w, h];
        self
    }
    pub fn font(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
    pub fn colors(mut self, bg: Color, border: Color, text: Color) -> Self {
        self.bg = bg;
        self.border = border;
        self.color = text;
        self
    }
    pub fn rounded(mut self, r: f32) -> Self {
        self.radius = r;
        self
    }
}

/// # Safety
/// Wire contract: `sig` is a `Signal<String>`; the recipe holds the field index.
unsafe fn patch_caret(rt: &mut Rt, w: &Wire, _sig: *const u8, _owner: *const u8) {
    let f = u32::from_le_bytes(w.recipe[..4].try_into().unwrap());
    place_caret(rt, f);
}

/// Put field `f`'s caret quad at its byte index (pen of that slot, or the label's end).
fn place_caret(rt: &mut Rt, f: u32) {
    let (ti, cn, caret, color, focused) = {
        let fl = &rt.input.fields[f as usize];
        (fl.ti, fl.caret_node, fl.caret, fl.caret_color, fl.focused && fl.lit)
    };
    let len = rt.texts[ti as usize].bytes().len();
    let caret = caret.min(len);
    rt.input.fields[f as usize].caret = caret;
    let (ox, size_px) = (rt.texts[ti as usize].ox, rt.texts[ti as usize].size_px);
    let scale = size_px / rt.text.shaper.script.upem as f32;
    let units = match &rt.texts[ti as usize].shape {
        crate::text::TShape::Label(lb) => {
            if caret >= len || !lb.is_identity() {
                lb.width()
            } else {
                let h = rt.glyphs.runs[lb.run as usize];
                rt.glyphs.x[(h.base as usize) + caret].wrapping_sub(h.x0)
            }
        }
        _ => 0,
    };
    let x = bough_text::pool::pen_px(units, scale, ox + 0.5) as f32;
    let n = &mut rt.nodes[cn as usize];
    let want = if focused { color } else { color & 0x00ff_ffff };
    let (moved, recolor) = (n.x != x, n.color != want);
    if !moved && !recolor {
        return;
    }
    let (inst, h, old, (y, w, hh)) = (rt.runs[n.run as usize].first, n.handle, n.color, (n.y, n.w, n.h));
    n.color = want;
    rt.inst_mut(inst).color = want;
    if moved {
        // a recoloured caret changes everywhere it is: old and new ink; a moved one only near its edges
        if recolor {
            rt.bough.touch_handle(h);
        }
        rt.quad_geom(cn, x, y, w, hh);
        if recolor {
            rt.bough.touch_handle(h);
        }
    } else {
        rt.paint_write(h, old as u64, want as u64);
    }
}

impl<V: 'static> Element<V> for TextField<'_, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_opaque(m, |m| self.mount_in(m));
    }
    /// The field edits its signal (state); a re-description steps over it.
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}
impl<V: 'static> TextField<'_, V> {
    fn mount_in(self, m: &mut Mounter<'_, V>) {
        use crate::elem::{quad, text};
        root_owner_only(m, "text_field");
        let [x, y, w, h] = self.rect;
        let (before, nodes_before) = (m.recs.len(), m.scope.nodes.len());
        let s = m.rt.scale;
        // the box (a node with a handle: the caret colour and focus need it addressable)
        quad(x, y, w, h).bg(self.bg).rounded(self.radius).border(1.0, self.border).children(()).mount(m);
        let a = act_for(m, before, nodes_before, KF_FOCUS);
        let f = m.rt.input.fields.len() as u32;
        m.rt.input.fields.push(Field { off: self.sig.off, ti: NONE, caret_node: NONE, caret: 0, caret_color: self.color.0, focused: false, lit: true });
        m.rt.input.acts[a as usize].field = f;
        // children inside the border, clipped to the inner box
        let (ox, oy, depth, clip) = (m.ox, m.oy, m.depth, m.clip);
        m.ox += x + 1.0;
        m.oy += y + 1.0;
        m.depth += 1;
        let o = m.rt.space(m.space);
        let inner = [o[0] + m.ox * s, o[1] + m.oy * s, o[0] + (m.ox + w - 2.0) * s, o[1] + (m.oy + h - 2.0) * s];
        let r = if clip != 0 {
            let p = m.rt.bough.clip_rect(clip).to_xyxy();
            [inner[0].max(p[0] as f32), inner[1].max(p[1] as f32), inner[2].min(p[2] as f32), inner[3].min(p[3] as f32)]
        } else {
            inner
        };
        m.clip = m.rt.scoped_clip(r, clip);
        // the caret wire first: wires run newest first, so it runs after the label's patch
        let lh = (self.size * 1.4).ceil();
        let ty = ((h - 2.0) - lh) * 0.5;
        let caret_h = lh;
        let cb = m.recs.len();
        let cn_before = m.scope.nodes.len();
        no_hit(quad(self.pad, ty, 1.0, caret_h).bg(sig_caret(self.sig, f, self.color))).mount(m);
        let cnode = m.scope.nodes[cn_before..].iter().copied().find(|&n| m.rt.nodes[n as usize].handle.idx == m.recs[cb].handle).expect("caret node");
        m.rt.input.fields[f as usize].caret_node = cnode;
        let tn_before = m.scope.nodes.len();
        text(self.sig).at(self.pad, ty).size(self.size).line_height(lh).color(self.color).mount(m);
        let tnode = m.scope.nodes[tn_before..].iter().copied().find(|&n| m.rt.nodes[n as usize].text != NONE).expect("label node");
        m.rt.input.fields[f as usize].ti = m.rt.nodes[tnode as usize].text;
        let len = m.rt.texts[m.rt.input.fields[f as usize].ti as usize].bytes().len();
        m.rt.input.fields[f as usize].caret = len;
        (m.ox, m.oy, m.depth, m.clip) = (ox, oy, depth, clip);
        // the caret sits at the start, transparent, until the field is focused (`place_caret`)
    }
}

/// The caret's colour operand: a wire on the field's signal that only re-places the caret (its colour is
/// written by focus changes); the initial colour is transparent.
struct CaretOp<'a, V> {
    sig: Wired<'a, String, V>,
    f: u32,
    color: Color,
}
fn sig_caret<V>(sig: Wired<'_, String, V>, f: u32, color: Color) -> CaretOp<'_, V> {
    CaretOp { sig, f, color }
}
impl<'a, V> crate::elem::IntoProp<Color, V> for CaretOp<'a, V> {
    fn lower<K: crate::elem::Apply<P = Color>>(self, m: &mut Mounter<'_, V>, _node: u32) -> Color {
        let mut r = [0u8; 16];
        r[..4].copy_from_slice(&self.f.to_le_bytes());
        m.wire(&self.sig, patch_caret, NONE, r);
        Color(self.color.0 & 0x00ff_ffff)
    }
    fn value(self) -> Color {
        Color(self.color.0 & 0x00ff_ffff)
    }
}

// =============================================================================== dispatch

impl<V: View> App<V> {
    #[inline]
    fn owner_mut(&mut self) -> *mut u8 {
        &mut *self.owner as *mut V as *mut u8
    }

    /// Settle what routes queued and lay out what changed (the end of an input turn).
    fn settle_input(&mut self) {
        let owner = &mut *self.owner as *mut V as *const u8;
        // SAFETY: the owner is the mounted view (uniquely borrowed: list patches write their cell); every
        // wire was installed from a `sig!` proof for it.
        unsafe { self.rt.settle(owner) };
        // an arbitrary handler: `changed` waves, then a re-describing owner describes again (API.md)
        self.effects();
        self.redescribe();
        self.rt.lay_flush();
        self.sync_ring();
    }

    #[inline]
    fn dev(&self, x: f32, y: f32) -> (i32, i32) {
        let s = self.rt.scale;
        ((x * s).floor() as i32, (y * s).floor() as i32)
    }

    /// The pointer moved to (x, y) points. Hover chains change (enter/leave writes and routes) only when
    /// the pointer leaves its hit cell; while a press with a drag route is held, the drag route runs
    /// instead (capture: no hit test).
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.sync_pub();
        let (px, py) = self.dev(x, y);
        self.rt.input.pointer = Some((px, py));
        let pa = self.rt.input.pressed;
        if pa != 0 && self.rt.input.dragging {
            if let Some(r) = self.rt.input.acts[pa as usize].drag {
                let (x0, y0) = self.rt.input.press_at;
                let e = Ev { dx: x - x0, dy: y - y0, ..Ev::at(x, y) };
                let o = self.owner_mut();
                self.rt.input.stats.routes += 1;
                // SAFETY: the route was lowered for this owner type.
                unsafe { r.call(o, &e) };
                self.settle_input();
                return;
            }
        }
        if self.update_hover(px, py, x, y) {
            self.settle_input();
        }
    }

    /// Recompute the hover chain at (px, py) unless the hit cell still answers. Returns whether routes or
    /// styles ran.
    fn update_hover(&mut self, px: i32, py: i32, x: f32, y: f32) -> bool {
        let inp = &self.rt.input;
        if inp.cell_ok && inp.cell.contains_point(px, py) && self.rt.bough.cell_valid(inp.cell, inp.cell_epoch) {
            self.rt.input.stats.cell_hits += 1;
            return false;
        }
        self.rt.input.stats.hits += 1;
        let b = &self.rt.bough;
        let top = b.hit_top(px, py);
        let cell = b.hit_cell(px, py, top);
        let epoch = b.epoch();
        let mut chain = std::mem::take(&mut self.rt.input.chain);
        let mut ch = std::mem::take(&mut self.rt.input.chain_h);
        chain.clear();
        ch.clear();
        if let Some(t) = top {
            b.flagged_chain(t, KF_INTERACT, &mut chain);
            ch.extend(chain.iter().filter_map(|&p| b.handle_at(p)));
        }
        {
            let inp = &mut self.rt.input;
            inp.cell = cell;
            inp.cell_epoch = epoch;
            inp.cell_ok = true;
        }
        let old = std::mem::take(&mut self.rt.input.hover);
        let mut ran = false;
        if old != ch {
            self.rt.input.stats.crossings += 1;
            for &h in &old {
                if !ch.contains(&h) {
                    ran |= self.hover_set(h, false, x, y);
                }
            }
            for &h in &ch {
                if !old.contains(&h) {
                    ran |= self.hover_set(h, true, x, y);
                }
            }
        }
        self.rt.input.hover = ch;
        self.rt.input.chain_h = old;
        self.rt.input.chain = chain;
        ran
    }

    /// Hover on/off for one record: its hover background (a paint write), tooltip clip and route.
    fn hover_set(&mut self, h: Handle, on: bool, x: f32, y: f32) -> bool {
        let a = self.rt.input.act_of(h);
        if a == 0 {
            return false;
        }
        self.rt.input.acts[a as usize].hovered = on;
        let act = self.rt.input.acts[a as usize];
        let mut ran = false;
        if let Some(c) = act.hover_bg {
            let base = if act.node != NONE { self.rt.nodes[act.node as usize].color } else { act.base };
            let new = if on { c } else { base };
            if act.node != NONE {
                // while hovered the base colour wire writes `n.color` only (ApplyBg reads the flag)
                let n = &mut self.rt.nodes[act.node as usize];
                n.kind = if on { n.kind | crate::rt::NK_HOVERED } else { n.kind & !crate::rt::NK_HOVERED };
            }
            let p = self.rt.inst_mut(act.inst);
            let old = pair(p.color, p.color2);
            if act.bordered {
                p.color2 = new;
            } else {
                p.color = new;
            }
            let now = pair(p.color, p.color2);
            if old != now {
                self.rt.paint_write(h, old, now);
            }
            ran = true;
        }
        if act.tip != NONE {
            let k = act.tip as usize;
            let (clip, rect, fade) = {
                let t = &self.rt.input.tips[k];
                (t.clip, t.rect, t.style.fade)
            };
            self.rt.bough.set_clip_rect(clip, if on { rect } else { NRect::EMPTY });
            if on && fade > 0.0 {
                self.rt.input.tips[k].fading = true;
                tip_alpha(&mut self.rt, k, 0.0);
            } else {
                self.rt.input.tips[k].fading = false;
                tip_alpha(&mut self.rt, k, if on { 1.0 } else { 0.0 });
            }
            ran = true;
        }
        if let Some(r) = act.hover {
            let e = Ev { on, ..Ev::at(x, y) };
            let o = self.owner_mut();
            self.rt.input.stats.routes += 1;
            // SAFETY: the route was lowered for this owner type.
            unsafe { r.call(o, &e) };
            ran = true;
        }
        ran
    }

    /// A button pressed at (x, y): the hover chain is brought up to date, focus moves to the nearest
    /// focusable record of the chain (zgui / the DOM: pressing the background blurs), the press route
    /// runs, and a drag route captures the pointer.
    pub fn pointer_down(&mut self, x: f32, y: f32, button: Button) {
        self.sync_pub();
        let (px, py) = self.dev(x, y);
        self.rt.input.pointer = Some((px, py));
        self.update_hover(px, py, x, y);
        let chain = self.rt.input.hover.clone();
        // nearest focusable in the chain
        let mut focus = None;
        for &h in &chain {
            if let Some(p) = self.rt.bough.pos_of(h) {
                if self.rt.bough.kind_at(p) & KF_FOCUS != 0 {
                    focus = Some(h);
                    break;
                }
            }
        }
        if button == Button::Primary {
            self.set_focus(focus, false);
        }
        let target = chain.first().map_or(0, |&h| self.rt.input.act_of(h));
        self.rt.input.pressed = target;
        self.rt.input.press_at = (x, y);
        self.rt.input.dragging = false;
        // the press route and drag capture: nearest act in the chain that has one
        for &h in &chain {
            let a = self.rt.input.act_of(h);
            let act = self.rt.input.acts[a as usize];
            if let Some(r) = act.press {
                let e = Ev { button, ..Ev::at(x, y) };
                let o = self.owner_mut();
                self.rt.input.stats.routes += 1;
                // SAFETY: the route was lowered for this owner type.
                unsafe { r.call(o, &e) };
                break;
            }
        }
        if let Some(a) = chain.iter().map(|&h| self.rt.input.act_of(h)).find(|&a| self.rt.input.acts[a as usize].drag.is_some()) {
            self.rt.input.pressed = a;
            self.rt.input.dragging = true;
        }
        self.settle_input();
    }

    /// The button released at (x, y): a drag ends (its route sees `ev.on == false`); otherwise, if the
    /// pointer is still over the pressed element, it is activated (the click route of the nearest act in
    /// its chain that has one).
    pub fn pointer_up(&mut self, x: f32, y: f32, button: Button) {
        self.sync_pub();
        let pa = std::mem::replace(&mut self.rt.input.pressed, 0);
        let dragging = std::mem::replace(&mut self.rt.input.dragging, false);
        let (px, py) = self.dev(x, y);
        self.rt.input.pointer = Some((px, py));
        if dragging {
            if let Some(r) = self.rt.input.acts[pa as usize].drag {
                let (x0, y0) = self.rt.input.press_at;
                let e = Ev { dx: x - x0, dy: y - y0, on: false, ..Ev::at(x, y) };
                let o = self.owner_mut();
                // SAFETY: the route was lowered for this owner type.
                unsafe { r.call(o, &e) };
            }
            self.update_hover(px, py, x, y);
            self.settle_input();
            return;
        }
        self.update_hover(px, py, x, y);
        if pa != 0 && button == Button::Primary {
            let ph = self.rt.input.acts[pa as usize].handle;
            if let Some(i) = self.rt.input.hover.iter().position(|&h| h == ph) {
                let chain = self.rt.input.hover[i..].to_vec();
                for h in chain {
                    let a = self.rt.input.act_of(h);
                    if let Some(r) = self.rt.input.acts[a as usize].click {
                        let o = self.owner_mut();
                        self.rt.input.stats.routes += 1;
                        // SAFETY: the route was lowered for this owner type.
                        unsafe { r.call(o, &Ev::at(x, y)) };
                        break;
                    }
                }
            }
        }
        self.settle_input();
    }

    /// The pointer left the window: every hovered record is left.
    pub fn pointer_leave(&mut self) {
        self.sync_pub();
        self.rt.input.pointer = None;
        self.rt.input.cell_ok = false;
        let old = std::mem::take(&mut self.rt.input.hover);
        for h in old {
            self.hover_set(h, false, -1.0, -1.0);
        }
        self.settle_input();
    }

    /// A wheel turned at (x, y) by (dx, dy) lines: the nearest scroller of the chain aims its glide
    /// (detents add to the destination). The glide runs in `tick`.
    pub fn wheel(&mut self, x: f32, y: f32, _dx: f32, dy: f32) {
        self.sync_pub();
        let (px, py) = self.dev(x, y);
        self.rt.input.pointer = Some((px, py));
        self.update_hover(px, py, x, y);
        let chain = self.rt.input.hover.clone();
        for h in chain {
            let a = self.rt.input.act_of(h);
            let sc = self.rt.input.acts[a as usize].scroll;
            if sc == NONE {
                continue;
            }
            let o = self.owner_mut();
            let s = &mut self.rt.input.scrollers[sc as usize];
            // SAFETY: `off` is the offset_of!-proven offset of a Signal<f32> field of the owner (Wired).
            let at = **unsafe { &*(o.add(s.off as usize) as *const Signal<f32>) };
            let heading = s.glide.map_or(at, |g| g.to);
            let max = if s.root != NONE { (self.rt.lay.root_height(s.root) / self.rt.scale - s.port_h).max(0.0) } else { s.max };
            let s = &mut self.rt.input.scrollers[sc as usize];
            let to = (heading + dy * s.line).clamp(0.0, max);
            if to != heading || s.glide.is_some() {
                s.glide = Some(Tween { from: at, to, elapsed: 0.0 });
            }
            break;
        }
        self.settle_input();
    }

    /// The focused field's caret blinks every `blink_half` seconds: one paint write per flip (the caret's
    /// colour word), so after two frames every flip finds a surface that already shows it (content
    /// addressing: no GPU work). Returns whether it flipped.
    fn blink(&mut self, dt: f32) -> bool {
        let fa = self.rt.input.focus_act;
        if fa == 0 || self.rt.input.blink_half <= 0.0 {
            return false;
        }
        let f = self.rt.input.acts[fa as usize].field;
        if f == NONE {
            return false;
        }
        self.rt.input.blink_t += dt;
        if self.rt.input.blink_t < self.rt.input.blink_half {
            return false;
        }
        self.rt.input.blink_t -= self.rt.input.blink_half * (self.rt.input.blink_t / self.rt.input.blink_half).floor();
        self.sync_pub();
        let lit = !self.rt.input.fields[f as usize].lit;
        self.rt.input.fields[f as usize].lit = lit;
        place_caret(&mut self.rt, f);
        true
    }

    /// Seconds until the focused caret's next blink flip (`None`: nothing blinks): a host arms a timer for it.
    pub fn next_blink(&self) -> Option<f32> {
        let fa = self.rt.input.focus_act;
        (fa != 0 && self.rt.input.blink_half > 0.0 && self.rt.input.acts[fa as usize].field != NONE).then(|| self.rt.input.blink_half - self.rt.input.blink_t)
    }

    /// Precise (trackpad) scroll at (x, y) by `dy` points: the nearest scroller moves at once (the device's
    /// own deltas are already smooth; no glide).
    pub fn wheel_points(&mut self, x: f32, y: f32, dy: f32) {
        self.sync_pub();
        let (px, py) = self.dev(x, y);
        self.rt.input.pointer = Some((px, py));
        self.update_hover(px, py, x, y);
        let chain = self.rt.input.hover.clone();
        for h in chain {
            let a = self.rt.input.act_of(h);
            let sc = self.rt.input.acts[a as usize].scroll;
            if sc == NONE {
                continue;
            }
            let o = self.owner_mut();
            let max = {
                let s = &self.rt.input.scrollers[sc as usize];
                if s.root != NONE { (self.rt.lay.root_height(s.root) / self.rt.scale - s.port_h).max(0.0) } else { s.max }
            };
            let s = &mut self.rt.input.scrollers[sc as usize];
            s.glide = None;
            // SAFETY: as in `wheel`.
            let sig = unsafe { &mut *(o.add(s.off as usize) as *mut Signal<f32>) };
            let to = (**sig + dy).clamp(0.0, max);
            sig.set(to);
            break;
        }
        self.settle_input();
    }

    /// Is a glide (or any input animation) running?
    pub fn animating(&self) -> bool {
        self.rt.input.scrollers.iter().any(|s| s.glide.is_some()) || self.rt.input.tips.iter().any(|t| t.fading) || self.rt.transitions.running() || self.rt.lists.animating()
    }

    /// Advance input animations by `dt` seconds (glides write their scroll signals). Returns whether any
    /// ran; pair with `frame()`.
    pub fn tick(&mut self, dt: f32) -> bool {
        let blinked = self.blink(dt);
        if !self.animating() {
            // the frame clock runs regardless (a transition retargeted later starts from this time)
            self.rt.transitions.now += dt as f64;
            if blinked {
                self.settle_input();
            }
            return blinked;
        }
        self.sync_pub();
        // transitions: every running one applies its value through its property's applier
        self.rt.step_transitions(dt);
        let o = self.owner_mut();
        for s in &mut self.rt.input.scrollers {
            let Some(g) = s.glide.as_mut() else { continue };
            let (y, done) = g.advance(dt);
            if done {
                s.glide = None;
            }
            // SAFETY: as in `wheel`.
            let sig = unsafe { &mut *(o.add(s.off as usize) as *mut Signal<f32>) };
            sig.set(y);
        }
        for k in 0..self.rt.input.tips.len() {
            let t = &self.rt.input.tips[k];
            if !t.fading {
                continue;
            }
            let a = (t.alpha + dt / t.style.fade).min(1.0);
            self.rt.input.tips[k].fading = a < 1.0;
            tip_alpha(&mut self.rt, k, a);
        }
        // list moves (listview.rs): space writes and record translations, no layout
        self.rt.lists_tick(dt);
        self.settle_input();
        true
    }

    /// A key went down. Tab / Shift+Tab move focus (preorder scan of the kind column); a focused text
    /// field edits; otherwise the key routes of the focused record's chain run, nearest first.
    pub fn key_down(&mut self, key: Key, mods: Mods) {
        self.sync_pub();
        if key == Key::Tab {
            let cur = self.rt.input.focus.and_then(|h| self.rt.bough.pos_of(h));
            let next = self.rt.bough.next_flagged(cur, KF_FOCUS, mods.shift);
            let h = next.and_then(|p| self.rt.bough.handle_at(p));
            self.set_focus(h, true);
            self.settle_input();
            return;
        }
        let fa = self.rt.input.focus_act;
        if fa != 0 {
            let f = self.rt.input.acts[fa as usize].field;
            if f != NONE && !mods.cmd && !mods.ctrl && self.edit_field(f, key) {
                self.settle_input();
                return;
            }
        }
        if let Some(fh) = self.rt.input.focus {
            if let Some(p) = self.rt.bough.pos_of(fh) {
                let mut chain = Vec::new();
                self.rt.bough.flagged_chain(p, KF_INTERACT, &mut chain);
                for q in chain {
                    let Some(h) = self.rt.bough.handle_at(q) else { continue };
                    let a = self.rt.input.act_of(h);
                    if let Some(r) = self.rt.input.acts[a as usize].key {
                        let e = Ev { key: Some(key), mods, ..Ev::at(0.0, 0.0) };
                        let o = self.owner_mut();
                        self.rt.input.stats.routes += 1;
                        // SAFETY: the route was lowered for this owner type.
                        unsafe { r.call(o, &e) };
                        break;
                    }
                }
            }
        }
        self.settle_input();
    }

    /// Text typed (IME commit, or a printable key's text): inserted at the focused field's caret.
    pub fn text_input(&mut self, s: &str) {
        self.sync_pub();
        let fa = self.rt.input.focus_act;
        if fa != 0 {
            let f = self.rt.input.acts[fa as usize].field;
            if f != NONE {
                for c in s.chars() {
                    self.edit_field(f, Key::Char(c));
                }
            }
        }
        self.settle_input();
    }

    /// One edit of field `f` (the owner's string: QUEUED; the caret index here). Returns whether it was
    /// an editing key.
    fn edit_field(&mut self, f: u32, key: Key) -> bool {
        let o = self.owner_mut();
        let (off, caret) = {
            let fl = &self.rt.input.fields[f as usize];
            (fl.off, fl.caret)
        };
        // SAFETY: `off` is the offset_of!-proven offset of a Signal<String> field of the owner (Wired).
        let sig = unsafe { &mut *(o.add(off as usize) as *mut Signal<String>) };
        let len = sig.len();
        let caret = caret.min(len);
        let prev = |s: &str, i: usize| s[..i].char_indices().next_back().map_or(0, |(j, _)| j);
        let next = |s: &str, i: usize| s[i..].chars().next().map_or(i, |c| i + c.len_utf8());
        let new_caret = match key {
            Key::Char(c) => {
                sig.insert(caret, c);
                caret + c.len_utf8()
            }
            Key::Backspace if caret > 0 => {
                let p = prev(sig, caret);
                sig.replace_range(p..caret, "");
                p
            }
            Key::Delete if caret < len => {
                let n = next(sig, caret);
                sig.replace_range(caret..n, "");
                caret
            }
            Key::Left => prev(sig, caret),
            Key::Right => next(sig, caret),
            Key::Home => 0,
            Key::End => len,
            Key::Backspace | Key::Delete => caret,
            _ => return false,
        };
        self.rt.input.fields[f as usize].caret = new_caret;
        self.rt.input.fields[f as usize].lit = true;
        self.rt.input.blink_t = 0.0;
        if matches!(key, Key::Left | Key::Right | Key::Home | Key::End) {
            place_caret(&mut self.rt, f);
        }
        true
    }

    /// Move focus to `h` (None: blur). `visible`: keyboard focus, which shows the ring.
    pub fn set_focus(&mut self, h: Option<Handle>, visible: bool) {
        let old = self.rt.input.focus;
        if old == h && self.rt.input.focus_visible == visible {
            return;
        }
        if let Some(oh) = old {
            let a = self.rt.input.act_of(oh);
            let f = self.rt.input.acts[a as usize].field;
            if f != NONE {
                self.rt.input.fields[f as usize].focused = false;
                place_caret(&mut self.rt, f);
            }
        }
        self.rt.input.focus = h;
        self.rt.input.focus_visible = visible;
        let a = h.map_or(0, |h| self.rt.input.act_of(h));
        self.rt.input.focus_act = a;
        if a != 0 {
            let f = self.rt.input.acts[a as usize].field;
            if f != NONE {
                self.rt.input.fields[f as usize].focused = true;
                self.rt.input.fields[f as usize].lit = true;
                self.rt.input.blink_t = 0.0;
                place_caret(&mut self.rt, f);
            }
        }
        self.sync_ring();
    }

    /// Keep the focus ring on the focused element (keyboard focus only): one record at the end of the
    /// paint order, moved when its target's rect differs (an O(1) check per turn while shown).
    fn sync_ring(&mut self) {
        let want = match (self.rt.input.focus, self.rt.input.focus_visible) {
            (Some(h), true) => self.rt.bough.visible_ink_handle(h).map(|v| {
                let ink = self.rt.bough.pos_of(h).map_or(v, |p| self.rt.bough.record(p).ink);
                let o = (self.rt.input.ring_offset * self.rt.scale).round() as i32;
                let [x0, y0, x1, y1] = ink.to_xyxy();
                NRect::new(x0 - o, y0 - o, x1 + o, y1 + o)
            }),
            _ => None,
        }
        .unwrap_or(NRect::EMPTY);
        if want == self.rt.input.ring_rect {
            return;
        }
        let (rh, inst) = match self.rt.input.ring {
            Some(r) => r,
            None => {
                if want.is_empty() {
                    return;
                }
                let run = self.rt.alloc_run(1);
                self.rt.runs[run as usize].len = 1;
                let inst = self.rt.runs[run as usize].first;
                let s = self.rt.scale;
                *self.rt.inst_mut(inst) = bough_frame::Instance {
                    kind: KIND_BORDER,
                    color: self.rt.input.ring_color.0,
                    color2: 0,
                    bw: self.rt.input.ring_width * s,
                    radius: 4.0 * s,
                    ..Default::default()
                };
                let h = self.rt.bough.new_handle();
                let n = self.rt.bough.len();
                self.rt.bough.insert_run(n, &[bough_core::RecordInit { depth: 0, ink: NRect::EMPTY, kind: REC_QUAD | KF_NOHIT, payload: inst, handle: h.idx, clip: 0 }]);
                self.rt.input.ring = Some((h, inst));
                (h, inst)
            }
        };
        self.rt.input.ring_rect = want;
        let p = self.rt.inst_mut(inst);
        if want.is_empty() {
            p.size = [0.0, 0.0];
            self.rt.bough.set_ink_handle(rh, NRect::EMPTY);
        } else {
            let [x0, y0, x1, y1] = want.to_xyxy();
            p.pos = [x0 as f32 + 1.0, y0 as f32 + 1.0];
            p.size = [(x1 - x0) as f32 - 2.0, (y1 - y0) as f32 - 2.0];
            self.rt.bough.set_ink_handle(rh, want);
        }
    }

    /// The window was resized to (w, h) points: the damage rings, tile grid and layout groups take the new
    /// size; the groups re-lay out through cephal's incremental path (root size changed), and every
    /// surface is repainted whole.
    pub fn resize(&mut self, w_pt: f32, h_pt: f32) {
        self.sync_pub();
        let s = self.rt.scale;
        let (w, h) = ((w_pt * s).round() as i32, (h_pt * s).round() as i32);
        let (old_w, old_h) = (self.rt.win_w, self.rt.win_h);
        if w == old_w && h == old_h {
            return;
        }
        self.rt.win_w = w;
        self.rt.win_h = h;
        self.rt.bough.set_window(w, h);
        self.rt.paint.set_window(w, h);
        self.rt.layout.w = w as u32;
        self.rt.layout.h = h as u32;
        self.rt.tiles_x = self.rt.layout.tiles_x() as usize;
        self.rt.tiles_y = self.rt.layout.tiles_y() as usize;
        self.resized(w, h);
        // layout groups laid out at the window's size follow it (rows of lists keep their own widths)
        let (ow, oh) = (old_w as f32, old_h as f32);
        for r in 0..self.rt.lay.info.len() {
            let l = &mut self.rt.lay.info[r];
            if !(l.live && l.parent == NONE && l.avail_w == ow) {
                continue;
            }
            l.avail_w = w as f32;
            if l.avail_h == oh {
                l.avail_h = h as f32;
            }
            if !l.queued {
                l.queued = true;
                self.rt.lay.pending.push(r as u32);
            }
        }
        // scroll views whose port is the window: the port and the content's available width follow it
        for v in 0..self.rt.input.views.len() {
            let view = &self.rt.input.views[v];
            if !view.follows {
                continue;
            }
            let (clip, root, inset) = (view.clip, view.root, view.inset_w);
            self.rt.bough.set_clip_rect(clip, NRect::new(0, 0, w, h));
            if root != NONE {
                let l = &mut self.rt.lay.info[root as usize];
                l.avail_w = w as f32 - inset;
                if !l.queued {
                    l.queued = true;
                    self.rt.lay.pending.push(root);
                }
            }
            for sc in self.rt.input.scrollers.iter_mut().filter(|s| s.root == root && root != NONE) {
                sc.port_h = h as f32 / s;
            }
        }
        self.rt.lay.mount_avail = (w as f32, h as f32);
        self.rt.bough.damage_rect(NRect::new(0, 0, w, h));
        self.rt.input.cell_ok = false;
        self.settle_input();
    }
}
