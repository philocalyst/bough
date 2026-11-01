//! Change traces: a workload is a pure function `tick -> full scene` over a
//! fixed, paint-ordered slot array. Diffing consecutive scenes yields the
//! write-time events a retained framework would know: per node, the old and
//! new state of each of its slots, and per scroll group, the move.

use crate::geom::R;

/// Primitive kinds. The software renderer's pixel function per kind decides
/// what "exact" damage is (see `render::shade`).
pub const SOLID: u8 = 0; // opaque, uniform colour: pixel = key
pub const TINT: u8 = 1; // translucent, uniform: pixel = mix(below, key)
pub const ROUND: u8 = 2; // rounded rect: uniform interior, corner squares depend on corner offset
pub const GLYPH: u8 = 3; // translucent, position-dependent content (local offset from ink origin)

/// One slot's full state. `ink` is unclipped geometry (its origin anchors
/// position-dependent content); `cov = ink ∩ clip ∩ viewport` is what it paints.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PState {
    pub ink: R,
    pub cov: R,
    pub key: u32,
    pub kind: u8,
    pub rad: u8,
}

impl PState {
    pub const EMPTY: PState = PState { ink: R::E, cov: R::E, key: 0, kind: 0, rad: 0 };
}

#[derive(Clone, Debug)]
pub struct NodeDef {
    pub slot0: u32,
    pub n: u32,
    pub text: bool,
    pub group: u32,
}

#[derive(Clone, Debug)]
pub struct GroupDef {
    pub clip: R,
    pub nodes: std::ops::Range<u32>,
}

/// A node changed: slots `slot0..slot0+n`; `olds/news[at..at+n]` hold their states.
#[derive(Clone, Copy, Debug)]
pub struct NodeEv {
    pub slot0: u32,
    pub n: u32,
    pub at: u32,
    pub text: bool,
    pub old_b: R,
    pub new_b: R,
}

/// A scroll group moved by (dx, dy): `slots[at..at+n]` lists the slots that
/// were or are non-empty, with `olds/news` aligned (`gold/gnew`).
#[derive(Clone, Copy, Debug)]
pub struct GroupEv {
    pub at: u32,
    pub n: u32,
    pub dx: i32,
    pub dy: i32,
    pub clip: R,
    pub old_b: R,
    pub new_b: R,
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub nodes: Vec<NodeEv>,
    pub olds: Vec<PState>,
    pub news: Vec<PState>,
    pub groups: Vec<GroupEv>,
    pub gslots: Vec<u32>,
    pub gold: Vec<PState>,
    pub gnew: Vec<PState>,
}

impl Frame {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.groups.is_empty()
    }
    pub fn events(&self) -> usize {
        self.nodes.len() + self.groups.len()
    }
}

pub struct Trace {
    pub name: String,
    pub w: i32,
    pub h: i32,
    /// Slot capacity, padded to a multiple of 64.
    pub slots: usize,
    pub kinds: Vec<u8>,
    pub init: Vec<PState>,
    pub frames: Vec<Frame>,
    pub nodes: Vec<NodeDef>,
    pub groups: Vec<GroupDef>,
}

/// Scene under construction: the workload writes every slot each tick.
pub struct SceneBuf {
    pub w: i32,
    pub h: i32,
    pub viewport: R,
    pub st: Vec<PState>,
    pub kinds: Vec<u8>,
    pub nodes: Vec<NodeDef>,
    pub groups: Vec<GroupDef>,
    pub group_off: Vec<(i32, i32)>,
    defining: bool,
    cur_group: u32,
}

pub const SCALE: f32 = 2.0;
/// Fixed advance per character: DejaVu Sans ≈ 0.577 em (13 px ≈ 7.5 px/char).
pub const ADV_EM: f32 = 0.577;

#[inline]
pub fn px(v: f32) -> i32 {
    (v * SCALE).round() as i32
}

pub fn hk(a: u32, b: u32) -> u32 {
    let mut h = (a ^ 0x9e37_79b9).wrapping_mul(0x85eb_ca6b) ^ b.rotate_left(16);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

impl SceneBuf {
    /// Start defining the layout (first build); later builds must touch the same slots.
    pub fn new(w_pt: f32, h_pt: f32) -> Self {
        let (w, h) = (px(w_pt), px(h_pt));
        SceneBuf {
            w,
            h,
            viewport: R::new(0, 0, w, h),
            st: vec![],
            kinds: vec![],
            nodes: vec![],
            groups: vec![],
            group_off: vec![],
            defining: true,
            cur_group: u32::MAX,
        }
    }
    pub fn finish_layout(&mut self) {
        self.defining = false;
        let pad = self.st.len().div_ceil(64) * 64;
        self.st.resize(pad, PState::EMPTY);
        self.kinds.resize(pad, SOLID);
    }
    /// Allocate (first build) or address (later builds) a node of `n` slots.
    /// Builds must call `node` in the same order every tick.
    fn node(&mut self, idx: &mut usize, n: u32, text: bool, kind: u8) -> u32 {
        if self.defining {
            let slot0 = self.st.len() as u32;
            self.nodes.push(NodeDef { slot0, n, text, group: self.cur_group });
            for _ in 0..n {
                self.st.push(PState::EMPTY);
                self.kinds.push(kind);
            }
            *idx += 1;
            slot0
        } else {
            let d = &self.nodes[*idx];
            assert_eq!(d.n, n);
            *idx += 1;
            d.slot0
        }
    }
    pub fn begin_group(&mut self, gi: usize, clip: R, off: (i32, i32)) {
        if self.defining {
            let ni = self.nodes.len() as u32;
            self.groups.push(GroupDef { clip, nodes: ni..ni });
            self.group_off.push(off);
            self.cur_group = (self.groups.len() - 1) as u32;
        } else {
            self.groups[gi].clip = clip;
            self.group_off[gi] = off;
        }
    }
    pub fn end_group(&mut self, gi: usize) {
        if self.defining {
            self.groups[gi].nodes.end = self.nodes.len() as u32;
            self.cur_group = u32::MAX;
        }
    }
    fn set(&mut self, slot: u32, ink: R, clip: R, key: u32, kind: u8, rad: u8) {
        let cov = ink.clip(clip).clip(self.viewport);
        let s = &mut self.st[slot as usize];
        if cov.is_empty() {
            *s = PState::EMPTY;
        } else {
            *s = PState { ink, cov, key, kind, rad };
        }
    }
    pub fn clear(&mut self, slot0: u32, n: u32) {
        for s in &mut self.st[slot0 as usize..(slot0 + n) as usize] {
            *s = PState::EMPTY;
        }
    }
    /// A rect node (one slot). `ink` in device px.
    pub fn rect(&mut self, idx: &mut usize, ink: Option<R>, clip: R, kind: u8, color: u32, rad: u8) {
        let slot = self.node(idx, 1, false, kind);
        match ink {
            Some(ink) => self.set(slot, ink, clip, hk(color, kind as u32 | (rad as u32) << 8), kind, rad),
            None => self.clear(slot, 1),
        }
    }
    /// A text node with `cap` glyph slots: lines at (x, y) pt, `size` px font,
    /// `lh` pt line height. One glyph per non-space char; box = advance x 1.2 em.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        idx: &mut usize,
        cap: u32,
        x: f32,
        y: f32,
        size: f32,
        lh: f32,
        lines: Option<&[&str]>,
        color: u32,
        clip: R,
    ) {
        let slot0 = self.node(idx, cap, true, GLYPH);
        self.clear(slot0, cap);
        let Some(lines) = lines else { return };
        let adv = size * ADV_EM;
        let gh = (size * 1.2 * SCALE).round() as i32;
        let gw = (adv * SCALE).ceil() as i32;
        let mut k = 0u32;
        for (li, line) in lines.iter().enumerate() {
            let top = px(y + li as f32 * lh + (lh - size * 1.2) * 0.5);
            for (ci, ch) in line.chars().enumerate() {
                if ch == ' ' {
                    k += 1;
                    continue;
                }
                assert!(k < cap, "glyph cap {cap} exceeded");
                let gx = px(x + ci as f32 * adv);
                let ink = R::xywh(gx, top, gw, gh);
                self.set(slot0 + k, ink, clip, hk(ch as u32, color), GLYPH, 0);
                k += 1;
            }
        }
        assert!(k <= cap, "glyph cap {cap} exceeded ({k})");
    }
}

/// Build a trace: `build(sb, tick)` writes the whole scene for `tick`.
pub fn make_trace(
    name: &str,
    w_pt: f32,
    h_pt: f32,
    ticks: std::ops::Range<u64>,
    mut build: impl FnMut(&mut SceneBuf, u64),
) -> Trace {
    let mut sb = SceneBuf::new(w_pt, h_pt);
    build(&mut sb, ticks.start);
    sb.finish_layout();
    // Rebuild the first tick in non-defining mode to verify determinism.
    let init = sb.st.clone();
    build(&mut sb, ticks.start);
    assert!(sb.st == init, "build not deterministic");
    let mut prev = init.clone();
    let mut prev_off = sb.group_off.clone();
    let mut frames = vec![];
    for t in ticks.start + 1..ticks.end {
        build(&mut sb, t);
        let f = diff(&sb, &prev, &prev_off);
        prev.clone_from(&sb.st);
        prev_off.clone_from(&sb.group_off);
        if !f.is_empty() {
            frames.push(f);
        }
    }
    Trace {
        name: name.to_string(),
        w: sb.w,
        h: sb.h,
        slots: sb.st.len(),
        kinds: sb.kinds.clone(),
        init,
        frames,
        nodes: sb.nodes.clone(),
        groups: sb.groups.clone(),
    }
}

fn diff(sb: &SceneBuf, prev: &[PState], prev_off: &[(i32, i32)]) -> Frame {
    let mut f = Frame::default();
    let cur = &sb.st;
    // Which nodes are fully explained by their group's move.
    let mut moved_by_group = vec![false; sb.nodes.len()];
    for (gi, g) in sb.groups.iter().enumerate() {
        let (ox, oy) = sb.group_off[gi];
        let (px_, py_) = prev_off[gi];
        let (dx, dy) = (ox - px_, oy - py_);
        if dx == 0 && dy == 0 {
            continue;
        }
        let at = f.gslots.len() as u32;
        let (mut ob, mut nb) = (R::E, R::E);
        for ni in g.nodes.clone() {
            let nd = &sb.nodes[ni as usize];
            let mut pure = true;
            for s in nd.slot0..nd.slot0 + nd.n {
                let (o, n) = (prev[s as usize], cur[s as usize]);
                if o.cov.is_empty() && n.cov.is_empty() {
                    continue;
                }
                // Pure move: same content, ink translated by the group delta.
                if !(o.key == n.key && o.kind == n.kind && o.rad == n.rad && o.ink.tr(dx, dy) == n.ink) {
                    pure = false;
                }
                f.gslots.push(s);
                f.gold.push(o);
                f.gnew.push(n);
                ob = ob.union(o.cov);
                nb = nb.union(n.cov);
            }
            moved_by_group[ni as usize] = pure;
        }
        let n = f.gslots.len() as u32 - at;
        f.groups.push(GroupEv { at, n, dx, dy, clip: g.clip, old_b: ob, new_b: nb });
    }
    for (ni, nd) in sb.nodes.iter().enumerate() {
        if moved_by_group[ni] {
            continue;
        }
        let r = nd.slot0 as usize..(nd.slot0 + nd.n) as usize;
        if prev[r.clone()] == cur[r.clone()] {
            continue;
        }
        // Trim trailing slots empty in both (text runs have phantom capacity).
        let mut end = r.end;
        while end > r.start && prev[end - 1] == PState::EMPTY && cur[end - 1] == PState::EMPTY {
            end -= 1;
        }
        let r = r.start..end;
        let at = f.olds.len() as u32;
        let (mut ob, mut nb) = (R::E, R::E);
        for s in r.clone() {
            f.olds.push(prev[s]);
            f.news.push(cur[s]);
            ob = ob.union(prev[s].cov);
            nb = nb.union(cur[s].cov);
        }
        f.nodes.push(NodeEv { slot0: nd.slot0, n: (r.end - r.start) as u32, at, text: nd.text, old_b: ob, new_b: nb });
    }
    f
}

impl Trace {
    pub fn stats(&self) -> String {
        let nonempty = self.init.iter().filter(|s| !s.cov.is_empty()).count();
        let ev: usize = self.frames.iter().map(|f| f.nodes.len()).sum();
        let gs: usize = self.frames.iter().map(|f| f.gslots.len()).sum();
        let sl: usize = self.frames.iter().map(|f| f.olds.len()).sum();
        let nf = self.frames.len().max(1);
        format!(
            "{}: {}x{} px, {} slots ({} live), {} frames, {:.1} node ev/frame ({:.0} slots), {:.0} group slots/frame",
            self.name,
            self.w,
            self.h,
            self.slots,
            nonempty,
            self.frames.len(),
            ev as f64 / nf as f64,
            sl as f64 / nf as f64,
            gs as f64 / nf as f64
        )
    }
}
