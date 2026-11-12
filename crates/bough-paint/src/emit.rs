//! The generated GPU functions. For each archetype, its code tree (`Paint::CODE`, a static mirror of the
//! type) is rendered into pure functions of (rect, record words, block words, pixel):
//!
//! * `a{k}_shade`: the accumulator with the record's layers blended over it (what `SpecSink` computes);
//! * `a{k}_texel`: the atlas texel its glyph layer reads (the caller fetches the byte: the only buffer read
//!   a paint needs that is not its own record);
//! * `a{k}_win`, `a{k}_cover`: the window lane's values (what `WinSink` and `Paint::cover` compute);
//! * `a{k}_chg`: some operand that reads the block differs under the old and the new block (`Paint::changed`);
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

pub fn words(c: &Code) -> usize {
    match c {
        Code::Rec => 1,
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
/// `Paint::cover` as an expression.
fn cover(p: &PCode, base: usize, clip: &str) -> String {
    match p {
        PCode::Prim { mk, args, opaque, .. } => {
            if *opaque {
                format!("c_core({})", staged(mk, args, base, clip, 'u'))
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
    /// The layers of `p` blended into `a`, in paint order.
    fn shade(&mut self, p: &PCode, base: usize, clip: &str, hide: Option<&str>) {
        match p {
            PCode::Prim { mk, p, args, glyph, .. } => {
                let c = self.fresh("c");
                self.decl(T::Cmd, &c, &staged(mk, args, base, clip, 'u'), false);
                let call = if *glyph { format!("{p}({c}, x, y, cov8)") } else { format!("{p}({c}, x, y)") };
                match hide {
                    Some(h) => writeln!(self.out, "    if (!x_in({h}, x, y)) {{ a = x_over(a, {call}); }}"),
                    None => writeln!(self.out, "    a = x_over(a, {call});"),
                }
                .unwrap();
            }
            PCode::Over { a, b, cut } => {
                let wa = pwords(a);
                if *cut {
                    // the upper layer's opaque core replaces whatever the lower layers put there
                    let h = self.fresh("h");
                    self.decl(T::V4i, &h, &cover(b, base + wa, clip), false);
                    self.shade(a, base, clip, Some(&h));
                } else {
                    self.shade(a, base, clip, hide);
                }
                self.shade(b, base + wa, clip, hide);
            }
            PCode::Clip { p, k } => {
                let mut kb = base + pwords(p);
                let kx = expr(k, &mut kb, 'u');
                let kn = self.fresh("k");
                self.decl(T::V4i, &kn, &format!("x_and({clip}, {kx})"), false);
                self.shade(p, base, &kn, hide);
            }
            PCode::Hit { p, .. } => self.shade(p, base, clip, hide),
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
/// The comparisons of every operand that reads the block, old against new.
fn chg(p: &PCode, base: usize, out: &mut Vec<String>) {
    let mut one = |c: &Code, t: Ty, b: usize| {
        if reads(c) == 0 {
            return;
        }
        let ne = match t {
            Ty::U32 => "x_neu",
            Ty::I32 => "x_nei",
            Ty::V4u => "x_nev",
            Ty::V4i => "x_ner",
        };
        out.push(format!("{ne}({}, {})", expr(c, &mut { b }, 'o'), expr(c, &mut { b }, 'u')));
    };
    match p {
        PCode::Prim { args, tys, .. } => {
            let mut b = base;
            for (c, t) in args.iter().zip(tys.iter()) {
                one(c, *t, b);
                b += words(c);
            }
        }
        PCode::Over { a, b, .. } => {
            chg(a, base, out);
            chg(b, base + pwords(a), out);
        }
        PCode::Clip { p, k } => {
            one(k, Ty::V4i, base + pwords(p));
            chg(p, base, out);
        }
        PCode::Hit { p, .. } => chg(p, base, out),
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
pub fn has_glyph(p: &PCode) -> bool {
    match p {
        PCode::Prim { glyph, .. } => *glyph,
        PCode::Over { a, b, .. } => has_glyph(a) || has_glyph(b),
        PCode::Clip { p, .. } | PCode::Hit { p, .. } => has_glyph(p),
    }
}

fn header(lang: Lang, out: &mut String, ret: T, name: &str, params: &[(T, String)]) {
    match lang {
        Lang::Wgsl => {
            let p: Vec<String> = params.iter().map(|(t, n)| format!("{n}: {}", lang.ty(*t))).collect();
            writeln!(out, "fn {name}({}) -> {} {{", p.join(", "), lang.ty(ret))
        }
        Lang::Msl => {
            let p: Vec<String> = params.iter().map(|(t, n)| format!("{} {n}", lang.ty(*t))).collect();
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
    header(lang, out, T::V4u, &format!("a{k}_shade"), &ps);
    let mut f = Fn { lang, out, n: 0 };
    f.decl(T::V4u, "a", "acc", true);
    f.shade(p, 0, NO_CLIP, None);
    writeln!(out, "    return a;\n}}").unwrap();

    let mut ps = vec![rect.clone()];
    ps.extend(xy.iter().cloned());
    ps.push((T::U32, "aw".into()));
    ps.extend(ws.iter().cloned());
    ps.extend(us.iter().cloned());
    header(lang, out, T::U32, &format!("a{k}_texel"), &ps);
    writeln!(out, "    return {};\n}}", texel(p, 0, NO_CLIP).unwrap_or("4294967295u".into())).unwrap();

    let mut ps = vec![rect.clone()];
    ps.extend(ws.iter().cloned());
    ps.extend(us.iter().cloned());
    header(lang, out, T::V4i, &format!("a{k}_win"), &ps);
    let mut f = Fn { lang, out, n: 0 };
    f.decl(T::V4i, "w", EMPTY, true);
    f.win(p, 0, NO_CLIP);
    writeln!(out, "    return w;\n}}").unwrap();

    header(lang, out, T::V4i, &format!("a{k}_cover"), &ps);
    let cv = if cfg!(feature = "mutant_cut") && d.opaque_core { format!("a{k}_win({})", ps.iter().map(|p| p.1.clone()).collect::<Vec<_>>().join(", ")) } else { cover(p, 0, NO_CLIP) };
    writeln!(out, "    return {cv};\n}}").unwrap();

    header(lang, out, T::U32, &format!("a{k}_pick"), &ps);
    writeln!(out, "    return {};\n}}", pick(p, 0).unwrap_or("4294967295u".into())).unwrap();

    let mut ps = vec![rect.clone()];
    ps.extend(ws.iter().cloned());
    ps.extend(os.iter().cloned());
    ps.extend(us.iter().cloned());
    header(lang, out, T::Bool, &format!("a{k}_chg"), &ps);
    let mut c = Vec::new();
    chg(p, 0, &mut c);
    writeln!(out, "    return {};\n}}", if c.is_empty() { "false".to_string() } else { c.join(" || ") }).unwrap();
}

/// The leaves and every archetype's functions as one WGSL module (for validation; the run-time path on
/// Metal is `metal::source`).
pub fn wgsl(descs: &[&Desc]) -> String {
    let mut s = String::from(LEAVES_WGSL);
    s.push('\n');
    for (k, d) in descs.iter().enumerate() {
        archetype(Lang::Wgsl, k, d, &mut s);
    }
    s
}
