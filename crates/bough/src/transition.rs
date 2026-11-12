//! Built-in transitions (round 4, input agent): a transition is a typed operand, a value plus a curve.
//!
//! `sig!(v.width).transition(Curve::out(0.24))` (or `.map(f).transition(..)`) lowers like any wired operand
//! for any property applier, so every animatable property (colour, width, span, angle, opacity, a
//! transform's matrix...) transitions with no code of its own: the wire's patch does not apply the new
//! value, it *retargets* the transition (from = the value shown now, to = the new value, start = now); the
//! frame clock (`App::tick`) evaluates every active transition and applies its value through the same
//! applier (a paint write, a layout restyle, ...), and the host publishes those frames paced
//! (`frame_paced`). A layout property relayouts per frame because its applier does.
//!
//! Interrupting is continuous by construction: a retarget starts from the value displayed at that moment
//! (value continuity, C0), and with `Curve::smooth` from the displayed velocity too (C1: a cubic Hermite
//! segment from (value, velocity) to (target, 0)).
use crate::color::Color;
use crate::elem::{Apply, IntoProp, Mounter};
use crate::rt::{Rt, Wire};
use crate::signal::{WMap, Wired};

/// Values a transition can interpolate.
pub trait Lerp: Copy + PartialEq + 'static {
    /// `a + (b - a) t`.
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self::add_scaled(a, b, a, t)
    }
    /// `a + (b - c) k` per component (the Hermite form's tangent term).
    fn add_scaled(a: Self, b: Self, c: Self, k: f32) -> Self;
}
impl Lerp for f32 {
    #[inline]
    fn add_scaled(a: f32, b: f32, c: f32, k: f32) -> f32 {
        a + (b - c) * k
    }
}
impl Lerp for (f32, f32) {
    #[inline]
    fn add_scaled(a: Self, b: Self, c: Self, k: f32) -> Self {
        (a.0 + (b.0 - c.0) * k, a.1 + (b.1 - c.1) * k)
    }
}
impl Lerp for [f32; 4] {
    #[inline]
    fn add_scaled(a: Self, b: Self, c: Self, k: f32) -> Self {
        [0, 1, 2, 3].map(|i| a[i] + (b[i] - c[i]) * k)
    }
}
impl Lerp for Color {
    /// Per channel (straight RGBA8), rounded and clamped.
    #[inline]
    fn add_scaled(a: Color, b: Color, c: Color, k: f32) -> Color {
        let ch = |v: u32, i: u32| ((v >> (8 * i)) & 255) as f32;
        let v = |i: u32| (ch(a.0, i) + (ch(b.0, i) - ch(c.0, i)) * k).round().clamp(0.0, 255.0) as u32;
        Color(v(0) | v(1) << 8 | v(2) << 16 | v(3) << 24)
    }
}

/// Easing of a transition's progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ease {
    Linear,
    InCubic,
    OutCubic,
    InOutCubic,
    /// C1 retargets: a cubic Hermite segment from the displayed (value, velocity) to (target, 0).
    Smooth,
}

impl Ease {
    #[inline]
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::InCubic => t * t * t,
            Ease::OutCubic => 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t),
            Ease::InOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) * 0.5
                }
            }
            Ease::Smooth => t * t * (3.0 - 2.0 * t),
        }
    }
}

/// A transition's timing: duration (seconds) and easing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub dur: f32,
    pub ease: Ease,
}

impl Curve {
    pub const fn new(dur: f32, ease: Ease) -> Curve {
        Curve { dur, ease }
    }
    pub const fn linear(dur: f32) -> Curve {
        Curve { dur, ease: Ease::Linear }
    }
    /// Ease-out cubic (zgui's tween).
    pub const fn out(dur: f32) -> Curve {
        Curve { dur, ease: Ease::OutCubic }
    }
    pub const fn in_out(dur: f32) -> Curve {
        Curve { dur, ease: Ease::InOutCubic }
    }
    /// Velocity-continuous retargets (Hermite).
    pub const fn smooth(dur: f32) -> Curve {
        Curve { dur, ease: Ease::Smooth }
    }
}

/// One running (or resting) transition of a property of node `node`.
struct State<T: Lerp> {
    node: u32,
    apply: Box<dyn Fn(&mut Rt, u32, &T)>,
    curve: Curve,
    from: T,
    to: T,
    /// Hermite: `from + velocity * dur` (the start tangent as a point).
    vel_to: T,
    t0: f64,
    shown: T,
    active: bool,
}

impl<T: Lerp> State<T> {
    /// The value at time `now`.
    fn value(&self, now: f64) -> T {
        let p = if self.curve.dur <= 0.0 { 1.0 } else { ((now - self.t0) / self.curve.dur as f64) as f32 };
        if p >= 1.0 {
            return self.to;
        }
        let p = p.max(0.0);
        if self.curve.ease == Ease::Smooth {
            // cubic Hermite: from h00 + (dur v) h10 + to h01 = lerp(from, to, h01) + (vel_to - from) h10
            let h01 = -2.0 * p * p * p + 3.0 * p * p;
            let h10 = p * p * p - 2.0 * p * p + p;
            return T::add_scaled(T::lerp(self.from, self.to, h01), self.vel_to, self.from, h10);
        }
        T::lerp(self.from, self.to, self.curve.ease.at(p))
    }
}

/// The type-erased half the frame clock drives.
trait Running {
    fn step(&mut self, now: f64) -> Option<(u32, *const u8)>;
    fn apply_shown(&self, rt: &mut Rt);
    fn is_active(&self) -> bool;
    fn as_any(&mut self) -> &mut dyn std::any::Any;
}

impl<T: Lerp> Running for State<T> {
    fn step(&mut self, now: f64) -> Option<(u32, *const u8)> {
        if !self.active {
            return None;
        }
        let v = self.value(now);
        let p = if self.curve.dur <= 0.0 { 1.0 } else { (now - self.t0) / self.curve.dur as f64 };
        if p >= 1.0 {
            self.active = false;
        }
        if v == self.shown {
            return None;
        }
        self.shown = v;
        Some((self.node, &self.shown as *const T as *const u8))
    }
    fn apply_shown(&self, rt: &mut Rt) {
        (self.apply)(rt, self.node, &self.shown);
    }
    fn is_active(&self) -> bool {
        self.active
    }
    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Every transition of a runtime and the frame clock's time.
///
/// **Ownership** (round 7): a transition belongs to the scope that mounted the element it animates, like the
/// element's wires (`Scope::trans`). Unmounting the element releases it: it stops, and its slot is reused. A
/// transition therefore cannot outlive its element, apply to a dead layout node or reach whatever element is
/// later given the same id. (A slot is `None` while free.)
pub struct Transitions {
    items: Vec<Option<Box<dyn Running>>>,
    free: Vec<u32>,
    /// The transition whose applier is running (`u32::MAX`: none), and whether it was released meanwhile.
    stepping: u32,
    stepping_released: bool,
    /// Seconds since the runtime started, advanced by `App::tick`.
    pub now: f64,
    pub active: u32,
    /// Retargets and steps so far (for the benchmarks).
    pub retargets: u64,
    pub steps: u64,
}

impl Default for Transitions {
    fn default() -> Self {
        Transitions { items: Vec::new(), free: Vec::new(), stepping: u32::MAX, stepping_released: false, now: 0.0, active: 0, retargets: 0, steps: 0 }
    }
}

impl Transitions {
    pub(crate) fn add<T: Lerp>(&mut self, node: u32, apply: Box<dyn Fn(&mut Rt, u32, &T)>, init: T, curve: Curve) -> u32 {
        let st: Box<dyn Running> = Box::new(State { node, apply, curve, from: init, to: init, vel_to: init, t0: 0.0, shown: init, active: false });
        match self.free.pop() {
            Some(i) => {
                self.items[i as usize] = Some(st);
                i
            }
            None => {
                self.items.push(Some(st));
                (self.items.len() - 1) as u32
            }
        }
    }

    /// The owner of transition `id` unmounted: it stops and its slot is free.
    pub(crate) fn release(&mut self, id: u32) {
        if id == self.stepping {
            // the one being stepped (out of its slot): `step_transitions` drops it when its applier returns
            self.stepping_released = true;
            return;
        }
        if let Some(it) = self.items[id as usize].take() {
            if it.is_active() {
                self.active -= 1;
            }
            self.free.push(id);
        }
    }

    /// The transition `id` (of value type `T`) retargets to `to` at the current time, from the value it
    /// shows now (and, smooth, the velocity it has now).
    pub(crate) fn retarget<T: Lerp>(&mut self, id: u32, to: T) {
        let now = self.now;
        let Some(s) = self.items[id as usize].as_mut().and_then(|it| it.as_any().downcast_mut::<State<T>>()) else { return };
        if s.to == to && s.active {
            return;
        }
        let cur = s.value(now);
        if s.curve.ease == Ease::Smooth && s.active {
            // the velocity now (a 1 ms difference), as the point `cur + velocity * dur`
            let h = 1e-3f64;
            let ahead = s.value(now + h);
            s.vel_to = T::add_scaled(cur, ahead, cur, (s.curve.dur as f64 / h) as f32);
        } else {
            s.vel_to = cur;
        }
        s.from = cur;
        s.to = to;
        s.t0 = now;
        if !s.active {
            s.active = true;
            self.active += 1;
        }
        self.retargets += 1;
    }

    /// Any transition running?
    pub fn running(&self) -> bool {
        self.active > 0
    }

    /// The value transition `id` (mount order: the n-th transition operand lowered) shows now.
    pub fn shown<T: Lerp>(&mut self, id: u32) -> Option<T> {
        self.items.get_mut(id as usize)?.as_mut()?.as_any().downcast_mut::<State<T>>().map(|s| s.shown)
    }

    /// Transitions created so far.
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Rt {
    /// The frame clock: advance by `dt` seconds and apply every running transition's value (through its
    /// property's applier: paint writes, restyles). Returns whether any ran.
    pub fn step_transitions(&mut self, dt: f32) -> bool {
        self.transitions.now += dt as f64;
        if self.transitions.active == 0 {
            return false;
        }
        let now = self.transitions.now;
        // one at a time, taken out of its slot while its applier runs: an applier may unmount elements (a
        // restyle that re-windows a list), which releases their transitions, this one included
        for i in 0..self.transitions.items.len() {
            let Some(mut it) = self.transitions.items[i].take() else { continue };
            if !it.is_active() {
                self.transitions.items[i] = Some(it);
                continue;
            }
            self.transitions.stepping = i as u32;
            if it.step(now).is_some() {
                it.apply_shown(self);
            }
            self.transitions.steps += 1;
            if std::mem::take(&mut self.transitions.stepping_released) {
                // its owner unmounted while its applier ran: it is gone (it was running: one fewer)
                self.transitions.stepping = u32::MAX;
                self.transitions.active -= 1;
                self.transitions.free.push(i as u32);
                continue;
            }
            self.transitions.stepping = u32::MAX;
            if !it.is_active() {
                self.transitions.active -= 1;
            }
            self.transitions.items[i] = Some(it);
        }
        true
    }
}

/// A transition operand: a wired signal (or a projection of one) whose changes are eased.
pub struct Transit<W> {
    pub(crate) w: W,
    pub(crate) curve: Curve,
}

impl<'a, T, V> Wired<'a, T, V> {
    /// This signal's changes transition along `curve` (any property the operand drives).
    pub fn transition(self, curve: Curve) -> Transit<Wired<'a, T, V>> {
        Transit { w: self, curve }
    }
}

impl<'a, T, V, F> WMap<'a, T, V, F> {
    /// The projection's changes transition along `curve`.
    pub fn transition(self, curve: Curve) -> Transit<WMap<'a, T, V, F>> {
        Transit { w: self, curve }
    }
}

/// # Safety
/// As every patch: `sig` points at a `Signal<T>`; the recipe holds the transition id.
pub(crate) unsafe fn patch_transit<T: Lerp>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { &*(sig as *const crate::signal::Signal<T>) };
    let id = u32::from_le_bytes(w.recipe[..4].try_into().unwrap());
    rt.transitions.retarget::<T>(id, **s);
}

/// # Safety
/// As `patch_transit`; the recipe holds the id then an `F` (at most 12 bytes).
unsafe fn patch_transit_map<T: 'static, P: Lerp, F: Fn(&T) -> P + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { &*(sig as *const crate::signal::Signal<T>) };
    let id = u32::from_le_bytes(w.recipe[..4].try_into().unwrap());
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr().add(4) as *const F) };
    rt.transitions.retarget::<P>(id, f(s));
}

fn apply_fn<K: Apply>(rt: &mut Rt, node: u32, v: &K::P) {
    K::apply(rt, node, v)
}

impl<'a, T: Lerp, V> IntoProp<T, V> for Transit<Wired<'a, T, V>> {
    fn lower<K: Apply<P = T>>(self, m: &mut Mounter<'_, V>, node: u32) -> T {
        let init = *self.w.get();
        let id = m.rt.transitions.add::<T>(node, Box::new(apply_fn::<K> as fn(&mut Rt, u32, &T)), init, self.curve);
        m.scope.trans.push(id);
        let mut recipe = [0u8; 16];
        recipe[..4].copy_from_slice(&id.to_le_bytes());
        m.wire(&self.w, patch_transit::<T>, node, recipe);
        init
    }
    /// At a re-description the wire keeps driving the transition; the value is its target.
    fn value(self) -> T {
        *self.w.get()
    }
}

impl<'a, T: 'static, V, F, P: Lerp> IntoProp<P, V> for Transit<WMap<'a, T, V, F>>
where
    F: Fn(&T) -> P + Copy + 'static,
{
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        assert!(size_of::<F>() <= 12, "a transition's projection must capture at most 12 bytes");
        let init = (self.w.f)(self.w.w.get());
        let id = m.rt.transitions.add::<P>(node, Box::new(apply_fn::<K> as fn(&mut Rt, u32, &P)), init, self.curve);
        m.scope.trans.push(id);
        let mut recipe = [0u8; 16];
        recipe[..4].copy_from_slice(&id.to_le_bytes());
        // SAFETY: F is Copy and at most 12 bytes (asserted): it fits after the id.
        unsafe { std::ptr::write_unaligned(recipe.as_mut_ptr().add(4) as *mut F, self.w.f) };
        m.wire(&self.w.w, patch_transit_map::<T, P, F>, node, recipe);
        init
    }
    fn value(self) -> P {
        (self.w.f)(self.w.w.get())
    }
}
