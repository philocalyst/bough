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

/// A typed route lowered: a trampoline monomorphised for (receiver type, closure type), the closure's
/// bytes, and the receiver's path from the root (`OwnerRef`, resolved along the owner chain when the route
/// is mounted: the design's L4). The receiver is the nearest enclosing owner whose type is the handler's
/// `&mut R`: the element's own owner (the root view, a list row, an entity), or an ancestor of it.
pub(crate) type RouteFn = unsafe fn(*mut u8, &[u8; 16], &Ev);

#[derive(Clone, Copy)]
pub(crate) struct Route {
    f: RouteFn,
    r: [u8; 16],
    own: OwnerRef,
}

impl Route {
    /// # Safety
    /// `owner` points at the live owner the route was lowered for (the one `self.own` names), uniquely
    /// borrowed for the call.
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

fn route_plain<V, F: Fn(&mut V) + Copy + 'static>(f: F, own: OwnerRef) -> Route {
    Route { f: call_plain::<V, F>, r: recipe(f), own }
}
fn route_ev<V, F: Fn(&mut V, &Ev) + Copy + 'static>(f: F, own: OwnerRef) -> Route {
    Route { f: call_ev::<V, F>, r: recipe(f), own }
}

/// The receiver of a route whose handler takes `&mut R`, lowered by a mounter of owner type `V`: the nearest
/// owner of type `R` on the chain from the element's own owner up to the root. None: a mount error.
#[track_caller]
fn receiver<V: 'static, R: 'static>(m: &Mounter<'_, V>, what: &str) -> OwnerRef {
    use std::any::TypeId;
    let want = TypeId::of::<R>();
    if want == TypeId::of::<V>() {
        return m.own;
    }
    let mut o = m.own;
    while !o.is_root() {
        let (ty, parent) = m.rt.owner_parent(o);
        if ty == want {
            return parent;
        }
        o = parent;
    }
    panic!("{what}: the handler takes `&mut {}`, but no owner of that type encloses the element (it is mounted by `{}`)", std::any::type_name::<R>(), std::any::type_name::<V>());
}

/// A re-description's closure for a route already installed: the trampoline and the receiver are fixed by
/// the description's type and place; the captures are this description's (a row id, an index).
#[inline]
fn refresh_route<F: Copy>(slot: &mut Option<Route>, f: F) {
    if cfg!(feature = "mutant_route_stale") {
        return;
    }
    if let Some(r) = slot {
        r.r = recipe(f);
    }
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
    /// Its variant table (`Input::vtabs`, `flags.rs`), or NONE.
    pub vt: u32,
    /// Its state flags (`flags.rs`): its own, those it sets for its subtree, those it inherits, and the
    /// effective word (own | inherited) its table is indexed by.
    pub own: u16,
    pub sub: u16,
    pub inh: u16,
    pub eff: u16,
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
        Act { handle, node: NONE, inst: NONE, vt: NONE, own: 0, sub: 0, inh: 0, eff: 0, click: None, press: None, hover: None, key: None, drag: None, scroll: NONE, field: NONE, tip: NONE }
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
    /// Released views' slots (`Rt::release_view`), reused by the next view.
    pub(crate) view_free: Vec<u32>,
    /// Layout placement hooks (`ltemplate::PlaceHook`, by `LInfo::hook`).
    pub(crate) lhooks: Vec<(crate::ltemplate::PlaceHook, u32)>,
    /// Hook slots of removed layout nodes (`Rt::lay_node_dead`), and tooltip slots of released scopes.
    pub(crate) lhook_free: Vec<u32>,
    pub(crate) tip_free: Vec<u32>,
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
    /// Variant tables (`flags.rs`), by `Act::vt`.
    pub(crate) vtabs: Vec<crate::flags::VTab>,
    /// Acts whose subtree flags wait for their records to be in the Bough (`Rt::flags_flush`).
    pub(crate) flags_pending: Vec<(u32, u16)>,
    pub(crate) flag_scratch: Vec<usize>,
    pub(crate) flag_stack: Vec<(u16, u16)>,
    /// The records `PRESSED` and `DRAG_OVER` are set on now.
    pressed_chain: Vec<Handle>,
    drag_over: Vec<Handle>,
    focus_within: Vec<Handle>,
    /// Some record sets flags for its subtree (a record mounted later may inherit them).
    pub(crate) any_sub: bool,
    /// Direct wires (`direct.rs`), boxed: the thread's write path finds it by address.
    pub(crate) direct: Box<crate::direct::Table>,
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
            view_free: Vec::new(),
            lhooks: Vec::new(),
            lhook_free: Vec::new(),
            tip_free: Vec::new(),
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
            vtabs: Vec::new(),
            flags_pending: Vec::new(),
            flag_scratch: Vec::new(),
            flag_stack: Vec::new(),
            pressed_chain: Vec::new(),
            drag_over: Vec::new(),
            focus_within: Vec::new(),
            any_sub: false,
            direct: Box::default(),
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
pub(crate) fn act_for<V>(m: &mut Mounter<'_, V>, before: usize, flags: u8) -> u32 {
    assert!(m.recs.len() > before, "an interactive element mounts a record");
    let rec = m.recs[before];
    // a node's index is its record's handle index
    let node = if m.rt.node_pending(rec.handle) { rec.handle } else { NONE };
    let h = if node != NONE {
        m.rt.handle(node)
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
    let node = m.rt.node_pending(r.handle).then_some(r.handle);
    let h = match node {
        Some(n) => m.rt.handle(n),
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

/// Scrollers and text fields write a signal of the app's root owner at a root-relative offset (`App`
/// reaches it from the root), so they may only be mounted by the root owner's description: in a list row or
/// an entity view the offset is another owner type's. Refused at mount (a panic, not UB) until they carry
/// their owner's path as routes do (`Route::own`).
#[track_caller]
fn root_owner_only<V>(m: &Mounter<'_, V>, what: &str) {
    assert!(m.own.is_root() && m.cls == 0, "{what} inside a list row or an entity view: it writes a signal of the app's root owner; put it on an element of the root view");
}

/// One interaction op on an element (`On<E, O>` installs it on E's first record).
pub trait ActOp<V> {
    /// Kind flags the record gets besides [`KF_INTERACT`].
    const FLAGS: u8 = 0;
    fn install(self, m: &mut Mounter<'_, V>, act: u32);
    /// A re-description: bring what the op captured by value up to date (a route's closure, a hover
    /// colour); what it installed (wires, scrollers, the act) stays.
    fn refresh(self, m: &mut Mounter<'_, V>, act: u32);
    /// The op's part of its element's seal (`seal.rs`): what `refresh` would write. An op with state of its
    /// own that a description does not determine (a scroller, a wire) refuses, the default.
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.refuse()
    }
}

/// A route's seal: which route it is and its closure's bytes. The trampoline and the receiver are fixed by
/// the description's type and place (a seal is only ever compared with one of the same item type at the same
/// place in the owner tree), so equal bytes are the same handler with the same captures; different bytes are
/// refreshed by the ordinary update. (Padding inside the captures has no defined value: it can only make two
/// equal closures look different, which costs a refresh.)
#[inline(always)]
fn seal_route<S: crate::seal::Sealer, F: Copy>(s: &mut S, which: u32, f: F) {
    s.word(0x0100 | which);
    s.bytes(&recipe(f)[..size_of::<F>()]);
}

/// An element with an interaction op on its first record.
pub struct On<E, O>(pub E, pub O);

impl<V, E: Element<V>, O: ActOp<V>> Element<V> for On<E, O> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let before = m.recs.len();
        self.0.mount(m);
        let a = act_for(m, before, O::FLAGS);
        self.1.install(m, a);
        m.tape_push(a);
    }
    /// The interaction stays installed on the element's first record; the element re-describes, and the
    /// op takes this description's captures (the act is on the tape, after the element's own entries).
    fn update(self, m: &mut Mounter<'_, V>) {
        self.0.update(m);
        let a = m.tape_next();
        self.1.refresh(m, a);
    }
    fn dyn_key(&self) -> u64 {
        self.0.dyn_key()
    }
    /// The element's operands, then the op's: an unchanged closure on an unchanged element is stepped over.
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        self.0.seal(s);
        self.1.seal(s);
    }
}

pub struct Click<R, F>(pub F, pub std::marker::PhantomData<fn(&mut R)>);
impl<R, F> Click<R, F> {
    pub fn new(f: F) -> Self {
        Click(f, std::marker::PhantomData)
    }
}
impl<V: 'static, R: 'static, F: Fn(&mut R) + Copy + 'static> ActOp<V> for Click<R, F> {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        let own = receiver::<V, R>(m, "on_click");
        m.rt.input.acts[a as usize].click = Some(route_plain::<R, F>(self.0, own));
    }
    fn refresh(self, m: &mut Mounter<'_, V>, a: u32) {
        refresh_route(&mut m.rt.input.acts[a as usize].click, self.0);
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        seal_route(s, 1, self.0);
    }
}
pub struct Press<R, F>(pub F, pub std::marker::PhantomData<fn(&mut R)>);
impl<R, F> Press<R, F> {
    pub fn new(f: F) -> Self {
        Press(f, std::marker::PhantomData)
    }
}
impl<V: 'static, R: 'static, F: Fn(&mut R, &Ev) + Copy + 'static> ActOp<V> for Press<R, F> {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        let own = receiver::<V, R>(m, "on_press");
        m.rt.input.acts[a as usize].press = Some(route_ev::<R, F>(self.0, own));
    }
    fn refresh(self, m: &mut Mounter<'_, V>, a: u32) {
        refresh_route(&mut m.rt.input.acts[a as usize].press, self.0);
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        seal_route(s, 2, self.0);
    }
}
pub struct Hover<R, F>(pub F, pub std::marker::PhantomData<fn(&mut R)>);
impl<R, F> Hover<R, F> {
    pub fn new(f: F) -> Self {
        Hover(f, std::marker::PhantomData)
    }
}
impl<V: 'static, R: 'static, F: Fn(&mut R, &Ev) + Copy + 'static> ActOp<V> for Hover<R, F> {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        let own = receiver::<V, R>(m, "on_hover");
        m.rt.input.acts[a as usize].hover = Some(route_ev::<R, F>(self.0, own));
    }
    fn refresh(self, m: &mut Mounter<'_, V>, a: u32) {
        refresh_route(&mut m.rt.input.acts[a as usize].hover, self.0);
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        seal_route(s, 3, self.0);
    }
}
/// The background while hovered: one variant of the record's flags (`.hover(|s| s.bg(c))`, `flags.rs`).
pub struct HoverBg(pub Color);
impl<V> ActOp<V> for HoverBg {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        let act = m.rt.input.acts[a as usize];
        assert!(act.inst != NONE, "hover_bg needs a painted quad");
        crate::flags::install_variants(m.rt, a, &[crate::flags::Variant { when: crate::flags::Flag::HOVERED.bit(), look: crate::flags::Look::default().bg(self.0) }]);
    }
    /// The hover colour is a paint operand (it never changes a shape).
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x0107);
        s.paint(self.0 .0);
    }
    /// A new hover colour: the table is resolved again (shown at once if the pointer is over the element).
    fn refresh(self, m: &mut Mounter<'_, V>, a: u32) {
        crate::flags::refresh_variants(m.rt, a, &[crate::flags::Variant { when: crate::flags::Flag::HOVERED.bit(), look: crate::flags::Look::default().bg(self.0) }]);
    }
}
pub struct Focusable;
impl<V> ActOp<V> for Focusable {
    const FLAGS: u8 = KF_FOCUS;
    fn install(self, _: &mut Mounter<'_, V>, _: u32) {}
    fn refresh(self, _: &mut Mounter<'_, V>, _: u32) {}
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x0104);
    }
}
pub struct OnKey<R, F>(pub F, pub std::marker::PhantomData<fn(&mut R)>);
impl<R, F> OnKey<R, F> {
    pub fn new(f: F) -> Self {
        OnKey(f, std::marker::PhantomData)
    }
}
impl<V: 'static, R: 'static, F: Fn(&mut R, &Ev) + Copy + 'static> ActOp<V> for OnKey<R, F> {
    const FLAGS: u8 = KF_FOCUS;
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        let own = receiver::<V, R>(m, "on_key");
        m.rt.input.acts[a as usize].key = Some(route_ev::<R, F>(self.0, own));
    }
    fn refresh(self, m: &mut Mounter<'_, V>, a: u32) {
        refresh_route(&mut m.rt.input.acts[a as usize].key, self.0);
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        seal_route(s, 5, self.0);
    }
}
pub struct Drag<R, F>(pub F, pub std::marker::PhantomData<fn(&mut R)>);
impl<R, F> Drag<R, F> {
    pub fn new(f: F) -> Self {
        Drag(f, std::marker::PhantomData)
    }
}
impl<V: 'static, R: 'static, F: Fn(&mut R, &Ev) + Copy + 'static> ActOp<V> for Drag<R, F> {
    fn install(self, m: &mut Mounter<'_, V>, a: u32) {
        let own = receiver::<V, R>(m, "on_drag");
        m.rt.input.acts[a as usize].drag = Some(route_ev::<R, F>(self.0, own));
    }
    fn refresh(self, m: &mut Mounter<'_, V>, a: u32) {
        refresh_route(&mut m.rt.input.acts[a as usize].drag, self.0);
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        seal_route(s, 6, self.0);
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
    /// The scroller stays (its glide, its content root): only the by-value step and limit follow.
    fn refresh(self, m: &mut Mounter<'_, V>, a: u32) {
        let sc = m.rt.input.acts[a as usize].scroll;
        let s = &mut m.rt.input.scrollers[sc as usize];
        assert!(s.off == self.sig.off, "a re-description cannot change which signal an element scrolls");
        s.line = self.line;
        s.max = self.max;
    }
    /// The scrolled field, the step and the limit (what `refresh` writes).
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x0108);
        s.word(self.sig.off);
        s.f32(self.line);
        s.f32(self.max);
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
        let p = &mut m.rt.inst[inst as usize];
        if bordered {
            p.color = c1.0;
            p.color2 = c2.0;
        } else {
            p.color = c2.0;
        }
        m.wire(&self.sig, patch_paint2::<T, F>, a, recipe(self.f));
    }
    /// The wire stays; this description's colours are applied (as the wire would).
    fn refresh(self, m: &mut Mounter<'_, V>, a: u32) {
        let (c1, c2) = (self.f)(self.sig.get());
        paint2(m.rt, a, c1, c2);
    }
    /// The wire's field and the mapping (its colours travel through the wire).
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x0109);
        s.word(self.sig.off);
        crate::seal::closure(s, &self.f);
    }
}

/// # Safety
/// Wire contract: `sig` is a `Signal<T>`; the recipe holds an `F`; `w.node` is the act.
unsafe fn patch_paint2<T: 'static, F: Fn(&T) -> (Color, Color) + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { (*(sig as *const Signal<T>)).peek() };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    let (c1, c2) = f(s);
    paint2(rt, w.node, c1, c2);
}

/// Write act `a`'s two colour words: its base look under variants (`flags.rs`), else the instance itself.
fn paint2(rt: &mut Rt, a: u32, c1: Color, c2: Color) {
    let act = rt.input.acts[a as usize];
    if act.vt != NONE {
        return rt.flags_rebase(a, Some(c2.0), Some(c1.0));
    }
    let p = rt.inst_mut(act.inst);
    let bordered = p.kind == KIND_BORDER;
    let old = pair(p.color, p.color2);
    if bordered {
        p.color = c1.0;
        p.color2 = c2.0;
    } else {
        p.color = c2.0;
    }
    let now = pair(p.color, p.color2);
    if now != old {
        rt.paint_write(act.handle, old, now);
    }
}

/// Interaction ops on any element (they apply to its first record). A handler's receiver is its `&mut`
/// argument's type: the owner that describes the element (the view, a list row, an entity) or an owner
/// enclosing it (a row's button calling the app: `.on_click(|app: &mut App| ..)`), resolved when the
/// element is mounted.
pub trait Interact: Sized {
    /// Activation (a primary press and release on the element, zgui's / the DOM's click).
    fn on_click<V, F: Fn(&mut V) + Copy + 'static>(self, f: F) -> On<Self, Click<V, F>> {
        On(self, Click(f, std::marker::PhantomData))
    }
    /// The press itself.
    fn on_press<V, F: Fn(&mut V, &Ev) + Copy + 'static>(self, f: F) -> On<Self, Press<V, F>> {
        On(self, Press(f, std::marker::PhantomData))
    }
    /// Pointer enters (`ev.on`) or leaves the element.
    fn on_hover<V, F: Fn(&mut V, &Ev) + Copy + 'static>(self, f: F) -> On<Self, Hover<V, F>> {
        On(self, Hover(f, std::marker::PhantomData))
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
    fn on_key<V, F: Fn(&mut V, &Ev) + Copy + 'static>(self, f: F) -> On<Self, OnKey<V, F>> {
        On(self, OnKey(f, std::marker::PhantomData))
    }
    /// Pointer travel while pressed (capture: no hit test until release). `ev.dx/dy` since the press.
    fn on_drag<V, F: Fn(&mut V, &Ev) + Copy + 'static>(self, f: F) -> On<Self, Drag<V, F>> {
        On(self, Drag(f, std::marker::PhantomData))
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
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        self.0.seal(s);
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
    /// The view whose content it is in (its rect follows that view), or NONE.
    view: u32,
}

impl Rt {
    /// The scope that mounted tooltip `k` is released: its fade stops (its instance and label are the
    /// scope's, about to be reused), it leaves its view, and its clip and slot return.
    pub(crate) fn release_tip(&mut self, k: u32) {
        let (clip, view) = {
            let t = &mut self.input.tips[k as usize];
            t.fading = false;
            (t.clip, std::mem::replace(&mut t.view, NONE))
        };
        if view != NONE {
            let tips = &mut self.input.views[view as usize].tips;
            if let Some(i) = tips.iter().position(|&x| x == k) {
                tips.swap_remove(i);
            }
        }
        self.free_clip(clip);
        self.input.tip_free.push(k);
    }
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
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x45);
    }
}
impl<T> Tooltip<T> {
    fn mount_in<V: 'static>(self, m: &mut Mounter<'_, V>)
    where
        T: Element<V>,
    {
        use crate::elem::{quad, text};
        let before = m.recs.len();
        self.trigger.mount(m);
        let a = act_for(m, before, 0);
        let s = m.rt.scale;
        let id = m.rt.own_clip(NRect::EMPTY);
        let [x, y, w, h] = self.rect;
        let o = m.rt.space(m.space);
        let r = NRect::new((o[0] + (m.ox + x) * s).floor() as i32, (o[1] + (m.oy + y) * s).floor() as i32, (o[0] + (m.ox + x + w) * s).ceil() as i32, (o[1] + (m.oy + y + h) * s).ceil() as i32);
        let prev = m.clip;
        m.clip = id;
        let st = self.style;
        // the bubble and its label, transparent until shown; both get handles (their paint is written)
        let b = m.recs.len();
        quad(x, y, w, h).bg(Color(with_alpha(st.bg.0, 0.0))).rounded(st.radius).border(1.0, Color(with_alpha(st.border.0, 0.0))).mount(m);
        // the element's own handle when it has one (a re-describing owner's quad is a node already)
        let bubble = if m.recs[b].handle != 0 { m.rt.bough.handle_gen(m.recs[b].handle) } else { m.rt.bough.new_handle() };
        m.recs[b].handle = bubble.idx;
        let inst = m.recs[b].payload;
        let t = m.recs.len();
        let lh = (st.size * 1.4).ceil();
        text(self.text).at(x + 8.0, y + (h - lh) * 0.5).size(st.size).line_height(lh).color(Color(with_alpha(st.fg.0, 0.0))).mount(m);
        let label = if m.recs[t].handle != 0 { m.rt.bough.handle_gen(m.recs[t].handle) } else { m.rt.bough.new_handle() };
        m.recs[t].handle = label.idx;
        let ti = *m.scope.texts.last().expect("tooltip label");
        m.rt.texts[ti as usize].handle = label;
        for rec in &mut m.recs[b..] {
            rec.kind |= KF_NOHIT;
        }
        m.clip = prev;
        let view = m.rt.input.mounting_view.unwrap_or(NONE);
        let tip = Tip { clip: id, rect: r, bubble, inst, label, ti, style: st, alpha: 0.0, fading: false, view };
        let k = match m.rt.input.tip_free.pop() {
            Some(k) => {
                m.rt.input.tips[k as usize] = tip;
                k
            }
            None => {
                m.rt.input.tips.push(tip);
                (m.rt.input.tips.len() - 1) as u32
            }
        };
        m.scope.owned.push(Owned::Tip(k));
        m.rt.input.acts[a as usize].tip = k;
        if view != NONE {
            m.rt.input.views[view as usize].tips.push(k);
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
    // the caret quad's state is its instance
    let (inst, h) = (rt.inst_of(cn), rt.handle(cn));
    let p = &mut rt.inst[inst as usize];
    let want = if focused { color } else { color & 0x00ff_ffff };
    let (moved, recolor) = (p.pos[0] != x, p.color != want);
    if !moved && !recolor {
        return;
    }
    let (old, (y, w, hh)) = (p.color, (p.pos[1], p.size[0], p.size[1]));
    p.color = want;
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
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x46);
    }
}
impl<V: 'static> TextField<'_, V> {
    fn mount_in(self, m: &mut Mounter<'_, V>) {
        use crate::elem::{quad, text};
        root_owner_only(m, "text_field");
        let [x, y, w, h] = self.rect;
        let before = m.recs.len();
        let s = m.rt.scale;
        // the box (a node with a handle: the caret colour and focus need it addressable)
        quad(x, y, w, h).bg(self.bg).rounded(self.radius).border(1.0, self.border).children(()).mount(m);
        let a = act_for(m, before, KF_FOCUS);
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
        m.clip = m.rt.scoped_clip(m.scope, r, clip);
        // the caret wire first: wires run newest first, so it runs after the label's patch
        let lh = (self.size * 1.4).ceil();
        let ty = ((h - 2.0) - lh) * 0.5;
        let caret_h = lh;
        let cb = m.recs.len();
        no_hit(quad(self.pad, ty, 1.0, caret_h).bg(sig_caret(self.sig, f, self.color))).mount(m);
        let cnode = m.recs[cb].handle;
        assert!(m.rt.node_pending(cnode), "caret node");
        m.rt.input.fields[f as usize].caret_node = cnode;
        text(self.sig).at(self.pad, ty).size(self.size).line_height(lh).color(self.color).mount(m);
        m.rt.input.fields[f as usize].ti = *m.scope.texts.last().expect("label text");
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

    /// Run route `r` on its receiver. The root is called directly; any other owner is reached along its
    /// path from the root (`Rt::owner_mut`), and a route whose receiver is gone (its row or entity removed,
    /// its slot reused by another item) does not run. Returns whether it ran.
    fn run_route(&mut self, r: Route, e: &Ev) -> bool {
        let root = self.owner_mut();
        let o = if r.own.is_root() {
            root
        } else {
            // SAFETY: `root` derives from the unique borrow of the app's owner (the type the runtime was mounted
            // for), and no other reference into it is live during dispatch.
            match unsafe { self.rt.owner_mut(root, r.own) } {
                Some(o) => o,
                None => return false,
            }
        };
        self.rt.input.stats.routes += 1;
        self.root_handled |= r.own.is_root();
        // the handler's receiver describes itself again if the handler changed what its description sees: the
        // root by its plain bytes and its subscriptions (`grain.rs`, `App::root_dirty`); a row through its
        // list's edit log; an entity's views with the owner that shows them (`Rt::owner_mut` marked those)
        // SAFETY: `o` is the live owner `r.own` names, whose type the route's receiver was matched against when
        // it was lowered (`receiver`), uniquely borrowed (derived from the root's unique borrow).
        unsafe { r.call(o, e) };
        true
    }

    /// The pressed act, if its element is still mounted. An element removed while pressed (a re-description
    /// dropped it, its list row left) ends its press and its drag capture: no route of it runs again.
    fn pressed_live(&mut self) -> u32 {
        let pa = self.rt.input.pressed;
        if pa != 0 && !cfg!(feature = "mutant_route_owner") && self.rt.bough.pos_of(self.rt.input.acts[pa as usize].handle).is_none() {
            self.rt.input.pressed = 0;
            self.rt.input.dragging = false;
            return 0;
        }
        pa
    }

    /// Settle what routes queued and lay out what changed (the end of an input turn). Hover is a function
    /// of the pointer and of the tree: what the turn moved under a resting pointer (a scroll, a list edit, a
    /// re-description) is entered and left now, not at the next pointer event.
    fn settle_input(&mut self) {
        self.settle_pass();
        if self.rehover() {
            self.settle_pass();
        }
        self.blur_disabled();
        self.sync_ring();
        self.rt.direct_arm();
    }

    /// A focused record that became disabled (its own flag, or a subtree's) loses focus, as a disabled
    /// control does.
    pub(crate) fn blur_disabled(&mut self) {
        if let Some(h) = self.rt.input.focus {
            if self.rt.flags_of(h) & crate::flags::Flag::DISABLED.bit() != 0 {
                self.set_focus(None, false);
            }
        }
    }

    /// Settle, run the `changed` waves, re-describe the owners a handler ran on, lay out.
    fn settle_pass(&mut self) {
        let owner = &mut *self.owner as *mut V as *const u8;
        // SAFETY: the owner is the mounted view (uniquely borrowed: list patches write their cell); every
        // wire was installed from a `sig!` proof for it.
        unsafe { self.rt.settle(owner) };
        self.effects();
        // Only owners whose described state a handler (a route, a `changed`) changed describe again: the root
        // if its plain bytes or a cell its description read changed, the rows the turn edited. A turn of wires
        // alone (a hover colour, a wheel, a glide's tick, a keystroke in a field) re-describes nothing.
        self.redescribe_dirty();
        self.rt.lay_flush();
        self.rt.flags_flush();
    }

    /// Bring the hover chain up to date with the tree under a resting pointer (no pointer, or a drag's
    /// capture: nothing). Returns whether a hover route ran (the caller settles again).
    pub(crate) fn rehover(&mut self) -> bool {
        let Some((px, py)) = self.rt.input.pointer else { return false };
        if self.rt.input.pressed != 0 && self.rt.input.dragging {
            return false;
        }
        let inp = &self.rt.input;
        if inp.cell_ok && inp.cell.contains_point(px, py) && self.rt.bough.cell_valid(inp.cell, inp.cell_epoch) {
            return false;
        }
        let s = self.rt.scale;
        self.update_hover(px, py, px as f32 / s, py as f32 / s)
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
        let pa = self.pressed_live();
        if pa != 0 && self.rt.input.dragging {
            if let Some(r) = self.rt.input.acts[pa as usize].drag {
                let (x0, y0) = self.rt.input.press_at;
                let e = Ev { dx: x - x0, dy: y - y0, ..Ev::at(x, y) };
                self.run_route(r, &e);
                self.settle_input();
                self.set_drag_over(Some((px, py)));
                return;
            }
        }
        if self.update_hover(px, py, x, y) {
            self.settle_input();
        }
    }

    /// Recompute the hover chain at (px, py) unless the hit cell still answers. Returns whether a hover
    /// route ran (hover colours and tooltips are paint writes: nothing to settle, nothing to re-describe).
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

    /// Hover on/off for one record: its hover background (a paint write), tooltip clip and route. Returns
    /// whether a route ran.
    fn hover_set(&mut self, h: Handle, on: bool, x: f32, y: f32) -> bool {
        let a = self.rt.input.act_of(h);
        if a == 0 {
            return false;
        }
        if self.rt.bough.pos_of(h).is_none() {
            // the element was removed while hovered (a re-description dropped it, its row left): what its act
            // names (an instance, a tooltip, a route's captures) is no longer its own, so nothing is written or run
            let act = &mut self.rt.input.acts[a as usize];
            let hb = crate::flags::Flag::HOVERED.bit();
            (act.own, act.eff) = (act.own & !hb, act.eff & !(hb & !act.inh));
            return false;
        }
        // the hover colour and every other `HOVERED` variant: a flag write (a paint write, `flags.rs`)
        self.rt.flags_write(a, crate::flags::Flag::HOVERED.bit(), on, false);
        let act = self.rt.input.acts[a as usize];
        let mut ran = false;
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
        }
        if let Some(r) = act.hover.filter(|_| act.eff & crate::flags::Flag::DISABLED.bit() == 0) {
            let e = Ev { on, ..Ev::at(x, y) };
            self.run_route(r, &e);
            ran = true;
        }
        ran
    }

    /// `PRESSED` on the records of `chain` and on no other (the press chain: the pressed record and its
    /// interactive ancestors, as CSS's `:active`).
    fn set_pressed(&mut self, chain: &[Handle]) {
        let old = std::mem::take(&mut self.rt.input.pressed_chain);
        let b = crate::flags::Flag::PRESSED.bit();
        for h in &old {
            if !chain.contains(h) {
                let a = self.rt.input.act_of(*h);
                if a != 0 {
                    self.rt.flags_write(a, b, false, false);
                }
            }
        }
        for h in chain {
            if !old.contains(h) {
                let a = self.rt.input.act_of(*h);
                if a != 0 {
                    self.rt.flags_write(a, b, true, false);
                }
            }
        }
        let mut old = old;
        old.clear();
        old.extend_from_slice(chain);
        self.rt.input.pressed_chain = old;
    }

    /// `DRAG_OVER` on the interactive records under the pointer while a drag holds it (a drop target's
    /// variant), cleared when the drag ends (`None`).
    fn set_drag_over(&mut self, at: Option<(i32, i32)>) {
        let mut chain: Vec<Handle> = Vec::new();
        if let Some((px, py)) = at {
            let b = &self.rt.bough;
            if let Some(t) = b.hit_top(px, py) {
                let mut c = std::mem::take(&mut self.rt.input.chain);
                b.flagged_chain(t, KF_INTERACT, &mut c);
                chain.extend(c.iter().filter_map(|&p| b.handle_at(p)));
                self.rt.input.chain = c;
            }
        }
        let old = std::mem::take(&mut self.rt.input.drag_over);
        let bit = crate::flags::Flag::DRAG_OVER.bit();
        for h in &old {
            if !chain.contains(h) {
                let a = self.rt.input.act_of(*h);
                if a != 0 {
                    self.rt.flags_write(a, bit, false, false);
                }
            }
        }
        for h in &chain {
            if !old.contains(h) {
                let a = self.rt.input.act_of(*h);
                if a != 0 {
                    self.rt.flags_write(a, bit, true, false);
                }
            }
        }
        self.rt.input.drag_over = chain;
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
        // nearest focusable in the chain (a disabled one takes no focus)
        let mut focus = None;
        for &h in &chain {
            if let Some(p) = self.rt.bough.pos_of(h) {
                if self.rt.bough.kind_at(p) & KF_FOCUS != 0 && self.rt.flags_of(h) & crate::flags::Flag::DISABLED.bit() == 0 {
                    focus = Some(h);
                    break;
                }
            }
        }
        self.set_pressed(&chain);
        if button == Button::Primary {
            self.set_focus(focus, false);
        }
        let target = chain.first().map_or(0, |&h| self.rt.input.act_of(h));
        self.rt.input.pressed = target;
        self.rt.input.press_at = (x, y);
        self.rt.input.dragging = false;
        // the press route and drag capture: nearest act in the chain that has one (a disabled record runs none
        // and stops the search, as a disabled control takes no pointer events)
        for &h in &chain {
            let a = self.rt.input.act_of(h);
            let act = self.rt.input.acts[a as usize];
            if act.eff & crate::flags::Flag::DISABLED.bit() != 0 {
                break;
            }
            if let Some(r) = act.press {
                let e = Ev { button, ..Ev::at(x, y) };
                self.run_route(r, &e);
                break;
            }
        }
        let live = chain.iter().map(|&h| self.rt.input.act_of(h)).take_while(|&a| self.rt.input.acts[a as usize].eff & crate::flags::Flag::DISABLED.bit() == 0);
        if let Some(a) = live.into_iter().find(|&a| self.rt.input.acts[a as usize].drag.is_some()) {
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
        let pa = self.pressed_live();
        self.rt.input.pressed = 0;
        let dragging = std::mem::replace(&mut self.rt.input.dragging, false);
        let (px, py) = self.dev(x, y);
        self.rt.input.pointer = Some((px, py));
        self.set_pressed(&[]);
        if dragging {
            self.set_drag_over(None);
            if let Some(r) = self.rt.input.acts[pa as usize].drag {
                let (x0, y0) = self.rt.input.press_at;
                let e = Ev { dx: x - x0, dy: y - y0, on: false, ..Ev::at(x, y) };
                self.run_route(r, &e);
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
                    if self.rt.input.acts[a as usize].eff & crate::flags::Flag::DISABLED.bit() != 0 {
                        break;
                    }
                    if let Some(r) = self.rt.input.acts[a as usize].click {
                        self.run_route(r, &Ev::at(x, y));
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
        let mut ran = false;
        for h in old {
            ran |= self.hover_set(h, false, -1.0, -1.0);
        }
        if ran {
            self.settle_input();
        }
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
            let at = *unsafe { (*(o.add(s.off as usize) as *const Signal<f32>)).peek() };
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
            // the next tab stop that is not disabled
            let mut next = self.rt.bough.next_flagged(cur, KF_FOCUS, mods.shift);
            let first = next;
            while let Some(p) = next {
                if self.rt.bough.handle_at(p).is_none_or(|h| self.rt.flags_of(h) & crate::flags::Flag::DISABLED.bit() == 0) {
                    break;
                }
                next = self.rt.bough.next_flagged(Some(p), KF_FOCUS, mods.shift).filter(|&q| Some(q) != first);
            }
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
                    if self.rt.input.acts[a as usize].eff & crate::flags::Flag::DISABLED.bit() != 0 {
                        break;
                    }
                    if let Some(r) = self.rt.input.acts[a as usize].key {
                        let e = Ev { key: Some(key), mods, ..Ev::at(0.0, 0.0) };
                        self.run_route(r, &e);
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
        self.focus_flags(false);
        self.rt.input.focus = h;
        self.rt.input.focus_visible = visible;
        self.focus_flags(true);
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

    /// `FOCUSED` on the focused record and `FOCUS_WITHIN` on its ancestors that have acts (`on`), or off them
    /// again (the ancestors are those it had when focus came: kept, so a removed record's are found).
    fn focus_flags(&mut self, on: bool) {
        let Some(h) = self.rt.input.focus else { return };
        let a = self.rt.input.act_of(h);
        if a != 0 {
            self.rt.flags_write(a, crate::flags::Flag::FOCUSED.bit(), on, false);
        }
        let fw = crate::flags::Flag::FOCUS_WITHIN.bit();
        if on {
            let mut anc = std::mem::take(&mut self.rt.input.flag_scratch);
            anc.clear();
            if let Some(p) = self.rt.bough.pos_of(h) {
                self.rt.bough.ancestors(p, &mut anc);
            }
            let mut within = std::mem::take(&mut self.rt.input.focus_within);
            within.clear();
            for &p in &anc {
                if let Some(x) = self.rt.bough.handle_at(p) {
                    let b = self.rt.input.act_of(x);
                    if b != 0 {
                        within.push(x);
                        self.rt.flags_write(b, fw, true, false);
                    }
                }
            }
            self.rt.input.focus_within = within;
            self.rt.input.flag_scratch = anc;
        } else {
            for x in std::mem::take(&mut self.rt.input.focus_within) {
                let b = self.rt.input.act_of(x);
                if b != 0 {
                    self.rt.flags_write(b, fw, false, false);
                }
            }
        }
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
                let inst = self.rt.alloc_inst();
                let s = self.rt.scale;
                *self.rt.inst_mut(inst) = bough_metal::instance::Instance {
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
