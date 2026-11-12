//! The shape memo (`compute::memo`, lane `layout`, round 7): a tree whose rows repeat a few random
//! templates is edited at random, inside and outside repeated subtrees, and after every edit its
//! layout (memo on) must equal, bit for bit, the same tree edited without the memo and a cold
//! layout without the memo; `changed()` must be the same set with and without. The memo must have
//! served answers, or the test proves nothing.
//!
//! `CEPHAL_SEEDS`, `CEPHAL_STEPS`, `CEPHAL_SEED_START` as in `tests/incremental.rs`. Negative controls:
//! features `mutant-memo-served` and `mutant-memo-shape` must fail it.

mod common;

use cephal::geometry::{AvailableSpace, Rect, Size};
use cephal::style::{
    AlignContent, AlignItems, Dimension, Display, FlexDirection, FlexWrap, LengthPercentage, LengthPercentageAuto, Overflow,
    Position, Style,
};
use cephal::tree::{Layout, NodeId};
use cephal::Tree;
use common::*;

/// A subtree to instantiate many times.
#[derive(Clone)]
enum Tpl {
    Leaf(Style, Option<u32>),
    Node(Style, Vec<Tpl>),
}

fn len(rng: &mut Rng, max: u64) -> Dimension {
    match rng.below(5) {
        0 | 1 => Dimension::AUTO,
        2 => Dimension::percent(rng.pick(&[0.25, 0.5, 1.0])),
        _ => Dimension::length(rng.below(max) as f32 + rng.pick(&[0.0, 0.0, 0.5, 0.25])),
    }
}

/// Styles from a small palette of shapes, so that edits make subtrees equal to others again.
fn style(rng: &mut Rng, container: bool) -> Style {
    let mut s = Style::DEFAULT;
    if container {
        s.display = rng.pick(&[Display::Flex, Display::Flex, Display::Flex, Display::Block]);
        s.flex_direction = rng.pick(&[FlexDirection::Row, FlexDirection::Row, FlexDirection::Column, FlexDirection::RowReverse]);
        s.flex_wrap = rng.pick(&[FlexWrap::NoWrap, FlexWrap::NoWrap, FlexWrap::Wrap]);
        s.align_items = rng.pick(&[None, None, Some(AlignItems::CENTER), Some(AlignItems::BASELINE), Some(AlignItems::FLEX_END)]);
        s.justify_content = rng.pick(&[None, None, Some(AlignContent::CENTER), Some(AlignContent::SPACE_BETWEEN)]);
        if rng.chance(1, 3) {
            s.gap = Size { width: LengthPercentage::length(rng.below(8) as f32), height: LengthPercentage::length(rng.below(8) as f32) };
        }
        if rng.chance(1, 3) {
            s.padding = Rect::splat(LengthPercentage::length(rng.below(10) as f32));
        }
        if rng.chance(1, 8) {
            s.overflow = cephal::geometry::Point { x: Overflow::Clip, y: Overflow::Clip };
        }
        if rng.chance(1, 10) {
            s.position = Position::Relative;
        }
    }
    s.size = Size { width: len(rng, 120), height: len(rng, 40) };
    if rng.chance(1, 4) {
        s.min_size.width = LengthPercentageAuto::length(rng.below(30) as f32);
    }
    if rng.chance(1, 5) {
        s.max_size.width = LengthPercentageAuto::length(rng.below(150) as f32 + 20.0);
    }
    if rng.chance(1, 4) {
        s.margin = Rect::splat(LengthPercentageAuto::length(rng.below(6) as f32));
    }
    s.flex_grow = rng.pick(&[0.0, 0.0, 1.0, 2.0]);
    s.flex_shrink = rng.pick(&[1.0, 1.0, 0.0]);
    // rarely, things the memo must not share
    match rng.below(40) {
        0 => s.position = Position::Absolute,
        1 => s.display = Display::None,
        2 if container => s.display = Display::Grid,
        _ => {}
    }
    s
}

fn template(rng: &mut Rng, depth: u32) -> Tpl {
    let kids = if depth >= 3 { 0 } else { rng.below(4) };
    if kids == 0 {
        let text = if rng.chance(1, 4) { Some(rng.below(8) as u32 + 1) } else { None };
        Tpl::Leaf(style(rng, false), text)
    } else {
        Tpl::Node(style(rng, true), (0..kids).map(|_| template(rng, depth + 1)).collect())
    }
}

fn make(tree: &mut Tree<Text>, t: &Tpl) -> NodeId {
    match t {
        Tpl::Leaf(s, Some(n)) => tree.new_leaf_with_context(s.clone(), Text(*n)),
        Tpl::Leaf(s, None) => tree.new_leaf(s.clone()),
        Tpl::Node(s, kids) => {
            let node = tree.new_leaf(s.clone());
            for k in kids {
                let c = make(tree, k);
                tree.append_child(node, c);
            }
            node
        }
    }
}

fn layouts(tree: &Tree<Text>, root: NodeId) -> Vec<(NodeId, Layout)> {
    let mut nodes = Vec::new();
    all_nodes(tree, root, &mut nodes);
    nodes.into_iter().map(|n| (n, *tree.unrounded_layout(n))).collect()
}

fn same(label: &str, a: &Tree<Text>, b: &Tree<Text>, root: NodeId) {
    let (x, y) = (layouts(a, root), layouts(b, root));
    assert_eq!(x.len(), y.len(), "{label}: node count");
    for ((n, la), (_, lb)) in x.iter().zip(&y) {
        // `{:?}` tells -0.0 from 0.0
        assert!(format!("{la:?}") == format!("{lb:?}"), "{label}: layout of {n:?} differs\n memo:    {la:?}\n without: {lb:?}");
    }
}

/// Applies the same edit to every tree (they hold the same node ids).
fn edit(rng: &mut Rng, trees: &mut [&mut Tree<Text>], root: NodeId, tpls: &[Tpl]) -> String {
    let mut nodes = Vec::new();
    all_nodes(trees[0], root, &mut nodes);
    let target = rng.pick(&nodes[1..]);
    let container = trees[0].child_count(target) > 0;
    match rng.below(9) {
        0 | 1 => {
            let s = style(rng, container);
            trees.iter_mut().for_each(|t| t.set_style(target, s.clone()));
            format!("set_style {target:?}")
        }
        2 => {
            let w = len(rng, 120);
            trees.iter_mut().for_each(|t| t.patch_style(target, |s| s.size.width = w));
            format!("width {target:?}")
        }
        3 => {
            let g = rng.pick(&[0.0, 1.0, 3.0]);
            trees.iter_mut().for_each(|t| t.patch_style(target, |s| s.flex_grow = g));
            format!("grow {target:?}")
        }
        4 | 5 => {
            // replace a row with a fresh instance of a template (the same ids in every tree)
            let rows: Vec<NodeId> = trees[0].children(root).collect();
            let i = rng.below(rows.len() as u64) as usize;
            let t = &tpls[rng.below(tpls.len() as u64) as usize];
            let mut made = Vec::new();
            for tr in trees.iter_mut() {
                tr.remove(rows[i]);
                let n = make(tr, t);
                tr.insert_child_at(root, i, n);
                made.push(n);
            }
            assert!(made.windows(2).all(|w| w[0] == w[1]), "trees allocate the same ids");
            format!("replace row {i}")
        }
        6 => {
            let t = &tpls[rng.below(tpls.len() as u64) as usize];
            for tr in trees.iter_mut() {
                let n = make(tr, t);
                tr.append_child(root, n);
            }
            "append row".into()
        }
        7 => {
            // measured content changes, or a leaf gains or loses its content
            let n = rng.below(9) as u32;
            let ctx = (n > 0).then_some(Text(n));
            if container {
                return "noop".into();
            }
            trees.iter_mut().for_each(|t| t.set_context(target, ctx));
            format!("content {target:?}")
        }
        _ => {
            let m = rng.pick(&[0.0, 2.0, 5.0]);
            trees.iter_mut().for_each(|t| t.patch_style(target, |s| s.margin.left = LengthPercentageAuto::length(m)));
            format!("margin {target:?}")
        }
    }
}

fn viewport(rng: &mut Rng) -> Size<AvailableSpace> {
    let w = match rng.below(6) {
        0 => AvailableSpace::MaxContent,
        1 => AvailableSpace::MinContent,
        _ => AvailableSpace::Definite(rng.below(600) as f32 + 0.5 * rng.below(2) as f32),
    };
    Size { width: w, height: if rng.chance(1, 2) { AvailableSpace::MaxContent } else { AvailableSpace::Definite(rng.below(900) as f32) } }
}

fn run(seed: u64, steps: usize) -> u64 {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let tpls: Vec<Tpl> = (0..rng.below(4) + 1).map(|_| template(&mut rng, 1)).collect();
    let mut memo: Tree<Text> = Tree::new();
    memo.set_memo(true);
    let mut plain: Tree<Text> = Tree::new();
    plain.set_memo(false);
    let mut root_style = style(&mut rng, true);
    root_style.display = Display::Flex;
    root_style.position = Position::Static;
    let rows = rng.below(20) + 4;
    let mut root = NodeId::new(0);
    for t in [&mut memo, &mut plain] {
        let r = t.new_leaf(root_style.clone());
        for _ in 0..rows {
            let k = (r.raw() as usize + t.child_count(r)) % tpls.len();
            let n = make(t, &tpls[k]);
            t.append_child(r, n);
        }
        root = r;
    }
    for step in 0..=steps {
        let what = if step > 0 { edit(&mut rng, &mut [&mut memo, &mut plain], root, &tpls) } else { "cold".into() };
        let view = viewport(&mut rng);
        let label = format!("seed {seed} step {step} ({what}, {view:?})");
        memo.compute_layout_with_measure(root, view, measure);
        plain.compute_layout_with_measure(root, view, measure);
        same(&label, &memo, &plain, root);
        let mut cold = memo.clone_cold();
        cold.compute_layout_with_measure(root, view, measure);
        same(&format!("{label} cold"), &memo, &cold, root);
        let set = |t: &Tree<Text>| {
            let mut v: Vec<NodeId> = t.changed().to_vec();
            v.sort();
            v
        };
        assert_eq!(set(&memo), set(&plain), "{label}: changed()");
    }
    memo.memo_counts().0
}

#[test]
fn memo_matches_layout_without_it() {
    let seeds: u64 = std::env::var("CEPHAL_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let steps: usize = std::env::var("CEPHAL_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
    let start: u64 = std::env::var("CEPHAL_SEED_START").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let mut hits = 0;
    for seed in start..=seeds {
        hits += run(seed, steps);
    }
    assert!(hits > 1000, "the memo served only {hits} answers: the test does not exercise it");
}
