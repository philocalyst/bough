//! Device-pixel integer geometry.
//!
//! `R` is a half-open rectangle in i32 device pixels (the lab's general type).
//! `N` is the hot-path form: i16 lanes in negated-max order `(x0, y0, -x1, -y1)`,
//! so intersection is a lane-wise `max`, union a lane-wise `min`, and
//! "non-empty" is `x0 + nx1 < 0 && y0 + ny1 < 0` (computed widened, so the
//! `EMPTY` sentinel `[i16::MAX; 4]` never overflows into a false positive).

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct R {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl R {
    pub const E: R = R { x0: 0, y0: 0, x1: 0, y1: 0 };
    #[inline(always)]
    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> R {
        R { x0, y0, x1, y1 }
    }
    #[inline(always)]
    pub const fn xywh(x: i32, y: i32, w: i32, h: i32) -> R {
        R { x0: x, y0: y, x1: x + w, y1: y + h }
    }
    #[inline(always)]
    pub fn is_empty(self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    #[inline(always)]
    pub fn area(self) -> i64 {
        if self.is_empty() { 0 } else { (self.x1 - self.x0) as i64 * (self.y1 - self.y0) as i64 }
    }
    /// Intersection; the result may be a non-canonical empty rect.
    #[inline(always)]
    pub fn inter(self, o: R) -> R {
        R { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }
    /// Intersection, canonical `E` when empty.
    #[inline(always)]
    pub fn clip(self, o: R) -> R {
        let r = self.inter(o);
        if r.is_empty() { R::E } else { r }
    }
    #[inline(always)]
    pub fn meets(self, o: R) -> bool {
        !self.inter(o).is_empty()
    }
    #[inline(always)]
    pub fn union(self, o: R) -> R {
        if self.is_empty() {
            o
        } else if o.is_empty() {
            self
        } else {
            R { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
        }
    }
    #[inline(always)]
    pub fn tr(self, dx: i32, dy: i32) -> R {
        R { x0: self.x0 + dx, y0: self.y0 + dy, x1: self.x1 + dx, y1: self.y1 + dy }
    }
    #[inline(always)]
    pub fn contains(self, o: R) -> bool {
        o.is_empty() || (self.x0 <= o.x0 && self.y0 <= o.y0 && self.x1 >= o.x1 && self.y1 >= o.y1)
    }
    /// `self \ o` as at most four disjoint rects (top, bottom, left, right bands).
    #[inline(always)]
    pub fn sub(self, o: R, out: &mut impl FnMut(R)) {
        if self.is_empty() {
            return;
        }
        if !self.meets(o) {
            out(self);
            return;
        }
        if o.y0 > self.y0 {
            out(R::new(self.x0, self.y0, self.x1, o.y0));
        }
        if o.y1 < self.y1 {
            out(R::new(self.x0, o.y1, self.x1, self.y1));
        }
        let (my0, my1) = (self.y0.max(o.y0), self.y1.min(o.y1));
        if o.x0 > self.x0 {
            out(R::new(self.x0, my0, o.x0, my1));
        }
        if o.x1 < self.x1 {
            out(R::new(o.x1, my0, self.x1, my1));
        }
    }
    /// Symmetric difference `a ^ b` as disjoint rects.
    #[inline(always)]
    pub fn xor(a: R, b: R, out: &mut impl FnMut(R)) {
        a.sub(b, out);
        b.sub(a, out);
    }
}

/// Negated-max i16 rect: `(x0, y0, -x1, -y1)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(C, align(8))]
pub struct N(pub [i16; 4]);

impl N {
    pub const EMPTY: N = N([i16::MAX; 4]);
    #[inline(always)]
    pub fn from_r(r: R) -> N {
        if r.is_empty() {
            N::EMPTY
        } else {
            N([r.x0 as i16, r.y0 as i16, (-r.x1) as i16, (-r.y1) as i16])
        }
    }
    #[inline(always)]
    pub fn to_r(self) -> R {
        if self.is_empty() {
            R::E
        } else {
            R::new(self.0[0] as i32, self.0[1] as i32, -(self.0[2] as i32), -(self.0[3] as i32))
        }
    }
    #[inline(always)]
    pub fn is_empty(self) -> bool {
        (self.0[0] as i32 + self.0[2] as i32) >= 0 || (self.0[1] as i32 + self.0[3] as i32) >= 0
    }
    #[inline(always)]
    pub fn max(self, o: N) -> N {
        N([self.0[0].max(o.0[0]), self.0[1].max(o.0[1]), self.0[2].max(o.0[2]), self.0[3].max(o.0[3])])
    }
    #[inline(always)]
    pub fn min(self, o: N) -> N {
        N([self.0[0].min(o.0[0]), self.0[1].min(o.0[1]), self.0[2].min(o.0[2]), self.0[3].min(o.0[3])])
    }
    /// Non-empty intersection: one lane-wise max and two widened adds.
    #[inline(always)]
    pub fn meets(self, o: N) -> bool {
        !self.max(o).is_empty()
    }
    #[inline(always)]
    pub fn as_u64(self) -> u64 {
        unsafe { core::mem::transmute::<N, u64>(self) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nrect_algebra_matches_r_exhaustively_small() {
        // All rects with coordinates in -2..5 (both empty and non-empty).
        let vals = -2..5;
        let mut rs = vec![];
        for x0 in vals.clone() {
            for x1 in vals.clone() {
                for y0 in vals.clone() {
                    for y1 in vals.clone() {
                        rs.push(R::new(x0, y0, x1, y1));
                    }
                }
            }
        }
        rs.retain(|r| (r.x0 < r.x1 && r.y0 < r.y1) || (r.x0 == 0 && r.x1 == 0 && r.y0 == 0 && r.y1 == 0));
        for &a in &rs {
            for &b in rs.iter().step_by(7) {
                let (na, nb) = (N::from_r(a), N::from_r(b));
                assert_eq!(na.meets(nb), a.meets(b), "{a:?} {b:?}");
                assert_eq!(na.min(nb).to_r(), a.union(b), "{a:?} {b:?}");
                // xor area identity
                let mut s = 0;
                R::xor(a, b, &mut |r| s += r.area());
                assert_eq!(s, a.area() + b.area() - 2 * a.inter(b).area().max(0) * (a.meets(b) as i64));
            }
        }
    }
}
