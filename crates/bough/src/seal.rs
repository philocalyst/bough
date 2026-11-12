//! Seals: the operands of a description, bit-exact, kept per dynamic item so that a re-description can
//! tell "this item is described exactly as before" by streaming over contiguous bytes, without entering the
//! item's elements (round 6).
//!
//! A re-description of an owner with a thousand rows used to walk every row's elements, each of which
//! reaches into the layout table, the node table, the text table and the instance pool to compare one
//! operand at a time. Almost always nothing in the row changed. A seal is what the row's description
//! *said*: every by-value operand in description order (layout specs as indices into the window's
//! [`SpecTable`], colours, radii, text bytes), preceded by the mounting context the item inherits (clip,
//! space, depth, origin). Checking it is a compare of the new description against those bytes in place
//! (nothing is written); an equal seal means the retained records are what `update` would leave, so the
//! item is stepped over by its record and layout-child counts.
//!
//! Exactness: a seal is the operands themselves, never a hash of them. An element that owns state a
//! description does not determine (a route, a scroll view, a text field, a nested dynamic part) refuses
//! (`Sealer::refuse`), and an item containing one is always updated the ordinary way. Wired operands are
//! sealed by their wire's identity (offset and recipe): their values travel through the wire.
use crate::layout::{Spec, SpecTable};

/// What an element writes its operands to: a writer (after an update) or a checker (before one).
pub trait Sealer {
    fn word(&mut self, w: u32);
    fn bytes(&mut self, b: &[u8]);
    fn spec(&mut self, s: &Spec);
    /// A paint-only operand (a colour): one whose change is a write to an instance field, with no layout,
    /// shaping or structure behind it. A checker keeps "equal but for paint" apart from "different"
    /// (`SealCheck::shape`), which is what lets a shifted sequence be recognised under zebra stripes.
    #[inline(always)]
    fn paint(&mut self, w: u32) {
        self.word(w)
    }
    /// An integer whose text is the operand (`text(row.id)`): the integer itself, two words. Its own entry so
    /// the gate's mutant reaches it as it reaches text bytes.
    #[inline(always)]
    fn int(&mut self, v: u64) {
        self.word(v as u32);
        self.word((v >> 32) as u32);
    }
    /// This description cannot be sealed.
    fn refuse(&mut self);
    #[inline(always)]
    fn f32(&mut self, v: f32) {
        self.word(v.to_bits())
    }
}

/// Appends a seal to `out`. `ok` is false once an element refused.
pub struct SealWrite<'a> {
    pub out: &'a mut Vec<u8>,
    pub descs: &'a mut SpecTable,
    pub ok: bool,
}
impl Sealer for SealWrite<'_> {
    #[inline]
    fn word(&mut self, w: u32) {
        self.out.extend_from_slice(&w.to_ne_bytes());
    }
    #[inline]
    fn bytes(&mut self, b: &[u8]) {
        self.word(b.len() as u32);
        self.out.extend_from_slice(b);
    }
    #[inline]
    fn spec(&mut self, s: &Spec) {
        let ix = self.descs.intern(s);
        self.word(ix);
    }
    #[inline]
    fn refuse(&mut self) {
        self.ok = false;
    }
}

/// Compares a description against a retained seal in place. Equal iff `ok` and nothing is left over.
pub struct SealCheck<'a> {
    pub cur: &'a [u8],
    pub descs: &'a SpecTable,
    pub ok: bool,
    /// No paint operand differed (so far).
    pub paint_ok: bool,
}
impl<'a> SealCheck<'a> {
    #[inline(always)]
    fn take_word(&mut self) -> u32 {
        match self.cur.split_first_chunk::<4>() {
            Some((w, rest)) => {
                self.cur = rest;
                u32::from_ne_bytes(*w)
            }
            None => {
                self.ok = false;
                0
            }
        }
    }
    pub fn new(cur: &'a [u8], descs: &'a SpecTable) -> Self {
        SealCheck { cur, descs, ok: true, paint_ok: true }
    }
    /// Bit-identical description.
    #[inline(always)]
    pub fn equal(&self) -> bool {
        self.ok && self.paint_ok && self.cur.is_empty()
    }
    /// Identical but possibly for paint operands: same structure, layout and text.
    #[inline(always)]
    pub fn shape(&self) -> bool {
        self.ok && self.cur.is_empty()
    }
}
impl Sealer for SealCheck<'_> {
    #[inline(always)]
    fn word(&mut self, w: u32) {
        self.ok &= self.take_word() == w;
    }
    #[inline(always)]
    fn bytes(&mut self, b: &[u8]) {
        let n = self.take_word() as usize;
        if n != b.len() || self.cur.len() < n {
            self.ok = false;
            return;
        }
        let (head, rest) = self.cur.split_at(n);
        // (IDEAS-OPUS.md, finding A1c) short operands compared as two overlapping words, no
        // `memcmp` call (bbtree: "load the bytes as integers and compare the integers").
        let eq = if (8..=16).contains(&n) {
            let w = |s: &[u8], o: usize| u64::from_ne_bytes(s[o..o + 8].try_into().unwrap());
            (w(head, 0) ^ w(b, 0)) | (w(head, n - 8) ^ w(b, n - 8)) == 0
        } else if (4..8).contains(&n) {
            let w = |s: &[u8], o: usize| u32::from_ne_bytes(s[o..o + 4].try_into().unwrap());
            (w(head, 0) ^ w(b, 0)) | (w(head, n - 4) ^ w(b, n - 4)) == 0
        } else {
            head == b
        };
        self.ok &= eq || mutant_mode() == 1;
        self.cur = rest;
    }
    #[inline(always)]
    fn int(&mut self, v: u64) {
        let (lo, hi) = (self.take_word(), self.take_word());
        self.ok &= ((lo as u64 | (hi as u64) << 32) == v) || mutant_mode() == 2;
    }
    #[inline(always)]
    fn spec(&mut self, s: &Spec) {
        let ix = self.take_word();
        self.ok &= self.descs.is(ix, s);
    }
    #[inline(always)]
    fn paint(&mut self, w: u32) {
        self.paint_ok &= self.take_word() == w;
    }
    #[inline(always)]
    fn refuse(&mut self) {
        self.ok = false;
    }
}

/// A closure operand (a route, a projection, a mapping) as its bytes: which code runs is fixed by the
/// description's type and place, so equal bytes are the same function with the same captures. (Padding inside
/// the captures has no defined value: it can only make two equal closures look different, which costs an update.)
#[inline(always)]
pub fn closure<S: Sealer, F: Copy>(s: &mut S, f: &F) {
    // SAFETY: `f` is `size_of::<F>()` bytes of a `Copy` value; a byte copy is what a recipe stores too.
    let b = unsafe { std::slice::from_raw_parts(f as *const F as *const u8, size_of::<F>()) };
    s.bytes(b);
}

/// The gate's negative controls (`tests/seals.rs`): `BOUGH_SEAL_MUTANT` makes a check accept any text bytes
/// of the right length (mode 1), or, set to `int`, any integer operand (mode 2), so a changed text or number
/// inside a sealed item is missed. Read once; 0 in every normal run.
#[inline]
fn mutant_mode() -> u8 {
    static M: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *M.get_or_init(|| match std::env::var("BOUGH_SEAL_MUTANT") {
        Ok(v) if v == "int" => 2,
        Ok(_) => 1,
        Err(_) => 0,
    })
}
