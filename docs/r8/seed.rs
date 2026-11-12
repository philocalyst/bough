// Proof that the type machinery of ROUND8.md sections 1-5 is expressible: staged expressions with the stage
// inferred from types, structural derivatives, archetype columns, and a view described by continuation.
#![allow(dead_code)]
use std::marker::PhantomData;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Stage { Const, Mount, Turn, Frame }
const fn later(a: Stage, b: Stage) -> Stage { if (a as u8) > (b as u8) { a } else { b } }

/// The uniform block: every piece of state a Frame expression may read is a word here.
pub struct Uniforms { pub w: [u32; 16] }
/// What a change of the uniform block touches in one archetype's column.
pub enum Affected { None, All, Rows([u32; 2], u8) }

pub trait Expr: Copy {
    type Out: Copy;
    type Consts: Copy;            // per-record constants: one column
    const STAGE: Stage;
    fn eval(u: &Uniforms, c: &Self::Consts) -> Self::Out;
    fn delta(old: &Uniforms, new: &Uniforms, col: &[Self::Consts]) -> Affected;
}

/// A state word read at frame time (hover id, theme, time, scroll...). `SLOT` is where it lives.
#[derive(Clone, Copy)] pub struct U<const SLOT: usize>;
impl<const SLOT: usize> Expr for U<SLOT> {
    type Out = u32; type Consts = ();
    const STAGE: Stage = Stage::Frame;
    #[inline(always)] fn eval(u: &Uniforms, _: &()) -> u32 { u.w[SLOT] }
    fn delta(o: &Uniforms, n: &Uniforms, _: &[()]) -> Affected { if o.w[SLOT] == n.w[SLOT] { Affected::None } else { Affected::All } }
}
/// A per-record constant.
#[derive(Clone, Copy)] pub struct Rec;
impl Expr for Rec {
    type Out = u32; type Consts = u32;
    const STAGE: Stage = Stage::Mount;
    #[inline(always)] fn eval(_: &Uniforms, c: &u32) -> u32 { *c }
    fn delta(_: &Uniforms, _: &Uniforms, _: &[u32]) -> Affected { Affected::None }
}
/// `uniform == record constant`: the derivative is the two records whose constant is the old or new value.
#[derive(Clone, Copy)] pub struct EqRow<const SLOT: usize>;
impl<const SLOT: usize> Expr for EqRow<SLOT> {
    type Out = bool; type Consts = u32;
    const STAGE: Stage = later(<U<SLOT> as Expr>::STAGE, <Rec as Expr>::STAGE);
    #[inline(always)] fn eval(u: &Uniforms, c: &u32) -> bool { u.w[SLOT] == *c }
    fn delta(o: &Uniforms, n: &Uniforms, col: &[u32]) -> Affected {
        let (a, b) = (o.w[SLOT], n.w[SLOT]);
        if a == b { return Affected::None; }
        // constants are the row numbers here, so the index is arithmetic; a sorted or hashed column otherwise
        let mut r = [0u32; 2]; let mut k = 0u8;
        for v in [a, b] { if (v as usize) < col.len() { r[k as usize] = v; k += 1; } }
        Affected::Rows(r, k)
    }
}
/// `if c { a } else { b }` with constant arms.
#[derive(Clone, Copy)] pub struct Select<C, const A: u32, const B: u32>(PhantomData<C>);
impl<C: Expr<Out = bool>, const A: u32, const B: u32> Expr for Select<C, A, B> {
    type Out = u32; type Consts = C::Consts;
    const STAGE: Stage = C::STAGE;
    #[inline(always)] fn eval(u: &Uniforms, c: &C::Consts) -> u32 { if C::eval(u, c) { A } else { B } }
    fn delta(o: &Uniforms, n: &Uniforms, col: &[C::Consts]) -> Affected { C::delta(o, n, col) }
}

/// An archetype: one column of constants per expression, one monomorphised kernel. No wires, no fn pointers.
pub struct Archetype<E: Expr> { pub consts: Vec<E::Consts>, pub rect: Vec<[i16; 4]> }
impl<E: Expr<Out = u32>> Archetype<E> {
    /// The CPU kernel for a run of records: the expression is inlined, the loop is over a column.
    #[inline(never)]
    pub fn shade(&self, u: &Uniforms, rows: std::ops::Range<usize>, out: &mut [u32]) {
        for (o, c) in out.iter_mut().zip(&self.consts[rows]) { *o = E::eval(u, c); }
    }
}

/// Description by continuation: `view` drives a sink; nothing is returned, state is borrowed straight through.
pub trait Sink<'a>: Sized {
    fn cell<E: Expr<Out = u32, Consts = u32>>(&mut self, rect: [i16; 4], consts: u32);
    fn rows<F: for<'r> FnMut(&mut Self, usize)>(&mut self, n: usize, mut f: F) { for i in 0..n { f(self, i); } }
}
pub struct Mount<'a, E: Expr> { arch: &'a mut Archetype<E> }
impl<'a, E0: Expr<Consts = u32>> Sink<'a> for Mount<'a, E0> {
    #[inline(always)]
    fn cell<E: Expr<Out = u32, Consts = u32>>(&mut self, rect: [i16; 4], consts: u32) {
        self.arch.consts.push(consts); self.arch.rect.push(rect);
    }
}

pub struct Sheet { pub hover: u32, pub rows: usize }
type CellFill = Select<EqRow<0>, 0xffcce0ff, 0xffffffff>;
impl Sheet {
    pub fn view<'a, S: Sink<'a>>(&'a self, s: &mut S) {
        s.rows(self.rows, |s, i| s.cell::<CellFill>([0, (i * 32) as i16, 110, 32], i as u32));
    }
}

const _: () = assert!(matches!(<CellFill as Expr>::STAGE, Stage::Frame));

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let sheet = Sheet { hover: 7, rows: n };
    let mut arch = Archetype::<CellFill> { consts: Vec::with_capacity(n), rect: Vec::with_capacity(n) };
    sheet.view(&mut Mount { arch: &mut arch });
    let mut u = Uniforms { w: [0; 16] }; u.w[0] = 7;
    let mut out = [0u32; 2];
    let mut acc = 0u64;
    let reps = 1_000_000u32;
    let t = std::time::Instant::now();
    for k in 0..reps {
        // a hover write: one store to the uniform block, damage from the derivative, shade only what it names
        let old = Uniforms { w: u.w };
        u.w[0] = (k.wrapping_mul(2654435761)) % n as u32;
        if let Affected::Rows(r, cnt) = CellFill::delta(&old, &u, &arch.consts) {
            for j in 0..cnt as usize { let i = r[j] as usize; arch.shade(&u, i..i + 1, &mut out[j..j + 1]); acc += out[j] as u64; }
        }
    }
    let ns = t.elapsed().as_nanos() as f64 / reps as f64;
    println!("records={n} hover write: {ns:.1} ns per write (delta + shade of the 2 affected records), acc={acc}");
}
