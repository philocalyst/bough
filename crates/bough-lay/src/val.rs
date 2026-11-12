//! Sizes that know how they depend on the available space, and the validity interval that follows.
//!
//! Every definite size in a layout run is a [`V`]: a value in device pixels plus, per axis of the root's
//! available space, whether it is independent of it, equal to it plus a constant ([`UNIT`]), or some other
//! function of it ([`OTHER`]). A *decision* (a comparison whose outcome changes the layout) on a `UNIT` size
//! narrows the interval of available space over which the answer holds, exactly; a rect that is emitted from a
//! dependent size, or a decision on an `OTHER` size, makes the interval a point. The interval is the derivative
//! of the layout with respect to the available size: a resize inside it needs no layout.
use bough_expr::U;

pub const INDEP: u8 = 0;
pub const UNIT: u8 = 1;
pub const OTHER: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct V {
    pub v: f32,
    /// Dependence on the available width (bits 0-1) and height (bits 2-3).
    pub d: u8,
}

#[inline(always)]
const fn join1(a: u8, b: u8) -> u8 {
    if a == 0 {
        b
    } else if b == 0 {
        a
    } else {
        OTHER
    }
}
#[inline(always)]
const fn join(a: u8, b: u8) -> u8 {
    join1(a & 3, b & 3) | join1(a >> 2, b >> 2) << 2
}
/// Any dependence becomes `OTHER`.
#[inline(always)]
const fn other(d: u8) -> u8 {
    (if d & 3 != 0 { OTHER } else { 0 }) | (if d >> 2 != 0 { OTHER << 2 } else { 0 })
}

impl V {
    #[inline(always)]
    pub const fn k(v: f32) -> V {
        V { v, d: 0 }
    }
    /// The available size on `axis` itself.
    #[inline(always)]
    pub const fn avail(v: f32, axis: usize) -> V {
        V { v, d: UNIT << (2 * axis) }
    }
    #[inline(always)]
    pub fn add(self, o: V) -> V {
        V { v: self.v + o.v, d: join(self.d, o.d) }
    }
    #[inline(always)]
    pub fn sub(self, o: V) -> V {
        V { v: self.v - o.v, d: join(self.d, other(o.d)) }
    }
    #[inline(always)]
    pub fn addk(self, k: f32) -> V {
        V { v: self.v + k, d: self.d }
    }
    #[inline(always)]
    pub fn subk(self, k: f32) -> V {
        V { v: self.v - k, d: self.d }
    }
    #[inline(always)]
    pub fn mulk(self, k: f32) -> V {
        V { v: self.v * k, d: if k == 1.0 { self.d } else { other(self.d) } }
    }
    /// The same value as some other function of whatever it depended on.
    #[inline(always)]
    pub fn blur(self) -> V {
        V { v: self.v, d: other(self.d) }
    }
    #[inline(always)]
    pub fn with(self, d: u8) -> V {
        V { v: self.v, d: join(self.d, d) }
    }
}

/// Available space on one axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Av {
    Def(V),
    Min,
    Max,
}
impl Av {
    #[inline(always)]
    pub fn subk(self, k: f32) -> Av {
        match self {
            Av::Def(v) => Av::Def(v.subk(k)),
            o => o,
        }
    }
    #[inline(always)]
    pub fn def(self) -> Option<V> {
        match self {
            Av::Def(v) => Some(v),
            _ => None,
        }
    }
}

/// The interval of available space, per axis and closed, over which an answer is the same.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Valid {
    pub lo: [f32; 2],
    pub hi: [f32; 2],
}
impl Valid {
    pub const ALL: Valid = Valid { lo: [f32::NEG_INFINITY; 2], hi: [f32::INFINITY; 2] };
    #[inline(always)]
    pub fn holds(&self, avail: [f32; 2]) -> bool {
        (avail[0] >= self.lo[0]) & (avail[0] <= self.hi[0]) & (avail[1] >= self.lo[1]) & (avail[1] <= self.hi[1])
    }
    #[inline(always)]
    pub fn and(self, o: Valid) -> Valid {
        Valid { lo: [self.lo[0].max(o.lo[0]), self.lo[1].max(o.lo[1])], hi: [self.hi[0].min(o.hi[0]), self.hi[1].min(o.hi[1])] }
    }
}

/// Thresholds are computed in floats from sizes that follow the available size; an interval stops this far
/// short of one, so that a width just inside the interval takes the same branch in the same arithmetic.
pub const EPS: f32 = 1.0 / 64.0;

/// What a layout run carries: the scale, the block (for sizes that read state), the available space it was
/// asked about and the interval gathered so far.
pub struct Cx<'a> {
    /// Device pixels per point.
    pub s: f32,
    pub u: &'a U,
    pub avail: [f32; 2],
    pub valid: Valid,
}

impl<'a> Cx<'a> {
    #[inline(always)]
    pub fn new(s: f32, u: &'a U, avail: [f32; 2]) -> Cx<'a> {
        Cx { s, u, avail, valid: Valid::ALL }
    }
    #[inline(always)]
    fn point(&mut self, ax: usize) {
        self.valid.lo[ax] = self.avail[ax];
        self.valid.hi[ax] = self.avail[ax];
    }
    /// The answer is at most this good above / below: `avail[ax] < t` (or `> t`) must keep holding.
    #[inline(always)]
    fn below(&mut self, ax: usize, t: f32) {
        self.valid.hi[ax] = self.valid.hi[ax].min((t - EPS).max(self.avail[ax]));
    }
    #[inline(always)]
    fn above(&mut self, ax: usize, t: f32) {
        self.valid.lo[ax] = self.valid.lo[ax].max((t + EPS).min(self.avail[ax]));
    }
    /// The decision `a < b`.
    #[inline(always)]
    pub fn lt(&mut self, a: V, b: V) -> bool {
        let r = a.v < b.v;
        if (a.d | b.d) != 0 {
            self.decided(a, b, r);
        }
        r
    }
    #[inline(always)]
    fn decided(&mut self, a: V, b: V, r: bool) {
        #[cfg(feature = "mutant_valid")]
        if true {
            return;
        }
        for ax in 0..2 {
            let (ka, kb) = ((a.d >> (2 * ax)) & 3, (b.d >> (2 * ax)) & 3);
            match (ka, kb) {
                (INDEP, INDEP) | (UNIT, UNIT) => {}
                (UNIT, INDEP) => {
                    // a = avail + c: a < b while avail < b - c
                    let t = b.v - (a.v - self.avail[ax]);
                    if r { self.below(ax, t) } else { self.above(ax, t) }
                }
                (INDEP, UNIT) => {
                    // b = avail + c: a < b while avail > a - c
                    let t = a.v - (b.v - self.avail[ax]);
                    if r { self.above(ax, t) } else { self.below(ax, t) }
                }
                _ => self.point(ax),
            }
        }
    }
    /// The larger of two sizes, as a decision.
    #[inline(always)]
    pub fn max(&mut self, a: V, b: V) -> V {
        if self.lt(a, b) { b } else { a }
    }
    #[inline(always)]
    pub fn min(&mut self, a: V, b: V) -> V {
        if self.lt(b, a) { b } else { a }
    }
    /// `v` must stay inside `[lo, hi)` for the answer to hold (a wrapped paragraph's line structure).
    #[inline(always)]
    pub fn range(&mut self, v: V, lo: f32, hi: f32) {
        if v.d == 0 {
            return;
        }
        for ax in 0..2 {
            match (v.d >> (2 * ax)) & 3 {
                INDEP => {}
                UNIT => {
                    let c = v.v - self.avail[ax];
                    self.above(ax, lo - c);
                    self.below(ax, hi - c);
                }
                _ => self.point(ax),
            }
        }
    }
    /// A rect was emitted whose edges depend on the available space in the ways `d` says.
    #[inline(always)]
    pub fn emitted(&mut self, d: u8) {
        if d != 0 {
            if d & 3 != 0 {
                self.point(0);
            }
            if d >> 2 != 0 {
                self.point(1);
            }
        }
    }
}

/// cephal's rounding (`floor(v + 0.5)` for |v| < 2^22), so rounded edges agree with the oracle's.
#[inline(always)]
pub fn round(v: f32) -> i32 {
    const MAGIC: f32 = 12_582_912.0;
    let y = v + 0.5;
    let t = (y + MAGIC) - MAGIC;
    (if t > y { t - 1.0 } else { t }) as i32
}
