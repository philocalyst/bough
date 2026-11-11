//! Input queries against brute-force oracles over random trees: the topmost hit (with clips, page offsets
//! and NOHIT records), flagged ancestor chains, hit cells (every pixel of the cell answers like its
//! seed pixel), cell validity across random edits, and tab order.
use bough_core::rng::Rng;
use bough_core::{Bough, KF_FOCUS, KF_INTERACT, KF_NOHIT, NRect, RecordInit};

const W: i32 = 256;
const H: i32 = 192;

/// Record identity across splices: the payload is unique per record.
#[derive(Clone, Copy, Debug)]
struct ORec {
    depth: u16,
    vis: NRect,
    kind: u8,
    payload: u32,
}

fn snapshot(b: &Bough) -> Vec<ORec> {
    b.records()
        .iter()
        .map(|r| ORec { depth: r.depth, vis: if r.clip == 0 { r.ink } else { r.ink.intersect(b.clip_rect(r.clip)).canon() }, kind: r.kind, payload: r.payload })
        .collect()
}

fn o_hit(v: &[ORec], x: i32, y: i32) -> Option<usize> {
    v.iter().rposition(|r| r.kind & KF_NOHIT == 0 && !r.vis.is_empty() && r.vis.contains_point(x, y))
}

fn o_parent(v: &[ORec], i: usize) -> Option<usize> {
    let d = v[i].depth;
    (0..i).rev().find(|&j| v[j].depth < d)
}

fn o_chain(v: &[ORec], i: usize, flag: u8) -> Vec<usize> {
    let mut out = Vec::new();
    let mut c = Some(i);
    while let Some(j) = c {
        if v[j].kind & flag != 0 {
            out.push(j);
        }
        c = o_parent(v, j);
    }
    out
}

fn rand_rect(rng: &mut Rng) -> NRect {
    let x0 = rng.range(-30, W + 10);
    let y0 = rng.range(-30, H + 10);
    let (w, h) = if rng.chance(1, 6) { (rng.range(0, W), rng.range(0, H)) } else { (rng.range(0, 60), rng.range(0, 50)) };
    NRect::new(x0, y0, x0 + w, y0 + h)
}

struct T {
    b: Bough,
    rng: Rng,
    payload: u32,
    clips: Vec<u16>,
}

impl T {
    fn new(seed: u64) -> T {
        let mut b = Bough::new(W, H);
        let mut rng = Rng::new(seed);
        let mut clips = vec![0u16];
        for _ in 0..3 {
            let r = rand_rect(&mut rng);
            clips.push(b.new_clip(r));
        }
        T { b, rng, payload: 1, clips }
    }

    fn kind(&mut self) -> u8 {
        let mut k = self.rng.range(1, 4) as u8;
        if self.rng.chance(1, 3) {
            k |= KF_INTERACT;
        }
        if self.rng.chance(1, 5) {
            k |= KF_FOCUS;
        }
        if self.rng.chance(1, 7) {
            k |= KF_NOHIT;
        }
        k
    }

    fn subtree(&mut self, root_depth: u16, size: usize) -> Vec<RecordInit> {
        let mut v: Vec<RecordInit> = Vec::with_capacity(size);
        for i in 0..size {
            let depth = if i == 0 {
                root_depth
            } else {
                let prev = v[i - 1].depth;
                if self.rng.chance(1, 2) { prev + 1 } else { self.rng.range(root_depth as i32 + 1, prev as i32 + 1) as u16 }
            };
            let ink = rand_rect(&mut self.rng);
            let clip = if self.rng.chance(1, 4) { self.clips[self.rng.usize_below(self.clips.len())] } else { 0 };
            let kind = self.kind();
            v.push(RecordInit { depth, ink, kind, payload: self.payload, handle: 0, clip });
            self.payload += 1;
        }
        v
    }

    /// A random tree: roots at depth 0 with subtrees, inserted in chunks (so pages split and merge).
    fn build(&mut self, n: usize) {
        while self.b.len() < n {
            let size = self.rng.range(1, 40) as usize;
            let recs = self.subtree(0, size);
            let pos = if self.b.is_empty() { 0 } else { self.root_boundary() };
            self.b.insert_run(pos, &recs);
        }
    }

    /// A position where a depth-0 subtree may be inserted (before a root, or at the end).
    fn root_boundary(&mut self) -> usize {
        let n = self.b.len();
        let roots: Vec<usize> = (0..n).filter(|&i| self.b.depth_at(i) == 0).chain([n]).collect();
        roots[self.rng.usize_below(roots.len())]
    }

    /// A random edit that changes geometry or structure.
    fn edit(&mut self) {
        let n = self.b.len();
        match self.rng.below(6) {
            0 => {
                let pos = self.root_boundary();
                let size = self.rng.range(1, 20) as usize;
                let recs = self.subtree(0, size);
                self.b.insert_run(pos, &recs);
            }
            1 if n > 1 => {
                let roots: Vec<usize> = (0..n).filter(|&i| self.b.depth_at(i) == 0).collect();
                let r = roots[self.rng.usize_below(roots.len())];
                let end = self.b.subtree_end(r);
                self.b.remove_run(r, end - r);
            }
            2 if n > 0 => {
                let s = self.rng.usize_below(n);
                let len = self.rng.range(1, (n - s).min(200) as i32) as usize;
                let (dx, dy) = (self.rng.range(-20, 20), self.rng.range(-20, 20));
                self.b.translate_run(s, len, dx, dy);
            }
            3 if n > 0 => {
                let s = self.rng.usize_below(n);
                let len = self.rng.range(1, (n - s).min(200) as i32) as usize;
                // `scroll_run`'s contract: the run is a scroll container's content, clipped to its port
                let id = self.clips[1];
                self.b.set_clip_run(s, len, id);
                let clip = self.b.clip_rect(id);
                self.b.scroll_run(s, len, 0, self.rng.range(-30, 30), clip);
            }
            4 if n > 0 => {
                let s = self.rng.usize_below(n);
                let ink = rand_rect(&mut self.rng);
                self.b.set_ink(s, ink);
            }
            _ => {
                let id = self.clips[1 + self.rng.usize_below(3)];
                let r = rand_rect(&mut self.rng);
                self.b.set_clip_rect(id, r);
            }
        }
    }
}

#[test]
fn hit_top_and_chains_equal_the_oracle() {
    for seed in 0..60u64 {
        let mut t = T::new(seed);
        let n = [30usize, 150, 700, 2000][seed as usize % 4];
        t.build(n);
        for round in 0..6 {
            let v = snapshot(&t.b);
            for _ in 0..400 {
                let (x, y) = (t.rng.range(-5, W + 5), t.rng.range(-5, H + 5));
                let got = t.b.hit_top(x, y);
                let want = o_hit(&v, x, y);
                assert_eq!(got, want, "seed {seed} round {round} ({x},{y})");
                if let Some(i) = got {
                    let mut chain = Vec::new();
                    t.b.flagged_chain(i, KF_INTERACT, &mut chain);
                    assert_eq!(chain, o_chain(&v, i, KF_INTERACT), "seed {seed} chain of {i}");
                }
            }
            for _ in 0..8 {
                t.edit();
            }
        }
    }
}

#[test]
fn hit_cells_answer_like_their_seed_pixel_everywhere() {
    for seed in 0..40u64 {
        let mut t = T::new(seed ^ 0xCE11);
        t.build([20usize, 100, 400, 1500][seed as usize % 4]);
        let v = snapshot(&t.b);
        let win = NRect::new(0, 0, W, H);
        for _ in 0..120 {
            let (x, y) = (t.rng.range(0, W - 1), t.rng.range(0, H - 1));
            let top = t.b.hit_top(x, y);
            let c = t.b.hit_cell(x, y, top);
            assert!(c.contains_point(x, y), "seed {seed}: cell {c:?} misses ({x},{y})");
            assert_eq!(c.intersect(win).canon(), c, "cell leaves the window");
            if let Some(i) = top {
                assert_eq!(c.intersect(v[i].vis).canon(), c, "cell outside the record's visible ink");
            }
            // every pixel of the cell (exhaustive)
            let [x0, y0, x1, y1] = c.to_xyxy();
            for qy in y0..y1 {
                for qx in x0..x1 {
                    assert_eq!(o_hit(&v, qx, qy), top, "seed {seed}: cell {c:?} of ({x},{y}) disagrees at ({qx},{qy})");
                }
            }
        }
    }
}

#[test]
fn a_valid_cell_survives_edits_exactly() {
    let mut kept = 0u64;
    let mut dropped = 0u64;
    for seed in 0..80u64 {
        let mut t = T::new(seed ^ 0x7A11D);
        t.build([40usize, 200, 900][seed as usize % 3]);
        for _ in 0..30 {
            let (x, y) = (t.rng.range(0, W - 1), t.rng.range(0, H - 1));
            let top = t.b.hit_top(x, y);
            let v0 = snapshot(&t.b);
            let id = top.map(|i| v0[i].payload);
            let c = t.b.hit_cell(x, y, top);
            let since = t.b.epoch();
            // a few edits, sometimes across epochs
            for _ in 0..t.rng.range(0, 3) {
                t.edit();
                if t.rng.chance(1, 2) {
                    t.b.next_epoch();
                }
            }
            if t.b.cell_valid(c, since) {
                kept += 1;
                let v = snapshot(&t.b);
                let [x0, y0, x1, y1] = c.to_xyxy();
                for qy in y0..y1 {
                    for qx in x0..x1 {
                        let got = o_hit(&v, qx, qy).map(|i| v[i].payload);
                        assert_eq!(got, id, "seed {seed}: a valid cell {c:?} changed its answer at ({qx},{qy})");
                    }
                }
            } else {
                dropped += 1;
            }
        }
    }
    assert!(kept > 100 && dropped > 100, "both outcomes exercised: kept {kept}, dropped {dropped}");
}

#[test]
fn tab_order_equals_the_oracle() {
    for seed in 0..50u64 {
        let mut t = T::new(seed ^ 0x7AB);
        t.build([0usize, 1, 64, 65, 300, 2000][seed as usize % 6]);
        for round in 0..3 {
            let v = snapshot(&t.b);
            let n = v.len();
            let stops: Vec<usize> = (0..n).filter(|&i| v[i].kind & KF_FOCUS != 0).collect();
            let mut cur = None;
            for _ in 0..stops.len() + 2 {
                let want = match cur {
                    None => stops.first().copied(),
                    Some(c) => stops.iter().copied().find(|&s| s > c).or(stops.first().copied()),
                };
                let got = t.b.next_flagged(cur, KF_FOCUS, false);
                assert_eq!(got, want, "seed {seed} round {round} forward from {cur:?}");
                cur = got;
            }
            let mut cur = None;
            for _ in 0..stops.len() + 2 {
                let want = match cur {
                    None => stops.last().copied(),
                    Some(c) => stops.iter().rev().copied().find(|&s| s < c).or(stops.last().copied()),
                };
                let got = t.b.next_flagged(cur, KF_FOCUS, true);
                assert_eq!(got, want, "seed {seed} round {round} backward from {cur:?}");
                cur = got;
            }
            for p in 0..n.min(300) {
                let want = (p..n).find(|&i| v[i].kind & KF_INTERACT != 0);
                assert_eq!(t.b.first_flagged_from(p, KF_INTERACT), want);
                let want = (0..p).rev().find(|&i| v[i].kind & KF_INTERACT != 0);
                assert_eq!(t.b.last_flagged_before(p, KF_INTERACT), want);
            }
            for _ in 0..5 {
                if n > 0 {
                    t.edit();
                }
            }
        }
    }
}
