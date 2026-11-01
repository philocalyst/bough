//! The per-font `SimpleScript` table: everything a table-driven shaper needs to shape
//! Latin text exactly like a full OpenType shaper, plus the *trigger* analysis that says
//! which strings it must not touch.
//!
//! # Model
//!
//! For a covered codepoint set `C` (ASCII 0x20..=0x7E and Latin-1 0xA0..=0xFF without U+00AD)
//! the table holds, per codepoint, the glyph id and the `hmtx` advance, and for every ordered
//! pair `(a, b)` in `C x C` a *pair value*: the sum, over every pair-adjustment lookup that the
//! default feature set enables for the run's script, of the first glyph's `XAdvance`
//! adjustment, or `TRIG` when the pair (or either member) can be affected by something this
//! model does not represent.
//!
//! The model is exact when no trigger fires because of the following argument, which is also
//! what the analysis implements:
//!
//! * Every GSUB/GPOS lookup that HarfBuzz would run for the default features is enumerated.
//! * A lookup can only change the glyph run if one of its rules *matches*. A rule matches
//!   only if every glyph position of the rule (backtrack, input, lookahead) is satisfiable by
//!   glyphs of `C`. A rule with an unsatisfiable position is dead (this is what discards the
//!   mark/accent contexts of `ccmp` for text without combining marks).
//! * A live rule with positions `P0..Pk` can fire only on a string that contains two adjacent
//!   characters from `Pi x Pi+1` (or one character from `P0` if `k = 0`). That adjacent pair
//!   (the cheapest one) is made a trigger. The same holds for ligature rules.
//! * Eligible strings contain no trigger, so no earlier lookup modified the run, so later
//!   lookups see the original glyphs; the argument is inductive.
//! * Pair-adjustment lookups are evaluated, not triggered, when their values are plain
//!   first-glyph `XAdvance` adjustments with default lookup flags.
//! * HarfBuzz picks the OpenType script from the text (Latin when it holds a Latin letter,
//!   `DFLT` when it is all Common characters); a pair of two Common characters whose value
//!   differs between the two scripts' lookup sets is a trigger.
//!
//! Assumptions the caller must provide (documented in the results): horizontal LTR text, no
//! language tag (or one for which the font's `latn` default language system applies), no
//! user features, no variations, no letter spacing.

use std::collections::BTreeSet;
use ttf_parser::gdef::GlyphClass;
use ttf_parser::gpos::{PairAdjustment, PositioningSubtable, ValueRecord};
use ttf_parser::gsub::SubstitutionSubtable;
use ttf_parser::opentype_layout::{
    ChainedContextLookup, ContextLookup, Coverage, LayoutTable, LookupFlags,
};
use ttf_parser::{Face, GlyphId, Tag};

/// Pair value meaning "full shaping required".
pub const TRIG: i16 = i16::MIN;

/// A set of codepoints in `0..=255`.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct CpSet(pub [u64; 4]);

impl CpSet {
    pub fn insert(&mut self, cp: u8) {
        self.0[(cp >> 6) as usize] |= 1 << (cp & 63);
    }
    pub fn contains(&self, cp: u8) -> bool {
        self.0[(cp >> 6) as usize] >> (cp & 63) & 1 != 0
    }
    pub fn len(&self) -> usize {
        self.0.iter().map(|w| w.count_ones() as usize).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.0 == [0; 4]
    }
    pub fn union_with(&mut self, o: &CpSet) {
        for i in 0..4 {
            self.0[i] |= o.0[i];
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = u8> + '_ {
        (0..=255u8).filter(move |c| self.contains(*c))
    }
}

/// What the analysis found; kept for the results document and tests.
#[derive(Default, Debug, Clone)]
pub struct BuildReport {
    pub covered: usize,
    pub single_triggers: Vec<u8>,
    pub pair_triggers: usize,
    pub pair_triggers_example: Vec<(u8, u8)>,
    pub script_dependent_pairs: usize,
    pub active_gsub_latn: Vec<u16>,
    pub active_gsub_dflt: Vec<u16>,
    pub active_gpos_latn: Vec<u16>,
    pub active_gpos_dflt: Vec<u16>,
    pub live_rules: usize,
    pub dead_rules: usize,
    pub left_classes: usize,
    pub right_classes: usize,
    pub nonzero_kern_pairs: usize,
    pub notes: Vec<String>,
}

/// The built table. All arrays are indexed by codepoint (`0..=255`); non-covered codepoints
/// hold zeros / the trigger classes.
pub struct SimpleScript {
    pub upem: u16,
    pub gid: [u16; 256],
    pub adv: [u16; 256],
    /// Row (first glyph) class of each codepoint; class 0 is the all-`TRIG` row.
    pub class1: [u8; 256],
    /// Column (second glyph) class of each codepoint; class 0 is the neutral column (also the
    /// column of the end-of-string pad).
    pub class2: [u8; 256],
    pub n1: usize,
    pub n2: usize,
    /// `n1 * n2` pair values, row-major, `TRIG` for "full shaping".
    pub matrix: Vec<i16>,
    /// Covered codepoints.
    pub covered: CpSet,
    /// Largest `adv + max positive kern` over covered codepoints (the SIMD scan needs
    /// `16 * this < 65536`).
    pub max_step: u32,
    pub report: BuildReport,
    /// The token model (ligatures and Latin-1 in one pair table), when the font's live rules
    /// are all modelled ligatures. See [`TokenTable`].
    pub tokens: Option<TokenTable>,
}

/// Token pad (end of string / no next glyph).
pub const TOK_PAD: u8 = 0;
/// Phantom slot: a byte that belongs to the previous glyph's cluster (UTF-8 continuation byte,
/// ligature component after the first). Zero step, no glyph.
pub const TOK_PH: u8 = 1;
/// First ligature token; ligature `k` (application order) is `TOK_LIG0 + k`.
pub const TOK_LIG0: u8 = 2;

/// The token model: the run is rewritten into one token per glyph (ASCII byte, Latin-1
/// codepoint, or ligature), and every glyph's pen step is `step[t << 8 | next]`, exactly like
/// the ASCII dense table but over a 256-token alphabet that includes the ligature glyphs.
///
/// Exactness argument (on top of the module-level one): the only live GSUB rules are the
/// recorded ligatures of a single lookup `L`, applied by HarfBuzz left to right, at each glyph
/// trying the font's ligatures for that glyph in order (we replay exactly that); no active
/// GSUB lookup runs after `L` (so nothing sees the ligature glyphs); no active non-pair,
/// non-mark GPOS lookup covers a ligature glyph (mark lookups need a mark, and no covered
/// glyph is a mark); pairs involving a ligature glyph are evaluated with the `latn` lookups
/// (every ligature component is a Latin letter, so such a run is Latin) and are `TRIG` when
/// the value is not plain first-glyph `XAdvance`. All other pair values are the matrix's.
pub struct TokenTable {
    /// `step[t << 8 | u]`: `adv(t) + kern(t, u)`, or `0xFFFF` (needs full shaping).
    /// `u = TOK_PAD` is the end of the run. Row `TOK_PH` is zero.
    pub step: Box<[u16]>,
    pub gid: [u16; 256],
    /// `(first byte, remaining component bytes, token)` in application order.
    pub ligs: Vec<(u8, Vec<u8>, u8)>,
    /// `lig_first[b]`: some ligature starts with ASCII byte `b`.
    pub lig_first: [bool; 128],
    pub max_step: u32,
}

#[allow(clippy::too_many_arguments)]
fn build_tokens(
    b: &Builder,
    cps: &[u8],
    adv: &[u16; 256],
    m: &[Vec<i32>],
    pv_trig: &[Vec<bool>],
    single: &CpSet,
    gsub_latn: &BTreeSet<u16>,
    gpos_latn: &BTreeSet<u16>,
    script_dep: usize,
) -> Option<TokenTable> {
    // Conditions under which the ligature replay is the whole GSUB story.
    if !single.is_empty() || script_dep != 0 || b.ligs.is_empty() {
        return None;
    }
    let lookup = b.ligs[0].0;
    if b.ligs.iter().any(|l| l.0 != lookup) || !gsub_latn.contains(&lookup) {
        return None;
    }
    if b.rep.live_rules != b.ligs.len() {
        return None; // some other rule is live: its triggers must stay
    }
    if gsub_latn.iter().any(|&i| i > lookup) {
        return None; // a later GSUB lookup could see the ligature glyphs
    }
    let mut ligs = vec![];
    for (k, (_, first, comps, gid)) in b.ligs.iter().enumerate() {
        let rest: Option<Vec<u8>> = comps.iter().copied().collect();
        let rest = rest?;
        if *first >= 0x80 || rest.iter().any(|&c| c >= 0x80 || !is_latin_letter(c)) || !is_latin_letter(*first) {
            return None;
        }
        if rest.is_empty() {
            return None;
        }
        ligs.push((*first, rest, *gid, TOK_LIG0.checked_add(k as u8)?));
        if TOK_LIG0 as usize + k >= 0x20 {
            return None;
        }
    }
    let lig_gids: Vec<GlyphId> = ligs.iter().map(|l| GlyphId(l.2)).collect();
    // GPOS: what may touch a ligature glyph.
    let gpos = b.face.tables().gpos;
    if let Some(t) = gpos.as_ref() {
        for &li in gpos_latn {
            let l = t.lookups.get(li)?;
            let conservative = flags_conservative(l.flags);
            for si in 0..l.subtables.len() {
                let st = l.subtables.get::<PositioningSubtable>(si)?;
                let cov_hits = |c: &Coverage| lig_gids.iter().any(|&g| c.contains(g));
                match st {
                    PositioningSubtable::Pair(p) => {
                        if conservative && cov_hits(&p.coverage()) {
                            return None;
                        }
                    }
                    PositioningSubtable::Single(s) => {
                        if cov_hits(&s.coverage()) {
                            return None;
                        }
                    }
                    PositioningSubtable::Cursive(c) => {
                        if cov_hits(&c.coverage) {
                            return None;
                        }
                    }
                    PositioningSubtable::MarkToBase(_)
                    | PositioningSubtable::MarkToLigature(_)
                    | PositioningSubtable::MarkToMark(_) => {}
                    PositioningSubtable::Context(_) | PositioningSubtable::ChainContext(_) => return None,
                }
            }
        }
    }
    let face = b.face;
    let latn_l: Vec<u16> = gpos_latn.iter().copied().collect();
    // token -> (gid, adv, Some(cp) for codepoint tokens)
    let mut tgid = [0u16; 256];
    let mut tadv = [0u32; 256];
    let mut valid = [false; 256];
    for &c in cps {
        tgid[c as usize] = b.gid_of[c as usize];
        tadv[c as usize] = adv[c as usize] as u32;
        valid[c as usize] = true;
    }
    for l in &ligs {
        tgid[l.3 as usize] = l.2;
        tadv[l.3 as usize] = face.glyph_hor_advance(GlyphId(l.2))? as u32;
        valid[l.3 as usize] = true;
    }
    let lig_tok = |t: usize| ligs.iter().find(|l| l.3 as usize == t);
    let kern = |t: usize, u: usize| -> Option<i32> {
        if u == TOK_PAD as usize {
            return Some(0);
        }
        match (lig_tok(t), lig_tok(u)) {
            (None, None) => {
                if pv_trig[t][u] { None } else { Some(m[t][u]) }
            }
            _ => b.pair_value_g(&latn_l, GlyphId(tgid[t]), GlyphId(tgid[u])),
        }
    };
    let mut step = vec![0xFFFFu16; 65536].into_boxed_slice();
    let mut max_step = 0u32;
    for t in 0..256usize {
        if t == TOK_PH as usize {
            for u in 0..256 {
                step[t << 8 | u] = 0;
            }
            continue;
        }
        if !valid[t] {
            continue;
        }
        for u in 0..256usize {
            if u != TOK_PAD as usize && !valid[u] {
                continue;
            }
            if let Some(k) = kern(t, u) {
                let v = tadv[t] as i32 + k;
                if !(0..0x1000).contains(&v) {
                    return None;
                }
                step[t << 8 | u] = v as u16;
                max_step = max_step.max(v as u32);
            }
        }
    }
    // The original ligature triggers (a, c) must be pairs that never survive the replay: the
    // token table keeps them TRIG, so if the replay were wrong the run would be refused.
    for l in &ligs {
        step[(l.0 as usize) << 8 | l.1[0] as usize] = 0xFFFF;
    }
    let mut lig_first = [false; 128];
    for l in &ligs {
        lig_first[l.0 as usize] = true;
    }
    Some(TokenTable {
        step,
        gid: tgid,
        ligs: ligs.iter().map(|l| (l.0, l.1.clone(), l.3)).collect(),
        lig_first,
        max_step,
    })
}

struct Builder<'a> {
    face: &'a Face<'a>,
    cps: Vec<u8>,
    gid_of: [u16; 256],
    covered: CpSet,
    single: CpSet,
    pair: Vec<CpSet>, // pair[a] = set of b
    rep: BuildReport,
    unsupported: bool,
    /// Every ligature rule seen in an active GSUB lookup (plain flags), in application order:
    /// (lookup index, first codepoint, component codepoints or `None` if a component is not a
    /// covered glyph, ligature glyph).
    ligs: Vec<(u16, u8, Vec<Option<u8>>, u16)>,
}

fn tag(b: &[u8; 4]) -> Tag {
    Tag::from_bytes(b)
}

const DEFAULT_FEATURES: [&[u8; 4]; 15] = [
    b"ccmp", b"locl", b"mark", b"mkmk", b"rlig", b"calt", b"clig", b"curs", b"dist", b"kern",
    b"liga", b"rclt", b"rvrn", b"ltra", b"ltrm",
];

/// Lookup indices enabled by the default features for the default language system of the
/// first script found among `scripts`.
fn active_lookups(t: &LayoutTable, scripts: &[&[u8; 4]]) -> BTreeSet<u16> {
    let mut out = BTreeSet::new();
    for s in scripts {
        let Some(script) = t.scripts.find(tag(s)) else { continue };
        if let Some(ls) = script.default_language {
            let mut idx: Vec<u16> = ls.feature_indices.into_iter().collect();
            if let Some(r) = ls.required_feature {
                idx.push(r);
            }
            for fi in idx {
                let f = t.features.get(fi).unwrap();
                let required = ls.required_feature == Some(fi);
                if required || DEFAULT_FEATURES.iter().any(|d| tag(d) == f.tag) {
                    out.extend(f.lookup_indices);
                }
            }
        }
        break;
    }
    out
}

fn value_is_plain_xadv(v: &ValueRecord) -> Option<i32> {
    if v.x_placement != 0
        || v.y_placement != 0
        || v.y_advance != 0
        || v.x_placement_device.is_some()
        || v.y_placement_device.is_some()
        || v.x_advance_device.is_some()
        || v.y_advance_device.is_some()
    {
        None
    } else {
        Some(v.x_advance as i32)
    }
}

fn value_is_zero(v: &ValueRecord) -> bool {
    value_is_plain_xadv(v) == Some(0)
}

enum PairEval {
    /// No subtable applied.
    None,
    /// Applied with this first-glyph advance adjustment.
    Adj(i32),
    /// Applied with something the table cannot represent.
    Unsupported,
}

fn eval_pair_subtable(st: &PairAdjustment, a: GlyphId, b: GlyphId) -> PairEval {
    let (v1, v2) = match st {
        PairAdjustment::Format1 { coverage, sets } => {
            let Some(ci) = coverage.get(a) else { return PairEval::None };
            let Some(set) = sets.get(ci) else { return PairEval::None };
            match set.get(b) {
                Some(p) => p,
                None => return PairEval::None,
            }
        }
        PairAdjustment::Format2 { coverage, classes, matrix } => {
            if !coverage.contains(a) {
                return PairEval::None;
            }
            match matrix.get((classes.0.get(a), classes.1.get(b))) {
                Some(p) => p,
                None => return PairEval::None,
            }
        }
    };
    if !value_is_zero(&v2) {
        return PairEval::Unsupported;
    }
    match value_is_plain_xadv(&v1) {
        Some(x) => PairEval::Adj(x),
        None => PairEval::Unsupported,
    }
}

/// Which of the covered glyphs are in this coverage table.
fn flags_conservative(f: LookupFlags) -> bool {
    // Ignore-base / ignore-ligature flags make the matcher skip covered glyphs.
    f.0 & 0x0006 != 0
}

impl<'a> Builder<'a> {
    fn cps_where(&self, f: impl Fn(GlyphId) -> bool) -> CpSet {
        let mut s = CpSet::default();
        for &c in &self.cps {
            if f(GlyphId(self.gid_of[c as usize])) {
                s.insert(c);
            }
        }
        s
    }
    fn cov_set(&self, c: &Coverage) -> CpSet {
        self.cps_where(|g| c.contains(g))
    }
    fn glyph_set(&self, g: u16) -> CpSet {
        self.cps_where(|x| x.0 == g)
    }

    fn mark_single(&mut self, s: &CpSet) {
        self.single.union_with(s);
    }

    fn mark_window(&mut self, positions: Vec<CpSet>) {
        if positions.iter().any(|p| p.is_empty()) {
            self.rep.dead_rules += 1;
            return;
        }
        self.rep.live_rules += 1;
        if positions.len() == 1 {
            let p = positions[0];
            self.mark_single(&p);
            return;
        }
        let mut best = 0;
        let mut best_cost = usize::MAX;
        for i in 0..positions.len() - 1 {
            let cost = positions[i].len() * positions[i + 1].len();
            if cost < best_cost {
                best = i;
                best_cost = cost;
            }
        }
        let (p, q) = (positions[best], positions[best + 1]);
        for a in p.iter() {
            self.pair[a as usize].union_with(&q);
        }
    }

    fn context(&mut self, cl: &ContextLookup) {
        match cl {
            ContextLookup::Format1 { coverage, sets } => {
                for &c in &self.cps.clone() {
                    let g = GlyphId(self.gid_of[c as usize]);
                    let Some(ci) = coverage.get(g) else { continue };
                    let Some(set) = sets.get(ci) else { continue };
                    for ri in 0..set.len() {
                        let Some(rule) = set.get(ri) else { continue };
                        let mut pos = vec![self.glyph_set(g.0)];
                        pos.extend(rule.input.into_iter().map(|x| self.glyph_set(x)));
                        self.mark_window(pos);
                    }
                }
            }
            ContextLookup::Format2 { coverage, classes, sets } => {
                for k in 0..sets.len() {
                    let Some(set) = sets.get(k) else { continue };
                    let first = self.cps_where(|g| coverage.contains(g) && classes.get(g) == k);
                    for ri in 0..set.len() {
                        let Some(rule) = set.get(ri) else { continue };
                        let mut pos = vec![first];
                        pos.extend(rule.input.into_iter().map(|c| self.cps_where(|g| classes.get(g) == c)));
                        self.mark_window(pos);
                    }
                }
            }
            ContextLookup::Format3 { coverage, coverages, .. } => {
                let mut pos = vec![self.cov_set(coverage)];
                for i in 0..coverages.len() {
                    match coverages.get(i) {
                        Some(c) => pos.push(self.cov_set(&c)),
                        None => self.unsupported = true,
                    }
                }
                self.mark_window(pos);
            }
        }
    }

    fn chained(&mut self, cl: &ChainedContextLookup) {
        match cl {
            ChainedContextLookup::Format1 { coverage, sets } => {
                for &c in &self.cps.clone() {
                    let g = GlyphId(self.gid_of[c as usize]);
                    let Some(ci) = coverage.get(g) else { continue };
                    let Some(set) = sets.get(ci) else { continue };
                    for ri in 0..set.len() {
                        let Some(rule) = set.get(ri) else { continue };
                        let mut pos: Vec<CpSet> =
                            rule.backtrack.into_iter().map(|x| self.glyph_set(x)).collect();
                        pos.reverse();
                        pos.push(self.glyph_set(g.0));
                        pos.extend(rule.input.into_iter().map(|x| self.glyph_set(x)));
                        pos.extend(rule.lookahead.into_iter().map(|x| self.glyph_set(x)));
                        self.mark_window(pos);
                    }
                }
            }
            ChainedContextLookup::Format2 {
                coverage,
                backtrack_classes,
                input_classes,
                lookahead_classes,
                sets,
            } => {
                for k in 0..sets.len() {
                    let Some(set) = sets.get(k) else { continue };
                    let first = self.cps_where(|g| coverage.contains(g) && input_classes.get(g) == k);
                    for ri in 0..set.len() {
                        let Some(rule) = set.get(ri) else { continue };
                        let mut pos: Vec<CpSet> = rule
                            .backtrack
                            .into_iter()
                            .map(|c| self.cps_where(|g| backtrack_classes.get(g) == c))
                            .collect();
                        pos.reverse();
                        pos.push(first);
                        pos.extend(rule.input.into_iter().map(|c| self.cps_where(|g| input_classes.get(g) == c)));
                        pos.extend(
                            rule.lookahead.into_iter().map(|c| self.cps_where(|g| lookahead_classes.get(g) == c)),
                        );
                        self.mark_window(pos);
                    }
                }
            }
            ChainedContextLookup::Format3 {
                coverage,
                backtrack_coverages,
                input_coverages,
                lookahead_coverages,
                ..
            } => {
                let mut pos = vec![];
                let mut bt = vec![];
                for i in 0..backtrack_coverages.len() {
                    match backtrack_coverages.get(i) {
                        Some(c) => bt.push(self.cov_set(&c)),
                        None => self.unsupported = true,
                    }
                }
                bt.reverse();
                pos.extend(bt);
                pos.push(self.cov_set(coverage));
                for i in 0..input_coverages.len() {
                    match input_coverages.get(i) {
                        Some(c) => pos.push(self.cov_set(&c)),
                        None => self.unsupported = true,
                    }
                }
                for i in 0..lookahead_coverages.len() {
                    match lookahead_coverages.get(i) {
                        Some(c) => pos.push(self.cov_set(&c)),
                        None => self.unsupported = true,
                    }
                }
                self.mark_window(pos);
            }
        }
    }

    fn gsub_lookup(&mut self, idx: u16) {
        let t = self.face.tables().gsub.unwrap();
        let l = t.lookups.get(idx).unwrap();
        let conservative = flags_conservative(l.flags);
        for si in 0..l.subtables.len() {
            let Some(st) = l.subtables.get::<SubstitutionSubtable>(si) else {
                self.unsupported = true;
                continue;
            };
            if conservative {
                let s = self.cov_set(&st.coverage());
                self.mark_single(&s);
                continue;
            }
            match st {
                SubstitutionSubtable::Single(s) => {
                    // A single substitution that maps a glyph to itself is a no-op.
                    let cov = s.coverage();
                    let mut set = CpSet::default();
                    for &c in &self.cps {
                        let g = GlyphId(self.gid_of[c as usize]);
                        let Some(ci) = cov.get(g) else { continue };
                        let out = match &s {
                            ttf_parser::gsub::SingleSubstitution::Format1 { delta, .. } => {
                                GlyphId((g.0 as i32 + *delta as i32) as u16)
                            }
                            ttf_parser::gsub::SingleSubstitution::Format2 { substitutes, .. } => {
                                substitutes.get(ci).unwrap_or(g)
                            }
                        };
                        if out != g {
                            set.insert(c);
                        }
                    }
                    self.mark_single(&set);
                }
                SubstitutionSubtable::Multiple(m) => {
                    let s = self.cov_set(&m.coverage);
                    self.mark_single(&s);
                }
                SubstitutionSubtable::Alternate(a) => {
                    let s = self.cov_set(&a.coverage);
                    self.mark_single(&s);
                }
                SubstitutionSubtable::Ligature(lg) => {
                    for &c in &self.cps.clone() {
                        let g = GlyphId(self.gid_of[c as usize]);
                        let Some(ci) = lg.coverage.get(g) else { continue };
                        let Some(set) = lg.ligature_sets.get(ci) else { continue };
                        for li in 0..set.len() {
                            let Some(lig) = set.get(li) else { continue };
                            let mut pos = vec![self.glyph_set(g.0)];
                            pos.extend(lig.components.into_iter().map(|x| self.glyph_set(x.0)));
                            // Record it for the token model (a component that is not exactly one
                            // covered codepoint makes the model refuse the font).
                            let comps: Vec<Option<u8>> = lig
                                .components
                                .into_iter()
                                .map(|x| {
                                    let set = self.glyph_set(x.0);
                                    if set.len() == 1 { set.iter().next() } else { None }
                                })
                                .collect();
                            self.ligs.push((idx, c, comps, lig.glyph.0));
                            self.mark_window(pos);
                        }
                    }
                }
                SubstitutionSubtable::Context(c) => self.context(&c),
                SubstitutionSubtable::ChainContext(c) => self.chained(&c),
                SubstitutionSubtable::ReverseChainSingle(r) => {
                    let mut pos: Vec<CpSet> = vec![];
                    let mut bt = vec![];
                    for i in 0..r.backtrack_coverages.len() {
                        match r.backtrack_coverages.get(i) {
                            Some(c) => bt.push(self.cov_set(&c)),
                            None => self.unsupported = true,
                        }
                    }
                    bt.reverse();
                    pos.extend(bt);
                    pos.push(self.cov_set(&r.coverage));
                    for i in 0..r.lookahead_coverages.len() {
                        match r.lookahead_coverages.get(i) {
                            Some(c) => pos.push(self.cov_set(&c)),
                            None => self.unsupported = true,
                        }
                    }
                    self.mark_window(pos);
                }
            }
        }
    }

    /// Non-pair GPOS lookups are triggers; pair lookups are modelled by `pair_value`.
    fn gpos_lookup_triggers(&mut self, idx: u16) {
        let t = self.face.tables().gpos.unwrap();
        let l = t.lookups.get(idx).unwrap();
        let conservative = flags_conservative(l.flags);
        for si in 0..l.subtables.len() {
            let Some(st) = l.subtables.get::<PositioningSubtable>(si) else {
                self.unsupported = true;
                continue;
            };
            match st {
                PositioningSubtable::Pair(p) => {
                    if conservative {
                        let s = self.cov_set(&p.coverage());
                        self.mark_single(&s);
                    }
                    // Otherwise evaluated in `pair_value`.
                }
                PositioningSubtable::Single(s) => {
                    let set = self.cov_set(&s.coverage());
                    self.mark_single(&set);
                }
                PositioningSubtable::Cursive(c) => {
                    let s = self.cov_set(&c.coverage);
                    self.mark_single(&s);
                }
                PositioningSubtable::MarkToBase(m) => {
                    let s = self.cov_set(&m.mark_coverage);
                    self.mark_single(&s);
                }
                PositioningSubtable::MarkToLigature(m) => {
                    let s = self.cov_set(&m.mark_coverage);
                    self.mark_single(&s);
                }
                PositioningSubtable::MarkToMark(m) => {
                    let s = self.cov_set(&m.mark1_coverage);
                    self.mark_single(&s);
                }
                PositioningSubtable::Context(c) => self.context(&c),
                PositioningSubtable::ChainContext(c) => self.chained(&c),
            }
        }
    }

    /// Sum of pair adjustments of `lookups` for the glyph pair, or `None` = trigger.
    fn pair_value(&self, lookups: &[u16], a: u8, b: u8) -> Option<i32> {
        self.pair_value_g(lookups, GlyphId(self.gid_of[a as usize]), GlyphId(self.gid_of[b as usize]))
    }

    /// `pair_value` for arbitrary glyphs (ligature glyphs of the token model).
    fn pair_value_g(&self, lookups: &[u16], ga: GlyphId, gb: GlyphId) -> Option<i32> {
        let t = self.face.tables().gpos?;
        let mut sum = 0;
        for &li in lookups {
            let l = t.lookups.get(li).unwrap();
            if !matches!(l.subtables.get::<PositioningSubtable>(0), Some(PositioningSubtable::Pair(_))) {
                continue;
            }
            if flags_conservative(l.flags) {
                continue; // coverage already made single triggers
            }
            for si in 0..l.subtables.len() {
                let Some(PositioningSubtable::Pair(p)) = l.subtables.get::<PositioningSubtable>(si) else {
                    continue;
                };
                match eval_pair_subtable(&p, ga, gb) {
                    PairEval::None => continue,
                    PairEval::Adj(x) => {
                        sum += x;
                        break;
                    }
                    PairEval::Unsupported => return None,
                }
            }
        }
        Some(sum)
    }
}

/// Latin-script letters inside the covered range (Unicode `Script=Latin`).
pub fn is_latin_letter(cp: u8) -> bool {
    matches!(cp, b'A'..=b'Z' | b'a'..=b'z' | 0xAA | 0xBA | 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0xFF)
}

impl SimpleScript {
    /// Build the table from font bytes. Returns `None` for fonts the model cannot cover
    /// exactly (legacy `kern` only, feature variations, unparsable lookups, advances too wide
    /// for the 16-bit SIMD scan).
    pub fn build(data: &[u8], face_index: u32) -> Option<Self> {
        let face = Face::parse(data, face_index).ok()?;
        let upem = face.units_per_em();
        let mut rep = BuildReport::default();

        // Covered set.
        let mut cps = vec![];
        let mut gid_of = [0u16; 256];
        let mut adv = [0u16; 256];
        let mut covered = CpSet::default();
        let gdef = face.tables().gdef;
        for cp in (0x20u8..=0x7E).chain(0xA0..=0xFF) {
            if cp == 0xAD {
                continue; // default-ignorable: HarfBuzz hides it
            }
            let Some(g) = face.glyph_index(cp as char) else { continue };
            if g.0 == 0 {
                continue;
            }
            let Some(a) = face.glyph_hor_advance(g) else { continue };
            if gdef.is_some_and(|d| d.glyph_class(g) == Some(GlyphClass::Mark)) {
                continue; // marks are never eligible
            }
            cps.push(cp);
            gid_of[cp as usize] = g.0;
            adv[cp as usize] = a;
            covered.insert(cp);
        }
        rep.covered = cps.len();

        let mut b = Builder {
            face: &face,
            cps: cps.clone(),
            gid_of,
            covered,
            single: CpSet::default(),
            pair: vec![CpSet::default(); 256],
            rep: rep.clone(),
            unsupported: false,
            ligs: vec![],
        };

        // Active lookups. A run is Latin when it has a Latin letter, otherwise DFLT.
        let latn: [&[u8; 4]; 3] = [b"latn", b"DFLT", b"dflt"];
        let dflt: [&[u8; 4]; 3] = [b"DFLT", b"dflt", b"latn"];
        let gsub = face.tables().gsub;
        let gpos = face.tables().gpos;
        if gsub.as_ref().is_some_and(|t| t.variations.is_some())
            || gpos.as_ref().is_some_and(|t| t.variations.is_some())
        {
            return None;
        }
        let gsub_latn = gsub.as_ref().map(|t| active_lookups(t, &latn)).unwrap_or_default();
        let gsub_dflt = gsub.as_ref().map(|t| active_lookups(t, &dflt)).unwrap_or_default();
        let gpos_latn = gpos.as_ref().map(|t| active_lookups(t, &latn)).unwrap_or_default();
        let gpos_dflt = gpos.as_ref().map(|t| active_lookups(t, &dflt)).unwrap_or_default();
        b.rep.active_gsub_latn = gsub_latn.iter().copied().collect();
        b.rep.active_gsub_dflt = gsub_dflt.iter().copied().collect();
        b.rep.active_gpos_latn = gpos_latn.iter().copied().collect();
        b.rep.active_gpos_dflt = gpos_dflt.iter().copied().collect();

        // Legacy kern is used by HarfBuzz only when GPOS has no kern feature.
        let gpos_has_kern = gpos.as_ref().is_some_and(|t| {
            (0..t.features.len()).any(|i| t.features.get(i).is_some_and(|f| f.tag == tag(b"kern")))
        });
        if face.tables().kern.is_some() && !gpos_has_kern {
            return None;
        }

        let gsub_all: BTreeSet<u16> = gsub_latn.union(&gsub_dflt).copied().collect();
        let gpos_all: BTreeSet<u16> = gpos_latn.union(&gpos_dflt).copied().collect();
        for &i in &gsub_all {
            b.gsub_lookup(i);
        }
        for &i in &gpos_all {
            b.gpos_lookup_triggers(i);
        }
        if b.unsupported {
            return None;
        }

        // Pair values for Latin and Common runs.
        let latn_l: Vec<u16> = gpos_latn.iter().copied().collect();
        let dflt_l: Vec<u16> = gpos_dflt.iter().copied().collect();
        // Index 256 is the end-of-string pad column.
        let mut m = vec![vec![0i32; 257]; 256]; // m[a][b]
        let mut trig = vec![vec![false; 257]; 256];
        let mut script_dep = 0;
        let mut max_pos_kern = 0i32;
        for &a in &cps {
            for &c in &cps {
                let kl = b.pair_value(&latn_l, a, c);
                let kd = b.pair_value(&dflt_l, a, c);
                let common = !is_latin_letter(a) && !is_latin_letter(c);
                let v = if common {
                    match (kl, kd) {
                        (Some(x), Some(y)) if x == y => Some(x),
                        _ => {
                            script_dep += (kl != kd) as usize;
                            None
                        }
                    }
                } else {
                    kl
                };
                match v {
                    Some(x) if x > i16::MIN as i32 && x <= i16::MAX as i32 => {
                        m[a as usize][c as usize] = x;
                        max_pos_kern = max_pos_kern.max(x);
                    }
                    _ => trig[a as usize][c as usize] = true,
                }
            }
        }
        b.rep.script_dependent_pairs = script_dep;

        // Pair-value triggers only (before the GSUB-rule triggers are folded in): the token
        // model removes the ligature triggers but must keep these.
        let pv_trig = trig.clone();
        // Fold in the trigger analysis.
        let mut single = b.single;
        for &a in &cps {
            for &c in &cps {
                if b.pair[a as usize].contains(c) {
                    trig[a as usize][c as usize] = true;
                }
            }
        }
        // Rows: non-covered => all TRIG; single trigger => all TRIG.
        let mut row_trig = [true; 256];
        for &a in &cps {
            row_trig[a as usize] = single.contains(a);
        }
        for a in 0..256 {
            if row_trig[a] {
                for c in 0..257 {
                    trig[a][c] = true;
                }
            }
        }
        single = CpSet::default();
        for a in 0..256 {
            if row_trig[a] && b.covered.contains(a as u8) {
                single.insert(a as u8);
            }
        }
        b.rep.single_triggers = single.iter().collect();
        let mut ptc = 0;
        for &a in &cps {
            if row_trig[a as usize] {
                continue;
            }
            for &c in &cps {
                if trig[a as usize][c as usize] {
                    ptc += 1;
                    if b.rep.pair_triggers_example.len() < 24 {
                        b.rep.pair_triggers_example.push((a, c));
                    }
                }
            }
        }
        b.rep.pair_triggers = ptc;

        // Value at (a, col) with TRIG folded in.
        let val = |a: usize, c: usize| -> i16 {
            if trig[a][c] { TRIG } else { m[a][c] as i16 }
        };

        // Row classes: class 0 = all-TRIG. Dedupe identical rows.
        let mut rows: Vec<Vec<i16>> = vec![vec![TRIG; 257]];
        let mut class1 = [0u8; 256];
        for a in 0..256 {
            let row: Vec<i16> = (0..257).map(|c| val(a, c)).collect();
            let id = match rows.iter().position(|r| *r == row) {
                Some(i) => i,
                None => {
                    rows.push(row);
                    rows.len() - 1
                }
            };
            class1[a] = u8::try_from(id).ok()?;
        }
        // Column classes: class 0 = the pad column (zero for every non-TRIG row).
        let col_of = |c: usize| -> Vec<i16> { rows.iter().map(|r| r[c]).collect() };
        let mut cols: Vec<Vec<i16>> = vec![col_of(256)];
        let mut class2 = [0u8; 256];
        for &c in &cps {
            let col = col_of(c as usize);
            let id = match cols.iter().position(|r| *r == col) {
                Some(i) => i,
                None => {
                    cols.push(col);
                    cols.len() - 1
                }
            };
            class2[c as usize] = u8::try_from(id).ok()?;
        }
        let (n1, n2) = (rows.len(), cols.len());
        let mut matrix = vec![0i16; n1 * n2];
        for r in 0..n1 {
            for c in 0..n2 {
                matrix[r * n2 + c] = cols[c][r];
            }
        }
        // Row 0 must be all TRIG for the pad/unknown lanes.
        debug_assert!(matrix[..n2].iter().all(|&v| v == TRIG));
        b.rep.left_classes = n1;
        b.rep.right_classes = n2;
        b.rep.nonzero_kern_pairs = cps
            .iter()
            .map(|&a| cps.iter().filter(|&&c| !trig[a as usize][c as usize] && m[a as usize][c as usize] != 0).count())
            .sum();

        let mut max_adv = 0u32;
        for &c in &cps {
            max_adv = max_adv.max(adv[c as usize] as u32);
        }
        let max_step = max_adv + max_pos_kern.max(0) as u32;
        if max_step * 16 >= 65536 {
            return None;
        }
        // Minimum step must be non-negative for the u16 scan.
        for &a in &cps {
            for &c in &cps {
                if !trig[a as usize][c as usize]
                    && adv[a as usize] as i32 + m[a as usize][c as usize] < 0
                {
                    return None;
                }
            }
        }

        // Class-0 col for pad must be neutral: verified by construction (col_of(256) is
        // zero for non-TRIG rows).
        let _ = &b.covered;
        let tokens = build_tokens(&b, &cps, &adv, &m, &pv_trig, &single, &gsub_latn, &gpos_latn, script_dep);
        b.rep.notes.push(match &tokens {
            Some(t) => format!("token model: {} ligatures, max token step {}", t.ligs.len(), t.max_step),
            None => "token model: unsupported for this font".into(),
        });
        Some(SimpleScript {
            upem,
            gid: gid_of,
            adv,
            class1,
            class2,
            n1,
            n2,
            matrix,
            covered,
            max_step,
            report: b.rep,
            tokens,
        })
    }

    /// Reference kern lookup for scalar code and tests (`TRIG` possible).
    #[inline]
    pub fn pair(&self, a: u8, b: u8) -> i16 {
        self.matrix[self.class1[a as usize] as usize * self.n2 + self.class2[b as usize] as usize]
    }
}
