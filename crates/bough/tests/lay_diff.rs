//! Incremental layout equals fresh layout, over random trees and random edits (lane `layout`, round 6).
//!
//! A three-level tree (a root box, boxes in it, leaf boxes holding an optional label, an optional wrapping
//! paragraph and an optional block) with random specs (fixed, percentage and automatic sizes, minimums,
//! padding, margins, gaps, grow and shrink, alignment, clipping, both directions) is described by plain data,
//! mounted, and edited one random step at a time through ordinary re-description: a spec field, a text (its
//! width, its wrap), a leaf's parts appearing or going, a box inserted or removed. After every step:
//!
//! * (a) the engine's incremental layout equals a cold layout of a copy of the same tree, bit for bit
//!   (every node's rect, as `f32` bits);
//! * (b) the retained records and what they draw equal a fresh mount of the same data;
//! * (c) the write-back placed every node where the layout puts it (no node is left where it was);
//! * (d) the retained layout tree is the one a fresh mount builds (shape, styles, rects, in order).
//!
//! The edits are the ones a change below a box can cut short (a label resizing inside a fixed box, content
//! moving inside a container that keeps its size) and the ones it must not (a shrinking parent reading a
//! child's content minimum, percentages, wrapping), so a cut that hides a change its parent can observe
//! fails (a). `LAY_DIFF_SEEDS` / `LAY_DIFF_STEPS` widen the run.
use bough::layout::prelude::*;
use bough::layout::{Dim, LBox, Spec};
use bough::raw::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, a: u64, b: u64) -> bool {
        self.below(b) < a
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize]
    }
}

#[derive(Clone, Copy, Debug)]
struct NodeSpec {
    col: bool,
    w: Dim,
    h: Dim,
    min_w: f32,
    min_h: f32,
    pad: f32,
    margin: f32,
    gap: f32,
    grow: f32,
    shrink: f32,
    items: Option<Align>,
    self_align: Option<Align>,
    justify: Option<Justify>,
    clip: bool,
}

impl NodeSpec {
    fn apply(&self, s: &mut Spec) {
        (s.w, s.h, s.min_w, s.min_h) = (self.w, self.h, self.min_w, self.min_h);
        s.pad = [self.pad; 4];
        s.margin = [self.margin, 0.0, self.margin * 0.5, self.margin];
        (s.gap, s.grow, s.shrink) = (self.gap, self.grow, self.shrink);
        (s.items, s.self_align, s.justify, s.clip) =
            (self.items, self.self_align, self.justify, self.clip);
    }
}

fn dim(r: &mut Rng, fixed: &[f32]) -> Dim {
    match r.below(5) {
        0 | 1 => Dim::Auto,
        2 | 3 => Dim::Pt(r.pick(fixed)),
        _ => Dim::Pct(r.pick(&[0.25, 0.5, 1.0])),
    }
}

fn node_spec(r: &mut Rng, big: bool) -> NodeSpec {
    let sizes: &[f32] = if big {
        &[120.0, 200.0, 360.0, 640.0]
    } else {
        &[0.0, 8.0, 24.0, 32.0, 60.0, 90.0]
    };
    NodeSpec {
        col: r.chance(1, 2),
        w: dim(r, sizes),
        h: dim(r, sizes),
        min_w: if r.chance(1, 6) {
            r.pick(&[10.0, 40.0])
        } else {
            0.0
        },
        min_h: if r.chance(1, 6) {
            r.pick(&[10.0, 40.0])
        } else {
            0.0
        },
        pad: r.pick(&[0.0, 0.0, 2.0, 5.5]),
        margin: r.pick(&[0.0, 0.0, 0.0, 3.0]),
        gap: r.pick(&[0.0, 0.0, 4.0]),
        grow: r.pick(&[0.0, 0.0, 1.0, 2.0]),
        shrink: r.pick(&[1.0, 1.0, 0.0]),
        items: r.pick(&[
            None,
            None,
            Some(Align::Start),
            Some(Align::Center),
            Some(Align::End),
            Some(Align::Stretch),
        ]),
        self_align: r.pick(&[None, None, None, Some(Align::Center), Some(Align::Stretch)]),
        justify: r.pick(&[
            None,
            None,
            Some(Justify::Center),
            Some(Justify::SpaceBetween),
            Some(Justify::End),
        ]),
        clip: r.chance(1, 5),
    }
}

const WORDS: [&str; 12] = [
    "a", "row", "label", "with", "some", "longer", "words", "wrapping", "at", "the", "box", "edge",
];

fn words(r: &mut Rng, n: u64) -> String {
    let n = 1 + r.below(n);
    (0..n).map(|_| r.pick(&WORDS)).collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Debug)]
struct Leaf {
    spec: NodeSpec,
    label: Option<String>,
    para: Option<String>,
    /// The paragraph's width: automatic (it fills) or fixed.
    para_w: Option<f32>,
    block: Option<(f32, f32)>,
}

#[derive(Clone, Debug)]
struct Mid {
    spec: NodeSpec,
    kids: Vec<Leaf>,
}

#[derive(Clone, Debug)]
struct Tree {
    root: NodeSpec,
    mids: Vec<Mid>,
}

fn leaf(r: &mut Rng) -> Leaf {
    let mut l = Leaf {
        spec: node_spec(r, false),
        label: r.chance(2, 3).then(|| words(r, 3)),
        para: r.chance(1, 3).then(|| words(r, 14)),
        para_w: if r.chance(1, 2) {
            Some(r.pick(&[40.0, 90.0, 150.0]))
        } else {
            None
        },
        block: r
            .chance(1, 2)
            .then(|| (r.pick(&[0.0, 6.0, 20.0, 50.0]), r.pick(&[0.0, 6.0, 20.0]))),
    };
    if l.label.is_none() && l.para.is_none() && l.block.is_none() {
        l.block = Some((10.0, 10.0));
    }
    l
}

fn mid(r: &mut Rng) -> Mid {
    let n = 1 + r.below(4);
    Mid {
        spec: node_spec(r, false),
        kids: (0..n).map(|_| leaf(r)).collect(),
    }
}

fn tree(r: &mut Rng) -> Tree {
    let mut root = node_spec(r, true);
    root.col = r.chance(3, 4);
    let n = 1 + r.below(6);
    Tree {
        root,
        mids: (0..n).map(|_| mid(r)).collect(),
    }
}

fn boxed<Ch>(s: &NodeSpec, ch: Ch, tint: u32) -> LBox<Color, Ch> {
    let mut b = if s.col { column(ch) } else { row(ch) };
    s.apply(b.spec_mut());
    b.bg(rgb(tint))
}

fn leaf_el(l: &Leaf) -> impl Element<Tree> + '_ {
    let label = l.label.as_deref().map(|t| text(t));
    let para = l.para.as_deref().map(|t| {
        let p = para(t).font(12.0, 16.0);
        match l.para_w {
            Some(w) => p.w(w),
            None => p.grow(1.0),
        }
    });
    let block = l.block.map(|(w, h)| block(w, h).bg(rgb(0x335577)));
    boxed(&l.spec, (label, para, block), 0x202830)
}

fn mid_el(m: &Mid) -> impl Element<Tree> + '_ {
    boxed(&m.spec, each(m.kids.iter().map(leaf_el)), 0x404850)
}

impl View for Tree {
    fn view(&self) -> impl Element<Self> + '_ {
        boxed(&self.root, each(self.mids.iter().map(mid_el)), 0x101418)
    }
}

/// One random edit; a description for the failure message.
fn edit(r: &mut Rng, t: &mut Tree) -> String {
    let mi = r.below(t.mids.len() as u64) as usize;
    let m = &mut t.mids[mi];
    let ki = r.below(m.kids.len() as u64) as usize;
    match r.below(14) {
        // the common GUI edit: a label's text (its width) changes
        0..=3 => {
            let l = &mut m.kids[ki];
            l.label = Some(words(r, 3));
            format!("label {mi}.{ki}")
        }
        4 => {
            let l = &mut m.kids[ki];
            l.para = Some(words(r, 14));
            format!("para {mi}.{ki}")
        }
        5 => {
            let l = &mut m.kids[ki];
            match r.below(3) {
                0 => {
                    l.label = if l.label.is_some() {
                        None
                    } else {
                        Some(words(r, 2))
                    }
                }
                1 => {
                    l.para = if l.para.is_some() {
                        None
                    } else {
                        Some(words(r, 8))
                    }
                }
                _ => {
                    l.block = if l.block.is_some() {
                        None
                    } else {
                        Some((r.pick(&[4.0, 30.0]), 9.0))
                    }
                }
            }
            format!("parts {mi}.{ki}")
        }
        6 => {
            let l = &mut m.kids[ki];
            l.block = Some((
                r.pick(&[0.0, 6.0, 20.0, 50.0, 300.0]),
                r.pick(&[0.0, 6.0, 20.0, 80.0]),
            ));
            format!("block size {mi}.{ki}")
        }
        7 | 8 => {
            m.kids[ki].spec = node_spec(r, false);
            format!("leaf spec {mi}.{ki}")
        }
        9 => {
            m.spec = node_spec(r, false);
            format!("mid spec {mi}")
        }
        10 => {
            let col = t.root.col;
            t.root = node_spec(r, true);
            t.root.col = col || r.chance(1, 2);
            "root spec".into()
        }
        11 => {
            if m.kids.len() > 1 && r.chance(1, 2) {
                m.kids.remove(ki);
                format!("remove leaf {mi}.{ki}")
            } else {
                let l = leaf(r);
                m.kids.insert(ki, l);
                format!("insert leaf {mi}.{ki}")
            }
        }
        12 => {
            if t.mids.len() > 1 && r.chance(1, 2) {
                t.mids.remove(mi);
                format!("remove mid {mi}")
            } else {
                let x = mid(r);
                t.mids.insert(mi, x);
                format!("insert mid {mi}")
            }
        }
        _ => {
            let l = &mut m.kids[ki];
            l.para_w = if l.para_w.is_some() {
                None
            } else {
                Some(r.pick(&[40.0, 90.0]))
            };
            format!("para width {mi}.{ki}")
        }
    }
}

type Item = (u8, [f32; 4], u32, u32, f32, f32, [u16; 2]);
type Dump = Vec<(u8, u16, [i16; 4], Vec<Item>)>;
fn dump(app: &App<Tree>) -> Dump {
    app.rt
        .bough
        .records()
        .iter()
        .map(|r| {
            let items = if r.ink.is_empty() {
                Vec::new()
            } else {
                app.rt.expand_payload(r.payload)
            };
            (r.kind, r.depth, r.ink.0, items)
        })
        .collect()
}

/// Every layout node's rect under `root`, as bits, by node id.
fn rects(app: &App<Tree>, root: u32) -> Vec<(u32, [u32; 4])> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        let (x, y, w, h) = app.rt.lay.eng.rect(n);
        out.push((n, [x.to_bits(), y.to_bits(), w.to_bits(), h.to_bits()]));
        app.rt.lay.eng.children_into(&app.rt.bough, n, &mut stack);
    }
    out.sort_by_key(|e| e.0);
    out
}

fn check(app: &App<Tree>, (w, h): (f32, f32), what: &str) {
    let root = app.rt.lay.first_root();
    let li = &app.rt.lay.info[root as usize];
    let space = |v: f32| {
        if v.is_nan() {
            cephal::AvailableSpace::MaxContent
        } else {
            cephal::AvailableSpace::Definite(v)
        }
    };
    let avail = cephal::Size {
        width: space(li.avail_w),
        height: space(li.avail_h),
    };
    let cold: Vec<(u32, [u32; 4])> = app
        .rt
        .lay
        .eng
        .cold_rects(&app.rt.bough, root, avail)
        .into_iter()
        .map(|(n, (x, y, w, h))| (n, [x.to_bits(), y.to_bits(), w.to_bits(), h.to_bits()]))
        .collect();
    let now = rects(app, root);
    assert_eq!(now.len(), cold.len(), "{what}: node count");
    for (a, b) in now.iter().zip(&cold) {
        let f = |v: [u32; 4]| v.map(f32::from_bits);
        assert!(
            a == b,
            "{what}: node {} incremental {:?} != cold {:?}",
            a.0,
            f(a.1),
            f(b.1)
        );
    }
    // (c) the write-back placed every node where the layout puts it (its record of the last absolute rect
    // written equals the sum of the rects up its parent chain)
    let mut stack = vec![(root, 0.0f32, 0.0f32)];
    while let Some((n, px, py)) = stack.pop() {
        let (x, y, w, h) = app.rt.lay.eng.rect(n);
        let (ax, ay) = (px + x, py + y);
        let li = &app.rt.lay.info[n as usize];
        assert!(
            li.written && (li.ax, li.ay, li.aw, li.ah) == (ax, ay, w, h),
            "{what}: node {n} (kind {}, info parent {}, rect {:?}) written at {:?}, laid out at {:?}",
            li.kind,
            li.parent as i64,
            (x, y, w, h),
            (li.ax, li.ay, li.aw, li.ah),
            (ax, ay, w, h)
        );
        let mut kids = Vec::new();
        app.rt.lay.eng.children_into(&app.rt.bough, n, &mut kids);
        stack.extend(kids.into_iter().map(|k| (k, ax, ay)));
    }
    let fresh = App::new(
        (*app.owner).clone(),
        Config::new(w, h, 1.0, FONT.to_vec()),
        false,
    );
    // (d) the retained tree is the tree a fresh mount builds: same shape, same styles, same rects, in order
    let dfs = |a: &App<Tree>| {
        let mut out = Vec::new();
        let mut stack = vec![a.rt.lay.first_root()];
        while let Some(n) = stack.pop() {
            out.push((
                format!("{:?}", a.rt.lay.eng.style(n)),
                a.rt.lay.eng.rect(n),
                a.rt.lay.eng.kid_count(n),
            ));
            let mut kids = Vec::new();
            a.rt.lay.eng.children_into(&a.rt.bough, n, &mut kids);
            stack.extend(kids.into_iter().rev());
        }
        out
    };
    let (ra, rb) = (dfs(app), dfs(&fresh));
    for (i, (p, q)) in ra.iter().zip(&rb).enumerate() {
        assert!(
            p.2 == q.2,
            "{what}: dfs node {i}: {} children retained, {} fresh",
            p.2,
            q.2
        );
        assert!(
            p.0 == q.0,
            "{what}: dfs node {i}: style retained {} != fresh {}",
            p.0,
            q.0
        );
        assert!(
            p.1 == q.1,
            "{what}: dfs node {i}: rect retained {:?} != fresh {:?}",
            p.1,
            q.1
        );
    }
    assert_eq!(ra.len(), rb.len(), "{what}: layout node count");
    let (x, y) = (dump(app), dump(&fresh));
    assert_eq!(x.len(), y.len(), "{what}: record count");
    let recs = app.rt.bough.records();
    for (i, (p, q)) in x.iter().zip(&y).enumerate() {
        if p != q {
            let h = recs[i].handle;
            // an element's id is its record's handle index (one index space)
            let ti = app.rt.text_of(h) as usize;
            let t = &app.rt.texts[ti];
            let shape = match &t.shape {
                bough::text::TShape::Label(_) => "label",
                bough::text::TShape::Lines { .. } => "lines",
                bough::text::TShape::Glyphs { .. } => "glyphs",
                bough::text::TShape::Set(_) => "set",
                bough::text::TShape::None => "none",
            };
            let li = &app.rt.lay.info[h as usize];
            panic!(
                "{what}: record {i} (node {h}, {shape} {:?}, ox {} oy {}, written at {:?}) retained {:?} != fresh {:?}",
                t.text,
                t.ox,
                t.oy,
                (li.ax, li.ay, li.aw, li.ah),
                (p.0, p.1, p.2, p.3.first()),
                (q.0, q.1, q.2, q.3.first())
            );
        }
    }
}

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
fn incremental_layout_equals_fresh_over_random_trees_and_edits() {
    let (seeds, steps) = (env("LAY_DIFF_SEEDS", 120), env("LAY_DIFF_STEPS", 25));
    let start = env("LAY_DIFF_SEED_START", 1);
    let mut cases = 0;
    for seed in start..start + seeds {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let size = (r.pick(&[320.0, 500.0, 800.0]), r.pick(&[240.0, 600.0]));
        let mut app = App::new(
            tree(&mut r),
            Config::new(size.0, size.1, 1.0, FONT.to_vec()),
            false,
        );
        app.turn(|_| {});
        check(&app, size, &format!("seed {seed} initial"));
        for step in 0..steps {
            let mut what = String::new();
            let trace = std::env::var("LAY_DIFF_TRACE")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                == Some(seed);
            if trace {
                eprintln!("seed {seed} before step {step}: {:#?}", app.owner);
            }
            app.turn(|t| what = edit(&mut r, t));
            if trace {
                eprintln!("step {step}: {what}");
            }
            check(&app, size, &format!("seed {seed} step {step} ({what})"));
            cases += 1;
        }
    }
    eprintln!("lay_diff: {cases} edits checked");
}
