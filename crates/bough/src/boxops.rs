//! Reactive operands on a layout box (`column`/`row`/`block` … `.opacity(op)`, `.translate(op)`,
//! `.w_op(op)`, `.h_op(op)`): what zgx's `reactive_style` drives.
//!
//! * **opacity** multiplies the box's paint alpha: one 4-byte write into its instance, a paint-only write
//!   (content-addressable);
//! * **translate** offsets the box's paint from its laid-out place: the instance position and the ink move,
//!   no layout runs (the write-back keeps adding it);
//! * **width / height** restyle the box (its spec with the new size, interned once per distinct size) and
//!   invalidate it: cephal re-lays out what that size reaches.
//!
//! Each is an ordinary operand (`IntoProp`: a constant, `sig!(..)`, `sig!(..).map(f)`), lowered once into a
//! monomorphic patch like every other property.
use crate::elem::{Apply, IntoProp, Mounter};
use crate::layout::{restyle, Dim, Spec};
use crate::rt::*;

/// Multiply a colour word's alpha by `a` (0..=1).
#[inline]
pub fn mul_alpha(c: u32, a: f32) -> u32 {
    if a >= 1.0 {
        return c;
    }
    let al = ((c >> 24) as f32 * a.max(0.0)).round() as u32;
    (c & 0x00ff_ffff) | al.min(255) << 24
}

/// A quad's paint state for the paint log: its base colour and, under an opacity operand, the alpha.
#[inline]
pub fn paint_state(base: u32, alpha: f32) -> u64 {
    base as u64 | if alpha != 1.0 { (alpha.to_bits() as u64) << 32 } else { 0 }
}

pub struct ApplyOpacity;
impl Apply for ApplyOpacity {
    type P = f32;
    fn apply(rt: &mut Rt, node: u32, v: &f32) {
        let bordered = rt.hot[node as usize].kind & !NK_HOVERED == NK_BORDER;
        let (h, inst) = (rt.handle(node), rt.inst_of(node));
        // an opacity makes the drawn colour differ from the described one: the cold record holds the latter
        let n = rt.mod_of(node);
        if n.alpha == *v {
            return;
        }
        let old_a = std::mem::replace(&mut n.alpha, *v);
        let base = n.color;
        let p = rt.inst_mut(inst);
        if bordered {
            p.color2 = mul_alpha(base, *v);
            p.color = mul_alpha(n_border(p.color, base), *v);
        } else {
            p.color = mul_alpha(base, *v);
        }
        rt.paint_write(h, paint_state(base, old_a), paint_state(base, *v));
    }
}
/// A bordered box keeps its border colour's rgb; its alpha follows the box's opacity.
#[inline]
fn n_border(cur: u32, _base: u32) -> u32 {
    cur | 0xff00_0000
}

pub struct ApplyTranslate;
impl Apply for ApplyTranslate {
    type P = (f32, f32);
    fn apply(rt: &mut Rt, node: u32, v: &(f32, f32)) {
        let s = rt.scale;
        let inst = rt.inst_of(node);
        let n = rt.mod_of(node);
        let (tx, ty) = (v.0 * s, v.1 * s);
        if n.tx == tx && n.ty == ty {
            return;
        }
        n.tx = tx;
        n.ty = ty;
        let (x, y) = (n.x + tx, n.y + ty);
        rt.inst_mut(inst).pos = [x, y];
        rt.quad_ink(node);
    }
}

/// Width / height operands address the layout node (`node` = its id), whose user spec is kept.
pub struct ApplyBoxW;
pub struct ApplyBoxH;
fn resize(rt: &mut Rt, lid: u32, w: Option<f32>, h: Option<f32>) {
    let Some(mut spec) = rt.lay.specs.get(&lid).copied() else { return };
    let old = spec;
    if let Some(w) = w {
        spec.w = Dim::Pt(w);
    }
    if let Some(h) = h {
        spec.h = Dim::Pt(h);
    }
    if spec == old {
        return;
    }
    rt.lay.specs.insert(lid, spec);
    restyle(rt, lid, &spec);
}
impl Apply for ApplyBoxW {
    type P = f32;
    fn apply(rt: &mut Rt, lid: u32, v: &f32) {
        resize(rt, lid, Some(*v), None);
    }
}
impl Apply for ApplyBoxH {
    type P = f32;
    fn apply(rt: &mut Rt, lid: u32, v: &f32) {
        resize(rt, lid, None, Some(*v));
    }
}

/// The reactive operands of one box (`()`: none; a tuple chains them).
pub trait BoxOps<V>: Sized {
    /// Does any operand restyle the box (its spec is kept)?
    const SIZE: bool;
    /// Does any operand write the box's paint (it must have paint)?
    const PAINT: bool;
    fn install(self, m: &mut Mounter<'_, V>, lid: u32, node: u32, spec: &mut Spec);
    /// Re-description: apply the operands' current values.
    fn refresh(self, rt: &mut Rt, lid: u32, node: u32);
    /// The operands' part of the box's seal (`seal.rs`); refuses by default.
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.refuse()
    }
}
impl<V> BoxOps<V> for () {
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, _: &mut S) {}
    const SIZE: bool = false;
    const PAINT: bool = false;
    #[inline]
    fn install(self, _: &mut Mounter<'_, V>, _: u32, _: u32, _: &mut Spec) {}
    #[inline]
    fn refresh(self, _: &mut Rt, _: u32, _: u32) {}
}
impl<V, A: BoxOps<V>, B: BoxOps<V>> BoxOps<V> for (A, B) {
    const SIZE: bool = A::SIZE || B::SIZE;
    const PAINT: bool = A::PAINT || B::PAINT;
    fn install(self, m: &mut Mounter<'_, V>, lid: u32, node: u32, spec: &mut Spec) {
        self.0.install(m, lid, node, spec);
        self.1.install(m, lid, node, spec);
    }
    fn refresh(self, rt: &mut Rt, lid: u32, node: u32) {
        self.0.refresh(rt, lid, node);
        self.1.refresh(rt, lid, node);
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        self.0.seal(s);
        self.1.seal(s);
    }
}

pub struct Opacity<P>(pub P);
pub struct Translate<P>(pub P);
pub struct WidthOp<P>(pub P);
pub struct HeightOp<P>(pub P);

impl<V, P: IntoProp<f32, V>> BoxOps<V> for Opacity<P> {
    const SIZE: bool = false;
    const PAINT: bool = true;
    fn install(self, m: &mut Mounter<'_, V>, _lid: u32, node: u32, _: &mut Spec) {
        let v = self.0.lower::<ApplyOpacity>(m, node);
        ApplyOpacity::apply(m.rt, node, &v);
    }
    fn refresh(self, rt: &mut Rt, _lid: u32, node: u32) {
        ApplyOpacity::apply(rt, node, &self.0.value());
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x50);
        self.0.seal(s);
    }
}
impl<V, P: IntoProp<(f32, f32), V>> BoxOps<V> for Translate<P> {
    const SIZE: bool = false;
    const PAINT: bool = true;
    fn install(self, m: &mut Mounter<'_, V>, _lid: u32, node: u32, _: &mut Spec) {
        let v = self.0.lower::<ApplyTranslate>(m, node);
        let s = m.rt.scale;
        let n = m.rt.mod_of(node);
        (n.tx, n.ty) = (v.0 * s, v.1 * s);
    }
    fn refresh(self, rt: &mut Rt, _lid: u32, node: u32) {
        ApplyTranslate::apply(rt, node, &self.0.value());
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x51);
        self.0.seal(s);
    }
}
impl<V, P: IntoProp<f32, V>> BoxOps<V> for WidthOp<P> {
    const SIZE: bool = true;
    const PAINT: bool = false;
    fn install(self, m: &mut Mounter<'_, V>, lid: u32, _node: u32, spec: &mut Spec) {
        spec.w = Dim::Pt(self.0.lower::<ApplyBoxW>(m, lid));
    }
    fn refresh(self, rt: &mut Rt, lid: u32, _node: u32) {
        ApplyBoxW::apply(rt, lid, &self.0.value());
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x52);
        self.0.seal(s);
    }
}
impl<V, P: IntoProp<f32, V>> BoxOps<V> for HeightOp<P> {
    const SIZE: bool = true;
    const PAINT: bool = false;
    fn install(self, m: &mut Mounter<'_, V>, lid: u32, _node: u32, spec: &mut Spec) {
        spec.h = Dim::Pt(self.0.lower::<ApplyBoxH>(m, lid));
    }
    fn refresh(self, rt: &mut Rt, lid: u32, _node: u32) {
        ApplyBoxH::apply(rt, lid, &self.0.value());
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x53);
        self.0.seal(s);
    }
}

/// A laid-out text's colour: the text node and its drawn lines (run headers, or a label's run) take the new
/// colour word; a paint-only write.
pub struct ApplyTextColor;
impl Apply for ApplyTextColor {
    type P = crate::color::Color;
    fn apply(rt: &mut Rt, node: u32, v: &crate::color::Color) {
        if rt.hot[node as usize].kind == 0 {
            return; // mounting: the element's text is created after its operands are lowered
        }
        if rt.hot[node as usize].kind & NK_HOVERED != 0 {
            // under variants (`flags.rs`): the colour is the base; the table shows what the flags select
            let a = rt.input.act_of_handle(rt.handle(node));
            return rt.flags_rebase(a, None, Some(v.0));
        }
        text_color(rt, node, v.0);
    }
}

/// Show colour `c` on laid-out text `node` (its lines' run headers, or its label's run): a paint write. The
/// text state's colour is what is drawn; under variants the described colour is the table's base.
pub(crate) fn text_color(rt: &mut Rt, node: u32, c: u32) {
    let (ti, h) = (rt.text_of(node), rt.handle(node));
    let v = crate::color::Color(c);
    {
        let old = rt.texts[ti as usize].color;
        if old == v.0 {
            return;
        }
        rt.texts[ti as usize].color = v.0;
        match &rt.texts[ti as usize].shape {
            crate::text::TShape::Lines { first, cap, .. } => {
                let (first, cap) = (*first, *cap);
                for r in first..first + cap {
                    rt.glyphs.runs[r as usize].color = v.0;
                }
            }
            crate::text::TShape::Label(lb) => {
                let r = lb.run as usize;
                rt.glyphs.runs[r].color = v.0;
            }
            _ => {
                // other representations take the colour when they are next placed
                return;
            }
        }
        rt.paint_write(h, old as u64, v.0 as u64);
    }
}
