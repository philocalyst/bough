//! Reflection gates (no GPU; the pixel gate is comparisons/bough-suite/tests/reflect.rs):
//! * the compile-time schema of a corpus (structs, tuple structs, tuples, enums with unit / tuple / struct
//!   variants, generics, arrays, nesting, reactive cells, std containers, types without `Debug`) equals a
//!   hand-written expectation (offsets from `offset_of!`, types named by `type_name::<T>()`);
//! * a value walk reads what direct reads read, and edits through reflection equal direct writes;
//! * a type with no `Debug` / `Edit` degrades to structure (walked, not shown, not editable);
//! * an app edited through `reflect_set` equals the same app written directly and a fresh mount.
#![feature(offset_of_enum)]
#![allow(dead_code)]
use bough::layout::prelude::*;
use bough::prelude::*;
use bough::reflect::{fields_of, schema, set_in, Kind};
use std::any::type_name;
use std::mem::{offset_of, size_of};

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Unit;
#[derive(Debug, Clone, PartialEq)]
struct Pair(u8, f64);
struct Gen<T> {
    a: T,
    b: (u8, T),
    c: [T; 3],
}
#[derive(Debug)]
enum Shape {
    Dot,
    Tup(u8, String),
    Rec { r: f32, label: Option<Vec<u16>> },
}
struct NoDebug {
    x: u32,
    inner: Unit,
}
struct Corpus {
    g: Gen<u16>,
    s: Shape,
    nd: NoDebug,
    sig: Signal<Pair>,
    list: List<Pair>,
    v: Vec<NoDebug>,
    o: Option<Pair>,
    u: Unit,
    t: (bool, char),
}

/// One expected dump line: `depth`, `name`, kind, offset, the node's type `T` (size and name), the
/// container count `n` (opened nodes only), flags, the head offset.
fn line<T>(depth: usize, name: &str, kind: Kind, off: usize, n: Option<usize>, flags: &str, head: Option<isize>) -> String {
    variant_line(depth, name, kind, off, size_of::<T>(), type_name::<T>(), n, flags, head)
}
#[allow(clippy::too_many_arguments)]
fn variant_line(depth: usize, name: &str, kind: Kind, off: usize, size: usize, tn: &str, n: Option<usize>, flags: &str, head: Option<isize>) -> String {
    let mut s = format!("{}{}: {:?} @{} [{}] {}", "  ".repeat(depth), name, kind, off, size, tn);
    if let Some(n) = n {
        s += &format!(" n={n}");
    }
    for f in flags.split_whitespace() {
        s += " ";
        s += f;
    }
    if let Some(h) = head {
        s += &format!(" head@{h}");
    }
    s
}

#[test]
fn schema_of_a_corpus_equals_the_hand_written_expectation() {
    use Kind::*;
    let sig_value = size_of::<Signal<Pair>>() - size_of::<Pair>(); // `Signal` is `repr(C)`: head, then value
    let e = [
        line::<Corpus>(0, "", Struct, 0, Some(9), "", None),
        line::<Gen<u16>>(1, "g", Struct, offset_of!(Corpus, g), Some(3), "", None),
        line::<u16>(2, "a", Int, offset_of!(Gen<u16>, a), None, "debug edit", None),
        line::<(u8, u16)>(2, "b", Tuple, offset_of!(Gen<u16>, b), Some(2), "debug", None),
        line::<u8>(3, "0", Int, offset_of!((u8, u16), 0), None, "debug edit", None),
        line::<u16>(3, "1", Int, offset_of!((u8, u16), 1), None, "debug edit", None),
        line::<[u16; 3]>(2, "c", Array, offset_of!(Gen<u16>, c), Some(3), "debug", None),
        line::<u16>(3, "[]", Int, 0, None, "debug edit", None),
        line::<Shape>(1, "s", Enum, offset_of!(Corpus, s), Some(3), "debug", None),
        variant_line(2, "Dot", Variant, 0, size_of::<Shape>(), "?", Some(0), "", None),
        variant_line(2, "Tup", Variant, 0, size_of::<Shape>(), "?", Some(2), "", None),
        line::<u8>(3, "0", Int, offset_of!(Shape, Tup.0), None, "debug edit", None),
        line::<String>(3, "1", Struct, offset_of!(Shape, Tup.1), None, "debug edit opaque", None),
        variant_line(2, "Rec", Variant, 0, size_of::<Shape>(), "?", Some(2), "", None),
        line::<f32>(3, "r", Float, offset_of!(Shape, Rec.r), None, "debug edit", None),
        line::<Option<Vec<u16>>>(3, "label", Enum, offset_of!(Shape, Rec.label), None, "debug items opaque", None),
        line::<NoDebug>(1, "nd", Struct, offset_of!(Corpus, nd), Some(2), "", None),
        line::<u32>(2, "x", Int, offset_of!(NoDebug, x), None, "debug edit", None),
        line::<Unit>(2, "inner", Struct, offset_of!(NoDebug, inner), Some(0), "", None),
        line::<Pair>(1, "sig", Struct, offset_of!(Corpus, sig) + sig_value, Some(2), "debug", Some(-(sig_value as isize))),
        line::<u8>(2, "0", Int, offset_of!(Pair, 0), None, "debug edit", None),
        line::<f64>(2, "1", Float, offset_of!(Pair, 1), None, "debug edit", None),
        line::<List<Pair>>(1, "list", Struct, offset_of!(Corpus, list), None, "items opaque", Some(0)),
        line::<Vec<NoDebug>>(1, "v", Struct, offset_of!(Corpus, v), None, "items opaque", None),
        line::<Option<Pair>>(1, "o", Enum, offset_of!(Corpus, o), None, "debug items opaque", None),
        line::<Unit>(1, "u", Struct, offset_of!(Corpus, u), Some(0), "", None),
        line::<(bool, char)>(1, "t", Tuple, offset_of!(Corpus, t), Some(2), "debug", None),
        line::<bool>(2, "0", Bool, offset_of!((bool, char), 0), None, "debug edit", None),
        line::<char>(2, "1", Char, offset_of!((bool, char), 1), None, "debug edit", None),
    ];
    let got = schema::<Corpus>().dump();
    let got: Vec<&str> = got.lines().collect();
    for (i, (g, w)) in got.iter().zip(&e).enumerate() {
        assert_eq!(g, w, "schema line {i}");
    }
    assert_eq!(got.len(), e.len(), "schema length");
    assert!(!schema::<Corpus>().truncated);

    // generics: a second instantiation has its own offsets and types
    let g = schema::<Gen<Pair>>().dump();
    let want = [
        line::<Gen<Pair>>(0, "", Struct, 0, Some(3), "", None),
        line::<Pair>(1, "a", Struct, offset_of!(Gen<Pair>, a), Some(2), "debug", None),
        line::<u8>(2, "0", Int, offset_of!(Pair, 0), None, "debug edit", None),
        line::<f64>(2, "1", Float, offset_of!(Pair, 1), None, "debug edit", None),
        line::<(u8, Pair)>(1, "b", Tuple, offset_of!(Gen<Pair>, b), Some(2), "debug", None),
        line::<u8>(2, "0", Int, offset_of!((u8, Pair), 0), None, "debug edit", None),
        line::<Pair>(2, "1", Struct, offset_of!((u8, Pair), 1), Some(2), "debug", None),
        line::<u8>(3, "0", Int, offset_of!(Pair, 0), None, "debug edit", None),
        line::<f64>(3, "1", Float, offset_of!(Pair, 1), None, "debug edit", None),
        line::<[Pair; 3]>(1, "c", Array, offset_of!(Gen<Pair>, c), Some(3), "debug", None),
        line::<Pair>(2, "[]", Struct, 0, Some(2), "debug", None),
        line::<u8>(3, "0", Int, offset_of!(Pair, 0), None, "debug edit", None),
        line::<f64>(3, "1", Float, offset_of!(Pair, 1), None, "debug edit", None),
    ];
    assert_eq!(g.lines().collect::<Vec<_>>(), want);
}

fn corpus() -> Corpus {
    Corpus {
        g: Gen { a: 1, b: (2, 3), c: [4, 5, 6] },
        s: Shape::Rec { r: 1.5, label: Some(vec![7, 8]) },
        nd: NoDebug { x: 9, inner: Unit },
        sig: Signal::new(Pair(10, 11.5)),
        list: List::from_iter_in([Pair(1, 0.5), Pair(2, 1.5), Pair(3, 2.5)]),
        v: vec![NoDebug { x: 20, inner: Unit }, NoDebug { x: 21, inner: Unit }],
        o: Some(Pair(30, 0.25)),
        u: Unit,
        t: (true, 'q'),
    }
}

#[test]
fn a_value_walk_reads_what_direct_reads_read() {
    let c = corpus();
    let f = fields_of(&c);
    let get = |p: &str| f.iter().find(|x| x.path == p).unwrap_or_else(|| panic!("no {p:?} in {:?}", f.iter().map(|x| &x.path).collect::<Vec<_>>()));
    let paths: Vec<&str> = f.iter().map(|x| x.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "", "g", "g.a", "g.b", "g.b.0", "g.b.1", "g.c", "g.c[0]", "g.c[1]", "g.c[2]", "s", "nd", "nd.x", "nd.inner", "sig", "sig.0", "sig.1", "list", "list[0]", "list[0].0",
            "list[0].1", "list[1]", "list[1].0", "list[1].1", "list[2]", "list[2].0", "list[2].1", "v", "v[0]", "v[0].x", "v[0].inner", "v[1]", "v[1].x", "v[1].inner", "o", "o[0]",
            "o[0].0", "o[0].1", "u", "t", "t.0", "t.1",
        ]
    );
    let val = |p: &str| get(p).value.clone();
    assert_eq!(val("g.c[1]"), Some(format!("{:?}", c.g.c[1])));
    assert_eq!(val("g.b.1"), Some(format!("{:?}", c.g.b.1)));
    assert_eq!(val("s"), Some(format!("{:?}", c.s)));
    assert_eq!(val("sig.1"), Some(format!("{:?}", c.sig.1)));
    assert_eq!(val("list[2].1"), Some(format!("{:?}", c.list.get(2).unwrap().1)));
    assert_eq!(val("o"), Some(format!("{:?}", c.o)));
    assert_eq!(val("v[1].x"), Some(format!("{:?}", c.v[1].x)));
    assert_eq!(val("t.1"), Some("'q'".into()));
    // no `Debug`: walked by structure, no value, not editable; its `Edit` fields still are
    for p in ["nd", "nd.inner", "v", "v[0]", "u", ""] {
        assert_eq!(get(p).value, None, "{p}");
        assert!(!get(p).editable, "{p}");
    }
    assert!(get("nd.x").editable && get("v[0].x").editable);
    assert!(get("sig").reactive && get("list").reactive && !get("sig.0").reactive);
    assert_eq!(get("list").type_name, type_name::<List<Pair>>());
}

#[test]
fn edits_through_reflection_equal_direct_writes() {
    let mut a = corpus();
    let mut b = corpus();
    let edits: [(&str, &str); 11] = [
        ("g.a", "70"),
        ("g.b.1", "90"),
        ("g.c[2]", "50"),
        ("sig.1", "2.25"),
        ("sig.0", "200"),
        ("list[1].0", "33"),
        ("o[0].1", "1.25"),
        ("t.1", "z"),
        ("nd.x", "11"),
        ("v[1].x", "44"),
        ("t.0", "false"),
    ];
    for (p, v) in edits {
        assert!(set_in(&mut a, p, v), "{p} = {v}");
    }
    b.g.a = 70;
    b.g.b.1 = 90;
    b.g.c[2] = 50;
    b.sig.1 = 2.25;
    b.sig.0 = 200;
    b.list.edit(1).0 = 33;
    b.o.as_mut().unwrap().1 = 1.25;
    b.t.1 = 'z';
    b.nd.x = 11;
    b.v[1].x = 44;
    b.t.0 = false;
    let show = |c: &Corpus| fields_of(c).into_iter().map(|f| (f.path, f.value)).collect::<Vec<_>>();
    assert_eq!(show(&a), show(&b));
    assert_eq!(bough::list::log_of(&a.list).0.len(), bough::list::log_of(&b.list).0.len(), "a list row edit is logged like a direct `edit`");
    // refused: enums are not opened, out of range, no `Edit`, bad text, unknown names, malformed paths
    for (p, v) in [("s", "x"), ("s.Rec.r", "1"), ("list[3].0", "1"), ("g.c[3]", "1"), ("nd", "1"), ("g.a", "x"), ("g.zz", "1"), ("g.c[", "1"), ("g.c[1]x", "1"), ("u", "")] {
        assert!(!set_in(&mut a, p, v), "{p} = {v} should be refused");
    }
    assert_eq!(show(&a), show(&b), "refused edits change nothing");
}

#[test]
fn shared_ownership_is_read_only() {
    struct S {
        rc: std::rc::Rc<Pair>,
        cell: std::cell::Cell<u32>,
        rf: std::cell::RefCell<Pair>,
        bx: Box<Pair>,
    }
    let mut s = S { rc: std::rc::Rc::new(Pair(1, 1.0)), cell: std::cell::Cell::new(5), rf: std::cell::RefCell::new(Pair(2, 2.0)), bx: Box::new(Pair(3, 3.0)) };
    let f = fields_of(&s);
    assert!(f.iter().any(|x| x.path == "rc[0].0" && !x.editable));
    assert!(!set_in(&mut s, "rc[0].0", "9"));
    assert!(set_in(&mut s, "cell[0]", "6") && s.cell.get() == 6);
    assert!(set_in(&mut s, "rf[0].1", "6.5") && s.rf.borrow().1 == 6.5);
    assert!(set_in(&mut s, "bx[0].0", "7") && s.bx.0 == 7);
    // a `RefCell` borrowed right now is not opened
    let g = s.rf.borrow_mut();
    std::mem::forget(g);
    assert!(!fields_of(&s).iter().any(|x| x.path.starts_with("rf[")));
    assert!(!set_in(&mut s, "rf[0].1", "1"));
}

// ------------------------------------------------------------------ an app

struct Theme {
    pad: f32,
    color: u32,
}
struct Item {
    label: Signal<String>,
    n: Signal<u32>,
}
impl View for Item {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        row((label(sig!(self.label)).font(12.0, 16.0), label(sig!(self.n).map(|n| format!("#{n}"))).font(10.0, 14.0))).gap(4.0)
    }
}
struct Root {
    title: Signal<String>,
    w: Signal<f32>,
    theme: Signal<Theme>,
    items: List<Item>,
}
impl View for Root {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            label(sig!(self.title)).font(14.0, 18.0),
            block(10.0, 12.0).bg(rgb(0x3366ff)).w_op(sig!(self.w)),
            block(30.0, 8.0).bg(sig!(self.theme).map(|t| rgb(t.color))).h_op(sig!(self.theme).map(|t| 4.0 + t.pad)),
            list(sig!(self.items)).gap(2.0),
        ))
        .gap(3.0)
        .w(260.0)
    }
}
fn root() -> Root {
    Root {
        title: Signal::new("title".into()),
        w: Signal::new(10.0),
        theme: Signal::new(Theme { pad: 2.0, color: 0x00ff00 }),
        items: List::from_iter_in((0..5).map(|i| Item { label: Signal::new(format!("row {i}")), n: Signal::new(i) })),
    }
}
fn copy(r: &Root) -> Root {
    Root {
        title: Signal::new((*r.title).clone()),
        w: Signal::new(*r.w),
        theme: Signal::new(Theme { pad: r.theme.pad, color: r.theme.color }),
        items: List::from_iter_in(r.items.iter().map(|i| Item { label: Signal::new((*i.label).clone()), n: Signal::new(*i.n) })),
    }
}
type Item7 = (u8, [f32; 4], u32, u32, f32, f32, [u16; 2]);
fn dump(app: &App<impl View>) -> Vec<(u8, u16, [i16; 4], Vec<Item7>)> {
    app.rt.bough.records().iter().map(|r| (r.kind, r.depth, r.ink.0, if r.ink.is_empty() { Vec::new() } else { app.rt.expand_payload(r.payload) })).collect()
}

/// Reflection edits of root signals, a field of a signal's struct value, and list rows' signals: the
/// retained records equal the same edits written directly, and a fresh mount.
#[test]
fn app_edits_through_reflection_equal_direct_writes_and_a_fresh_mount() {
    let cfg = || Config::new(300.0, 400.0, 1.0, FONT.to_vec());
    let mut a = App::new(root(), cfg(), false);
    let mut b = App::new(root(), cfg(), false);
    a.frame();
    b.frame();
    let mut r = 0x2545_f491_4f6c_dd1du64;
    for step in 0..60 {
        r ^= r << 13;
        r ^= r >> 7;
        r ^= r << 17;
        let x = r;
        let i = (x >> 8) as usize % 5;
        let (path, text) = match x % 6 {
            0 => ("title".to_string(), format!("t{}", x % 1000)),
            1 => ("w".to_string(), format!("{}", 4 + (x >> 12) % 80)),
            2 => ("theme.pad".to_string(), format!("{}", (x >> 12) % 20)),
            3 => ("theme.color".to_string(), format!("{}", (x >> 12) as u32 & 0xffffff)),
            4 => (format!("items[{i}].label"), format!("L{}", (x >> 20) % 10000)),
            _ => (format!("items[{i}].n"), format!("{}", (x >> 20) % 500)),
        };
        assert!(a.reflect_set(&path, &text), "step {step}: {path} = {text}");
        b.turn(|v| match x % 6 {
            0 => *v.title = text.clone(),
            1 => *v.w = text.parse().unwrap(),
            2 => v.theme.pad = text.parse().unwrap(),
            3 => v.theme.color = text.parse().unwrap(),
            4 => *v.items.edit(i).label = text.clone(),
            _ => *v.items.edit(i).n = text.parse().unwrap(),
        });
        a.frame();
        b.frame();
        assert!(dump(&a) == dump(&b), "step {step} ({path} = {text}): reflection != direct write");
        let mut f = App::new(copy(&a.owner), cfg(), false);
        f.frame();
        assert!(dump(&a) == dump(&f), "step {step} ({path} = {text}): retained != fresh mount");
    }
    // the inspector's read side over the same app
    let f = a.reflect_fields();
    assert!(f.iter().any(|x| x.path == "theme.pad" && x.editable));
    assert!(f.iter().any(|x| x.path == "items[4].label" && x.reactive));
    let recs = a.reflect_records();
    assert!(recs.iter().any(|r| r.element.is_some_and(|e| e.contains("Para"))));
    let h = recs.iter().find(|r| r.layout.is_some() && r.handle != 0).unwrap().handle;
    let st = a.reflect_style(h).unwrap();
    assert!(st.iter().any(|x| x.path == "flex_grow" && x.editable));
}

/// A style edit through reflection relays out like the equivalent `Spec`.
#[test]
fn style_edit_through_reflection_relays_out() {
    struct V;
    impl View for V {
        const FIXED: bool = true;
        fn view(&self) -> impl Element<Self> + '_ {
            row((block(20.0, 10.0).bg(rgb(0xff0000)), block(20.0, 10.0).bg(rgb(0x00ff00)))).w(200.0)
        }
    }
    struct W;
    impl View for W {
        const FIXED: bool = true;
        fn view(&self) -> impl Element<Self> + '_ {
            row((block(20.0, 10.0).bg(rgb(0xff0000)).grow(1.0), block(20.0, 10.0).bg(rgb(0x00ff00)))).w(200.0)
        }
    }
    let cfg = || Config::new(300.0, 100.0, 1.0, FONT.to_vec());
    let mut a = App::new(V, cfg(), false);
    a.frame();
    let recs = a.reflect_records();
    let first = recs.iter().filter(|r| r.layout.is_some()).nth(1).unwrap().handle;
    assert!(a.reflect_set_style(first, "flex_grow", "1"));
    a.frame();
    let mut b = App::new(W, cfg(), false);
    b.frame();
    assert!(dump(&a) == dump(&b), "style edit != the same style mounted");
}
