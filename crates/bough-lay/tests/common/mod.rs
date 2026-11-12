//! The differential oracle: an element's style tree laid out by `vendor/cephal`, its rects collected in the
//! element's record order.
#![allow(dead_code)]
use bough_lay::flex::{Align, Dim, Justify, Spec};
use bough_lay::tree::{Kind, MeasureFn, Node};
use cephal::style::{AlignItems, Dimension, Display, FlexDirection, JustifyContent, LengthPercentage, LengthPercentageAuto, Position};
use cephal::{AvailableSpace, MeasureInput, MeasureOutput, NodeId, Size, Style, Tree};

fn style(sp: &Spec, s: f32, flex: bool) -> Style {
    let mut st = Style::DEFAULT;
    if flex {
        st.display = Display::Flex;
        st.flex_direction = if sp.column { FlexDirection::Column } else { FlexDirection::Row };
    }
    let dim = |d: Dim| match d {
        Dim::Auto => Dimension::AUTO,
        Dim::Pt(v) => Dimension::length(v * s),
        Dim::Pct(p) => Dimension::percent(p),
    };
    st.size.width = dim(sp.w);
    st.size.height = dim(sp.h);
    if sp.min[0] > 0.0 {
        st.min_size.width = LengthPercentageAuto::length(sp.min[0] * s);
    }
    if sp.min[1] > 0.0 {
        st.min_size.height = LengthPercentageAuto::length(sp.min[1] * s);
    }
    let l = |v: f32| LengthPercentage::length(v * s);
    (st.padding.left, st.padding.top, st.padding.right, st.padding.bottom) = (l(sp.pad[0]), l(sp.pad[1]), l(sp.pad[2]), l(sp.pad[3]));
    let la = |v: f32| LengthPercentageAuto::length(v * s);
    (st.margin.left, st.margin.top, st.margin.right, st.margin.bottom) = (la(sp.margin[0]), la(sp.margin[1]), la(sp.margin[2]), la(sp.margin[3]));
    st.gap.width = l(sp.gap);
    st.gap.height = l(sp.gap);
    st.flex_grow = sp.grow;
    st.flex_shrink = sp.shrink;
    let al = |a: Align| match a {
        Align::Start => AlignItems::FLEX_START,
        Align::End => AlignItems::FLEX_END,
        Align::Center => AlignItems::CENTER,
        Align::Stretch => AlignItems::STRETCH,
    };
    st.align_items = sp.items.map(al);
    st.align_self = sp.self_align.map(al);
    st.justify_content = sp.justify.map(|j| match j {
        Justify::Start => JustifyContent::FLEX_START,
        Justify::End => JustifyContent::FLEX_END,
        Justify::Center => JustifyContent::CENTER,
        Justify::SpaceBetween => JustifyContent::SPACE_BETWEEN,
        Justify::SpaceAround => JustifyContent::SPACE_AROUND,
        Justify::SpaceEvenly => JustifyContent::SPACE_EVENLY,
    });
    if let Some((x, y)) = sp.abs {
        st.position = Position::Absolute;
        st.inset.left = la(x);
        st.inset.top = la(y);
    }
    st
}

fn build(t: &mut Tree<Option<MeasureFn>>, n: &Node, s: f32) -> Option<NodeId> {
    match &n.kind {
        Kind::Gone => None,
        Kind::Leaf(m) => Some(t.new_leaf_with_context(style(&n.spec, s, false), Some(m.clone()))),
        Kind::Flex => {
            let kids: Vec<NodeId> = n.kids.iter().filter_map(|k| build(t, k, s)).collect();
            let mut st = style(&n.spec, s, true);
            // an out-of-flow child is placed against its parent: the parent is its containing block
            if n.kids.iter().any(|k| k.spec.abs.is_some()) {
                st.position = Position::Relative;
            }
            let id = t.new_leaf_with_context(st, None);
            for k in kids {
                t.append_child(id, k);
            }
            Some(id)
        }
    }
}

fn collect(t: &Tree<Option<MeasureFn>>, n: &Node, id: Option<NodeId>, at: (f32, f32), out: &mut Vec<[i32; 4]>) {
    let Some(id) = id else {
        out.extend(std::iter::repeat_n([0; 4], n.total));
        return;
    };
    let l = t.layout(id);
    let (x, y) = (at.0 + l.location.x, at.1 + l.location.y);
    if n.leaves == 1 {
        out.push([x as i32, y as i32, (x + l.size.width) as i32, (y + l.size.height) as i32]);
    }
    let mut ids = t.children(id);
    for k in &n.kids {
        let kid = if matches!(k.kind, Kind::Gone) { None } else { ids.next() };
        collect(t, k, kid, (x, y), out);
    }
}

fn measure(ctx: &mut Option<MeasureFn>, input: MeasureInput, _: &Style) -> MeasureOutput {
    let Some(m) = ctx else { return MeasureOutput::from_size(input.known_dimensions.unwrap_or(Size::ZERO)) };
    let av = match input.available_space.width {
        AvailableSpace::Definite(a) => Some(a),
        AvailableSpace::MinContent => Some(f32::NAN),
        AvailableSpace::MaxContent => None,
    };
    let (w, h) = m(input.known_dimensions.width, input.known_dimensions.height, av);
    MeasureOutput::from_size(Size { width: w, height: h })
}

/// A list row: the row as the one child of a column `width` pixels wide. Rects relative to the row; its height.
pub fn oracle_row(n: &Node, s: f32, width: f32) -> (Vec<[i32; 4]>, i32) {
    let mut t: Tree<Option<MeasureFn>> = Tree::new();
    let row = build(&mut t, n, s).expect("a row");
    let mut st = Style::DEFAULT;
    st.display = Display::Flex;
    st.flex_direction = FlexDirection::Column;
    st.size.width = Dimension::length(width);
    let root = t.new_leaf_with_context(st, None);
    t.append_child(root, row);
    t.compute_layout_with_measure(root, Size { width: AvailableSpace::Definite(width), height: AvailableSpace::MaxContent }, measure);
    let mut out = Vec::new();
    collect(&t, n, Some(row), (0.0, 0.0), &mut out);
    (out, t.layout(root).size.height as i32)
}

/// A window root.
pub fn oracle_root(n: &Node, s: f32, avail: [f32; 2]) -> Vec<[i32; 4]> {
    let mut t: Tree<Option<MeasureFn>> = Tree::new();
    let root = build(&mut t, n, s).expect("a root");
    t.compute_layout_with_measure(root, Size { width: AvailableSpace::Definite(avail[0]), height: AvailableSpace::Definite(avail[1]) }, measure);
    let mut out = Vec::new();
    collect(&t, n, Some(root), (0.0, 0.0), &mut out);
    out
}

pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    pub fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[self.below(v.len())]
    }
}

/// A row kept in the solver across layouts: what a retained engine pays for a new width.
pub struct Warm {
    t: Tree<Option<MeasureFn>>,
    root: NodeId,
    row: NodeId,
}
impl Warm {
    pub fn new(n: &Node, s: f32, width: f32) -> Warm {
        let mut t: Tree<Option<MeasureFn>> = Tree::new();
        let row = build(&mut t, n, s).expect("a row");
        let mut st = Style::DEFAULT;
        st.display = Display::Flex;
        st.flex_direction = FlexDirection::Column;
        st.size.width = Dimension::length(width);
        let root = t.new_leaf_with_context(st, None);
        t.append_child(root, row);
        let mut w = Warm { t, root, row };
        w.relayout(width);
        w
    }
    pub fn relayout(&mut self, width: f32) -> f32 {
        self.t.patch_style(self.root, |s| s.size.width = Dimension::length(width));
        self.t.compute_layout_with_measure(self.root, Size { width: AvailableSpace::Definite(width), height: AvailableSpace::MaxContent }, measure);
        self.t.layout(self.row).size.height
    }
}
