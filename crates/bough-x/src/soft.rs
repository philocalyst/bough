//! A reference raster and an immediate-mode sink. Oracles, not products: the raster lane owns pixels.
//!
//! [`Soft`] is a [`Backend`] that paints columns into a framebuffer, restricted to a clip (a tile, for a
//! retained repair). [`Immediate`] is a [`Sink`] that paints each record the moment the view describes it, in
//! the view's own order, with no store at all: the ground truth the stores and their paint order are held to.
use crate::expr::U;
use crate::kinds::{FillArch, LabelArch, NumArch, OutlineArch};
use crate::sink::{Leaf, List, Row, ScopeOf, Sink, exposed};
use crate::store::Hov;
use core::any::TypeId;
use crate::store::{Arch, Backend, ColRef, Live, Page, Rect, Space, Tiles};
use core::marker::PhantomData;

pub struct Soft {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
    /// Painting is cut to this (the tile being repaired).
    pub clip: Rect,
    /// Records the backend was handed (a count of work, for the cost of a repair).
    pub records: u64,
    /// Real text: the application's faces and glyph lanes. Without it lines draw nothing and numbers are the
    /// 3 x 5 stand-in digits.
    pub text: Option<SoftText>,
}

/// The oracle's hold on the application's text: its faces and (shared, because the view shapes lines while it
/// is described) its glyph lanes.
#[derive(Clone)]
pub struct SoftText {
    pub faces: &'static [bough_face::Face],
    pub glyphs: std::rc::Rc<core::cell::RefCell<bough_face::Glyphs>>,
}

// 3 x 5 digits, rows top to bottom, bit 2 = left.
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];
const GLYPH_SCALE: i32 = 3;
const ADVANCE: i32 = 4 * GLYPH_SCALE;

fn glyph(b: u8) -> [u8; 5] {
    match b {
        b'0'..=b'9' => DIGITS[(b - b'0') as usize],
        b'.' => [0, 0, 0, 0, 2],
        b'-' => [0, 0, 7, 0, 0],
        b' ' => [0; 5],
        // any other byte: a pattern of its own, so different text is different pixels
        _ => {
            let h = (b as u32).wrapping_mul(0x9E37_79B9);
            [(h >> 3) as u8 & 7 | 4, (h >> 8) as u8 & 7 | 1, (h >> 13) as u8 & 7, (h >> 18) as u8 & 7 | 4, (h >> 23) as u8 & 7 | 1]
        }
    }
}

/// An integer scaled by `10^d`, with `d` decimals, as bytes. Integer arithmetic only.
pub fn format_num(n: i32, d: u8, out: &mut [u8; 24]) -> usize {
    let (neg, mut m) = (n < 0, n.unsigned_abs());
    let mut tmp = [0u8; 24];
    let mut k = 0;
    let mut digits = 0;
    loop {
        if d != 0 && digits == d {
            tmp[k] = b'.';
            k += 1;
        }
        tmp[k] = b'0' + (m % 10) as u8;
        k += 1;
        digits += 1;
        m /= 10;
        if m == 0 && digits > d {
            break;
        }
    }
    if neg {
        tmp[k] = b'-';
        k += 1;
    }
    for i in 0..k {
        out[i] = tmp[k - 1 - i];
    }
    k
}

impl Soft {
    pub fn new(w: i32, h: i32) -> Soft {
        Soft { w, h, px: vec![0; (w * h) as usize], clip: Rect { x0: 0, y0: 0, x1: w, y1: h }, records: 0, text: None }
    }
    pub fn rect(&mut self, r: Rect, c: u32) {
        let r = r.and(self.clip).and(Rect { x0: 0, y0: 0, x1: self.w, y1: self.h });
        if r.empty() {
            return;
        }
        for y in r.y0..r.y1 {
            let row = (y * self.w) as usize;
            self.px[row + r.x0 as usize..row + r.x1 as usize].fill(c);
        }
    }
    /// Coverage over what is there, in colour `c`: per channel, in integers. `cov` starts at pixel `(x, y)`,
    /// which the caller has cut to the frame and the clip.
    #[inline(always)]
    fn cover(&mut self, x: i32, y: i32, cov: &[u8], c: u32) {
        let row = &mut self.px[(y * self.w + x) as usize..][..cov.len()];
        for (d, &a) in row.iter_mut().zip(cov) {
            let a = a as u32;
            let ch = |sh: u32| (((c >> sh & 255) * a + (*d >> sh & 255) * (255 - a) + 127) / 255) << sh;
            *d = ch(16) | ch(8) | ch(0);
        }
    }
    /// With real text: where painting is allowed for a record's rect (its space's clip, the tile, the frame).
    fn cut(&self, clip: Rect) -> Rect {
        clip.and(self.clip).and(Rect { x0: 0, y0: 0, x1: self.w, y1: self.h })
    }
    /// Text in a box (screen coordinates), cut to `cut`.
    fn text(&mut self, r: Rect, cut: Rect, bytes: &[u8], c: u32) {
        let x = r.x0 + 12;
        let y = r.y0 + ((r.y1 - r.y0) - 5 * GLYPH_SCALE) / 2;
        for (k, &b) in bytes.iter().enumerate() {
            let g = glyph(b);
            for (gy, row) in g.iter().enumerate() {
                for gx in 0..3 {
                    if row & (4 >> gx) != 0 {
                        let px = Rect::new(x + k as i32 * ADVANCE + gx * GLYPH_SCALE, y + gy as i32 * GLYPH_SCALE, GLYPH_SCALE, GLYPH_SCALE);
                        self.rect(px.and(cut), c);
                    }
                }
            }
        }
    }
    /// Repair the damaged tiles of `tiles` from `paint` (which paints everything, cut to `self.clip`).
    pub fn repair(&mut self, tiles: &Tiles, mut paint: impl FnMut(&mut Soft)) {
        let whole = self.clip;
        tiles.each(|t| {
            self.clip = t;
            self.rect(t, 0);
            paint(self);
        });
        self.clip = whole;
    }
}

impl Backend for Soft {
    fn fill<A: FillArch>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            self.records += 1;
            self.rect(r.shift(org).and(clip), a.color(u).0)
        });
    }
    fn num<A: NumArch>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        let text = self.text.clone();
        col.each(|r, a| {
            self.records += 1;
            let r = r.shift(org);
            if let Some(t) = &text {
                let g = t.glyphs.borrow();
                let cut = self.cut(r.and(clip));
                crate::text::TextCx { faces: t.faces, glyphs: &g }.num(a.value(u), a.decimals(), A::PAD, r, cut, |x, y, cov| self.cover(x, y, cov, a.color(u).0));
                return;
            }
            let mut buf = [0u8; 24];
            let n = format_num(a.value(u), a.decimals(), &mut buf);
            self.text(r, r.and(clip), &buf[..n], a.color(u).0)
        });
    }
    fn line<A: crate::text::LineArch>(&mut self, col: ColRef<'_, A>, u: &U) {
        let Some(t) = self.text.clone() else { return };
        let g = t.glyphs.borrow();
        let cx = crate::text::TextCx { faces: t.faces, glyphs: &g };
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            self.records += 1;
            let r = r.shift(org);
            let cut = self.cut(r.and(clip));
            cx.line(a.run(), r, cut, |x, y, cov| self.cover(x, y, cov, a.color(u).0));
        });
    }
    fn label<A: LabelArch>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            self.records += 1;
            let r = r.shift(org);
            self.text(r, r.and(clip), a.str(), a.color(u).0)
        });
    }
    fn outline<A: OutlineArch>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            self.records += 1;
            let r = r.shift(org).shift(a.offset(u));
            let (w, c) = (a.width(u), a.color(u).0);
            for e in [Rect { y1: r.y0 + w, ..r }, Rect { y0: r.y1 - w, ..r }, Rect { x1: r.x0 + w, ..r }, Rect { x0: r.x1 - w, ..r }] {
                self.rect(e.and(clip), c);
            }
        });
    }
}

/// Paint while describing. No store, no columns: every record is painted where the view says, in view order.
pub struct Immediate<'s, V> {
    pub soft: &'s mut Soft,
    pub u: &'s U,
    pub space: Space,
    _p: PhantomData<fn(&mut V)>,
}
impl<'s, V> Immediate<'s, V> {
    pub fn new(soft: &'s mut Soft, u: &'s U) -> Self {
        let space = Space::root(soft.w, soft.h);
        Immediate { soft, u, space, _p: PhantomData }
    }
    fn one<A: Arch>(&mut self, r: Rect, a: &A) {
        // SAFETY: a page is plain data; every record type is valid as zero bytes or is overwritten before use
        // (only slot 0 is read: the live mask below names it alone).
        let mut pg: Box<Page<A>> = unsafe { Box::new_zeroed().assume_init() };
        pg.set_rect(0, r);
        pg.rec[0] = *a;
        let rows = [1u64];
        A::paint(self.soft, ColRef { pages: core::slice::from_ref(&*pg), space: &self.space, live: Live { cap: 1, rows: &rows, nrec: 1 } }, self.u);
    }
}
struct Now<'a, 'r, 's, V> {
    im: &'a mut Immediate<'s, V>,
    rects: &'r [Rect],
    at: (i32, i32),
    k: usize,
}
struct Rects(Vec<Rect>);
impl bough_lay::Out for Rects {
    #[inline]
    fn put(&mut self, k: usize, r: [i32; 4]) {
        self.0[k] = Rect { x0: r[0], y0: r[1], x1: r[2], y1: r[3] };
    }
}
impl<V> Leaf<V> for Now<'_, '_, '_, V> {
    fn hole(&mut self) {
        self.k += 1;
    }
    fn leaf<A: Arch>(&mut self, a: &A, _: TypeId, _: Option<Hov>) {
        self.im.one(self.rects[self.k].shift(self.at), a);
        self.k += 1;
    }
}
impl<'r, 's, V> ScopeOf<'r, V> for Immediate<'s, V> {
    type Scope = Immediate<'r, V>;
}
impl<'s, V> Sink<V> for Immediate<'s, V> {
    fn row<R: Row<V>, L: AsRef<[Rect]>, F: FnOnce() -> (L, R)>(&mut self, f: F) {
        let (rects, row) = f();
        row.visit(&mut Now { im: self, rects: rects.as_ref(), at: (0, 0), k: 0 });
    }
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(&mut self, space: Space, f: F) {
        let mut sub = Immediate { soft: &mut *self.soft, u: self.u, space, _p: PhantomData };
        f(&mut sub);
    }
    fn env(&self) -> crate::sink::Env<'_> {
        crate::sink::Env { scale: f32::from_bits(self.u.w[crate::expr::U_SCALE]), size: [self.soft.w as f32, self.soft.h as f32], u: self.u }
    }
    fn rows<R: Row<V> + bough_lay::Shape, F: FnMut(usize) -> R>(&mut self, n: u32, geo: Option<crate::sink::Geo>, mut f: F) -> i32 {
        let g = geo.expect("the immediate sink lays out");
        if n == 0 {
            return 0;
        }
        let scale = f32::from_bits(self.u.w[crate::expr::U_SCALE]);
        let mut rects = Rects(vec![Rect::default(); <R as bough_lay::Shape>::LEAVES]);
        // the height of a row is the first row's, as in the stores
        let row_h = bough_lay::shape::place(&f(0), scale, self.u, g.w as f32, &mut rects).0;
        if row_h == 0 {
            return 0;
        }
        let l = List { n, row_h, x: g.x, y: g.y, shape: &[], edits: &crate::sink::FIXED };
        let (lo, hi) = exposed(&l, self.space.origin(self.u), self.space.clip);
        for r in lo..hi {
            let row = f(r as usize);
            bough_lay::shape::place(&row, scale, self.u, g.w as f32, &mut rects);
            row.visit(&mut Now { im: self, rects: &rects.0, at: (g.x, g.y + r as i32 * row_h), k: 0 });
        }
        n as i32 * row_h
    }
    fn list<R: Row<V>, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, mut f: F) {
        let (lo, hi) = exposed(l, self.space.origin(self.u), self.space.clip);
        for r in lo..hi {
            f(r as usize).visit(&mut Now { im: self, rects: l.shape, at: (l.x, l.y + r as i32 * l.row_h), k: 0 });
        }
    }
}
