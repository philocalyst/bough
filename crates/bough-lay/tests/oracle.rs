//! Typed layout == the general solver, rect for rect, and every answer holds over its whole interval.
//!
//! Random trees are a dynamic element (`Dyn`) that runs the *same* generic flex code as the typed elements (a
//! `Vec` of children instead of a tuple), so the arithmetic is tested over structures no fixed type list
//! covers; the scenes' rows are tested as the types they are.
mod common;
use bough_expr::U;
use bough_lay::flex::{Item, Kids, Pass, place_kids, rect, solve};
use bough_lay::tree::standin::{Line, Para};
use bough_lay::tree::{Kind, Node, measurer};
use bough_lay::*;
use common::*;

#[derive(Clone)]
enum DynKind {
    Box(Vec<Dyn>),
    Line(Line),
    Para(Para),
}
#[derive(Clone)]
struct Dyn {
    spec: Spec,
    painted: bool,
    kind: DynKind,
}
struct DynKids<'a>(&'a [Dyn]);
impl Kids for DynKids<'_> {
    type St = Vec<Item>;
    const LEAVES: usize = 0;
    const WRAPS: bool = true;
    const VARIES: bool = true;
    fn st(&self) -> Vec<Item> {
        vec![Item::NONE; self.0.len()]
    }
    fn pass<P: Pass>(&self, st: &mut Vec<Item>, p: &mut P) {
        for (k, it) in self.0.iter().zip(st) {
            p.kid(k, it);
        }
    }
    fn each<F: FnMut(&Item)>(st: &Vec<Item>, f: &mut F) {
        st.iter().for_each(f)
    }
    fn each_mut<F: FnMut(&mut Item)>(st: &mut Vec<Item>, f: &mut F) {
        st.iter_mut().for_each(f)
    }
    fn empty(&self) -> bool {
        self.0.is_empty()
    }
    fn trees(&self, out: &mut Vec<Node>) {
        for k in self.0 {
            out.push(k.tree());
        }
    }
}
impl Lay for Dyn {
    const LEAVES: usize = 0;
    const WRAPS: bool = true;
    fn spec(&self) -> &Spec {
        &self.spec
    }
    fn size(&self, cx: &mut Cx, known: [Option<V>; 2], parent: [Option<V>; 2], avail: [Av; 2], content: bool) -> [V; 2] {
        match &self.kind {
            DynKind::Box(k) => solve(&self.spec, &DynKids(k), cx, known, parent, avail, content, false).outer,
            DynKind::Line(m) => bough_lay::el::leaf_size(&self.spec, m, cx, known, parent, avail, content),
            DynKind::Para(m) => bough_lay::el::leaf_size(&self.spec, m, cx, known, parent, avail, content),
        }
    }
    fn reported(&self, cx: &mut Cx, size: [V; 2]) -> [V; 2] {
        match &self.kind {
            DynKind::Box(_) => bough_lay::flex::reported(&self.spec, cx, size),
            _ => size,
        }
    }
    fn place<O: Out + ?Sized>(&self, cx: &mut Cx, size: [V; 2], org: [V; 2], out: &mut O, mut k: usize) -> usize {
        if self.painted {
            out.put(k, rect(cx, size, org));
            k += 1;
        }
        if let DynKind::Box(kids) = &self.kind {
            let mut sol = solve(&self.spec, &DynKids(kids), cx, [Some(size[0]), Some(size[1])], [None; 2], [Av::Def(size[0]), Av::Def(size[1])], false, true);
            k = place_kids(&DynKids(kids), cx, &mut sol, org, out, k);
        }
        k
    }
    fn tree(&self) -> Node {
        let p = self.painted as usize;
        match &self.kind {
            DynKind::Box(k) => {
                let kids: Vec<Node> = k.iter().map(|k| k.tree()).collect();
                let total = p + kids.iter().map(|k| k.total).sum::<usize>();
                Node { spec: self.spec, kind: Kind::Flex, kids, leaves: p, total }
            }
            DynKind::Line(m) => Node { spec: self.spec, kind: Kind::Leaf(measurer(*m)), kids: vec![], leaves: p, total: p },
            DynKind::Para(m) => Node { spec: self.spec, kind: Kind::Leaf(measurer(m.clone())), kids: vec![], leaves: p, total: p },
        }
    }
}

const SIZES: [f32; 9] = [4.0, 7.5, 10.0, 16.0, 22.0, 36.6, 56.0, 110.0, 240.0];
const SMALL: [f32; 6] = [0.0, 0.0, 2.0, 3.8, 6.0, 12.0];

fn gen_spec(r: &mut Rng, sp: &mut Spec, container: bool, pct: bool) {
    if r.below(3) == 0 {
        sp.w = Dim::Pt(r.pick(&SIZES) * if container { 3.0 } else { 1.0 });
    } else if pct && r.below(6) == 0 {
        // a percentage of a definite width only: of an indefinite one it is the general solver's case
        // (docs/r8/LAYOUT-SEAM.md section 6)
        sp.w = Dim::Pct(r.pick(&[0.25, 0.5, 0.74, 1.0]));
    }
    if r.below(3) == 0 {
        sp.h = Dim::Pt(r.pick(&SIZES));
    }
    if r.below(5) == 0 {
        sp.grow = r.pick(&[1.0, 1.0, 2.0, 0.5]);
    }
    if r.below(5) == 0 {
        sp.shrink = r.pick(&[0.0, 0.0, 2.0]);
    }
    if r.below(8) == 0 {
        sp.min[0] = r.pick(&SIZES);
    }
    if r.below(12) == 0 {
        sp.min[1] = r.pick(&SIZES);
    }
    if r.below(4) == 0 {
        sp.margin = [r.pick(&SMALL), r.pick(&SMALL), r.pick(&SMALL), r.pick(&SMALL)];
    }
    if r.below(4) == 0 {
        sp.self_align = Some(r.pick(&[Align::Start, Align::End, Align::Center, Align::Stretch]));
    }
    if container {
        if r.below(2) == 0 {
            sp.gap = r.pick(&SMALL);
        }
        if r.below(3) == 0 {
            sp.pad = [r.pick(&SMALL), r.pick(&SMALL), r.pick(&SMALL), r.pick(&SMALL)];
        }
        if r.below(3) == 0 {
            sp.items = Some(r.pick(&[Align::Start, Align::End, Align::Center, Align::Stretch]));
        }
        if r.below(4) == 0 {
            sp.justify = Some(r.pick(&[Justify::Start, Justify::End, Justify::Center, Justify::SpaceBetween, Justify::SpaceAround, Justify::SpaceEvenly]));
        }
    }
}

fn make(r: &mut Rng, depth: u32, level: u32, wraps: bool, pct: bool) -> Dyn {
    let leaf = depth == 0 || r.below(3) == 0;
    if leaf && level > 0 {
        let mut spec = Spec::new(false);
        return match r.below(if wraps { 4 } else { 3 }) {
            0 => {
                gen_spec(r, &mut spec, false, pct);
                Dyn { spec, painted: true, kind: DynKind::Box(vec![]) }
            }
            3 => {
                let n = 2 + r.below(14);
                let words: Vec<f32> = (0..n).map(|_| r.pick(&[9.0, 13.2, 21.0, 34.5, 48.0])).collect();
                if r.below(4) == 0 {
                    spec.margin = [r.pick(&SMALL), 0.0, r.pick(&SMALL), 0.0];
                }
                Dyn { spec, painted: true, kind: DynKind::Para(Para::new(words.into(), 7.0, 36.0)) }
            }
            _ => {
                if r.below(4) == 0 {
                    spec.margin = [r.pick(&SMALL), 0.0, r.pick(&SMALL), 0.0];
                }
                if r.below(6) == 0 {
                    spec.w = Dim::Pt(r.pick(&SIZES));
                }
                Dyn { spec, painted: true, kind: DynKind::Line(Line(r.pick(&[13.2, 26.4, 52.8, 80.0, 131.5]), r.pick(&[28.0, 32.0, 36.0]))) }
            }
        };
    }
    let mut spec = Spec::new(r.below(2) == 0);
    gen_spec(r, &mut spec, true, pct);
    let n = 1 + r.below(5);
    let mut kids: Vec<Dyn> = (0..n).map(|_| make(r, depth - 1, level + 1, wraps, matches!(spec.w, Dim::Pt(_)))).collect();
    if r.below(8) == 0 {
        // one child out of flow, with its own size
        let mut k = Dyn { spec: Spec::new(false), painted: true, kind: DynKind::Box(vec![]) };
        k.spec.w = Dim::Pt(r.pick(&SIZES));
        k.spec.h = Dim::Pt(r.pick(&SIZES));
        k.spec.abs = Some((r.pick(&SMALL), r.pick(&SMALL)));
        kids.push(k);
    }
    Dyn { spec, painted: r.below(2) == 0, kind: DynKind::Box(kids) }
}

fn dump(d: &Dyn, ind: usize, out: &mut String) {
    let sp = &d.spec;
    let dflt = Spec::new(sp.column);
    let mut f = String::new();
    macro_rules! fld { ($($n:ident)*) => { $(if sp.$n != dflt.$n { f += &format!(" {}={:?}", stringify!($n), sp.$n); })* }; }
    fld!(w h min pad margin gap grow shrink items self_align justify abs);
    let kind = match &d.kind {
        DynKind::Box(_) => if sp.column { "column".to_string() } else { "row".to_string() },
        DynKind::Line(l) => format!("line {}x{}", l.0, l.1),
        DynKind::Para(p) => format!("para {:?}", &p.words[..]),
    };
    *out += &format!("{}{}{}{}\n", " ".repeat(ind), kind, if d.painted { "*" } else { "" }, f);
    if let DynKind::Box(k) = &d.kind {
        for k in k {
            dump(k, ind + 2, out);
        }
    }
}

fn typed_row<L: Lay>(l: &L, s: f32, w: f32, n: usize) -> (Vec<[i32; 4]>, i32, Valid) {
    let mut out = vec![[i32::MIN; 4]; n];
    let (h, v) = place_row(l, s, &U::default(), w, &mut out[..]);
    (out, h, v)
}

fn check_row<L: Lay>(l: &L, s: f32, w: f32, what: &str) -> Valid {
    let t = l.tree();
    let (want, wh) = oracle_row(&t, s, w);
    let (got, gh, v) = typed_row(l, s, w, t.total);
    assert!(got == want && gh == wh, "{what}: width {w}\n typed  {got:?} h {gh}\n cephal {want:?} h {wh}");
    v
}

const WIDTHS: [f32; 8] = [200.0, 333.0, 480.0, 640.0, 801.0, 1024.0, 1437.0, 2560.0];

fn random_trees(n: u64, wraps: bool) -> (u64, u64) {
    let (mut inside, mut wide) = (0u64, 0u64);
    for seed in 1..=n {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        // shallow trees first, so that the first failure is a small one
        let depth = 1 + (seed * 4 / (n + 1)) as u32;
        let d = make(&mut r, depth, 0, wraps, false);
        let t = d.tree();
        for &w in &WIDTHS {
            let mut desc = String::new();
            dump(&d, 0, &mut desc);
            let v = check_row(&d, 2.0, w, &format!("seed {seed}\n{desc}"));
            assert!(v.lo[0] <= w && w <= v.hi[0], "seed {seed}: the interval {v:?} excludes its own width {w}");
            // every width inside the interval gives the same rects and the same height
            let (base, bh, _) = typed_row(&d, 2.0, w, t.total);
            let (lo, hi) = (v.lo[0].max(w - 600.0), v.hi[0].min(w + 600.0));
            if hi - lo >= 1.0 {
                wide += 1;
            }
            for k in 0..6 {
                let x = match k {
                    0 => lo,
                    1 => hi,
                    _ => lo + (hi - lo) * (r.below(1000) as f32 / 1000.0),
                };
                let x = if hi - lo >= 1.0 { x.round().clamp(lo.ceil(), hi.floor()) } else { x };
                if !(x >= lo && x <= hi) {
                    continue;
                }
                let (got, gh, _) = typed_row(&d, 2.0, x, t.total);
                assert!(got == base && gh == bh, "seed {seed}: width {x} is inside {v:?} (from width {w}) but lays out differently\n{desc}\n at {w}: {base:?}\n at {x}: {got:?}");
                inside += 1;
            }
        }
    }
    (inside, wide)
}

#[test]
fn random_trees_match_the_solver_and_hold_over_their_intervals() {
    let n = std::env::var("LAY_TREES").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000);
    let (a, wa) = random_trees(n, false);
    let (b, wb) = random_trees(n, true);
    println!("{n} trees x 8 widths, twice: {} widths sampled inside intervals; {} answers hold over more than one pixel", a + b, wa + wb);
    assert!(wa + wb > n, "the intervals are trivial");
}

#[test]
fn probe() {
    let Ok(_) = std::env::var("LAY_PROBE") else { return };
    let para = |w: &[f32]| Dyn { spec: Spec::new(false), painted: true, kind: DynKind::Para(Para::new(w.to_vec().into(), 7.0, 36.0)) };
    for al in [None, Some(Align::End)] {
        let mut root = Dyn { spec: Spec::new(false), painted: false, kind: DynKind::Box(vec![para(&[9.0, 13.2, 21.0, 48.0, 9.0, 48.0, 48.0, 48.0, 13.2, 34.5]), para(&[34.5, 9.0, 13.2, 13.2])]) };
        root.spec.self_align = al;
        let t = root.tree();
        println!("{al:?}: cephal {:?}\n        typed  {:?} size_row {:?}", oracle_row(&t, 2.0, 100.0), typed_row(&root, 2.0, 100.0, t.total), size_row(&root, 2.0, &U::default(), 100.0).0);
    }
}

#[path = "../scenes/rows.rs"]
mod rows;

fn check_shape<T: Shape>(t: &T, s: f32, w: f32, memo: &mut Memo, what: &str) -> Valid {
    let tree = bough_lay::shape::tree(t);
    let (want, wh) = oracle_row(&tree, s, w);
    let mut got = vec![[i32::MIN; 4]; T::LEAVES];
    let (gh, v) = bough_lay::shape::place(t, s, &U::default(), w, &mut got[..]);
    assert_eq!(tree.total, T::LEAVES, "{what}: the type's record count");
    assert!(got == want && gh == wh, "{what}: width {w}\n placed {got:?} h {gh}\n cephal {want:?} h {wh}");
    // through the memo, at an offset: the same rects, shifted
    let mut kept = vec![[i32::MIN; 4]; T::LEAVES];
    let mh = memo.row(t, s, &U::default(), w, (3, 1000), &mut kept[..]);
    let shifted: Vec<[i32; 4]> = want.iter().map(|r| [r[0] + 3, r[1] + 1000, r[2] + 3, r[3] + 1000]).collect();
    assert!(kept == shifted && mh == wh, "{what}: the memo's answer at width {w}");
    v
}

#[test]
fn scene_rows_match_the_solver() {
    fn hk<T: Shape>(_: &T) -> Hk {
        T::HK
    }
    // sheet: a row of fixed boxes; one shape for every row; holds from its content width up
    let mut memo = Memo::new();
    for r in [0usize, 7, 99_999] {
        let row = rows::sheet_row(r);
        for w in [1800.0, 2311.0, 2312.0, 2313.0, 2560.0, 3000.0] {
            check_shape(&row, 2.0, w, &mut memo, "sheet row");
        }
        assert_eq!(hk(&row), Hk::Fixed);
        assert_eq!(bough_lay::shape::hash(&row), bough_lay::shape::hash(&rows::sheet_row(3)), "every sheet row is one shape");
    }
    let mut out = [[0i32; 4]; 22];
    let (h, v) = bough_lay::shape::place(&rows::sheet_row(7), 2.0, &U::default(), 2560.0, &mut out[..]);
    assert_eq!(h, 44);
    assert!(v.lo[0] <= 2313.0 && v.hi[0] == f32::INFINITY, "the sheet row holds from its content width up: {v:?}");
    // 1,000 rows at widths inside the interval: one layout
    let mut memo = Memo::new();
    for r in 0..1000 {
        memo.row(&rows::sheet_row(r), 2.0, &U::default(), 2400.0 + (r % 300) as f32, (0, r as i32 * 44), &mut out[..]);
    }
    assert_eq!((memo.misses, memo.hits), (1, 999));
    // ide: lines of 3 to 8 tokens; fixed height by the author's `.h`
    let mut rng = Rng(0x1de);
    let mut memo = Memo::new();
    for i in 0..400 {
        let n = 2 + rng.below(7);
        let toks: [(f32, f32, f32); rows::MAX_TOK] = std::array::from_fn(|_| (13.2 * (1 + rng.below(9)) as f32, rng.pick(&[0.0, 3.8, 14.0, 28.0]), rng.pick(&[0.0, 3.8])));
        let l = rows::ide_line(n, &toks, 13.2 * rng.below(6) as f32);
        for w in [300.0, 900.0, 1860.0] {
            check_shape(&l, 2.0, w, &mut memo, &format!("ide line {i}"));
        }
        assert_eq!(hk(&l), Hk::Fixed);
    }
    // chat: wrapped text in a bubble that is a fraction of a column that takes the row's free space
    let mut memo = Memo::new();
    for i in 0..400 {
        let m = rows::msg(i);
        let c = rows::chat_row(&m);
        for w in [700.0, 1100.0, 1960.0, 2400.0] {
            let v = check_shape(&c, 2.0, w, &mut memo, &format!("chat row {i}"));
            assert!(v.lo[0] <= w && w <= v.hi[0]);
        }
        assert_eq!(hk(&c), Hk::Wraps);
    }
}
