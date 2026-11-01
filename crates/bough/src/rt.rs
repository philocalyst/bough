//! The runtime tables behind the façade: instance pool + size-classed run allocator, nodes (the
//! per-element state wires patch), the wire arena, text, keyed virtual-list regions, spaces/clips.
//!
//! Everything here is plain data + monomorphic patch functions; the generic builder layer
//! (`elem.rs`) lowers typed descriptions into it.
use crate::text::TextSys;
use bough_core::{Bough, Handle, NRect, Plane, RecordInit};
use bough_metal::instance::Hdr;
use bough_text::atlas::Instance;
use std::collections::VecDeque;
use std::ops::Range;

pub const MAX_INST: usize = 32768;
pub const MAX_SPACES: usize = 512;
pub const MAX_CLIPS: usize = 32;
pub const MAX_RECTS: usize = 512;
pub const SPACES_OFF: usize = 256;
pub const CLIPS_OFF: usize = SPACES_OFF + MAX_SPACES * 8;
pub const RECTS_OFF: usize = CLIPS_OFF + MAX_CLIPS * 16;
pub const INST_OFF: usize = 16384;
pub const LIST_OFF: usize = INST_OFF + MAX_INST * 48;
pub const POOL_BYTES: usize = LIST_OFF + MAX_INST * 4;
const _: () = assert!(RECTS_OFF + MAX_RECTS * 16 <= INST_OFF);

pub const KIND_SOLID: u16 = 0;
pub const KIND_RRECT: u16 = 1;
pub const KIND_BORDER: u16 = 2;

pub const REC_QUAD: u8 = 1;
pub const REC_TEXT: u8 = 2;
/// Node kinds (see `Node::kind`): 1 quad, 2 text, 3 bordered quad, 4 bars, 5 layout-owned text (see layout.rs).
pub const NK_LTEXT: u8 = 5;

/// The shared pool: header | spaces | clips | stencil rects | instances | draw list. Either a
/// `storageModeShared` MTLBuffer (the Metal backend) or heap memory (headless, no GPU).
pub struct Pool {
    pub ptr: *mut u8,
    _heap: Option<Vec<u128>>,
}

impl Pool {
    pub fn heap() -> Pool {
        let mut v = vec![0u128; POOL_BYTES / 16 + 1];
        Pool { ptr: v.as_mut_ptr() as *mut u8, _heap: Some(v) }
    }
    /// # Safety
    /// `ptr` must stay valid, 16-aligned and `POOL_BYTES` long for the life of the pool.
    pub unsafe fn external(ptr: *mut u8) -> Pool {
        Pool { ptr, _heap: None }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Run {
    pub first: u32,
    pub len: u32,
    /// log2 of the capacity class.
    pub class: u8,
    pub live: bool,
}

/// Mutable per-element state that wires patch. Quads and text share the struct (cold fields are small).
pub struct Node {
    pub handle: Handle,
    pub run: u32,
    pub space: u16,
    pub clip: u16,
    pub kind: u8,
    pub live: bool,
    pub dirty: bool,
    /// Quad: local px x, y(origin), w, h. Text: ox, baseline of the first line, -, -.
    pub x: f32,
    pub y: f32,
    /// Quad: space-local px y of the element's origin (the `span` operand is relative to it).
    pub y0: f32,
    pub w: f32,
    pub h: f32,
    pub radius: f32,
    pub color: u32,
    pub margin: f32,
    pub strike: u32,
    pub size_px: f32,
    pub line_h: f32,
    pub text: String,
    /// Series node (`bars`): bar count, memoised samples and the key they were computed for.
    pub series: Vec<f32>,
    pub series_key: u64,
    pub series_ok: bool,
    /// Multi-line text: the previous lines' shaped instances (a streaming log scrolls by one line
    /// per update, so every unchanged line is re-used at a new baseline instead of re-shaped).
    pub lines: Vec<(String, f32, Vec<Instance>)>,
    /// Layout node (index into `Rt::lay`) of an element laid out by cephal, else `u32::MAX`.
    pub lnode: u32,
}

impl Node {
    fn blank() -> Node {
        Node {
            handle: Handle { idx: 0, generation: 0 },
            run: 0,
            space: 0,
            clip: 0,
            kind: 0,
            live: false,
            dirty: false,
            x: 0.0,
            y: 0.0,
            y0: 0.0,
            w: 0.0,
            h: 0.0,
            radius: 0.0,
            color: 0,
            margin: 0.0,
            strike: 0,
            size_px: 0.0,
            line_h: 0.0,
            text: String::new(),
            series: Vec::new(),
            series_key: 0,
            series_ok: false,
            lines: Vec::new(),
            lnode: u32::MAX,
        }
    }
}

pub type PatchFn = unsafe fn(&mut Rt, &Wire, *const u8, *const u8);

#[derive(Clone, Copy)]
pub struct Wire {
    /// Next wire of the same signal (0 = end).
    pub next: u32,
    pub dead: bool,
    pub node: u32,
    pub patch: PatchFn,
    pub recipe: [u8; 16],
}

unsafe fn nop(_: &mut Rt, _: &Wire, _: *const u8, _: *const u8) {}

/// Everything created while mounting one subtree that must be released when it unmounts.
#[derive(Default)]
pub struct Scope {
    pub nodes: Vec<u32>,
    pub wires: Vec<u32>,
    pub runs: Vec<u32>,
    /// Layout roots (groups) created while mounting; removed from the layout engine on release.
    pub lroots: Vec<u32>,
}

pub struct RowInfo {
    pub nrec: u32,
    pub space: u16,
    pub scope: Scope,
}

pub type RowMountFn = unsafe fn(&mut Rt, *const u8, usize, usize, &mut Vec<RecordInit>) -> RowInfo;

pub struct Region {
    pub row_h: f32,
    pub vp_x: f32,
    pub vp_y: f32,
    pub clip: u16,
    pub depth: u16,
    pub base_pos: usize,
    pub range: Range<usize>,
    pub rows: VecDeque<RowInfo>,
    pub scroll_px: f32,
    pub visible: fn(f32) -> Range<usize>,
    pub mount_row: RowMountFn,
    /// The user's `fn(&V, usize, &mut Mounter<V>)`, type-erased (transmuted back by `mount_row`).
    pub row_fn: usize,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Counters {
    pub wires_run: u64,
    pub text_shaped: u64,
    pub rows_mounted: u64,
    pub rows_unmounted: u64,
}

pub struct Rt {
    pub bough: Bough,
    pub scale: f32,
    pub win_w: i32,
    pub win_h: i32,
    pub pool: Pool,
    pub text: TextSys,
    next_fresh: u32,
    free_blocks: [Vec<u32>; 16],
    pub runs: Vec<Run>,
    run_free: Vec<u32>,
    pub nodes: Vec<Node>,
    node_free: Vec<u32>,
    pub wires: Vec<Wire>,
    wire_free: Vec<u32>,
    /// Offsets (from the owner's address) of the owner's wired signals.
    pub wired: Vec<u32>,
    pub text_dirty: Vec<u32>,
    pub regions: Vec<Region>,
    n_clips: usize,
    space_free: Vec<u16>,
    next_space: u16,
    pub plane: Box<Plane>,
    pub cull_out: Vec<u32>,
    pub ctr: Counters,
    /// Atlas contents changed since the backend last uploaded it.
    pub atlas_gen: (u32, u64),
    /// The layout bridge (cephal over the mounted records) and the measured lists.
    pub lay: crate::layout::Lay,
    pub mregions: Vec<crate::mlist::MRegion>,
}

impl Rt {
    pub fn new(win_w: i32, win_h: i32, scale: f32, font: &[u8], pool: Pool) -> Rt {
        let rt = Rt {
            bough: Bough::new(win_w, win_h),
            scale,
            win_w,
            win_h,
            pool,
            text: TextSys::new(font),
            next_fresh: 0,
            free_blocks: Default::default(),
            runs: vec![Run::default()],
            run_free: Vec::new(),
            nodes: vec![Node::blank()],
            node_free: Vec::new(),
            wires: vec![Wire { next: 0, dead: true, node: 0, patch: nop, recipe: [0; 16] }],
            wire_free: Vec::new(),
            wired: Vec::new(),
            text_dirty: Vec::new(),
            regions: Vec::new(),
            n_clips: 1,
            space_free: Vec::new(),
            next_space: 1,
            plane: Box::new(Plane::EMPTY),
            cull_out: Vec::with_capacity(4096),
            ctr: Counters::default(),
            atlas_gen: (0, 0),
            lay: crate::layout::Lay::new(scale),
            mregions: Vec::new(),
        };
        let h = Hdr {
            viewport: [win_w as f32, win_h as f32],
            spaces_off: SPACES_OFF as u32,
            clips_off: CLIPS_OFF as u32,
            rects_off: RECTS_OFF as u32,
            inst_off: INST_OFF as u32,
            list_off: LIST_OFF as u32,
        };
        unsafe { (rt.pool.ptr as *mut Hdr).write(h) };
        for s in 0..MAX_SPACES {
            rt.set_space(s as u16, [0.0, 0.0]);
        }
        for c in 0..MAX_CLIPS {
            rt.write_clip(c, [0.0, 0.0, win_w as f32, win_h as f32]);
        }
        rt
    }

    // ------------------------------------------------------------------ pool access
    #[inline(always)]
    pub fn inst_ptr(&self, i: u32) -> *mut Instance {
        debug_assert!((i as usize) < MAX_INST);
        unsafe { (self.pool.ptr.add(INST_OFF) as *mut Instance).add(i as usize) }
    }
    #[inline(always)]
    pub fn list_ptr(&self) -> *mut u32 {
        unsafe { self.pool.ptr.add(LIST_OFF) as *mut u32 }
    }
    #[inline(always)]
    pub fn rects_ptr(&self) -> *mut [f32; 4] {
        unsafe { self.pool.ptr.add(RECTS_OFF) as *mut [f32; 4] }
    }
    #[inline]
    pub fn set_space(&self, s: u16, v: [f32; 2]) {
        unsafe { *(self.pool.ptr.add(SPACES_OFF) as *mut [f32; 2]).add(s as usize) = v };
    }
    #[inline]
    pub fn space(&self, s: u16) -> [f32; 2] {
        unsafe { *(self.pool.ptr.add(SPACES_OFF) as *const [f32; 2]).add(s as usize) }
    }
    fn write_clip(&self, c: usize, r: [f32; 4]) {
        unsafe { *(self.pool.ptr.add(CLIPS_OFF) as *mut [f32; 4]).add(c) = r };
    }
    /// A clip rect in device px (deduplicated); index 0 is the whole window.
    pub fn clip_rect(&mut self, r: [f32; 4]) -> u16 {
        for c in 1..self.n_clips {
            let e = unsafe { *(self.pool.ptr.add(CLIPS_OFF) as *const [f32; 4]).add(c) };
            if e == r {
                return c as u16;
            }
        }
        assert!(self.n_clips < MAX_CLIPS, "clip table full");
        self.write_clip(self.n_clips, r);
        self.n_clips += 1;
        (self.n_clips - 1) as u16
    }
    pub fn alloc_space(&mut self) -> u16 {
        if let Some(s) = self.space_free.pop() {
            return s;
        }
        assert!((self.next_space as usize) < MAX_SPACES, "space table full");
        self.next_space += 1;
        self.next_space - 1
    }
    pub fn free_space(&mut self, s: u16) {
        self.space_free.push(s);
    }

    // ------------------------------------------------------------------ instance runs
    /// A block of `1 << class` instances: a free block of that class, else a split of a larger free block, else
    /// fresh pool (aligned to its size, so buddies pair up). Blocks are buddy-coalesced when freed, so a long
    /// run of rows of varying sizes (a list jumping about) cannot fragment the pool away.
    fn alloc_block(&mut self, class: u8) -> u32 {
        if let Some(b) = self.free_blocks[class as usize].pop() {
            return b;
        }
        for c in class as usize + 1..16 {
            if let Some(b) = self.free_blocks[c].pop() {
                for lower in (class as usize..c).rev() {
                    self.free_blocks[lower].push(b + (1 << lower));
                }
                return b;
            }
        }
        let size = 1u32 << class;
        let b = (self.next_fresh + size - 1) & !(size - 1);
        assert!((b + size) as usize <= MAX_INST, "instance pool full");
        // the alignment gap becomes free blocks (largest aligned pieces that fit)
        let mut a = self.next_fresh;
        while a < b {
            let mut c = a.trailing_zeros().min(15);
            while a + (1 << c) > b {
                c -= 1;
            }
            self.free_blocks[c as usize].push(a);
            a += 1 << c;
        }
        self.next_fresh = b + size;
        b
    }
    fn free_block(&mut self, first: u32, class: u8) {
        let (mut b, mut c) = (first, class as usize);
        while c < 15 {
            let buddy = b ^ (1 << c);
            match self.free_blocks[c].iter().position(|&x| x == buddy) {
                Some(i) => {
                    self.free_blocks[c].swap_remove(i);
                    b = b.min(buddy);
                    c += 1;
                }
                None => break,
            }
        }
        self.free_blocks[c].push(b);
    }
    /// A run with capacity for at least `n` instances (power-of-two size classes, free lists).
    pub fn alloc_run(&mut self, n: usize) -> u32 {
        let class = (n.max(1).next_power_of_two().trailing_zeros()) as u8;
        let first = self.alloc_block(class);
        let r = Run { first, len: 0, class, live: true };
        if let Some(i) = self.run_free.pop() {
            self.runs[i as usize] = r;
            i
        } else {
            self.runs.push(r);
            (self.runs.len() - 1) as u32
        }
    }
    pub fn free_run(&mut self, i: u32) {
        let r = self.runs[i as usize];
        if r.live {
            self.free_block(r.first, r.class);
            self.runs[i as usize].live = false;
            self.run_free.push(i);
        }
    }
    /// Grow a run in place of its id: new block, same id (the record's payload never changes).
    pub(crate) fn regrow_run(&mut self, i: u32, n: usize) {
        let r = self.runs[i as usize];
        self.free_block(r.first, r.class);
        let class = (n.next_power_of_two().trailing_zeros()) as u8;
        let first = self.alloc_block(class);
        self.runs[i as usize] = Run { first, len: r.len, class, live: true };
    }

    // ------------------------------------------------------------------ nodes and wires
    pub fn alloc_node(&mut self) -> u32 {
        let mut n = Node::blank();
        n.live = true;
        if let Some(i) = self.node_free.pop() {
            let old = std::mem::take(&mut self.nodes[i as usize].text);
            n.text = old;
            n.text.clear();
            n.series = std::mem::take(&mut self.nodes[i as usize].series);
            n.lines = std::mem::take(&mut self.nodes[i as usize].lines);
            n.lines.clear();
            self.nodes[i as usize] = n;
            i
        } else {
            self.nodes.push(n);
            (self.nodes.len() - 1) as u32
        }
    }
    pub fn free_node(&mut self, i: u32) {
        self.nodes[i as usize].live = false;
        self.node_free.push(i);
    }
    /// Install a wire on the signal whose head is `head` (the signal lives at `owner + off`).
    pub fn add_wire(&mut self, head: &std::cell::Cell<u32>, off: u32, patch: PatchFn, node: u32, recipe: [u8; 16]) -> u32 {
        let h = head.get();
        let first = h & !crate::signal::QUEUED;
        if first == 0 {
            self.wired.push(off);
        }
        let w = Wire { next: first, dead: false, node, patch, recipe };
        let idx = if let Some(i) = self.wire_free.pop() {
            self.wires[i as usize] = w;
            i
        } else {
            self.wires.push(w);
            (self.wires.len() - 1) as u32
        };
        head.set(idx | (h & crate::signal::QUEUED));
        idx
    }

    /// Run every queued signal's wires. Dead wires are unlinked as they are met.
    pub fn settle(&mut self, owner: *const u8) {
        let mut k = 0;
        while k < self.wired.len() {
            let off = self.wired[k] as usize;
            k += 1;
            let sig = unsafe { owner.add(off) };
            let head = unsafe { &*(sig as *const std::cell::Cell<u32>) };
            let h = head.get();
            if h & crate::signal::QUEUED == 0 {
                continue;
            }
            head.set(h & !crate::signal::QUEUED);
            let mut i = h & !crate::signal::QUEUED;
            let mut prev = 0u32;
            while i != 0 {
                let w = self.wires[i as usize];
                if w.dead {
                    // unlink
                    if prev == 0 {
                        head.set(w.next | (head.get() & crate::signal::QUEUED));
                    } else {
                        self.wires[prev as usize].next = w.next;
                    }
                    self.wire_free.push(i);
                } else {
                    self.ctr.wires_run += 1;
                    unsafe { (w.patch)(self, &w, sig, owner) };
                    prev = i;
                }
                i = w.next;
            }
        }
    }

    // ------------------------------------------------------------------ geometry helpers
    /// Absolute ink (device px, outward rounded, with `margin`) of a space-local rect.
    #[inline]
    pub fn ink_rect(&self, space: u16, x: f32, y: f32, w: f32, h: f32, margin: f32) -> NRect {
        let o = self.space(space);
        let x0 = (o[0] + x - margin).floor() as i32;
        let y0 = (o[1] + y - margin).floor() as i32;
        let x1 = (o[0] + x + w + margin).ceil() as i32;
        let y1 = (o[1] + y + h + margin).ceil() as i32;
        NRect::new(x0, y0, x1, y1)
    }

    /// Re-derive a quad node's ink and write it through (old and new support to damage).
    #[inline]
    pub fn quad_ink(&mut self, node: u32) {
        let n = &self.nodes[node as usize];
        let ink = self.ink_rect(n.space, n.x, n.y, n.w, n.h, n.margin);
        let h = n.handle;
        self.bough.set_ink_handle(h, ink);
    }

    // ------------------------------------------------------------------ text
    /// Shape the node's text into its instance run and refresh ink. Used at mount and by the text phase.
    pub fn apply_text(&mut self, node: u32) {
        let ink = self.shape_node(node);
        let h = self.nodes[node as usize].handle;
        self.bough.set_ink_handle(h, ink);
    }

    /// Shape the node's text into its run; returns the new ink (the caller writes it through).
    pub fn shape_node(&mut self, node: u32) -> NRect {
        self.ctr.text_shaped += 1;
        let (strike, size_px, color, ox, oy0, lh, space, clip, run) = {
            let n = &self.nodes[node as usize];
            (n.strike as usize, n.size_px, n.color, n.x, n.y, n.line_h, n.space, n.clip, n.run)
        };
        let text = std::mem::take(&mut self.nodes[node as usize].text);
        self.text.scratch.clear();
        let mut oy = oy0;
        if text.contains('\n') {
            let mut old = std::mem::take(&mut self.nodes[node as usize].lines);
            let mut fresh: Vec<(String, f32, Vec<Instance>)> = Vec::with_capacity(old.len().max(4));
            for (i, line) in text.split('\n').enumerate() {
                let start = self.text.scratch.len();
                let hit = if old.get(i).is_some_and(|c| c.0 == line) { Some(i) } else { old.iter().position(|c| c.0 == line) };
                match hit {
                    Some(j) => {
                        let dy = oy - old[j].1;
                        self.text.scratch.extend(old[j].2.iter().map(|g| {
                            let mut g = *g;
                            g.xy[1] += dy;
                            g
                        }));
                    }
                    None => {
                        self.text.line(strike, size_px, line, ox, oy, color);
                    }
                }
                let (mut s, mut v) = old.pop().map(|e| (e.0, e.2)).unwrap_or_default();
                s.clear();
                s.push_str(line);
                v.clear();
                v.extend_from_slice(&self.text.scratch[start..]);
                fresh.push((s, oy, v));
                oy += lh;
            }
            self.nodes[node as usize].lines = fresh;
        } else {
            self.text.line(strike, size_px, &text, ox, oy, color);
        }
        self.nodes[node as usize].text = text;
        let cnt = self.text.scratch.len();
        if cnt > (1usize << self.runs[run as usize].class) {
            self.regrow_run(run, cnt);
        }
        let first = self.runs[run as usize].first;
        self.runs[run as usize].len = cnt as u32;
        let dst = self.inst_ptr(first);
        let src = self.text.scratch.as_ptr();
        // copy + stamp space/clip + union of glyph quads (ink) in one pass
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let clipw = clip as u32;
        for i in 0..cnt {
            let mut g = unsafe { *src.add(i) };
            x0 = x0.min(g.xy[0]);
            y0 = y0.min(g.xy[1]);
            x1 = x1.max(g.xy[0] + g.wh[0]);
            y1 = y1.max(g.xy[1] + g.wh[1]);
            g.space = space;
            g.aux[3] = clipw;
            unsafe { dst.add(i).write(g) };
        }
        if cnt == 0 { NRect::EMPTY } else { self.ink_rect(space, x0, y0, x1 - x0, y1 - y0, 1.0) }
    }

    /// The text phase: shape every node a wire marked dirty.
    pub fn flush_text(&mut self) {
        let mut d = std::mem::take(&mut self.text_dirty);
        for &n in &d {
            // a node freed and reallocated this turn leaves a stale entry behind: only a live node
            // that is still marked dirty is shaped (`alloc_node` resets the flag)
            let node = &mut self.nodes[n as usize];
            if node.dirty && node.live {
                node.dirty = false;
                if node.kind == NK_LTEXT {
                    // wrapped / laid-out text: re-measured and re-placed by the layout flush
                    self.lay_text_changed(n);
                } else {
                    self.apply_text(n);
                }
            }
        }
        d.clear();
        self.text_dirty = d;
    }

    // ------------------------------------------------------------------ release
    pub fn release_scope(&mut self, s: Scope) {
        for w in s.wires {
            self.wires[w as usize].dead = true;
        }
        for n in s.nodes {
            self.free_node(n);
        }
        for r in s.runs {
            self.free_run(r);
        }
        for r in s.lroots {
            self.lay_remove_root(r);
        }
    }

    // ------------------------------------------------------------------ virtual-list regions
    fn region_remove(&mut self, reg: usize, front: bool, count: usize) {
        let mut nrec = 0usize;
        for _ in 0..count {
            let row = if front { self.regions[reg].rows.pop_front() } else { self.regions[reg].rows.pop_back() }.unwrap();
            nrec += row.nrec as usize;
            self.free_space(row.space);
            self.release_scope(row.scope);
            self.ctr.rows_unmounted += 1;
        }
        let r = &self.regions[reg];
        let start = if front { r.base_pos } else { r.base_pos + r.rows.iter().map(|x| x.nrec as usize).sum::<usize>() };
        self.bough.remove_run(start, nrec);
    }

    /// Mount rows `range` (ascending) and insert them at the front or the back of the region.
    unsafe fn region_mount(&mut self, reg: usize, owner: *const u8, range: Range<usize>, front: bool) {
        if range.is_empty() {
            return;
        }
        let mut recs: Vec<RecordInit> = Vec::with_capacity(range.len() * 64);
        let f = self.regions[reg].mount_row;
        let mut built: Vec<RowInfo> = Vec::with_capacity(range.len());
        for row in range {
            built.push(unsafe { f(self, owner, reg, row, &mut recs) });
            self.ctr.rows_mounted += 1;
        }
        let r = &mut self.regions[reg];
        let pos = if front { r.base_pos } else { r.base_pos + r.rows.iter().map(|x| x.nrec as usize).sum::<usize>() };
        if front {
            for b in built.into_iter().rev() {
                r.rows.push_front(b);
            }
        } else {
            r.rows.extend(built);
        }
        self.bough.insert_run(pos, &recs);
    }

    /// Initial rows of a region at mount time: rows are built into `recs` (the caller inserts them).
    pub unsafe fn region_initial(&mut self, reg: usize, owner: *const u8, scroll_pt: f32, recs: &mut Vec<RecordInit>) {
        let r = &mut self.regions[reg];
        r.scroll_px = (scroll_pt * self.scale).round();
        let range = (r.visible)(scroll_pt);
        r.range = range.clone();
        let f = r.mount_row;
        for row in range {
            let info = unsafe { f(self, owner, reg, row, recs) };
            self.ctr.rows_mounted += 1;
            self.regions[reg].rows.push_back(info);
        }
    }

    pub fn region_row_space_origin(&self, reg: usize, row: usize) -> [f32; 2] {
        let r = &self.regions[reg];
        [r.vp_x, r.vp_y + row as f32 * r.row_h - r.scroll_px]
    }

    /// Scroll a region: unmount rows that left the range, move the survivors through their
    /// spaces (not rewritten), mount rows that entered.
    pub unsafe fn region_scroll(&mut self, reg: usize, owner: *const u8, scroll_pt: f32) {
        let new_px = (scroll_pt * self.scale).round();
        let new_range = (self.regions[reg].visible)(scroll_pt);
        let (old_px, old_range) = (self.regions[reg].scroll_px, self.regions[reg].range.clone());
        if new_px == old_px && new_range == old_range {
            return;
        }
        let disjoint = new_range.start >= old_range.end || new_range.end <= old_range.start;
        if disjoint {
            let n = self.regions[reg].rows.len();
            self.region_remove(reg, true, n);
            let r = &mut self.regions[reg];
            r.scroll_px = new_px;
            r.range = new_range.clone();
            unsafe { self.region_mount(reg, owner, new_range, false) };
            return;
        }
        // rows leaving at the front / back
        let drop_front = new_range.start.saturating_sub(old_range.start);
        let drop_back = old_range.end.saturating_sub(new_range.end);
        if drop_front > 0 {
            self.region_remove(reg, true, drop_front);
        }
        if drop_back > 0 {
            self.region_remove(reg, false, drop_back);
        }
        // survivors: new scroll through their spaces; ink translated as one contiguous run
        let dy = (new_px - old_px) as i32;
        let r = &mut self.regions[reg];
        r.scroll_px = new_px;
        let base = r.base_pos;
        let nrec: usize = r.rows.iter().map(|x| x.nrec as usize).sum();
        for row in r.rows.iter() {
            let s = row.space;
            let mut o = unsafe { *(self.pool.ptr.add(SPACES_OFF) as *const [f32; 2]).add(s as usize) };
            o[1] -= dy as f32;
            unsafe { *(self.pool.ptr.add(SPACES_OFF) as *mut [f32; 2]).add(s as usize) = o };
        }
        self.bough.translate_run(base, nrec, 0, -dy);
        // rows entering
        let surv_start = old_range.start.max(new_range.start);
        let surv_end = old_range.end.min(new_range.end);
        self.regions[reg].range = new_range.clone();
        unsafe {
            self.region_mount(reg, owner, new_range.start..surv_start, true);
            self.region_mount(reg, owner, surv_end..new_range.end, false);
        }
    }
}
