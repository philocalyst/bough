//! Events: the dataflow run backwards.
//!
//! Forward, state words and relation rows become records and records become pixels. Backward, a pixel becomes a
//! record and a record names its handlers:
//!
//! * **The pick.** The raster writes the *place* of the topmost record that has a pointer handler and inks the
//!   pixel under the pointer into the block's `Pick` word: `column << 20 | record`, the column numbered in paint
//!   order across the world ([`World::place`](crate::world::World)). Records with no pointer handler are
//!   transparent to the pointer. On a CPU-only frame the world computes the same word ([`Store::chain`]).
//! * **The handlers.** A record's handlers are a type (`On<On<Fill<..>, Click<F>>, Hover<G>>`), so the column
//!   created for it at discovery holds one `&'static` [`Ons`], constant per record-and-handler type: the event
//!   kinds it hears, its hit kernel and one monomorphised trampoline that runs the right handler for an event.
//!   An event costs one indirect call. Nothing is registered, boxed, or walked per record.
//! * **The captures.** A handler may capture (`move |s: &mut Chat| s.select(i)`): its captures are an operand
//!   of the record, kept in a CPU lane beside the column (never in the pages the GPU reads) and written with the
//!   row, so a list's ring slot always holds the captures of the row it shows.
//!
//! Authors write the façade's calls on any record ([`Interact`]): `.on_click(V::method)`, `.on_hover(|v, e| ..)`,
//! `.on_press`, `.on_key`, `.on_drag`, `.on_text`, `.on_wheel`, `.focusable()`. A wheel over a space whose origin
//! reads a scroll word writes that word: the space *is* its scroller, so a scroll box needs no handler.
//!
//! Core words (block words 56..59, written by the world, read by any expression): [`U_FOCUS`] (the place of the
//! focused record while its row is exposed, else [`NONE`]), [`U_PRESS`] (the place under a held primary button),
//! [`U_ENV`] (the platform's settings: [`ENV_REDUCED_MOTION`] and the rest), [`U_PRESS_AT`] (where the press
//! began, `x:16 | y:16`). With the raster's `PtrX` / `PtrY`, a drag preview is a Frame expression and needs no
//! turn while it moves.
use crate::expr::U;
use crate::sink::{Cx, Leaf, Mk, Row};
use crate::store::{Arch, HitFn, Site, hit_fn};
use core::any::TypeId;
use core::marker::PhantomData;

pub use crate::expr::{U_ENV, U_FOCUS, U_PRESS, U_PRESS_AT};
/// No place.
pub const NONE: u32 = u32::MAX;
/// The record bits of a place (the raster's order word).
pub const PLACE_REC_BITS: u32 = 20;

/// Bits of the `Env` word, written by the platform.
pub const ENV_REDUCED_MOTION: u32 = 1;
pub const ENV_INCREASE_CONTRAST: u32 = 2;
pub const ENV_REDUCE_TRANSPARENCY: u32 = 4;
pub const ENV_DARK: u32 = 8;
pub const ENV_DIFFERENTIATE_WITHOUT_COLOR: u32 = 16;
/// Reduced motion shows every one-shot transition at its end: the time the raster shades with runs this far
/// ahead of the clock (loops read the `Env` bit instead: they have no end).
pub const REDUCED_MOTION_AHEAD: u32 = 1 << 30;

/// Event kinds, one bit each; a handler type says which it hears.
pub mod kind {
    pub const HOVER: u32 = 1;
    pub const CLICK: u32 = 2;
    pub const PRESS: u32 = 4;
    pub const DRAG: u32 = 8;
    pub const WHEEL: u32 = 16;
    pub const KEY: u32 = 32;
    pub const TEXT: u32 = 64;
    pub const FOCUS: u32 = 128;
    /// A tab stop (no handler of its own needed).
    pub const FOCUSABLE: u32 = 256;
    /// The kinds that make a column answer the pick.
    pub const POINTER: u32 = HOVER | CLICK | PRESS | DRAG | WHEEL;
}

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

/// What a handler receives: the façade's `Ev`, field for field. Positions in device pixels, window coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Ev {
    pub x: f32,
    pub y: f32,
    /// Drag: travel since the press. Wheel: the delta (lines).
    pub dx: f32,
    pub dy: f32,
    pub key: Option<Key>,
    pub mods: Mods,
    pub button: Button,
    /// Hover: entered (true) or left. Drag: still held. Focus: gained.
    pub on: bool,
}
impl Ev {
    pub fn at(x: f32, y: f32) -> Ev {
        Ev { x, y, dx: 0.0, dy: 0.0, key: None, mods: Mods::default(), button: Button::Primary, on: true }
    }
}

/// An event as the trampoline sees it: its kind, the façade's fields, and text for a text event.
#[derive(Clone, Copy, Debug)]
pub struct Event<'a> {
    pub kind: u32,
    pub ev: Ev,
    pub text: &'a str,
}

// ---- handler components ---------------------------------------------------------------------------------------

/// A handler component: the kinds it hears and what it does. `A` is the record it sits on.
pub trait Handler<V, A>: Copy + 'static {
    const KINDS: u32;
    /// Run for `e` (one of `KINDS`); whether the event was consumed.
    fn handle(&self, v: &mut V, a: &A, e: &Event) -> bool;
}

macro_rules! handler {
    ($(#[$m:meta])* $name:ident, $kinds:expr, |$f:ident, $v:ident, $a:ident, $e:ident| $body:expr, $($bound:tt)*) => {
        $(#[$m])*
        #[derive(Clone, Copy)]
        pub struct $name<F>(pub F);
        impl<V, A, F: $($bound)* + Copy + 'static> Handler<V, A> for $name<F> {
            const KINDS: u32 = $kinds;
            #[inline(always)]
            fn handle(&self, $v: &mut V, $a: &A, $e: &Event) -> bool {
                let $f = &self.0;
                let _ = $a;
                $body
            }
        }
    };
}
handler!(/// Activation: a primary press and release on the record.
    Click, kind::CLICK, |f, v, a, e| { let _ = e; f(v); true }, Fn(&mut V));
handler!(/// The press itself.
    Press, kind::PRESS, |f, v, a, e| { f(v, &e.ev); true }, Fn(&mut V, &Ev));
handler!(/// The pointer enters (`ev.on`) or leaves.
    Hover, kind::HOVER, |f, v, a, e| { f(v, &e.ev); true }, Fn(&mut V, &Ev));
handler!(/// Keys while the record (or one it lies inside) has focus; a tab stop.
    OnKey, kind::KEY | kind::FOCUSABLE, |f, v, a, e| { f(v, &e.ev); true }, Fn(&mut V, &Ev));
handler!(/// Pointer travel while pressed (capture: no pick until release), `ev.dx/dy` since the press.
    Drag, kind::DRAG, |f, v, a, e| { f(v, &e.ev); true }, Fn(&mut V, &Ev));
handler!(/// Text typed while the record has focus (the text lane owns what an edit does with it); a tab stop.
    OnText, kind::TEXT | kind::FOCUSABLE, |f, v, a, e| { f(v, e.text); true }, Fn(&mut V, &str));
handler!(/// The wheel over the record; `true` consumes it (else the space under it scrolls).
    Wheel, kind::WHEEL, |f, v, a, e| f(v, &e.ev), Fn(&mut V, &Ev) -> bool);
handler!(/// Focus gained (`ev.on`) or lost.
    OnFocus, kind::FOCUS, |f, v, a, e| { f(v, &e.ev); true }, Fn(&mut V, &Ev));

/// A tab stop with no handler.
#[derive(Clone, Copy)]
pub struct Focusable;
impl<V, A> Handler<V, A> for Focusable {
    const KINDS: u32 = kind::FOCUSABLE;
    #[inline(always)]
    fn handle(&self, _: &mut V, _: &A, _: &Event) -> bool {
        false
    }
}

/// Hover with the record in hand (`hover(record, |v, a, on| ..)`): the form the core's fixtures use.
#[derive(Clone, Copy)]
pub struct HoverRec<F>(pub F);
impl<V, A, F: Fn(&mut V, &A, bool) + Copy + 'static> Handler<V, A> for HoverRec<F> {
    const KINDS: u32 = kind::HOVER;
    #[inline(always)]
    fn handle(&self, v: &mut V, a: &A, e: &Event) -> bool {
        (self.0)(v, a, e.ev.on);
        true
    }
}

// ---- a record with handlers ------------------------------------------------------------------------------------

/// A record (or a record that already has handlers) with one more handler.
#[derive(Clone, Copy)]
pub struct On<T, H>(pub T, pub H);

/// A record with its handlers: the record, the captures of every handler (one value, outermost first), and
/// the one function that runs whichever of them hears an event.
pub trait Target<V>: Copy + 'static {
    type Rec: Arch;
    type Caps: Copy + 'static;
    const KINDS: u32;
    fn rec(&self) -> &Self::Rec;
    fn caps(&self) -> Self::Caps;
    fn handle(c: &Self::Caps, v: &mut V, a: &Self::Rec, e: &Event) -> bool;
}
impl<V, A: Arch> Target<V> for A {
    type Rec = A;
    type Caps = ();
    const KINDS: u32 = 0;
    #[inline(always)]
    fn rec(&self) -> &A {
        self
    }
    #[inline(always)]
    fn caps(&self) {}
    #[inline(always)]
    fn handle(_: &(), _: &mut V, _: &A, _: &Event) -> bool {
        false
    }
}
impl<V, T: Target<V>, H: Handler<V, T::Rec>> Target<V> for On<T, H> {
    type Rec = T::Rec;
    type Caps = (H, T::Caps);
    const KINDS: u32 = T::KINDS | H::KINDS;
    #[inline(always)]
    fn rec(&self) -> &T::Rec {
        self.0.rec()
    }
    #[inline(always)]
    fn caps(&self) -> (H, T::Caps) {
        (self.1, self.0.caps())
    }
    #[inline(always)]
    fn handle(c: &(H, T::Caps), v: &mut V, a: &T::Rec, e: &Event) -> bool {
        (H::KINDS & e.kind != 0 && c.0.handle(v, a, e)) || T::handle(&c.1, v, a, e)
    }
}

/// The handler face of a column: constant per record-and-handler type, held by the column header.
pub struct Ons {
    pub kinds: u32,
    /// Bytes of captures per record (0: none, and no lane), and their alignment.
    pub caps: u32,
    pub caps_align: u32,
    pub hit: HitFn,
    /// Run the handler that hears `e` for the record at `rec` with captures at `caps`; whether one consumed it.
    pub call: unsafe fn(v: *mut (), caps: *const u8, rec: *const u8, e: &Event) -> bool,
    /// The handlers' type, written out by the compiler (reflection's).
    pub ty: fn() -> &'static str,
}

/// SAFETY: `v` is the `V` the view was described for; `rec` a record of `T::Rec`; `caps` the record's captures
/// (any non-null aligned pointer when they are zero-sized).
unsafe fn call<V, T: Target<V>>(v: *mut (), caps: *const u8, rec: *const u8, e: &Event) -> bool {
    // SAFETY: the function contract.
    unsafe {
        let c: T::Caps = core::ptr::read(caps as *const T::Caps);
        T::handle(&c, &mut *(v as *mut V), &*(rec as *const T::Rec), e)
    }
}

struct OnsOf<V, T>(PhantomData<(fn(&mut V), T)>);
impl<V: 'static, T: Target<V>> OnsOf<V, T> {
    const ONS: Ons = Ons {
        kinds: T::KINDS,
        caps: size_of::<T::Caps>() as u32,
        caps_align: align_of::<T::Caps>() as u32,
        hit: hit_fn::<T::Rec>(),
        call: call::<V, T>,
        ty: core::any::type_name::<T>,
    };
}

impl<V: 'static, T: Target<V>, H: Handler<V, T::Rec>> Row<V> for On<T, H> {
    const LEAVES: u32 = 1;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        l.leaf(self.rec(), TypeId::of::<Self>(), Some(&OnsOf::<V, Self>::ONS))
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the caller's contract, for the record and for the captures' lane of the same site.
        unsafe {
            <T::Rec as Row<V>>::write(self.rec(), sites, slot);
            put_caps::<<Self as Target<V>>::Caps>(&*sites, slot, self.caps());
        }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`. Captures are written, never compared: a slot holds the captures of the row it shows.
        unsafe {
            <T::Rec as Row<V>>::patch(self.rec(), sites, slot, mk, cx);
            put_caps::<<Self as Target<V>>::Caps>(&*sites, slot, self.caps());
        }
    }
}

/// SAFETY: `s` is a site of a column created for a record type whose captures are `C`, and `slot` is under the
/// store's capacity.
#[inline(always)]
unsafe fn put_caps<C: Copy>(s: &Site, slot: u32, c: C) {
    if size_of::<C>() != 0 {
        // SAFETY: the lane holds one aligned `C` per record of the column; `s.cap` is the site's first.
        unsafe { core::ptr::write((s.cap as *mut C).add(slot as usize), c) }
    }
}

/// The façade's interaction calls, on any record (and on a record that already has handlers).
pub trait Interact: Sized + Copy + 'static {
    /// Activation (a primary press and release on the record).
    fn on_click<F>(self, f: F) -> On<Self, Click<F>> {
        On(self, Click(f))
    }
    /// The press itself.
    fn on_press<F>(self, f: F) -> On<Self, Press<F>> {
        On(self, Press(f))
    }
    /// The pointer enters (`ev.on`) or leaves.
    fn on_hover<F>(self, f: F) -> On<Self, Hover<F>> {
        On(self, Hover(f))
    }
    /// Keys while focused; a tab stop.
    fn on_key<F>(self, f: F) -> On<Self, OnKey<F>> {
        On(self, OnKey(f))
    }
    /// Pointer travel while pressed (capture).
    fn on_drag<F>(self, f: F) -> On<Self, Drag<F>> {
        On(self, Drag(f))
    }
    /// Text typed while focused; a tab stop.
    fn on_text<F>(self, f: F) -> On<Self, OnText<F>> {
        On(self, OnText(f))
    }
    /// The wheel over the record; return `true` to consume it.
    fn on_wheel<F>(self, f: F) -> On<Self, Wheel<F>> {
        On(self, Wheel(f))
    }
    /// Focus gained or lost.
    fn on_focus<F>(self, f: F) -> On<Self, OnFocus<F>> {
        On(self, OnFocus(f))
    }
    /// A tab stop.
    fn focusable(self) -> On<Self, Focusable> {
        On(self, Focusable)
    }
}
impl<T: Copy + 'static> Interact for T {}

/// A record that hears the pointer enter and leave, given the record: `hover(record, |v, a, on| ..)`.
#[inline(always)]
pub fn hover<V, A: Arch, F: Fn(&mut V, &A, bool) + Copy + 'static>(a: A, f: F) -> On<A, HoverRec<F>> {
    On(a, HoverRec(f))
}

/// What dispatching one event did: which kind, at which place, whether a handler consumed it. Returned by the
/// world's entry points; a caller that drops it pays nothing for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dispatch {
    pub kind: u32,
    /// The place whose handler ran (`NONE`: none did).
    pub place: u32,
    pub consumed: bool,
}
impl Dispatch {
    pub const NOTHING: Dispatch = Dispatch { kind: 0, place: NONE, consumed: false };
}

/// The block as the raster would shade it, from the application's words and the world's.
#[inline(always)]
pub fn env_bit(u: &U, bit: u32) -> bool {
    u.w[U_ENV] & bit != 0
}

/// Two handler faces are the same handlers when they hear the same kinds with the same captures: the same
/// source handler is a different instantiation in each sink's `view` (see `sink::list_sync`).
impl PartialEq for Ons {
    fn eq(&self, o: &Ons) -> bool {
        self.kinds == o.kinds && self.caps == o.caps
    }
}
impl core::fmt::Debug for Ons {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Ons {{ kinds: {:#x}, caps: {} }}", self.kinds, self.caps)
    }
}
