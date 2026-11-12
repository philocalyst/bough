//! Parts: stable, typed addresses inside a component, and overrides applied through them
//! (`docs/lanes/composition/DESIGN.md` §2).
//!
//! A component's author names the places a consumer may reach with a **part key**, a `const` that carries the
//! style type of the element it marks:
//!
//! ```
//! use bough::prelude::*;
//! pub const DAY: Part<BoxStyle> = Part::new("date-picker.day");
//! pub const DAY_LABEL: Part<TextStyle> = Part::new("date-picker.day-label");
//! ```
//!
//! and marks the element: `cell.part_at(DAY, day)` (`.part(KEY)` when there is one of it). That is the whole
//! opt-in: no prop is exposed, no component macro, no context object. A consumer wraps *any* element that
//! contains such parts, however deeply:
//!
//! ```ignore
//! date_picker(sig!(self.date))
//!     .restyle(DAY, |s| if s.index() % 7 == 0 { s.bg(SUNDAY) } else { s }.rounded(14.0))
//!     .intercept(DAY, |me: &mut Booking, e| if me.closed(e.index) { Flow::Stop } else { Flow::Continue })
//! ```
//!
//! **How it resolves.** `restyle` / `intercept` / `after` wrap the element in a [`With`], whose mount (and
//! re-description) pushes one frame on a chain threaded through the `Mounter` for the duration of the inner
//! mount: a stack discipline, the frames live on the Rust stack, nothing is allocated and nothing is ambient.
//! A part site walks the chain, innermost frame first, so the outermost wrapper (the application) has the
//! last word. Style frames rewrite the *description* (the builder's own style fields, before it lowers), so a
//! re-description sees the same patched description and patches nothing: an override is not a second write
//! that fights the first. Route frames attach guards to the part's act: a `before` guard may stop the
//! component's own route, an `after` guard runs once it has run.
//!
//! **Cost when unused.** A part site with no wrapper around it tests one null pointer. An element that is not
//! a part site pays nothing, at mount, at re-description or at dispatch (one `is_empty` per dispatched click).
//!
//! **Misuse is a type error.** The key's type parameter is the style the closure receives, so a text
//! property on a box part does not compile:
//!
//! ```compile_fail
//! use bough::prelude::*;
//! const DAY: Part<BoxStyle> = Part::new("day");
//! struct V;
//! impl View for V {
//!     fn view(&self) -> impl Element<Self> + '_ {
//!         div().part(DAY).restyle(DAY, |s| s.font(12.0, 16.0)) // BoxStyle has no font
//!     }
//! }
//! ```
//!
//! and a key cannot mark an element of another kind:
//!
//! ```compile_fail
//! use bough::prelude::*;
//! const DAY: Part<BoxStyle> = Part::new("day");
//! struct V;
//! impl View for V {
//!     fn view(&self) -> impl Element<Self> + '_ {
//!         text("7").part(DAY) // a text is restyled through TextStyle
//!     }
//! }
//! ```
//!
//! A guard is a route: it captures at most 16 bytes, and it names the owner it runs on.
//!
//! ```compile_fail
//! use bough::prelude::*;
//! const DAY: Part<BoxStyle> = Part::new("day");
//! struct V;
//! impl View for V {
//!     fn view(&self) -> impl Element<Self> + '_ {
//!         let big = [0u64; 4];
//!         div().part(DAY).intercept(DAY, move |_: &mut V, _: &PartEv| if big[0] == 0 { Flow::Stop } else { Flow::Continue })
//!     }
//! }
//! let _ = V.view(); // (the check is at monomorphisation: the view must be used)
//! ```
//!
//! The same wrapper with the properties and types in agreement compiles:
//!
//! ```
//! use bough::prelude::*;
//! const DAY: Part<BoxStyle> = Part::new("day");
//! const LABEL: Part<TextStyle> = Part::new("label");
//! struct V;
//! impl View for V {
//!     fn view(&self) -> impl Element<Self> + '_ {
//!         div()
//!             .child(text("7").part(LABEL))
//!             .part(DAY)
//!             .restyle(DAY, |s| s.bg(rgb(0xff0000)).rounded(4.0).p(2.0))
//!             .restyle(LABEL, |t| t.color(rgb(0xffffff)))
//!             .intercept(DAY, |_: &mut V, e: &PartEv| if e.index == 0 { Flow::Continue } else { Flow::Stop })
//!     }
//! }
//! let _ = V.view();
//! ```
use crate::color::Color;
use crate::elem::{Apply, Element, IntoProp, Mounter};
use crate::input::{Ev, Guard, GuardFn};
use crate::layout::{Spec, Styled};
use std::any::TypeId;
use std::marker::PhantomData;
use std::panic::Location;

// =============================================================================== keys and styles

/// A part key: a name (unique by convention: `"crate.component.part"`) and the style type `S` of the element
/// it marks. `Copy`, `const`-constructible.
pub struct Part<S> {
    name: &'static str,
    _s: PhantomData<fn(S) -> S>,
}
impl<S> Part<S> {
    pub const fn new(name: &'static str) -> Part<S> {
        Part { name, _s: PhantomData }
    }
    pub const fn name(&self) -> &'static str {
        self.name
    }
}
impl<S> Clone for Part<S> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<S> Copy for Part<S> {}

/// The overridable style of a box (`div`, `row`, `column`, `button`): its layout spec (every [`Styled`]
/// method) and its paint.
#[derive(Clone, Copy, Debug)]
pub struct BoxStyle {
    pub(crate) spec: Spec,
    pub(crate) bg: Option<Color>,
    pub(crate) radius: f32,
    pub(crate) border: Option<(f32, Color)>,
    pub(crate) draw: bool,
    pub(crate) index: u32,
}
impl BoxStyle {
    /// A constant background (replaces the author's operand, wired or not).
    pub fn bg(mut self, c: Color) -> Self {
        self.bg = Some(c);
        self.draw = true;
        self
    }
    pub fn rounded(mut self, r: f32) -> Self {
        self.radius = r;
        self.draw = true;
        self
    }
    pub fn border(mut self, w: f32, c: Color) -> Self {
        self.border = Some((w, c));
        self.draw = true;
        self
    }
    /// The part's index (`part_at`'s second argument; 0 for `part`).
    pub fn index(&self) -> u32 {
        self.index
    }
}
impl Styled for BoxStyle {
    fn spec_mut(&mut self) -> &mut Spec {
        &mut self.spec
    }
}

/// The overridable style of a text (`text`, `para`, `label`).
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub(crate) spec: Spec,
    pub(crate) color: Option<Color>,
    pub(crate) size: f32,
    pub(crate) lh: f32,
    pub(crate) index: u32,
}
impl TextStyle {
    /// A constant colour (replaces the author's operand).
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }
    pub fn font(mut self, size_pt: f32, line_height_pt: f32) -> Self {
        self.size = size_pt;
        self.lh = line_height_pt;
        self
    }
    pub fn index(&self) -> u32 {
        self.index
    }
}
impl Styled for TextStyle {
    fn spec_mut(&mut self) -> &mut Spec {
        &mut self.spec
    }
}

/// A colour operand with an optional constant override in front of it (what a restyled builder holds).
pub struct Ov<P>(pub(crate) Option<Color>, pub(crate) P);
impl<V, P: IntoProp<Color, V>> IntoProp<Color, V> for Ov<P> {
    #[inline]
    fn lower<K: Apply<P = Color>>(self, m: &mut Mounter<'_, V>, node: u32) -> Color {
        match self.0 {
            Some(c) => c,
            None => self.1.lower::<K>(m, node),
        }
    }
    #[inline]
    fn value(self) -> Color {
        match self.0 {
            Some(c) => c,
            None => self.1.value(),
        }
    }
}

/// A builder whose style can be rewritten before it lowers. Implemented by the builders (`LBox`, `Para`) and
/// passed through by wrappers that keep their element's style reachable (`On`).
pub trait Restyle: Sized {
    type Style: Copy + 'static;
    /// The builder with its style replaced (same shape, same records).
    type Out;
    /// Rewrite the style: `f` receives the builder's current style with `index` set.
    fn restyled(self, index: u32, f: &dyn Fn(Self::Style) -> Self::Style) -> Self::Out;
}

impl<E: Restyle, O> Restyle for crate::input::On<E, O> {
    type Style = E::Style;
    type Out = crate::input::On<E::Out, O>;
    fn restyled(self, index: u32, f: &dyn Fn(E::Style) -> E::Style) -> Self::Out {
        crate::input::On(self.0.restyled(index, f), self.1)
    }
}

// =============================================================================== the override chain

/// What a guard receives: the event and the part's index.
#[derive(Clone, Copy, Debug)]
pub struct PartEv {
    pub ev: Ev,
    pub index: u32,
}

/// A `before` guard's verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Run the component's route.
    Continue,
    /// Do not run it (the guard replaced it).
    Stop,
}

#[derive(Clone, Copy)]
enum FrameKind {
    /// A style rewrite: the closure (borrowed from the `With` being mounted) and its monomorphic applier.
    Style { f: *const (), apply: unsafe fn(*const (), *mut ()) },
    Guard { f: GuardFn, r: [u8; 16], after: bool },
}

/// One wrapper's override, on the Rust stack while its element mounts or re-describes.
pub struct OvFrame {
    parent: *const OvFrame,
    name: &'static str,
    /// `TypeId` of the key's style type: the type the `Style` closure takes.
    ty: TypeId,
    kind: FrameKind,
}

/// An override a [`With`] carries.
pub trait Override<V> {
    /// A guard runs on the root owner (as routes do).
    const ROUTE: bool;
    fn frame(&self, parent: *const OvFrame) -> OvFrame;
}

/// `restyle`'s override.
pub struct StyleOv<S, F> {
    key: Part<S>,
    f: F,
}
/// # Safety
/// `f` points at an `F`, `s` at an `S`.
unsafe fn apply_style<S: Copy, F: Fn(S) -> S>(f: *const (), s: *mut ()) {
    // SAFETY: see the function contract.
    unsafe {
        let s = s as *mut S;
        *s = (*(f as *const F))(*s);
    }
}
impl<V, S: Copy + 'static, F: Fn(S) -> S> Override<V> for StyleOv<S, F> {
    const ROUTE: bool = false;
    fn frame(&self, parent: *const OvFrame) -> OvFrame {
        OvFrame { parent, name: self.key.name, ty: TypeId::of::<S>(), kind: FrameKind::Style { f: &self.f as *const F as *const (), apply: apply_style::<S, F> } }
    }
}

/// `intercept` / `after`'s override.
pub struct GuardOv<S, F> {
    key: Part<S>,
    f: F,
    after: bool,
}
/// # Safety
/// `o` is the owner `V`; `r` holds an `F`.
unsafe fn call_guard<V, F: Fn(&mut V, &PartEv) -> Flow + Copy>(o: *mut u8, r: &[u8; 16], e: &PartEv) -> Flow {
    // SAFETY: see the function contract.
    let f: F = unsafe { std::ptr::read_unaligned(r.as_ptr() as *const F) };
    f(unsafe { &mut *(o as *mut V) }, e)
}
/// # Safety
/// As `call_guard`.
unsafe fn call_after<V, F: Fn(&mut V, &PartEv) + Copy>(o: *mut u8, r: &[u8; 16], e: &PartEv) -> Flow {
    // SAFETY: see the function contract.
    let f: F = unsafe { std::ptr::read_unaligned(r.as_ptr() as *const F) };
    f(unsafe { &mut *(o as *mut V) }, e);
    Flow::Continue
}
impl<V: 'static, S: 'static, F: Fn(&mut V, &PartEv) -> Flow + Copy + 'static> Override<V> for GuardOv<S, F> {
    const ROUTE: bool = true;
    fn frame(&self, parent: *const OvFrame) -> OvFrame {
        OvFrame { parent, name: self.key.name, ty: TypeId::of::<S>(), kind: FrameKind::Guard { f: call_guard::<V, F>, r: crate::input::recipe(self.f), after: self.after } }
    }
}
/// `after`'s closure returns nothing.
pub struct AfterOv<S, F> {
    key: Part<S>,
    f: F,
}
impl<V: 'static, S: 'static, F: Fn(&mut V, &PartEv) + Copy + 'static> Override<V> for AfterOv<S, F> {
    const ROUTE: bool = true;
    fn frame(&self, parent: *const OvFrame) -> OvFrame {
        OvFrame { parent, name: self.key.name, ty: TypeId::of::<S>(), kind: FrameKind::Guard { f: call_after::<V, F>, r: crate::input::recipe(self.f), after: true } }
    }
}

/// An element with an override in force over everything it contains.
pub struct With<E, O>(E, O);

impl<V, E: Element<V>, O: Override<V>> Element<V> for With<E, O> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        assert!(!O::ROUTE || m.cls == 0, "intercept/after inside a list row or an entity view: guards run on the app's root owner");
        let With(e, o) = self;
        let fr = o.frame(m.ov);
        let saved = std::mem::replace(&mut m.ov, &fr);
        e.mount(m);
        m.ov = saved;
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        let With(e, o) = self;
        let fr = o.frame(m.ov);
        let saved = std::mem::replace(&mut m.ov, &fr);
        e.update(m);
        m.ov = saved;
    }
    fn dyn_key(&self) -> u64 {
        self.0.dyn_key()
    }
}
impl<E: Styled, O> Styled for With<E, O> {
    fn spec_mut(&mut self) -> &mut Spec {
        self.0.spec_mut()
    }
}

/// Apply every style frame of the chain for (`name`, `S`) to `s`, innermost first.
fn apply_chain<S: Copy + 'static>(mut fr: *const OvFrame, name: &'static str, mut s: S) -> S {
    let ty = TypeId::of::<S>();
    while !fr.is_null() {
        // SAFETY: a frame on the chain is a local of a `With::mount`/`update` still on the stack (it is linked
        // for exactly the duration of the inner mount, and sub-mounters copy the pointer only for that time).
        let f = unsafe { &*fr };
        if let FrameKind::Style { f: cl, apply } = f.kind {
            if f.ty == ty && (std::ptr::eq(f.name, name) || f.name == name) {
                // SAFETY: the frame was made by `StyleOv<S, F>::frame` for this `S` (same `TypeId`); `cl`
                // borrows the closure of the `With` being mounted.
                unsafe { apply(cl, &mut s as *mut S as *mut ()) };
            }
        }
        fr = f.parent;
    }
    s
}

// =============================================================================== the part site

/// An element marked as a part.
pub struct Parted<E, S> {
    e: E,
    key: Part<S>,
    index: u32,
    at: &'static Location<'static>,
}

impl<E: Styled, S> Styled for Parted<E, S> {
    fn spec_mut(&mut self) -> &mut Spec {
        self.e.spec_mut()
    }
}

/// A part passes its element's style through, so a part of a part (a component wrapping a component and
/// re-exporting its part under its own key) composes.
impl<E: Restyle<Style = S>, S: Copy + 'static> Restyle for Parted<E, S> {
    type Style = S;
    type Out = Parted<E::Out, S>;
    fn restyled(self, index: u32, f: &dyn Fn(S) -> S) -> Self::Out {
        Parted { e: self.e.restyled(index, f), key: self.key, index: self.index, at: self.at }
    }
}

impl<V: 'static, S: Copy + 'static, E> Element<V> for Parted<E, S>
where
    E: Restyle<Style = S>,
    E::Out: Element<V>,
{
    fn mount(self, m: &mut Mounter<'_, V>) {
        let (ov, name, index) = (m.ov, self.key.name, self.index);
        let before = m.recs.len();
        // one instantiation (the restyled builder) whether or not a wrapper is in force
        self.e.restyled(index, &|s| if ov.is_null() { s } else { apply_chain(ov, name, s) }).mount(m);
        let Some(rec) = m.recs.get(before).copied() else { return };
        if rec.handle == 0 {
            return;
        }
        let hd = m.rt.bough.handle_gen(rec.handle);
        m.rt.reflect.add_part(hd, name, index, self.at);
        // guards: on the part's act (its routes), innermost wrapper first, so dispatch runs the outermost first
        let mut fr = ov;
        while !fr.is_null() {
            // SAFETY: as in `apply_chain`.
            let f = unsafe { &*fr };
            if let FrameKind::Guard { f: g, r, after } = f.kind {
                if f.ty == TypeId::of::<S>() && (std::ptr::eq(f.name, name) || f.name == name) {
                    let a = m.rt.input.act_of_handle(hd);
                    assert!(a != 0, "intercept on part `{name}`: the part has no route (put `.part(..)` after `.on_click(..)`)");
                    m.rt.input.guards.push(Guard { act: a, f: g, r, after, index });
                }
            }
            fr = f.parent;
        }
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        let (ov, name, index) = (m.ov, self.key.name, self.index);
        self.e.restyled(index, &|s| if ov.is_null() { s } else { apply_chain(ov, name, s) }).update(m);
    }
}

// =============================================================================== the verbs

/// Marking parts (the author's side) and overriding them (the consumer's side), on any element.
pub trait Compose: Sized {
    /// This element is the part `key`.
    #[track_caller]
    fn part<S>(self, key: Part<S>) -> Parted<Self, S>
    where
        Self: Restyle<Style = S>,
    {
        Parted { e: self, key, index: 0, at: Location::caller() }
    }
    /// This element is the `index`-th part `key` (a day of a month, a tab, a row): styles and guards
    /// receive the index.
    #[track_caller]
    fn part_at<S>(self, key: Part<S>, index: u32) -> Parted<Self, S>
    where
        Self: Restyle<Style = S>,
    {
        Parted { e: self, key, index, at: Location::caller() }
    }
    /// Rewrite the style of every part `key` inside this element.
    fn restyle<S: Copy + 'static, F: Fn(S) -> S>(self, key: Part<S>, f: F) -> With<Self, StyleOv<S, F>> {
        With(self, StyleOv { key, f })
    }
    /// Run `f` on the owner before the click route of every part `key` inside this element; `Flow::Stop`
    /// keeps the component's route from running.
    fn intercept<V, S, F: Fn(&mut V, &PartEv) -> Flow + Copy + 'static>(self, key: Part<S>, f: F) -> With<Self, GuardOv<S, F>> {
        const { assert!(size_of::<F>() <= 16, "guard closure: at most 16 bytes of captures") };
        With(self, GuardOv { key, f, after: false })
    }
    /// Run `f` on the owner after the click route of every part `key` inside this element has run.
    fn after<V, S, F: Fn(&mut V, &PartEv) + Copy + 'static>(self, key: Part<S>, f: F) -> With<Self, AfterOv<S, F>> {
        const { assert!(size_of::<F>() <= 16, "guard closure: at most 16 bytes of captures") };
        With(self, AfterOv { key, f })
    }
}
impl<E> Compose for E {}

// =============================================================================== lenses

/// A field of an owner `V`, proven by `sig!`, as a `Copy` value a route can capture (4 bytes): what lets a
/// component from another crate, generic over its consumer's owner type, write the consumer's signal.
pub struct Lens<T, V> {
    off: u32,
    _p: PhantomData<fn(&mut V) -> &mut T>,
}
impl<T, V> Clone for Lens<T, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, V> Copy for Lens<T, V> {}
impl<T, V> Lens<T, V> {
    /// The signal in `v`.
    #[inline]
    pub fn of<'a>(&self, v: &'a mut V) -> &'a mut crate::signal::Signal<T> {
        // SAFETY: `off` is the `offset_of!`-proven offset of a `Signal<T>` struct field of `V` (a `Lens` is made
        // only from a `Wired<'_, T, V>`), reached from a unique borrow of the owner.
        unsafe { &mut *((v as *mut V as *mut u8).add(self.off as usize) as *mut crate::signal::Signal<T>) }
    }
}
impl<'a, T, V> crate::signal::Wired<'a, T, V> {
    /// This field as a lens (see [`Lens`]).
    pub fn lens(&self) -> Lens<T, V> {
        Lens { off: self.off, _p: PhantomData }
    }
}

// =============================================================================== reflection

/// A part as reflection shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct PartView {
    pub name: &'static str,
    pub index: u32,
    /// The record handle of the part's first record (what `reflect_style`, `reflect_set_color` take).
    pub handle: u32,
    /// Position in the paint preorder.
    pub pos: usize,
    /// `file:line:column` of the `.part(..)` call.
    pub source: String,
}

impl<V: crate::app::View> crate::app::App<V> {
    /// Every live part, in paint order.
    pub fn reflect_parts(&self) -> Vec<PartView> {
        let mut v: Vec<PartView> = self
            .rt
            .reflect
            .parts
            .iter()
            .filter_map(|(&idx, p)| {
                let hd = self.rt.bough.handle_gen(idx);
                if hd.generation != p.generation {
                    return None; // the handle was reused by another record
                }
                let pos = self.rt.bough.pos_of(hd)?;
                Some(PartView { name: p.name, index: p.index, handle: idx, pos, source: p.at.to_string() })
            })
            .collect();
        v.sort_by_key(|p| p.pos);
        v
    }
}
