//! cephal's own `Tree` on the layout_bench shape (a column of N fixed-height cards, one of which holds a
//! measured leaf whose content is edited), outside bough: what an edit costs in cephal itself.
use cephal::style::{Dimension, Display, FlexDirection, Overflow};
use cephal::{AvailableSpace, MeasureInput, MeasureOutput, Point, Size, Style, Tree};

fn main() {
    let n: usize = std::env::var("PER").ok().and_then(|v| v.parse().ok()).unwrap_or(400);
    let clip = std::env::var_os("CLIP").is_some();
    let mut tree: Tree<u32> = Tree::new();
    tree.disable_rounding();
    let mut col = Style::DEFAULT;
    col.display = Display::Flex;
    col.flex_direction = FlexDirection::Column;
    col.size.width = Dimension::length(30.0);
    let mut card = Style::DEFAULT;
    card.display = Display::Flex;
    card.size.height = Dimension::length(6.0);
    if clip {
        card.overflow = Point { x: Overflow::Clip, y: Overflow::Clip };
    }
    let mut leaf_style = Style::DEFAULT;
    leaf_style.size.width = Dimension::length(20.0);
    let mut block = Style::DEFAULT;
    block.size = Size { width: Dimension::length(2.0), height: Dimension::length(2.0) };
    let root = tree.new_leaf(col);
    let mut edited = None;
    for i in 0..n {
        let c = tree.new_leaf(card.clone());
        if i == 0 {
            let l = tree.new_leaf_with_context(leaf_style.clone(), 1u32);
            tree.append_child(c, l);
            edited = Some(l);
        }
        let b = tree.new_leaf(block.clone());
        tree.append_child(c, b);
        tree.append_child(root, c);
    }
    let avail = Size { width: AvailableSpace::Definite(30.0), height: AvailableSpace::MaxContent };
    let measure = |ctx: &mut u32, input: MeasureInput, _: &Style| MeasureOutput::from_size(Size { width: input.known_dimensions.width.unwrap_or(20.0), height: input.known_dimensions.height.unwrap_or(*ctx as f32 * 3.0) });
    tree.compute_layout_with_measure(root, avail, measure);
    let edited = edited.unwrap();
    let snap = || bough::sys::perf::snap().instrs;
    for w in 0..20u32 {
        *tree.context_mut(edited).unwrap() = 1 + (w & 1);
        tree.invalidate(edited);
        tree.compute_layout_with_measure(root, avail, measure);
    }
    let edits = 200u32;
    // a style edit that only re-places the item inside its (fixed-size) card: align-self toggles
    let a = snap();
    for e in 0..edits {
        tree.patch_style(edited, |st| st.align_self = if e & 1 == 0 { Some(cephal::style::AlignItems::CENTER) } else { None });
        tree.compute_layout_with_measure(root, avail, measure);
    }
    println!("cephal raw: align-self edit of an item in one of {n} fixed-height cards: {:.0} instr/edit", (snap() - a) as f64 / edits as f64);
    let a = snap();
    for e in 0..edits {
        *tree.context_mut(edited).unwrap() = 1 + (e & 1);
        tree.invalidate(edited);
        tree.compute_layout_with_measure(root, avail, measure);
    }
    println!("cephal raw: column of {n} fixed-height cards, edit of a measured leaf: {:.0} instr/edit (clip={clip})", (snap() - a) as f64 / edits as f64);
}
