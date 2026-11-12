//! Record state flags read by variant tables (round 6).
//!
//! **One mechanism for every boolean a record can be in**: hovered, pressed, focused, focus-within, disabled,
//! selected, checked, expanded, dragged-over, invalid, search-hit, current, loading, and three of the
//! author's own. A record's flags are a 16-bit word; a description says how the record looks under flags
//! (`.when(Flag::SELECTED, |s| s.bg(..))`; `.hover(..)`, `.active(..)`, `.focus(..)`, `.disabled(..)` are spellings of
//! it, and `hover_bg` is one instance), and those variants are resolved **once, at mount, into a table** indexed
//! by the flags the record's variants name. A flip is then a **paint write on exactly the records whose
//! resolved look changed**: no re-description, no signal per row, no per-flip interpretation of the variants.
//!
//! Who sets flags, all through [`write`] (one code path):
//! * input: hover crossings (`HOVERED`), the press chain (`PRESSED`), focus (`FOCUSED` on the record,
//!   `FOCUS_WITHIN` on its ancestors), a drag over a record (`DRAG_OVER`);
//! * a description: `.flag(Flag::DISABLED, on)` on an element (it and its subtree);
//! * a `List<T>`'s item flags (`List::set_flag`, `set_flag_range`, `clear_flag`): the item's row subtree, at
//!   once for mounted rows, at mount for the others (selection, checked items, search hits over 10,000 rows
//!   without a signal per row).
//!
//! **Own and effective flags.** A record's *own* flags are those set on it; a subtree flag (`.flag`, a list
//! item's) is also *inherited* by every record below it, so a row's label reads `SELECTED` from the row. The
//! effective word is own | inherited; it is what the table is indexed by and what dispatch reads (`DISABLED`:
//! no route runs, Tab skips it).
//!
//! **Storage** (the façade's, behind this module's functions, until lane `core`'s state planes take it): the
//! words live in the record's act (`input::Act`), which every record with variants or flags has; such
//! records carry `KF_INTERACT`, so a subtree write visits exactly them through the kind column's flagged scan.
use crate::color::Color;
use crate::elem::{Element, Mounter};
use crate::rt::*;
use crate::signal::Wired;
use bough_core::{Handle, KF_INTERACT};

/// A record state flag: one bit of a record's flag word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Flag(pub(crate) u8);
impl Flag {
    pub const HOVERED: Flag = Flag(0);
    pub const PRESSED: Flag = Flag(1);
    pub const FOCUSED: Flag = Flag(2);
    pub const FOCUS_WITHIN: Flag = Flag(3);
    pub const DISABLED: Flag = Flag(4);
    pub const SELECTED: Flag = Flag(5);
    pub const CHECKED: Flag = Flag(6);
    pub const EXPANDED: Flag = Flag(7);
    pub const DRAG_OVER: Flag = Flag(8);
    pub const INVALID: Flag = Flag(9);
    pub const SEARCH_HIT: Flag = Flag(10);
    pub const CURRENT: Flag = Flag(11);
    pub const LOADING: Flag = Flag(12);
    /// An author-defined flag, `0..3`.
    pub const fn user(n: u8) -> Flag {
        assert!(n < 3, "three author flags");
        Flag(13 + n)
    }
    #[inline(always)]
    pub const fn bit(self) -> u16 {
        1 << self.0
    }
}

/// Flags driven by the pointer (every record with variants carries `KF_INTERACT`, so it is in those chains).
#[allow(dead_code)]
pub(crate) const POINTER_FLAGS: u16 = Flag::HOVERED.bit() | Flag::PRESSED.bit() | Flag::DRAG_OVER.bit();

/// What a variant sets: a fill (the box's background, a quad's colour), a border colour, a text colour.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Look {
    pub(crate) bg: Option<u32>,
    pub(crate) border: Option<u32>,
    pub(crate) color: Option<u32>,
}
impl Look {
    pub fn bg(mut self, c: Color) -> Look {
        self.bg = Some(c.0);
        self
    }
    pub fn border(mut self, c: Color) -> Look {
        self.border = Some(c.0);
        self
    }
    /// A text's colour.
    pub fn color(mut self, c: Color) -> Look {
        self.color = Some(c.0);
        self
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        for c in [self.bg, self.border, self.color] {
            match c {
                Some(c) => s.paint(c),
                None => s.word(0x7000_0001),
            }
        }
    }
}

/// One variant: while every flag of `when` is set, `look` applies (later variants over earlier ones).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Variant {
    pub(crate) when: u16,
    pub(crate) look: Look,
}

/// Variants an element holds at most.
pub const MAX_VARIANTS: usize = 6;
/// Flags a table is indexed by at most (`2^MAX_KEYS` entries); more and the variants are resolved per flip.
const MAX_KEYS: usize = 4;

/// What the table writes: a quad's instance (its fill is `color2` in a border box) or a laid-out text's colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Target {
    Quad { inst: u32, bordered: bool },
    Text { node: u32 },
}

/// A record's resolved variants (`Input::vtabs`): the base look, the variants, and the table: per value of the
/// keyed flags (bit `i` of the index is flag `keys[i]`), the (fill, second) colour words to show. The second is
/// the border of a border box or the colour of a text.
#[derive(Clone, Copy)]
pub(crate) struct VTab {
    pub target: Target,
    pub base: (u32, u32),
    pub variants: [Variant; MAX_VARIANTS],
    pub n: u8,
    pub keys: [u8; MAX_KEYS],
    pub nk: u8,
    /// The flags any variant names.
    pub mask: u16,
    pub table: [(u32, u32); 1 << MAX_KEYS],
}

impl VTab {
    fn new(target: Target, base: (u32, u32), vs: &[Variant]) -> VTab {
        let mut v = VTab { target, base, variants: [Variant::default(); MAX_VARIANTS], n: vs.len() as u8, keys: [0; MAX_KEYS], nk: 0, mask: 0, table: [(0, 0); 1 << MAX_KEYS] };
        v.variants[..vs.len()].copy_from_slice(vs);
        for x in vs {
            v.mask |= x.when;
        }
        let mut m = v.mask;
        while m != 0 && (v.nk as usize) < MAX_KEYS {
            v.keys[v.nk as usize] = m.trailing_zeros() as u8;
            v.nk += 1;
            m &= m - 1;
        }
        v.resolve_all();
        v
    }
    /// Over-many keys: the table is not used, each flip resolves (`look_of`).
    #[inline(always)]
    fn tabled(&self) -> bool {
        self.mask.count_ones() as usize <= MAX_KEYS
    }
    /// The look under effective flags `f`, by applying the variants in order.
    fn resolve(&self, f: u16) -> (u32, u32) {
        let (mut fill, mut second) = self.base;
        for x in &self.variants[..self.n as usize] {
            if f & x.when == x.when {
                if let Some(c) = x.look.bg {
                    fill = c;
                }
                match self.target {
                    Target::Quad { .. } => {
                        if let Some(c) = x.look.border {
                            second = c;
                        }
                    }
                    Target::Text { .. } => {
                        if let Some(c) = x.look.color {
                            second = c;
                        }
                    }
                }
            }
        }
        (fill, second)
    }
    fn resolve_all(&mut self) {
        if !self.tabled() {
            return;
        }
        for i in 0..1usize << self.nk {
            let mut f = 0u16;
            for k in 0..self.nk as usize {
                f |= (((i >> k) & 1) as u16) << self.keys[k];
            }
            self.table[i] = self.resolve(f);
        }
    }
    /// The look under effective flags `f`: one table load (the index gathers the keyed bits).
    #[inline(always)]
    pub(crate) fn look_of(&self, f: u16) -> (u32, u32) {
        if !self.tabled() {
            return self.resolve(f);
        }
        let mut i = 0usize;
        for k in 0..self.nk as usize {
            i |= ((f >> self.keys[k]) as usize & 1) << k;
        }
        self.table[i]
    }
}

impl Rt {
    /// Write record `a`'s look under its effective flags (a paint write when it changed).
    #[inline]
    pub(crate) fn flags_show(&mut self, a: u32) {
        let act = &self.input.acts[a as usize];
        if act.vt == NONE {
            return;
        }
        let (h, f) = (act.handle, act.eff);
        let vt = &self.input.vtabs[act.vt as usize];
        let (fill, second) = vt.look_of(f);
        match vt.target {
            Target::Quad { inst, bordered } => {
                let p = self.inst_mut(inst);
                let old = (p.color as u64) << 32 | p.color2 as u64;
                if bordered {
                    p.color2 = fill;
                    p.color = second;
                } else {
                    p.color = fill;
                }
                let now = (p.color as u64) << 32 | p.color2 as u64;
                if now != old {
                    self.paint_write(h, old, now);
                }
            }
            Target::Text { node } => {
                // the laid-out text's lines take the colour (`ApplyTextColor` with the base untouched)
                crate::boxops::text_color(self, node, second);
            }
        }
    }

    /// A new base look for record `a` (a wire or a re-description wrote the element's colour): the table is
    /// resolved again and the record shows what its flags select.
    pub(crate) fn flags_rebase(&mut self, a: u32, fill: Option<u32>, second: Option<u32>) {
        let vt = self.input.acts[a as usize].vt;
        if vt == NONE {
            return;
        }
        let t = &mut self.input.vtabs[vt as usize];
        let base = (fill.unwrap_or(t.base.0), second.unwrap_or(t.base.1));
        if base != t.base {
            t.base = base;
            t.resolve_all();
        }
        self.flags_show(a);
    }

    /// Set (`on`) or clear the flags `bits` in record `a`'s own word; with `subtree`, its descendants inherit
    /// them. Every record whose effective word changed shows its new look. **The one write path.**
    pub(crate) fn flags_write(&mut self, a: u32, bits: u16, on: bool, subtree: bool) {
        let act = &mut self.input.acts[a as usize];
        let own = if on { act.own | bits } else { act.own & !bits };
        if own == act.own && !subtree {
            return;
        }
        let h = act.handle;
        act.own = own;
        if subtree {
            act.sub = if on { act.sub | bits } else { act.sub & !bits };
            self.input.any_sub |= on;
        }
        let Some(pos) = self.bough.pos_of(h) else {
            // not in the Bough (a removed record): the words are kept, nothing to show
            return;
        };
        if !subtree {
            let act = &mut self.input.acts[a as usize];
            let eff = own | act.inh;
            if eff != act.eff {
                act.eff = eff;
                self.flags_show(a);
            }
            return;
        }
        self.flags_subtree(pos, bits);
    }

    /// Recompute the inherited and effective flags `bits` of the records of the subtree at `pos` (its root
    /// included), in preorder: a record inherits a bit when an ancestor (in the subtree, or above it) sets it
    /// for its subtree. Visits only the records with acts (`KF_INTERACT`).
    pub(crate) fn flags_subtree(&mut self, pos: usize, bits: u16) {
        let end = self.bough.subtree_end(pos);
        // what the subtree's root inherits from above it
        let mut above = 0u16;
        let mut anc = std::mem::take(&mut self.input.flag_scratch);
        self.bough.ancestors(pos, &mut anc);
        for &p in &anc {
            if let Some(h) = self.bough.handle_at(p) {
                let b = self.input.act_of_handle(h);
                if b != 0 {
                    above |= self.input.acts[b as usize].sub & bits;
                }
            }
        }
        // a stack of (depth, the bits set for the subtree below it)
        let mut stack: Vec<(u16, u16)> = std::mem::take(&mut self.input.flag_stack);
        stack.clear();
        let mut p = Some(pos);
        while let Some(q) = p.filter(|&q| q < end) {
            let d = self.bough.depth_at(q);
            while stack.last().is_some_and(|&(sd, _)| sd >= d) {
                stack.pop();
            }
            let inherited = stack.last().map_or(above, |&(_, b)| b);
            if let Some(h) = self.bough.handle_at(q) {
                let a = self.input.act_of_handle(h);
                if a != 0 {
                    let act = &mut self.input.acts[a as usize];
                    let inh = (act.inh & !bits) | inherited;
                    let eff = act.own | inh;
                    act.inh = inh;
                    let sub = act.sub & bits;
                    if eff != act.eff {
                        act.eff = eff;
                        self.flags_show(a);
                    }
                    if sub != 0 {
                        stack.push((d, inherited | sub));
                    }
                }
            }
            p = self.bough.next_flagged(Some(q), KF_INTERACT, false).filter(|&n| n > q);
        }
        self.input.flag_stack = stack;
        self.input.flag_scratch = anc;
    }

    /// Record `h`'s effective flags (0 for a record without an act).
    pub(crate) fn flags_of(&self, h: Handle) -> u16 {
        let a = self.input.act_of_handle(h);
        if a == 0 { 0 } else { self.input.acts[a as usize].eff }
    }

    /// Subtree flags that waited for their records to be in the Bough (an element's `.flag(..)` at mount, a
    /// list row mounted with item flags, a record with variants mounted where subtree flags exist): applied.
    pub(crate) fn flags_flush(&mut self) {
        while let Some((a, bits)) = self.input.flags_pending.pop() {
            let h = self.input.acts[a as usize].handle;
            if let Some(pos) = self.bough.pos_of(h) {
                self.flags_subtree(pos, bits);
            }
        }
    }

    /// A variant-controlled node's instance was rewritten from its description: show its flags' look again.
    pub(crate) fn flags_reshow_node(&mut self, node: u32) {
        if self.hot[node as usize].kind & NK_HOVERED != 0 {
            let a = self.input.act_of_handle(self.handle(node));
            if a != 0 {
                self.flags_show(a);
            }
        }
    }

    /// The border colour a description gave the variant-controlled box `node` (its table's base), or `None`
    /// when the node has no table: a re-description compares its border operand with this, not with what a
    /// variant shows.
    pub(crate) fn flags_base_border(&self, node: u32) -> Option<u32> {
        if self.hot[node as usize].kind & NK_HOVERED == 0 {
            return None;
        }
        let a = self.input.act_of_handle(self.handle(node));
        let vt = self.input.acts[a as usize].vt;
        (a != 0 && vt != NONE).then(|| self.input.vtabs[vt as usize].base.1)
    }
}

/// Give act `a` (a painted quad's) the variants `vs` (`hover_bg`'s one).
pub(crate) fn install_variants(rt: &mut Rt, a: u32, vs: &[Variant]) {
    let act = rt.input.acts[a as usize];
    let i = &rt.inst[act.inst as usize];
    let bordered = i.kind == KIND_BORDER;
    let mut base = if bordered { (i.color2, i.color) } else { (i.color, 0) };
    if act.node != NONE {
        // the described colour moves to the element's cold record (`HF_MOD`) while the instance shows what
        // the flags select; its colour writes go through the table (`flags_rebase`)
        base.0 = rt.quad_color(act.node);
        rt.mod_of(act.node);
        rt.hot[act.node as usize].kind |= NK_HOVERED;
    }
    let vt = VTab::new(Target::Quad { inst: act.inst, bordered }, base, vs);
    let k = rt.input.vtabs.len() as u32;
    rt.input.vtabs.push(vt);
    rt.input.acts[a as usize].vt = k;
    if rt.input.any_sub {
        rt.input.flags_pending.push((a, u16::MAX));
    }
}

/// A re-description's variants for act `a`: the table is rebuilt only when they changed.
pub(crate) fn refresh_variants(rt: &mut Rt, a: u32, vs: &[Variant]) {
    let vt = rt.input.acts[a as usize].vt;
    if vt == NONE {
        return;
    }
    let t = &rt.input.vtabs[vt as usize];
    if &t.variants[..t.n as usize] != vs {
        let (target, base) = (t.target, t.base);
        rt.input.vtabs[vt as usize] = VTab::new(target, base, vs);
        rt.flags_show(a);
    }
}

// =============================================================================== the element

/// An element with variants (`.when(..)`): they apply to its first record (a box's paint, a quad, a text).
pub struct When<E> {
    pub(crate) e: E,
    pub(crate) v: [Variant; MAX_VARIANTS],
    pub(crate) n: u8,
    /// Flags the description sets on the element and its subtree (`.flag(..)`): (bits, on).
    pub(crate) set: u16,
    pub(crate) unset: u16,
}

impl<E> When<E> {
    /// One more variant.
    pub fn when(mut self, f: Flag, look: impl FnOnce(Look) -> Look) -> Self {
        self.push(f.bit(), look(Look::default()));
        self
    }
    /// A variant under several flags at once (all set).
    pub fn when_all(mut self, fs: &[Flag], look: impl FnOnce(Look) -> Look) -> Self {
        self.push(fs.iter().fold(0, |m, f| m | f.bit()), look(Look::default()));
        self
    }
    fn push(&mut self, when: u16, look: Look) {
        assert!((self.n as usize) < MAX_VARIANTS, "at most {MAX_VARIANTS} variants on one element");
        self.v[self.n as usize] = Variant { when, look };
        self.n += 1;
    }
    /// Set (or clear) flag `f` on the element and its subtree from the description (`.flag(Flag::DISABLED,
    /// self.locked)`).
    pub fn flag(mut self, f: Flag, on: bool) -> Self {
        if on {
            self.set |= f.bit();
            self.unset &= !f.bit();
        } else {
            self.unset |= f.bit();
            self.set &= !f.bit();
        }
        self
    }
    pub fn hover(self, look: impl FnOnce(Look) -> Look) -> Self {
        self.when(Flag::HOVERED, look)
    }
    pub fn active(self, look: impl FnOnce(Look) -> Look) -> Self {
        self.when(Flag::PRESSED, look)
    }
    pub fn focus(self, look: impl FnOnce(Look) -> Look) -> Self {
        self.when(Flag::FOCUSED, look)
    }
    pub fn disabled(self, look: impl FnOnce(Look) -> Look) -> Self {
        self.when(Flag::DISABLED, look)
    }
    pub fn selected(self, look: impl FnOnce(Look) -> Look) -> Self {
        self.when(Flag::SELECTED, look)
    }
}

/// Variants and flags on any element: `row(..).bg(c).when(Flag::SELECTED, |s| s.bg(sel))`.
pub trait Flagged: Sized {
    fn when(self, f: Flag, look: impl FnOnce(Look) -> Look) -> When<Self> {
        When { e: self, v: [Variant::default(); MAX_VARIANTS], n: 0, set: 0, unset: 0 }.when(f, look)
    }
    fn when_all(self, fs: &[Flag], look: impl FnOnce(Look) -> Look) -> When<Self> {
        When { e: self, v: [Variant::default(); MAX_VARIANTS], n: 0, set: 0, unset: 0 }.when_all(fs, look)
    }
    fn hover(self, look: impl FnOnce(Look) -> Look) -> When<Self> {
        self.when(Flag::HOVERED, look)
    }
    fn active(self, look: impl FnOnce(Look) -> Look) -> When<Self> {
        self.when(Flag::PRESSED, look)
    }
    fn focus(self, look: impl FnOnce(Look) -> Look) -> When<Self> {
        self.when(Flag::FOCUSED, look)
    }
    fn disabled(self, look: impl FnOnce(Look) -> Look) -> When<Self> {
        self.when(Flag::DISABLED, look)
    }
    fn selected(self, look: impl FnOnce(Look) -> Look) -> When<Self> {
        self.when(Flag::SELECTED, look)
    }
    /// Flag `f` on the element and its subtree, from the description.
    fn flag(self, f: Flag, on: bool) -> When<Self> {
        When { e: self, v: [Variant::default(); MAX_VARIANTS], n: 0, set: 0, unset: 0 }.flag(f, on)
    }
    /// Flag `f` on the element and its subtree while the owner's `Signal<bool>` holds: a wire, so a write
    /// flips it with no description (`.flag_by(Flag::DISABLED, sig!(self.locked))`).
    fn flag_by<'a, V>(self, f: Flag, sig: Wired<'a, bool, V>) -> FlagBy<'a, Self, V> {
        FlagBy { e: self, f, sig }
    }
}

/// An element whose subtree carries flag `f` while a signal holds (`Flagged::flag_by`).
pub struct FlagBy<'a, E, V> {
    e: E,
    f: Flag,
    sig: Wired<'a, bool, V>,
}

/// # Safety
/// Wire contract: `sig` is a `Signal<bool>`; `w.node` is the element's act; the recipe's first byte the flag.
unsafe fn patch_flag(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: the contract.
    let on = *unsafe { (*(sig as *const crate::signal::Signal<bool>)).peek() };
    let (a, bit) = (w.node, 1u16 << w.recipe[0]);
    if rt.bough.pos_of(rt.input.acts[a as usize].handle).is_some() {
        rt.flags_write(a, bit, on, true);
    } else {
        // not in the Bough yet (mounted this turn): its subtree takes the flag at the flush
        let act = &mut rt.input.acts[a as usize];
        act.own = if on { act.own | bit } else { act.own & !bit };
        act.sub = if on { act.sub | bit } else { act.sub & !bit };
        rt.input.any_sub |= on;
        rt.input.flags_pending.push((a, bit));
    }
}

impl<'a, V: 'static, E: Element<V>> Element<V> for FlagBy<'a, E, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let before = m.recs.len();
        self.e.mount(m);
        let a = crate::input::act_for(m, before, 0);
        let bit = self.f.bit();
        if *self.sig.get() {
            let act = &mut m.rt.input.acts[a as usize];
            act.own |= bit;
            act.sub |= bit;
            m.rt.input.any_sub = true;
            m.rt.input.flags_pending.push((a, bit));
        }
        let mut r = [0u8; 16];
        r[0] = self.f.0;
        m.wire(&self.sig, patch_flag, a, r);
        m.tape_push(a);
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        self.e.update(m);
        m.tape_next();
    }
    fn dyn_key(&self) -> u64 {
        self.e.dyn_key()
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        self.e.seal(s);
        s.word(0x0310 | (self.f.0 as u32) << 16);
        s.word(self.sig.off);
    }
}
impl<E: crate::layout::Styled, V> crate::layout::Styled for FlagBy<'_, E, V> {
    fn spec_mut(&mut self) -> &mut crate::layout::Spec {
        self.e.spec_mut()
    }
}
impl<E> Flagged for E {}

impl<E: crate::layout::Styled> crate::layout::Styled for When<E> {
    fn spec_mut(&mut self) -> &mut crate::layout::Spec {
        self.e.spec_mut()
    }
}

/// The target and base look of the first record mounted since `before` (a quad's instance or a laid-out text).
fn target_of<V>(m: &Mounter<'_, V>, before: usize) -> Option<(Target, (u32, u32), u32)> {
    let rec = *m.recs.get(before)?;
    // an element's id is its record's handle index (one index space)
    let node = (rec.handle != 0 && m.rt.node_pending(rec.handle)).then_some(rec.handle);
    if rec.kind & 0x1F == REC_QUAD && rec.payload & 0xC000_0000 == 0 {
        let i = &m.rt.inst[rec.payload as usize];
        let bordered = i.kind == KIND_BORDER;
        let base = if bordered { (i.color2, i.color) } else { (i.color, 0) };
        // a node's colour is its base (what wires and re-descriptions write)
        let base = match node {
            Some(n) if !bordered => (m.rt.quad_color(n), 0),
            Some(n) => (m.rt.quad_color(n), i.color),
            None => base,
        };
        return Some((Target::Quad { inst: rec.payload, bordered }, base, node.unwrap_or(NONE)));
    }
    let n = node?;
    if m.rt.hot[n as usize].kind & 0x7F == NK_LTEXT {
        return Some((Target::Text { node: n }, (0, m.rt.texts[m.rt.text_of(n) as usize].color), n));
    }
    None
}

impl<V: 'static, E: Element<V>> Element<V> for When<E> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let before = m.recs.len();
        self.e.mount(m);
        let a = crate::input::act_for(m, before, 0);
        if self.n > 0 {
            let (target, base, node) = target_of(m, before).expect("variants need a painted box, a quad or a text");
            let vt = VTab::new(target, base, &self.v[..self.n as usize]);
            let i = m.rt.input.vtabs.len() as u32;
            m.rt.input.vtabs.push(vt);
            m.rt.input.acts[a as usize].vt = i;
            if node != NONE {
                // its colour writes (wires, re-descriptions) go through the table (`flags_rebase`); a quad's
                // described colour moves to its cold record (`HF_MOD`) while the instance shows the look
                if matches!(target, Target::Quad { .. }) {
                    m.rt.mod_of(node);
                }
                m.rt.hot[node as usize].kind |= NK_HOVERED;
            }
        }
        let act = &mut m.rt.input.acts[a as usize];
        act.own = (act.own | self.set) & !self.unset;
        act.sub = (act.sub | self.set) & !self.unset;
        act.eff = act.own | act.inh;
        m.tape_push(a);
        // the records are not in the Bough yet: the look under the initial flags is written at once (a record
        // inheriting flags gets them when its row or subtree applies them)
        let (a2, vt) = (a, m.rt.input.acts[a as usize].vt);
        if vt != NONE {
            let f = m.rt.input.acts[a2 as usize].eff;
            let (fill, second) = m.rt.input.vtabs[vt as usize].look_of(f);
            match m.rt.input.vtabs[vt as usize].target {
                Target::Quad { inst, bordered } => {
                    let p = m.rt.inst_mut(inst);
                    if bordered {
                        p.color2 = fill;
                        p.color = second;
                    } else {
                        p.color = fill;
                    }
                }
                Target::Text { node } => crate::boxops::text_color(m.rt, node, second),
            }
        }
        if self.set != 0 {
            m.rt.input.any_sub = true;
            m.rt.input.flags_pending.push((a, self.set));
        } else if m.rt.input.any_sub {
            // it may sit in a subtree whose flags it inherits
            m.rt.input.flags_pending.push((a, u16::MAX));
        }
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        self.e.update(m);
        let a = m.tape_next();
        let vt = m.rt.input.acts[a as usize].vt;
        if vt != NONE {
            let t = &m.rt.input.vtabs[vt as usize];
            if t.variants[..t.n as usize] != self.v[..self.n as usize] {
                let (target, base) = (t.target, t.base);
                m.rt.input.vtabs[vt as usize] = VTab::new(target, base, &self.v[..self.n as usize]);
                m.rt.flags_show(a);
            }
        }
        let act = &m.rt.input.acts[a as usize];
        let sub = (act.sub | self.set) & !self.unset;
        if sub != act.sub {
            let changed = sub ^ act.sub;
            m.rt.flags_write(a, changed & sub, true, true);
            m.rt.flags_write(a, changed & !sub, false, true);
        }
    }
    fn dyn_key(&self) -> u64 {
        self.e.dyn_key()
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        self.e.seal(s);
        s.word(0x0300 | (self.n as u32) << 16);
        for v in &self.v[..self.n as usize] {
            s.word(v.when as u32);
            v.look.seal(s);
        }
        s.word(self.set as u32 | (self.unset as u32) << 16);
    }
}
