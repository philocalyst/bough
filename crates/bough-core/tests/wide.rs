//! Content outside the 16-bit pixel domain (round 6, overfitting finding 3): every record's exact rect
//! must survive any sequence of edits and translations, however far it travels and however often it
//! leaves and re-enters the domain. The oracle here shares nothing with the core's rect types: it keeps
//! `[i64; 4]` rects in a `Vec` and answers hits and culls by scanning them (`oracle.rs` builds its rects
//! with the core's clamping constructor, so it cannot see this class of error).
use bough_core::bough::RecordInit;
use bough_core::rng::Rng;
use bough_core::{Bough, NRect, WRect};

const W: i64 = 512;

#[derive(Clone, Copy, PartialEq, Debug)]
struct M {
    r: Option<[i64; 4]>,
    payload: u32,
}

fn wide(r: Option<[i64; 4]>) -> WRect {
    match r {
        None => WRect::EMPTY,
        Some([x0, y0, x1, y1]) => WRect::new(x0 as i32, y0 as i32, x1 as i32, y1 as i32),
    }
}
fn moved(r: Option<[i64; 4]>, dx: i64, dy: i64) -> Option<[i64; 4]> {
    r.map(|[x0, y0, x1, y1]| [x0 + dx, y0 + dy, x1 + dx, y1 + dy])
}
/// The 16-bit view the core must report: clamped to the domain, empty if nothing is left.
fn view(r: Option<[i64; 4]>) -> NRect {
    match r {
        None => NRect::EMPTY,
        Some([x0, y0, x1, y1]) => NRect::new(x0.clamp(-40000, 40000) as i32, y0.clamp(-40000, 40000) as i32, x1.clamp(-40000, 40000) as i32, y1.clamp(-40000, 40000) as i32),
    }
}

struct T {
    b: Bough,
    m: Vec<M>,
    rng: Rng,
    next: u32,
}

impl T {
    fn new(seed: u64) -> T {
        let mut t = T { b: Bough::new(W as i32, W as i32), m: Vec::new(), rng: Rng::new(seed), next: 1 };
        t.b.insert_run(0, &[RecordInit::new(0, NRect::EMPTY, 1, 0)]);
        t.m.push(M { r: None, payload: 0 });
        t
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.rng.below((hi - lo) as u64) as i64
    }
    /// A rect of one of the shapes that matter: in the window, just across a domain edge, far away
    /// (a row of a long column), enormous (a document background), empty.
    fn rect(&mut self, row: i64) -> Option<[i64; 4]> {
        let (x, y) = (self.range(-50, W), self.range(-50, W));
        match self.rng.below(12) {
            0 => None,
            1..=3 => Some([x, y, x + self.range(1, 80), y + self.range(1, 40)]),
            4..=7 => Some([x, row * 24, x + 120, row * 24 + 24]),
            8 => {
                let e = [32700, -32800, 65500, -65600][self.rng.below(4) as usize] + self.range(-80, 80);
                Some([x, e, x + 30, e + self.range(1, 200)])
            }
            9 => Some([self.range(-200_000, 200_000), self.range(-200_000, 200_000), 0, 0]).map(|[a, b, _, _]| [a, b, a + self.range(1, 500), b + self.range(1, 500)]),
            10 => Some([0, self.range(-90_000, 10), W, self.range(40_000, 300_000)]),
            _ => Some([self.range(-70_000, 0), y, self.range(W, 70_000), y + 9]),
        }
    }
    fn insert(&mut self) {
        let pos = 1 + self.rng.below(self.m.len() as u64) as usize;
        let k = [1, 2, 7, 40, 64, 150][self.rng.below(6) as usize];
        let row0 = self.range(-3000, 9000);
        let mut recs = Vec::new();
        let mut ms = Vec::new();
        for i in 0..k {
            let r = self.rect(row0 + i as i64);
            recs.push(RecordInit::new(1, NRect::EMPTY, 1, self.next));
            ms.push(M { r, payload: self.next });
            self.next += 1;
        }
        // a narrow insert when every rect is in the domain, half of the time: both entry points
        let narrow = ms.iter().all(|m| wide(m.r).in_domain()) && self.rng.below(2) == 0;
        if narrow {
            for (rec, m) in recs.iter_mut().zip(&ms) {
                rec.ink = view(m.r);
            }
            self.b.insert_run(pos, &recs);
        } else {
            let w: Vec<WRect> = ms.iter().map(|m| wide(m.r)).collect();
            self.b.insert_run_wide(pos, &recs, &w);
        }
        self.m.splice(pos..pos, ms);
    }
    fn run(&mut self) -> (usize, usize) {
        let n = self.m.len();
        let start = 1 + self.rng.below(n as u64 - 1) as usize;
        let len = match self.rng.below(4) {
            0 => 1,
            1 => 1 + self.rng.below(70) as usize,
            _ => 1 + self.rng.below((n - start) as u64) as usize,
        };
        (start, len.min(n - start))
    }
    fn step(&mut self) {
        let n = self.m.len();
        match self.rng.below(100) {
            0..=17 if n < 1500 => self.insert(),
            0..=27 if n > 1 => {
                let (start, len) = self.run();
                self.b.remove_run(start, len);
                self.m.drain(start..start + len);
            }
            28..=69 if n > 1 => {
                // the scroll of a long column: steps of a few px, of a few thousand, and jumps past the domain
                let (start, len) = self.run();
                let big = [40, 3000, 33_000, 70_000, 150_000][self.rng.below(5) as usize];
                let (dx, dy) = (if self.rng.below(4) == 0 { self.range(-big, big) } else { 0 }, self.range(-big, big));
                if self.rng.below(3) == 0 {
                    self.b.scroll_run(start, len, dx as i32, dy as i32, NRect::new(0, 0, W as i32, W as i32));
                } else {
                    self.b.translate_run(start, len, dx as i32, dy as i32);
                }
                for m in &mut self.m[start..start + len] {
                    m.r = moved(m.r, dx, dy);
                }
            }
            70..=79 if n > 1 => {
                let pos = 1 + self.rng.below(n as u64 - 1) as usize;
                let row = self.range(-3000, 9000);
                let r = self.rect(row);
                self.b.set_ink_wide(pos, wide(r));
                self.m[pos].r = r;
            }
            80..=89 if n > 2 => {
                let (start, len) = self.run();
                let to = 1 + self.rng.below((n - len) as u64) as usize;
                self.b.move_run(start, len, to.min(n - len));
                let run: Vec<M> = self.m.drain(start..start + len).collect();
                let to = to.min(n - len);
                self.m.splice(to..to, run);
            }
            _ => self.b.next_epoch(),
        }
    }
    fn verify(&mut self, what: &str) {
        self.b.check_invariants();
        assert_eq!(self.b.len(), self.m.len(), "{what}");
        let recs = self.b.records();
        for (pos, m) in self.m.iter().enumerate() {
            assert_eq!(recs[pos].payload, m.payload, "{what}: payload at {pos}");
            assert_eq!(self.b.ink_wide_at(pos), wide(m.r), "{what}: exact ink at {pos} (payload {})", m.payload);
            assert_eq!(recs[pos].ink, view(m.r), "{what}: 16-bit view at {pos}");
        }
        // what a window sees: hits and the cull of the whole window, against a scan of the exact rects
        for _ in 0..24 {
            let (x, y) = (self.range(0, W), self.range(0, W));
            let want = self.m.iter().rposition(|m| m.r.is_some_and(|[x0, y0, x1, y1]| x0 <= x && x < x1 && y0 <= y && y < y1));
            assert_eq!(self.b.hit(x as i32, y as i32), want, "{what}: hit ({x}, {y})");
        }
        let mut out = Vec::new();
        self.b.cull(NRect::new(0, 0, W as i32, W as i32), None, &mut out);
        let want: Vec<u32> = self.m.iter().filter(|m| m.r.is_some_and(|[x0, y0, x1, y1]| x0 < W && x1 > 0 && y0 < W && y1 > 0)).map(|m| m.payload).collect();
        assert_eq!(out, want, "{what}: cull of the window");
    }
}

#[test]
fn exact_rects_survive_edits_and_translations_beyond_the_domain() {
    let seeds = if cfg!(debug_assertions) { 6 } else { 60 };
    for seed in 0..seeds {
        let mut t = T::new(0xF3 + seed);
        for step in 0..400 {
            t.step();
            t.verify(&format!("seed {seed} step {step}"));
        }
    }
}

/// Finding 3 as it was reported: rows of a column mounted at offset 0, far below the domain, then
/// scrolled to the end in steps: the last rows must be where the arithmetic says, and visible.
#[test]
fn a_long_column_scrolled_to_its_end() {
    for (rows, h, step) in [(1030i64, 32i64, 100i64), (2100, 32, 640), (20_000, 20, 4000), (1, 40_000, 97), (1, 16_780_000, 250_000)] {
        let mut b = Bough::new(W as i32, W as i32);
        b.insert_run(0, &[RecordInit::new(0, NRect::EMPTY, 1, 0)]);
        let recs: Vec<_> = (0..rows).map(|i| RecordInit::new(1, NRect::EMPTY, 1, 1 + i as u32)).collect();
        let rect = |i: i64, off: i64| WRect::new(10, (i * h - off) as i32, 200, (i * h + h - off) as i32);
        let w: Vec<_> = (0..rows).map(|i| rect(i, 0)).collect();
        b.insert_run_wide(1, &recs, &w);
        let end = rows * h - W;
        let mut off = 0;
        while off < end {
            let d = step.min(end - off);
            b.scroll_run(1, rows as usize, 0, -d as i32, NRect::new(0, 0, W as i32, W as i32));
            off += d;
        }
        b.check_invariants();
        for i in 0..rows {
            assert_eq!(b.ink_wide_at(1 + i as usize), rect(i, end), "{rows} rows of {h}: row {i} after scrolling to the end");
        }
        // the last row's bottom edge is the window's; the pixel above the bottom edge hits it
        assert_eq!(b.hit(50, W as i32 - 1), Some(rows as usize), "{rows} rows of {h}");
        // and back to the top
        b.scroll_run(1, rows as usize, 0, end.min(1 << 28) as i32, NRect::new(0, 0, W as i32, W as i32));
        b.check_invariants();
        assert_eq!(b.ink_wide_at(1), rect(0, 0));
        assert_eq!(b.hit(50, 0), Some(1));
    }
}
