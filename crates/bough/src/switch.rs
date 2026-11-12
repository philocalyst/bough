//! Transitions of a dynamic part's item (`crossfade`, `slide`, `morph`), its identity (`keyed`) and the pairing
//! of parts across items (`tag`). STAGE 1: identity and pairing are real; the transitions are not built yet
//! and switch at once (see the lane journal); their kernel-parameter form is specified in
//! `docs/lanes/async/DESIGN.md` section 5.
use crate::elem::{Element, Mounter};
use crate::transition::Curve;

/// An element whose identity in a dynamic part is `key`: a new key is a new item (replaced, not updated).
pub struct Keyed<E> {
    key: u64,
    e: E,
}
pub fn keyed<E>(key: u64, e: E) -> Keyed<E> {
    Keyed { key, e }
}
impl<V, E: Element<V>> Element<V> for Keyed<E> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_dyn(m, std::iter::once(crate::describe::ArmItem(self.e, self.key.wrapping_add(1))));
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_dyn(m, std::iter::once(crate::describe::ArmItem(self.e, self.key.wrapping_add(1))));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Crossfade,
    Slide,
    Morph,
}

/// A dynamic part whose item changes are animated (see the module docs).
pub struct Switch<E> {
    pub(crate) kind: Kind,
    pub(crate) curve: Curve,
    e: E,
}

/// The leaving item fades out while the entering one fades in.
pub fn crossfade<E>(curve: Curve, e: E) -> Switch<E> {
    Switch { kind: Kind::Crossfade, curve, e }
}
/// The leaving item slides out and the entering one slides in (and fades).
pub fn slide<E>(curve: Curve, e: E) -> Switch<E> {
    Switch { kind: Kind::Slide, curve, e }
}
/// Parts with the same `tag` in the leaving and the entering item move from one's box to the other's while
/// cross-fading; parts without a partner fade.
pub fn morph<E>(curve: Curve, e: E) -> Switch<E> {
    Switch { kind: Kind::Morph, curve, e }
}

impl<V, E: Element<V>> Element<V> for Switch<E> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        self.e.mount(m)
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        self.e.update(m)
    }
}

/// A part's pairing name across the items of a `morph`.
pub struct Tagged<E> {
    pub(crate) tag: u32,
    e: E,
}
pub trait Tag: Sized {
    fn tag(self, t: u32) -> Tagged<Self> {
        Tagged { tag: t, e: self }
    }
}
impl<E> Tag for E {}
impl<V, E: Element<V>> Element<V> for Tagged<E> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        self.e.mount(m)
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        self.e.update(m)
    }
    fn dyn_key(&self) -> u64 {
        self.e.dyn_key()
    }
}
