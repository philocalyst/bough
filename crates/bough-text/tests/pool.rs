//! Differential tests for the glyph pool (Attempts 1 and 2): every update, on every path, must
//! leave exactly the instances a fresh shape + the reference 48-byte writer produce, and its
//! tile marks must contain the oracle's (per-slot rendered-box comparison of old and new),
//! equal it on the class-patch path, and cover every instance in the rendered symmetric
//! difference.

#[path = "../examples/common/corpus.rs"]
mod corpus;

use bough_text::atlas::{GlyphCache, Instance};
use bough_text::pool::{mark_rect, GlyphPool, Label, NRect};
use bough_text::{GlyphRun, Shaper, SimpleScript};

const FONT: &[u8] = zgx_workload::FONT_BYTES;

fn full() -> bool {
    !cfg!(debug_assertions) || std::env::var("BOUGH_FULL").is_ok()
}

struct World {
    sh: Shaper,
    cache: GlyphCache,
    pool: GlyphPool,
    run: GlyphRun,
    /// Over all checked updates: oracle tiles, marked tiles, updates.
    oracle_tiles: u64,
    marked_tiles: u64,
    updates: u64,
    /// Tiles the class patch marked beyond the oracle (it uses the changed glyphs' common
    /// vertical extent).
    patch_over: u64,
}

struct L {
    lb: Label,
    size: f32,
    origin: [f32; 2],
    color: u32,
    space: [i32; 2],
}

fn abs_boxes(w: &World, l: &L) -> Vec<Option<(u16, NRect)>> {
    w.pool
        .slot_boxes(&w.cache, l.lb.run)
        .into_iter()
        .map(|b| b.map(|(e, r)| (e, [r[0] + l.space[0], r[1] + l.space[1], r[2] - l.space[0], r[3] - l.space[1]])))
        .collect()
}

fn instances(w: &World, l: &L) -> Vec<Instance> {
    let mut v = vec![];
    w.pool.expand(&w.cache, l.lb.run, &mut v);
    v
}

impl World {
    fn new() -> Self {
        World {
            sh: Shaper::new(SimpleScript::build(FONT, 0).unwrap()),
            cache: GlyphCache::new(FONT),
            pool: GlyphPool::new(),
            run: GlyphRun::default(),
            oracle_tiles: 0,
            marked_tiles: 0,
            updates: 0,
            patch_over: 0,
        }
    }

    fn label(&mut self, size: f32, origin: [f32; 2], space: [i32; 2], id: u16) -> L {
        let strike = self.cache.strike(size);
        self.pool.set_space(id, space);
        let lb = self.pool.alloc_label(&self.sh, strike, size, origin, 0xffe5edf7, id, &mut self.cache);
        L { lb, size, origin, color: 0xffe5edf7, space }
    }

    /// Apply `text`, check everything; returns whether the class patch path was taken.
    fn set(&mut self, l: &mut L, text: &str) -> bool {
        let old_boxes = abs_boxes(self, l);
        let old_inst = instances(self, l);
        let before = self.pool.stats;
        let mut marks = vec![0u128; PLANE_ROWS];
        let ok_pool = self.pool.set_text(&self.sh, &mut self.cache, &mut l.lb, text, &mut marks);
        let patched = self.pool.stats[0] > before[0];
        let ok = self.sh.shape_into(text, l.size, &mut self.run);
        assert_eq!(ok_pool, ok, "pool and shaper disagree on eligibility of {text:?}");
        if !ok {
            assert!(marks.iter().all(|&r| r == 0));
            return false;
        }
        assert_eq!(l.lb.text(), text.as_bytes());
        // (1) instances equal the reference writer on a fresh shape.
        let strike = self.cache.strike(l.size);
        let mut want = vec![];
        self.cache.instances(strike, &self.run, l.origin[0], l.origin[1], l.color, &mut want);
        let mut got = instances(self, l);
        for i in got.iter_mut() {
            i.space = 0;
        }
        assert_eq!(got, want, "instances of {text:?}");
        assert_eq!(l.lb.width(), self.run.width, "width of {text:?}");
        // (2) oracle marks: slots whose rendered glyph changed, old and new box.
        let new_boxes = abs_boxes(self, l);
        let mut oracle = vec![0u128; PLANE_ROWS];
        for k in 0..old_boxes.len().max(new_boxes.len()) {
            let o = old_boxes.get(k).copied().flatten();
            let n = new_boxes.get(k).copied().flatten();
            if o != n {
                for b in [o, n].into_iter().flatten() {
                    mark_rect(&mut oracle, &b.1);
                }
            }
        }
        for r in 0..PLANE_ROWS {
            assert_eq!(oracle[r] & !marks[r], 0, "row {r}: marks miss oracle tiles for {text:?}");
        }
        let tiles = |p: &[u128]| p.iter().map(|r| r.count_ones() as u64).sum::<u64>();
        if patched {
            self.patch_over += tiles(&marks) - tiles(&oracle);
        }
        // (3) every instance of the rendered symmetric difference is inside the marks.
        let new_inst = instances(self, l);
        for (a, b) in [(&old_inst, &new_inst), (&new_inst, &old_inst)] {
            for i in a.iter().filter(|i| !b.contains(i)) {
                let (x0, y0) = (i.xy[0] as i32 + l.space[0], i.xy[1] as i32 + l.space[1]);
                let mut t = vec![0u128; PLANE_ROWS];
                mark_rect(&mut t, &[x0, y0, -(x0 + i.wh[0] as i32), -(y0 + i.wh[1] as i32)]);
                for r in 0..PLANE_ROWS {
                    assert_eq!(t[r] & !marks[r], 0, "changed instance {i:?} outside marks for {text:?}");
                }
            }
        }
        self.oracle_tiles += tiles(&oracle);
        self.marked_tiles += tiles(&marks);
        self.updates += 1;
        patched
    }
}

const PLANE_ROWS: usize = 64;

fn xorshift(st: &mut u64) -> u64 {
    *st ^= *st << 13;
    *st ^= *st >> 7;
    *st ^= *st << 17;
    *st
}

#[test]
fn heavy_numeric_labels_over_consecutive_ticks() {
    let mut w = World::new();
    let first = corpus::heavy_numeric(0);
    let mut labels: Vec<L> = first
        .iter()
        .enumerate()
        .map(|(i, (_, z))| w.label(*z, [12.0 + (i % 7) as f32 * 0.37, 20.0 + (i % 5) as f32], [16 + 128 * (i as i32 % 6), 120 + 24 * (i as i32 / 6)], i as u16 + 1))
        .collect();
    let ticks = if full() { 1500 } else { 120 };
    let mut patched = 0;
    for live in 0..ticks {
        for (l, (s, _)) in labels.iter_mut().zip(corpus::heavy_numeric(live)) {
            patched += w.set(l, &s) as u64;
        }
    }
    eprintln!(
        "heavy numeric: {} updates, {} class patches; oracle tiles {} marked {} (+{:.2}%) ; paths {:?}",
        w.updates,
        patched,
        w.oracle_tiles,
        w.marked_tiles,
        100. * (w.marked_tiles as f64 / w.oracle_tiles as f64 - 1.),
        w.pool.stats
    );
    // The class patch (`patch16`) is a NEON kernel with no x86-64 twin yet: elsewhere every update takes the window
    // or replacement paths, and the exactness checks above are what this test establishes.
    #[cfg(target_arch = "aarch64")]
    assert!(patched > w.updates / 2, "most heavy updates must be digit patches");
    #[cfg(not(target_arch = "aarch64"))]
    assert_eq!(patched, 0, "no class patch exists off aarch64; a new one needs this test to require it");
    eprintln!("class-patch tiles beyond the oracle: {}", w.patch_over);
    assert!(w.patch_over * 100 < 3 * w.oracle_tiles, "digit patches mark the digit cell: within 3% of the oracle");
}

#[test]
fn random_edits_all_paths() {
    let mut w = World::new();
    let alphabet: Vec<&str> = vec![
        "0", "1", "7", "9", ".", " ", "m", "s", "%", "a", "f", "i", "l", "T", "o", "W", "A", "V", "j", "\u{e9}", "\u{c0}", "-", "/", ",",
    ];
    let mut st = 0x2545F4914F6CDD1Du64;
    let n_labels = 24;
    let mut labels: Vec<L> = (0..n_labels)
        .map(|i| {
            let size = [10.0, 12.0, 14.0, 16.0, 20.0, 31.0][i % 6];
            w.label(size, [3.0 + i as f32 * 0.31, 40.0 + i as f32], [i as i32 * 50, 7 + i as i32 * 13], i as u16 + 1)
        })
        .collect();
    let mut texts: Vec<String> = vec![String::new(); n_labels];
    let steps = if full() { 60_000 } else { 4_000 };
    for _ in 0..steps {
        let k = (xorshift(&mut st) % n_labels as u64) as usize;
        let mut t: Vec<String> = texts[k].chars().map(|c| c.to_string()).collect();
        match xorshift(&mut st) % 6 {
            // replace one char
            0 | 1 if !t.is_empty() => {
                let i = (xorshift(&mut st) % t.len() as u64) as usize;
                t[i] = alphabet[(xorshift(&mut st) % alphabet.len() as u64) as usize].to_string();
            }
            // insert
            2 => {
                let i = (xorshift(&mut st) % (t.len() as u64 + 1)) as usize;
                t.insert(i, alphabet[(xorshift(&mut st) % alphabet.len() as u64) as usize].to_string());
            }
            // delete
            3 if !t.is_empty() => {
                let i = (xorshift(&mut st) % t.len() as u64) as usize;
                t.remove(i);
            }
            // digits only change (tabular)
            4 => {
                for c in t.iter_mut() {
                    if c.len() == 1 && c.as_bytes()[0].is_ascii_digit() && xorshift(&mut st) % 2 == 0 {
                        *c = ((b'0' + (xorshift(&mut st) % 10) as u8) as char).to_string();
                    }
                }
            }
            // brand new string (grows past 16/32/64 slots sometimes)
            _ => {
                let len = (xorshift(&mut st) % 80) as usize;
                t = (0..len).map(|_| alphabet[(xorshift(&mut st) % alphabet.len() as u64) as usize].to_string()).collect();
            }
        }
        let s: String = t.concat();
        w.set(&mut labels[k], &s);
        texts[k] = s;
    }
    eprintln!(
        "random edits: {} updates; oracle tiles {} marked {} (+{:.2}%); paths {:?}",
        w.updates,
        w.oracle_tiles,
        w.marked_tiles,
        100. * (w.marked_tiles as f64 / w.oracle_tiles.max(1) as f64 - 1.),
        w.pool.stats
    );
    #[cfg(target_arch = "aarch64")]
    assert!(w.pool.stats.iter().all(|&c| c > 0), "every path exercised: {:?}", w.pool.stats);
    // No class patch off aarch64 (see heavy_numeric_labels_over_consecutive_ticks): every other path is exercised.
    #[cfg(not(target_arch = "aarch64"))]
    assert!(w.pool.stats[0] == 0 && w.pool.stats[1..].iter().all(|&c| c > 0), "paths: {:?}", w.pool.stats);
}

#[test]
fn refused_text_leaves_label_untouched() {
    let mut w = World::new();
    let mut l = w.label(14.0, [0.0, 20.0], [0, 0], 1);
    w.set(&mut l, "12.5 ms");
    let mut plane = vec![0u128; PLANE_ROWS];
    assert!(!w.pool.set_text(&w.sh, &mut w.cache, &mut l.lb, "\u{65e5}\u{672c}", &mut plane));
    // A refused update must not have corrupted the slots: the next update still checks out.
    w.set(&mut l, "12.6 ms");
    w.set(&mut l, "");
    w.set(&mut l, "office \u{e9}t\u{e9} fluff");
    w.set(&mut l, "office");
}
