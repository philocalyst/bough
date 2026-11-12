//! Codec-style redundancy of real frames, per 32x32 tile (Opus collaborator probe, round 6; not a gate).
use std::collections::{HashMap, HashSet};

fn tile_hash(px: &[u8], w: usize, h: usize, tx: usize, ty: usize) -> (u64, u32) {
    // FNV-1a over the tile's bytes (rows clipped to the window) + the number of distinct colours (capped at 17)
    let mut hsh: u64 = 0xcbf29ce484222325;
    let mut cols: Vec<u32> = Vec::with_capacity(32);
    for y in ty * 32..(ty * 32 + 32).min(h) {
        let row = &px[(y * w + tx * 32) * 4..(y * w + (tx * 32 + 32).min(w)) * 4];
        for c in row.chunks_exact(4) {
            let v = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            hsh = (hsh ^ v as u64).wrapping_mul(0x100000001b3);
            if cols.len() <= 16 && !cols.contains(&v) {
                cols.push(v);
            }
        }
    }
    (hsh, cols.len() as u32)
}

#[derive(Default)]
pub struct Acc {
    pub frames: u64,
    pub tiles_total: u64,
    pub shaded: u64,
    pub changed: u64,
    pub shaded_unchanged: u64,
    pub changed_not_shaded: u64,
    pub changed_const: u64,
    pub changed_2col: u64,
    pub changed_le16: u64,
    pub changed_in_prev_same_pos: u64,
    pub changed_in_prev_anywhere: u64,
    pub changed_in_cur_elsewhere: u64,
    pub changed_any_cheap: u64,
    pub distinct: u64,
    pub uniform: u64,
    pub held_two_ago: u64,
}

pub struct Stats {
    w: usize,
    h: usize,
    tx: usize,
    ty: usize,
    prev: Option<Vec<(u64, u32)>>,
    prev2: Option<Vec<(u64, u32)>>,
    pub acc: Acc,
}

impl Stats {
    pub fn new(w: usize, h: usize) -> Stats {
        Stats { w, h, tx: w.div_ceil(32), ty: h.div_ceil(32), prev: None, prev2: None, acc: Acc::default() }
    }
    /// One presented frame and the tile rows the GPU shaded for it.
    pub fn frame(&mut self, px: &[u8], rows: &[u128]) {
        let cur: Vec<(u64, u32)> = (0..self.ty).flat_map(|y| (0..self.tx).map(move |x| (x, y))).map(|(x, y)| tile_hash(px, self.w, self.h, x, y)).collect();
        if let Some(prev) = &self.prev {
            let a = &mut self.acc;
            a.frames += 1;
            a.tiles_total += cur.len() as u64;
            let prev_set: HashSet<u64> = prev.iter().map(|t| t.0).collect();
            let mut cur_count: HashMap<u64, u32> = HashMap::new();
            for t in &cur {
                *cur_count.entry(t.0).or_default() += 1;
            }
            a.distinct += cur_count.len() as u64;
            a.uniform += cur.iter().filter(|t| t.1 == 1).count() as u64;
            for y in 0..self.ty {
                let bits = rows.get(y).copied().unwrap_or(0);
                for x in 0..self.tx {
                    let i = y * self.tx + x;
                    let shaded = (bits >> x) & 1 == 1;
                    let changed = cur[i].0 != prev[i].0;
                    a.shaded += shaded as u64;
                    a.changed += changed as u64;
                    if shaded && !changed {
                        a.shaded_unchanged += 1;
                    }
                    if changed && !shaded {
                        a.changed_not_shaded += 1;
                    }
                    if changed {
                        let c = cur[i].1;
                        a.changed_const += (c == 1) as u64;
                        a.changed_2col += (c == 2) as u64;
                        a.changed_le16 += (c <= 16) as u64;
                        let in_prev = prev_set.contains(&cur[i].0);
                        let elsewhere = cur_count[&cur[i].0] > 1;
                        a.changed_in_prev_anywhere += in_prev as u64;
                        a.changed_in_cur_elsewhere += elsewhere as u64;
                        a.changed_any_cheap += (c == 1 || in_prev || elsewhere) as u64;
                        if let Some(p2) = &self.prev2 {
                            a.held_two_ago += (p2[i].0 == cur[i].0) as u64;
                        }
                    }
                    let _ = &a.changed_in_prev_same_pos;
                }
            }
        }
        self.prev2 = self.prev.take();
        self.prev = Some(cur);
    }
    pub fn report(&self, label: &str) {
        let a = &self.acc;
        let f = a.frames.max(1) as f64;
        let pc = |n: u64, d: u64| if d == 0 { 0.0 } else { 100.0 * n as f64 / d as f64 };
        println!(
            "{label:16} frames {:4} | tiles/frame {:6.0} distinct {:5.1}% uniform {:5.1}% | shaded/frame {:7.1} changed/frame {:7.1} shaded-but-unchanged {:5.1}% of shaded | changed-not-shaded {:6.1}/frame",
            a.frames,
            a.tiles_total as f64 / f,
            pc(a.distinct, a.tiles_total),
            pc(a.uniform, a.tiles_total),
            a.shaded as f64 / f,
            a.changed as f64 / f,
            pc(a.shaded_unchanged, a.shaded),
            a.changed_not_shaded as f64 / f
        );
        println!(
            "{:16}   of changed tiles: constant {:5.1}%  2-colour {:5.1}%  <=16 colours {:5.1}%  equal to a tile of the previous frame {:5.1}%  equal to another tile of this frame {:5.1}%  any of these {:5.1}%  equal to the same tile 2 frames ago {:5.1}%",
            "",
            pc(a.changed_const, a.changed),
            pc(a.changed_2col, a.changed),
            pc(a.changed_le16, a.changed),
            pc(a.changed_in_prev_anywhere, a.changed),
            pc(a.changed_in_cur_elsewhere, a.changed),
            pc(a.changed_any_cheap, a.changed),
            pc(a.held_two_ago, a.changed)
        );
    }
}
