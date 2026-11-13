//! The generated GPU functions. For each archetype, its code tree (`Paint::CODE`, a static mirror of the
//! type) is rendered into pure functions of (rect, record words, block words, pixel):
//!
//! * `a{k}_shade`: the accumulator with the record's layers blended over it (what `SpecSink` computes);
//! * `a{k}_texel`: the atlas texel its glyph layer reads (the caller fetches the byte: the only buffer read
//!   a paint needs that is not its own record);
//! * `a{k}_win`, `a{k}_cover`: the window lane's values (what `WinSink` and `Paint::cover` compute);
//! * `a{k}_chgw`: the windows of the layers an operand of which differs under the old and the new block
//!   (`Paint::changed_win`);
//! * `a{k}_pick`: its pick id.
//!
//! Every expression is rendered as calls of leaves on parameter names and `123u` literals, which is valid
//! WGSL and valid MSL unchanged; only declarations differ (`Lang`). The leaves themselves went through
//! wgsl-rs and naga when this crate was built (`LEAVES_MSL`, `LEAVES_WGSL`), so nothing is translated at run
//! time: the application's part of the shader is text assembled from constants.
use crate::expr::{Code, Ty};
use crate::paint::PCode;
use crate::scene::Desc;
use std::fmt::Write;

/// The leaves as naga's MSL (no entry point).
pub const LEAVES_MSL: &str = include_str!(concat!(env!("OUT_DIR"), "/leaves.metal"));
/// The leaves as wgsl-rs emitted them.
pub const LEAVES_WGSL: &str = include_str!(concat!(env!("OUT_DIR"), "/leaves.wgsl"));

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Wgsl,
    Msl,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum T {
    U32,
    I32,
    V4u,
    V4i,
    Bool,
    Cmd,
}
impl Lang {
    fn ty(self, t: T) -> &'static str {
        match (self, t) {
            (Lang::Wgsl, T::U32) => "u32",
            (Lang::Wgsl, T::I32) => "i32",
            (Lang::Wgsl, T::V4u) => "vec4<u32>",
            (Lang::Wgsl, T::V4i) => "vec4<i32>",
            (Lang::Msl, T::U32) => "uint",
            (Lang::Msl, T::I32) => "int",
            (Lang::Msl, T::V4u) => "metal::uint4",
            (Lang::Msl, T::V4i) => "metal::int4",
            (_, T::Bool) => "bool",
            (_, T::Cmd) => "Cmd",
        }
    }
}

const EMPTY: &str = "x_rect(0u, 0u, 0u, 0u)";
/// `paint::NO_CLIP` as words.
const NO_CLIP: &str = "x_rect(4290871296u, 4290871296u, 4096000u, 4096000u)";

/// What has no generated form yet. `Code::Cpu` (a Rust function) is a value the store must keep current in
/// a record word; `Code::UAt` (a block word chosen by a record word) needs the block as an array in the
/// generated function. Neither occurs in a paint operand of this crate's scenes.
fn unsupported(c: &Code) -> ! {
    panic!("no GPU form for {c:?} in a paint operand")
}
pub fn words(c: &Code) -> usize {
    match c {
        Code::Rec | Code::UAt(..) => 1,
        Code::Cpu(a) => a.iter().map(|c| words(c)).sum(),
        Code::Call(_, a) => a.iter().map(|c| words(c)).sum(),
        _ => 0,
    }
}
pub fn pwords(p: &PCode) -> usize {
    match p {
        PCode::Prim { args, .. } => args.iter().map(|c| words(c)).sum(),
        PCode::Over { a, b, .. } => pwords(a) + pwords(b),
        PCode::Clip { p, k } => pwords(p) + words(k),
        PCode::Hit { p, id } => pwords(p) + words(id),
    }
}
fn reads(c: &Code) -> u64 {
    match c {
        Code::U(s) => 1 << s,
        Code::UAt(b, n) => ((1u64 << n) - 1) << b,
        Code::Cpu(a) => a.iter().fold(0, |m, c| m | reads(c)),
        Code::Call(_, a) => a.iter().fold(0, |m, c| m | reads(c)),
        _ => 0,
    }
}
/// Every leaf a code tree names (the gate checks each exists in `LEAVES_MSL` under that name).
pub fn leaves(p: &PCode, out: &mut Vec<&'static str>) {
    fn e(c: &Code, out: &mut Vec<&'static str>) {
        if let Code::Call(n, a) = c {
            out.push(n);
            a.iter().for_each(|c| e(c, out));
        }
    }
    match p {
        PCode::Prim { mk, p, args, .. } => {
            out.extend([*mk, *p]);
            args.iter().for_each(|c| e(c, out));
        }
        PCode::Over { a, b, .. } => {
            leaves(a, out);
            leaves(b, out);
        }
        PCode::Clip { p, k } => {
            leaves(p, out);
            e(k, out);
        }
        PCode::Hit { p, id } => {
            leaves(p, out);
            e(id, out);
        }
    }
}

/// An expression; `base` is the record word its first `Rec` is, `u` the prefix of block-word names.
fn expr(c: &Code, base: &mut usize, u: char) -> String {
    match c {
        Code::U(s) => format!("{u}{s}"),
        Code::Rec => {
            *base += 1;
            format!("w{}", *base - 1)
        }
        Code::Lane => "rect".into(),
        Code::Lit(v) => format!("{v}u"),
        Code::UAt(..) | Code::Cpu(_) => unsupported(c),
        Code::Call(n, a) => {
            let args: Vec<String> = a.iter().map(|c| expr(c, base, u)).collect();
            format!("{n}({})", args.join(", "))
        }
    }
}
/// `mk(args.., clip)` of a primitive whose first record word is `base`.
fn staged(mk: &str, args: &[&Code], mut base: usize, clip: &str, u: char) -> String {
    let mut a: Vec<String> = args.iter().map(|c| expr(c, &mut base, u)).collect();
    a.push(clip.into());
    format!("{mk}({})", a.join(", "))
}
/// `Alpha::full` of a colour expression whose first record word is `base`: `None` where the type admits no
/// full alpha (`Alpha::MAY` false).
fn full(c: &Code, base: usize) -> Option<String> {
    let Code::Call(n, a) = c else { return None };
    let both = |x: &Code, bx: usize, y: &Code, by: usize| Some(format!("({} && {})", full(x, bx)?, full(y, by)?));
    match (*n, *a) {
        ("x_pmo", _) => Some("true".into()),
        ("x_pm", [w]) if reads(w) == 0 => Some(format!("x_full({})", expr(w, &mut { base }, 'u'))),
        ("x_mixv", [x, y, _]) => both(x, base, y, base + words(x)),
        ("x_selv", [k, x, y]) => both(x, base + words(k), y, base + words(k) + words(x)),
        _ => None,
    }
}
/// The condition under which a primitive's core covers, if its type admits one.
fn prim_full(args: &[&Code], idx: &[usize], base: usize) -> Option<String> {
    let mut out = vec!["true".to_string()];
    for &i in idx {
        let b = base + args[..i].iter().map(|c| words(c)).sum::<usize>();
        out.push(full(args[i], b)?);
    }
    Some(out.join(" && "))
}
/// Every primitive's `opaque` flag (from `Alpha::MAY`, the CPU's side) agrees with what `full` derives from
/// the code tree (the GPU's side).
fn check_full(p: &PCode, name: &str) {
    match p {
        PCode::Prim { args, opaque, full: idx, .. } => assert_eq!(*opaque, !idx.is_empty() && prim_full(args, idx, 0).is_some(), "{name}: the two definitions of a full alpha disagree"),
        PCode::Over { a, b, .. } => {
            check_full(a, name);
            check_full(b, name);
        }
        PCode::Clip { p, .. } | PCode::Hit { p, .. } => check_full(p, name),
    }
}
/// `Paint::cover` as an expression.
fn cover(p: &PCode, base: usize, clip: &str) -> String {
    match p {
        PCode::Prim { mk, args, opaque, full: idx, .. } => {
            if *opaque {
                format!("x_keep(c_core({}), {})", staged(mk, args, base, clip, 'u'), prim_full(args, idx, base).unwrap())
            } else {
                EMPTY.into()
            }
        }
        PCode::Over { a, b, cut } => {
            if *cut {
                cover(b, base + pwords(a), clip)
            } else {
                EMPTY.into()
            }
        }
        PCode::Clip { p, k } => {
            let mut kb = base + pwords(p);
            let k = expr(k, &mut kb, 'u');
            cover(p, base, &format!("x_and({clip}, {k})"))
        }
        PCode::Hit { p, .. } => cover(p, base, clip),
    }
}

struct Fn<'a> {
    lang: Lang,
    out: &'a mut String,
    n: usize,
}
impl Fn<'_> {
    fn decl(&mut self, t: T, name: &str, init: &str, mutable: bool) {
        let ty = self.lang.ty(t);
        match self.lang {
            Lang::Wgsl => writeln!(self.out, "    {} {name}: {ty} = {init};", if mutable { "var" } else { "let" }),
            Lang::Msl => writeln!(self.out, "    {ty} {name} = {init};"),
        }
        .unwrap();
    }
    fn fresh(&mut self, p: &str) -> String {
        self.n += 1;
        format!("{p}{}", self.n)
    }
    /// The layers of `p` blended, in paint order, into each pixel of `px` (accumulator, x, y, coverage
    /// byte). A layer is staged once and shaded at every pixel: a GPU thread owns a 2 x 2 block.
    fn shade(&mut self, p: &PCode, base: usize, clip: &str, hide: Option<&str>, px: &[[&str; 4]]) {
        match p {
            PCode::Prim { mk, p, args, glyph, tab, .. } => {
                let c = self.fresh("c");
                self.decl(T::Cmd, &c, &staged(mk, args, base, clip, 'u'), false);
                // a memoised layer: is the table its handle names filled for its key? (two header reads)
                let ok = self.fresh("ok");
                let tb = if self.lang == Lang::Msl { "TAB, " } else { "" };
                if *tab {
                    self.decl(T::Bool, &ok, &format!("x_tabok({c}, tab_h({tb}c_tab({c}), 0u), tab_h({tb}c_tab({c}), 1u))"), false);
                }
                for [a, x, y, cov] in px {
                    let call = if *tab {
                        let t = self.fresh("t");
                        self.decl(T::V4u, &t, &format!("x_ctaps({c}, {x}, {y}, {ok})"), false);
                        let taps: Vec<String> = ["x", "y", "z", "w"].iter().map(|q| format!("tab_v({tb}c_tab({c}), {t}.{q})")).collect();
                        format!("{p}m({c}, {x}, {y}, {})", taps.join(", "))
                    } else if *glyph {
                        format!("{p}({c}, {x}, {y}, {cov})")
                    } else {
                        format!("{p}({c}, {x}, {y})")
                    };
                    match hide {
                        Some(h) => writeln!(self.out, "    if (!x_in({h}, {x}, {y})) {{ {a} = x_over({a}, {call}); }}"),
                        None => writeln!(self.out, "    {a} = x_over({a}, {call});"),
                    }
                    .unwrap();
                }
            }
            PCode::Over { a, b, cut } => {
                let wa = pwords(a);
                if *cut {
                    // the upper layer's opaque core replaces whatever the lower layers put there
                    let h = self.fresh("h");
                    self.decl(T::V4i, &h, &cover(b, base + wa, clip), false);
                    self.shade(a, base, clip, Some(&h), px);
                } else {
                    self.shade(a, base, clip, hide, px);
                }
                self.shade(b, base + wa, clip, hide, px);
            }
            PCode::Clip { p, k } => {
                let mut kb = base + pwords(p);
                let kx = expr(k, &mut kb, 'u');
                let kn = self.fresh("k");
                self.decl(T::V4i, &kn, &format!("x_and({clip}, {kx})"), false);
                self.shade(p, base, &kn, hide, px);
            }
            PCode::Hit { p, .. } => self.shade(p, base, clip, hide, px),
        }
    }
    fn win(&mut self, p: &PCode, base: usize, clip: &str) {
        match p {
            PCode::Prim { mk, args, .. } => writeln!(self.out, "    w = x_union(w, c_win({}));", staged(mk, args, base, clip, 'u')).unwrap(),
            PCode::Over { a, b, .. } => {
                self.win(a, base, clip);
                self.win(b, base + pwords(a), clip);
            }
            PCode::Clip { p, k } => {
                let mut kb = base + pwords(p);
                let kx = expr(k, &mut kb, 'u');
                let kn = self.fresh("k");
                self.decl(T::V4i, &kn, &format!("x_and({clip}, {kx})"), false);
                self.win(p, base, &kn);
            }
            PCode::Hit { p, .. } => self.win(p, base, clip),
        }
    }
}
/// The atlas texel of the first glyph layer.
fn texel(p: &PCode, base: usize, clip: &str) -> Option<String> {
    match p {
        PCode::Prim { mk, args, glyph, .. } => glyph.then(|| format!("x_texel({}, x, y, aw)", staged(mk, args, base, clip, 'u'))),
        PCode::Over { a, b, .. } => texel(a, base, clip).or_else(|| texel(b, base + pwords(a), clip)),
        PCode::Clip { p, k } => {
            let mut kb = base + pwords(p);
            let k = expr(k, &mut kb, 'u');
            texel(p, base, &format!("x_and({clip}, {k})"))
        }
        PCode::Hit { p, .. } => texel(p, base, clip),
    }
}
/// The comparisons of a primitive's operands that read the block, old against new.
fn prim_chg(args: &[&Code], tys: &[Ty], base: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut b = base;
    for (c, t) in args.iter().zip(tys.iter()) {
        if reads(c) != 0 {
            let ne = match t {
                Ty::U32 => "x_neu",
                Ty::I32 => "x_nei",
                Ty::V4u => "x_nev",
                Ty::V4i => "x_ner",
            };
            out.push(format!("{ne}({}, {})", expr(c, &mut { b }, 'o'), expr(c, &mut { b }, 'u')));
        }
        b += words(c);
    }
    out
}
impl Fn<'_> {
    /// `Paint::changed_win`: the union of the windows of the layers with a changed operand.
    fn chgw(&mut self, p: &PCode, base: usize, clip: &str) {
        match p {
            PCode::Prim { mk, args, tys, .. } => {
                let c = prim_chg(args, tys, base);
                if !c.is_empty() {
                    writeln!(self.out, "    w = x_union(w, x_keep(c_win({}), {}));", staged(mk, args, base, clip, 'u'), c.join(" || ")).unwrap();
                }
            }
            PCode::Over { a, b, .. } => {
                self.chgw(a, base, clip);
                self.chgw(b, base + pwords(a), clip);
            }
            PCode::Clip { p, k } => {
                let mut kb = base + pwords(p);
                let kx = expr(k, &mut kb, 'u');
                let kn = self.fresh("k");
                self.decl(T::V4i, &kn, &format!("x_and({clip}, {kx})"), false);
                self.chgw(p, base, &kn);
            }
            PCode::Hit { p, .. } => self.chgw(p, base, clip),
        }
    }
}
fn pick(p: &PCode, base: usize) -> Option<String> {
    match p {
        PCode::Prim { .. } => None,
        PCode::Over { a, b, .. } => pick(b, base + pwords(a)).or_else(|| pick(a, base)),
        PCode::Clip { p, .. } => pick(p, base),
        PCode::Hit { p, id } => Some(expr(id, &mut (base + pwords(p)), 'u')),
    }
}
pub fn has_tab(p: &PCode) -> bool {
    match p {
        PCode::Prim { tab, .. } => *tab,
        PCode::Over { a, b, .. } => has_tab(a) || has_tab(b),
        PCode::Clip { p, .. } | PCode::Hit { p, .. } => has_tab(p),
    }
}
pub fn has_glyph(p: &PCode) -> bool {
    match p {
        PCode::Prim { glyph, .. } => *glyph,
        PCode::Over { a, b, .. } => has_glyph(a) || has_glyph(b),
        PCode::Clip { p, .. } | PCode::Hit { p, .. } => has_glyph(p),
    }
}

fn header(lang: Lang, out: &mut String, ret: T, name: &str, params: &[(T, String)], tab: bool) {
    match lang {
        Lang::Wgsl => {
            let p: Vec<String> = params.iter().map(|(t, n)| format!("{n}: {}", lang.ty(*t))).collect();
            writeln!(out, "fn {name}({}) -> {} {{", p.join(", "), lang.ty(ret))
        }
        Lang::Msl => {
            let mut p: Vec<String> = params.iter().map(|(t, n)| format!("{} {n}", lang.ty(*t))).collect();
            if tab {
                p.insert(0, "const device uint* TAB".into());
            }
            writeln!(out, "static {} {name}({}) {{", lang.ty(ret), p.join(", "))
        }
    }
    .unwrap();
}
/// The names of the block words an archetype reads, with a prefix (`u` new, `o` old).
pub fn uniform_names(reads: u64, pre: char) -> Vec<String> {
    (0..64).filter(|s| reads >> s & 1 != 0).map(|s| format!("{pre}{s}")).collect()
}

/// The six functions of archetype `k`.
pub fn archetype(lang: Lang, k: usize, d: &Desc, out: &mut String) {
    let p = d.code;
    assert_eq!(pwords(p), d.words, "{}: the code tree and the record disagree about its words", d.name);
    check_full(p, d.name);
    let ws: Vec<(T, String)> = (0..d.words).map(|i| (T::U32, format!("w{i}"))).collect();
    let us: Vec<(T, String)> = uniform_names(d.reads, 'u').into_iter().map(|n| (T::U32, n)).collect();
    let os: Vec<(T, String)> = uniform_names(d.reads, 'o').into_iter().map(|n| (T::U32, n)).collect();
    let rect = (T::V4i, "rect".to_string());
    let xy = [(T::I32, "x".to_string()), (T::I32, "y".to_string())];
    writeln!(out, "// archetype {k}: {}", d.name).unwrap();

    let mut ps = vec![(T::V4u, "acc".to_string()), rect.clone()];
    ps.extend(xy.iter().cloned());
    ps.push((T::U32, "cov8".into()));
    ps.extend(ws.iter().cloned());
    ps.extend(us.iter().cloned());
    let tab = has_tab(p);
    header(lang, out, T::V4u, &format!("a{k}_shade"), &ps, tab);
    let mut f = Fn { lang, out, n: 0 };
    f.decl(T::V4u, "a", "acc", true);
    f.shade(p, 0, NO_CLIP, None, &[["a", "x", "y", "cov8"]]);
    writeln!(out, "    return a;\n}}").unwrap();
    if lang == Lang::Msl {
        // the 2 x 2 block a GPU thread owns: every layer staged once, shaded four times
        let mut ps: Vec<String> = (0..4).map(|i| format!("thread metal::uint4& a{i}")).collect();
        if tab {
            ps.insert(0, "const device uint* TAB".into());
        }
        ps.extend(["metal::int4 rect".to_string(), "int x".into(), "int y".into()]);
        ps.extend((0..4).map(|i| format!("uint cov{i}")));
        ps.extend(ws.iter().chain(us.iter()).map(|(_, n)| format!("uint {n}")));
        writeln!(out, "static void a{k}_shade4({}) {{", ps.join(", ")).unwrap();
        let mut f = Fn { lang, out, n: 0 };
        f.shade(p, 0, NO_CLIP, None, &[["a0", "x", "y", "cov0"], ["a1", "x + 1", "y", "cov1"], ["a2", "x", "y + 1", "cov2"], ["a3", "x + 1", "y + 1", "cov3"]]);
        writeln!(out, "}}").unwrap();
    }

    let mut ps = vec![rect.clone()];
    ps.extend(xy.iter().cloned());
    ps.push((T::U32, "aw".into()));
    ps.extend(ws.iter().cloned());
    ps.extend(us.iter().cloned());
    header(lang, out, T::U32, &format!("a{k}_texel"), &ps, false);
    writeln!(out, "    return {};\n}}", texel(p, 0, NO_CLIP).unwrap_or("4294967295u".into())).unwrap();

    let mut ps = vec![rect.clone()];
    ps.extend(ws.iter().cloned());
    ps.extend(us.iter().cloned());
    header(lang, out, T::V4i, &format!("a{k}_win"), &ps, false);
    let mut f = Fn { lang, out, n: 0 };
    f.decl(T::V4i, "w", EMPTY, true);
    f.win(p, 0, NO_CLIP);
    writeln!(out, "    return w;\n}}").unwrap();

    header(lang, out, T::V4i, &format!("a{k}_cover"), &ps, false);
    let cv = if cfg!(feature = "mutant_cut") && d.opaque_core { format!("a{k}_win({})", ps.iter().map(|p| p.1.clone()).collect::<Vec<_>>().join(", ")) } else { cover(p, 0, NO_CLIP) };
    writeln!(out, "    return {cv};\n}}").unwrap();

    header(lang, out, T::U32, &format!("a{k}_pick"), &ps, false);
    writeln!(out, "    return {};\n}}", pick(p, 0).unwrap_or("4294967295u".into())).unwrap();

    let mut ps = vec![rect.clone()];
    ps.extend(ws.iter().cloned());
    ps.extend(os.iter().cloned());
    ps.extend(us.iter().cloned());
    header(lang, out, T::V4i, &format!("a{k}_chgw"), &ps, false);
    let mut f = Fn { lang, out, n: 0 };
    f.decl(T::V4i, "w", EMPTY, true);
    f.chgw(p, 0, NO_CLIP);
    writeln!(out, "    return w;\n}}").unwrap();
}

/// The leaves and every archetype's functions as one WGSL module (for validation; the run-time path on
/// Metal is `metal::source`).
pub fn wgsl(descs: &[&Desc]) -> String {
    let mut s = String::from(LEAVES_WGSL);
    // the table reads, for validation only (the run-time path on Metal reads the arena: `metal::source`)
    s.push_str("\nfn tab_h(b: u32, i: u32) -> u32 { return 4294967295u; }\nfn tab_v(b: u32, i: u32) -> u32 { return 4294967295u; }\n");
    for (k, d) in descs.iter().enumerate() {
        archetype(Lang::Wgsl, k, d, &mut s);
    }
    s
}
