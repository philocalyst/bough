//! The presentation policy: a ring of buffers repaired by age, and commit on input.
//!
//! A compositor holds the buffer it shows until a newer one is shown, and holds a committed buffer until it
//! latches it or a newer commit supersedes it. So a frame is drawn into a buffer that last held some older
//! state, and has to repair everything that changed since: the ring keeps, per buffer, the tiles damaged since
//! that buffer was drawn (`stale`). A frame's repair is its buffer's stale set, which always includes the
//! frame's own damage; the ring picks the free buffer with the least to repair.
//!
//! **Commit on input.** The host does not wait for a frame callback before drawing again. An input that changes
//! the state is drawn and committed at once, into another buffer if the last commit is still unlatched; the
//! compositor latches whichever commit is newest at its deadline, so the newest input is never one refresh
//! late because an older frame was in flight. What the compositor is told is damaged with each commit is the
//! damage since the *previous commit* (not since the previous latch): a compositor accumulates buffer damage
//! over the commits it did not show, so the damage stays exact across superseded commits and nothing is
//! over-reported. Continuous animation is different: time advances only when a frame callback fires, so an
//! animation runs at the output's refresh and no faster.
//!
//! When every buffer is held the damage is kept (it is in every stale set) and the frame is drawn when a
//! release arrives; the ring grows up to `max` buffers before that happens.
use crate::TilesRef;

/// An owned tile set (the geometry of [`TilesRef`]).
#[derive(Clone)]
pub struct Tiles {
    pub bits: Vec<u64>,
    pub stride: usize,
    pub rows: usize,
    pub lo: usize,
    pub hi: usize,
}
impl Tiles {
    pub fn new(w: i32, h: i32) -> Tiles {
        let cols = ((w + 31) >> 5) as usize;
        let rows = ((h + 31) >> 5) as usize;
        let stride = cols.div_ceil(64).max(1);
        Tiles { bits: vec![0; stride * rows], stride, rows, lo: usize::MAX, hi: 0 }
    }
    pub fn as_ref(&self) -> TilesRef<'_> {
        TilesRef { bits: &self.bits, stride: self.stride, lo: self.lo, hi: self.hi }
    }
    pub fn any(&self) -> bool {
        self.lo < self.hi
    }
    pub fn clear(&mut self) {
        if self.lo < self.hi {
            self.bits[self.lo * self.stride..self.hi * self.stride].fill(0);
        }
        (self.lo, self.hi) = (usize::MAX, 0);
    }
    pub fn add(&mut self, t: TilesRef<'_>) {
        debug_assert!(t.stride == self.stride);
        if t.lo < t.hi {
            let (a, b) = (t.lo * t.stride, t.hi.min(self.rows) * t.stride);
            for (d, s) in self.bits[a..b].iter_mut().zip(&t.bits[a..b]) {
                *d |= *s;
            }
            self.lo = self.lo.min(t.lo);
            self.hi = self.hi.max(t.hi.min(self.rows));
        }
    }
    pub fn count(&self) -> u32 {
        if self.lo < self.hi { self.bits[self.lo * self.stride..self.hi * self.stride].iter().map(|w| w.count_ones()).sum() } else { 0 }
    }
    /// Rectangles (x, y, w, h in pixels, cut to `w` x `h`) covering exactly the set: per tile row one per
    /// maximal run of tiles, extended down while the next row has the same run.
    pub fn rects(&self, w: i32, h: i32, mut f: impl FnMut(i32, i32, i32, i32)) {
        const N: usize = 16;
        let mut open = [(0u32, 0u32, 0u32); N];
        let mut nopen = 0usize;
        let mut emit = |o: (u32, u32, u32), y1: u32| {
            let (x0, y0) = ((o.0 as i32) << 5, (o.2 as i32) << 5);
            let (x1, y1) = (((o.1 as i32) << 5).min(w), ((y1 as i32) << 5).min(h));
            if x1 > x0 && y1 > y0 {
                f(x0, y0, x1 - x0, y1 - y0)
            }
        };
        let (lo, hi) = (self.lo.min(self.hi), self.hi.min(self.rows));
        for ty in lo..=hi {
            let mut runs = [(0u32, 0u32); N];
            let mut nruns = 0usize;
            if ty < hi {
                let (mut x, n) = (0u32, (self.stride * 64) as u32);
                while x < n {
                    let wd = self.bits[ty * self.stride + (x >> 6) as usize] >> (x & 63);
                    if wd == 0 {
                        x = (x | 63) + 1;
                        continue;
                    }
                    x += wd.trailing_zeros();
                    let a = x;
                    while x < n && self.bits[ty * self.stride + (x >> 6) as usize] >> (x & 63) & 1 != 0 {
                        x += 1;
                    }
                    if nruns == N {
                        // more runs in a row than are tracked: the last one widens (never under-reports)
                        runs[N - 1].1 = x;
                    } else {
                        runs[nruns] = (a, x);
                        nruns += 1;
                    }
                }
            }
            let mut kept = [false; N];
            let mut k = 0;
            while k < nopen {
                let o = open[k];
                match runs[..nruns].iter().position(|r| *r == (o.0, o.1)) {
                    Some(j) => {
                        kept[j] = true;
                        k += 1;
                    }
                    None => {
                        emit(o, ty as u32);
                        nopen -= 1;
                        open[k] = open[nopen];
                    }
                }
            }
            for j in 0..nruns {
                if !kept[j] {
                    open[nopen] = (runs[j].0, runs[j].1, ty as u32);
                    nopen += 1;
                }
            }
        }
    }
}

struct Buf {
    /// The compositor holds it (committed and not released).
    held: bool,
    /// It has never been drawn: everything is to repair.
    blank: bool,
    /// Tiles damaged since it was drawn.
    stale: Tiles,
}

/// What a frame is drawn into.
pub struct Frame {
    pub buf: usize,
    /// Everything (the buffer has never been drawn); else [`Ring::repair`] says which tiles.
    pub full: bool,
}

pub struct Ring {
    w: i32,
    h: i32,
    bufs: Vec<Buf>,
    max: usize,
    /// Damage since the last commit: what the compositor is told with the next one.
    since: Tiles,
    /// The last commit damaged everything (the first, or the first after a resize).
    since_all: bool,
    scratch: Tiles,
    pub stats: Stats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub frames: u64,
    pub full: u64,
    pub tiles_repaired: u64,
    pub tiles_reported: u64,
    /// Frames that could not be drawn at once because every buffer was held.
    pub starved: u64,
    pub grown: u64,
}

impl Ring {
    /// A ring for a `w` x `h` window that starts with `n` buffers and may grow to `max`.
    pub fn new(w: i32, h: i32, n: usize, max: usize) -> Ring {
        let mut r = Ring { w, h, bufs: Vec::new(), max: max.max(n).max(1), since: Tiles::new(w, h), since_all: true, scratch: Tiles::new(w, h), stats: Stats::default() };
        for _ in 0..n.max(1) {
            r.push();
        }
        r
    }
    fn push(&mut self) {
        self.bufs.push(Buf { held: false, blank: true, stale: Tiles::new(self.w, self.h) });
    }
    pub fn len(&self) -> usize {
        self.bufs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bufs.is_empty()
    }
    pub fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }
    pub fn held(&self, i: usize) -> bool {
        self.bufs[i].held
    }
    /// Buffers that have been drawn at least once (their pages exist).
    pub fn drawn(&self, i: usize) -> bool {
        !self.bufs[i].blank
    }

    /// The state changed by `damage` since the last call. Every buffer is now stale there.
    pub fn damage(&mut self, damage: TilesRef<'_>) {
        for b in &mut self.bufs {
            if !b.blank {
                b.stale.add(damage);
            }
        }
        self.since.add(damage);
    }
    /// Something is owed to the screen: damage noted and not yet committed.
    pub fn owed(&self) -> bool {
        self.since.any() || self.since_all
    }

    /// A buffer to draw the current state into: a free one with the least to repair, a new one if all are held
    /// and the ring may grow, or `None` (draw when a release arrives; the damage is kept).
    pub fn begin(&mut self) -> Option<Frame> {
        let mut best: Option<(usize, u32)> = None;
        for (i, b) in self.bufs.iter().enumerate() {
            if b.held {
                continue;
            }
            let cost = if b.blank { u32::MAX } else { b.stale.count() };
            if best.is_none_or(|(_, c)| cost < c) {
                best = Some((i, cost));
            }
        }
        let i = match best {
            Some((i, _)) => i,
            None if self.bufs.len() < self.max => {
                self.push();
                self.stats.grown += 1;
                self.bufs.len() - 1
            }
            None => {
                self.stats.starved += 1;
                return None;
            }
        };
        let b = &mut self.bufs[i];
        self.stats.frames += 1;
        if b.blank {
            self.stats.full += 1;
            return Some(Frame { buf: i, full: true });
        }
        // the repair set is the buffer's stale set; it is handed out as `scratch` so the stale set can be
        // cleared now
        core::mem::swap(&mut self.scratch, &mut b.stale);
        b.stale.clear();
        if cfg!(feature = "mutant_age") {
            self.scratch.clear();
            self.scratch.add(self.since.as_ref());
        }
        self.stats.tiles_repaired += self.scratch.count() as u64;
        Some(Frame { buf: i, full: false })
    }
    /// The tiles to repaint in the frame `begin` returned (when it is not `full`).
    pub fn repair(&self) -> TilesRef<'_> {
        self.scratch.as_ref()
    }
    /// The frame was drawn into buffer `i` and is being committed: `f(x, y, w, h)` for each rectangle the
    /// compositor is to be told changed since the previous commit. The buffer is the compositor's until
    /// [`Ring::release`].
    pub fn commit(&mut self, i: usize, mut f: impl FnMut(i32, i32, i32, i32)) {
        let b = &mut self.bufs[i];
        b.held = true;
        b.blank = false;
        if self.since_all {
            f(0, 0, self.w, self.h);
            self.stats.tiles_reported += (self.since.stride * 64 * self.since.rows) as u64;
        } else {
            self.stats.tiles_reported += self.since.count() as u64;
            self.since.rects(self.w, self.h, &mut f);
        }
        self.since_all = false;
        self.since.clear();
    }
    /// The compositor released buffer `i`.
    pub fn release(&mut self, i: usize) {
        if let Some(b) = self.bufs.get_mut(i) {
            b.held = false;
        }
    }
    /// The commit about to be made supersedes one the compositor has not shown (`yes`). Nothing to do: the
    /// other's damage was reported with it and the compositor accumulated it. This is where the mutant that
    /// gets it wrong lives (it reports the damage as if the superseded commit had never been made known).
    #[doc(hidden)]
    pub fn superseded_mutant_if(&mut self, yes: bool) {
        if cfg!(feature = "mutant_superseded") && yes {
            self.since.clear();
        }
    }
}
