//! One surface, two executors, chosen per frame with no seam. The plan (`plan::Whole`) fixed everything it
//! could from types: whether a GPU pipeline exists at all, and the tile count below which one CPU core is
//! sooner than a GPU round trip. What is left to run time is area, and it is learned, not computed: the GPU
//! reports how many tiles a frame damaged (one word), remembered per block word, so the next write to that
//! word is sent to the executor its damage calls for without the CPU visiting a record to find out.
//!
//! Both executors write the same words into the same memory (the GPU's output buffer is the surface), so a
//! switch needs no repaint. Each keeps its own window lane and bins; the one that sat out is brought to the
//! block the surface shows before it draws again (`Scene::update` with the damage dropped, `Gpu::catch_up`).
use crate::expr::{U, U_PICK};
use crate::metal::Gpu;
use crate::plan::Whole;
use crate::scene::Scene;
use bough_kernel::cpu::Pool;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ran {
    Cpu,
    Gpu,
    /// The GPU, with this many overflowed tiles redrawn by the CPU.
    GpuFixed(u32),
}

pub struct Driver {
    pub scene: Scene,
    pub gpu: Option<Gpu>,
    whole: Whole,
    pool: Pool,
    /// The surface when there is no GPU.
    own: Vec<u32>,
    /// The block the surface shows.
    pub shown: U,
    /// Which executor's lanes are at `shown`.
    cpu_current: bool,
    gpu_current: bool,
    /// Tiles the last frame that changed block word `s` damaged.
    seen: [u32; 64],
    tiles: Vec<u32>,
    pub cap: usize,
    /// A record was rewritten since the last frame (its damage is unknown until an executor looks).
    pending: bool,
    /// Tiles the last frame after a record write damaged.
    seen_set: u32,
}

impl Driver {
    /// `whole` is the plan's whole-scene half for this scene's archetypes. No Metal object is created
    /// unless it asks for one.
    pub fn new(mut scene: Scene, whole: Whole, workers: usize) -> Driver {
        let words = (scene.fw * scene.fh) as usize;
        let cap = 192;
        let mut gpu = if whole.gpu { Gpu::new(&scene, words, cap) } else { None };
        if let Some(g) = gpu.as_mut() {
            g.fill_tabs(&mut scene);
        }
        let own = if gpu.is_some() { Vec::new() } else { vec![0; words] };
        // unknown damage is assumed large: the first write to a word goes to the GPU when there is one
        Driver { scene, gpu, whole, pool: Pool::new(workers), own, shown: U::default(), cpu_current: false, gpu_current: false, seen: [u32::MAX; 64], tiles: Vec::new(), cap, pending: false, seen_set: 0 }
    }
    pub fn pixels(&self) -> &[u32] {
        let words = (self.scene.fw * self.scene.fh) as usize;
        match &self.gpu {
            Some(g) => g.out(words),
            None => &self.own,
        }
    }
    /// Rewrite a record (`Scene::set`) on both executors' books.
    pub fn set<P: crate::paint::Paint>(&mut self, col: crate::scene::ColId<P>, i: usize, rect: [i32; 4], p: P) {
        let z = self.scene.set(col, i, rect, p);
        if let Some(g) = self.gpu.as_mut() {
            g.touch(z);
        }
        self.pending = true;
    }
    fn cpu_catch_up(&mut self) {
        if !self.cpu_current {
            let shown = self.shown;
            self.scene.update(&shown);
            self.scene.damage.clear();
            self.cpu_current = true;
        }
    }
    /// Draw block `u` (its `Pick` word is replaced by the pick under `pointer`, in the frame shown).
    pub fn frame(&mut self, u: &mut U, pointer: Option<(i32, i32)>) -> Ran {
        let stride = self.scene.fw as usize;
        let words = stride * self.scene.fh as usize;
        let started = self.cpu_current || self.gpu_current;
        u.w[U_PICK] = self.shown.w[U_PICK];
        let mut mask = if started { self.shown.diff(u, 64) } else { u64::MAX };
        if pointer.is_some() {
            mask |= 1 << U_PICK;
        }
        // predicted damage: the most any changed word damaged last time
        let mut predicted = (0..64).filter(|s| mask >> s & 1 != 0).map(|s| self.seen[s]).max().unwrap_or(0);
        if self.pending {
            predicted = predicted.max(self.seen_set);
        }
        let on_cpu = self.gpu.is_none() || (started && predicted <= self.whole.cpu_tiles);
        let (ran, tiles) = if on_cpu {
            self.cpu_catch_up();
            if let Some((x, y)) = pointer {
                let shown = self.shown;
                u.w[U_PICK] = self.scene.pick(&shown, x, y);
            }
            self.scene.update(u);
            let mut tl = std::mem::take(&mut self.tiles);
            self.scene.damage.tiles(&mut tl);
            self.scene.damage.clear();
            let out = match self.gpu.as_mut() {
                Some(g) => g.out_mut(words),
                None => &mut self.own[..],
            };
            let extra = if tl.len() > 64 { self.pool.workers() } else { 0 };
            self.scene.shade(u, &tl, out, stride, &self.pool, extra);
            let n = tl.len() as u32;
            self.tiles = tl;
            self.gpu_current = false;
            (Ran::Cpu, n)
        } else {
            let g = self.gpu.as_mut().unwrap();
            if !self.gpu_current && started {
                g.catch_up(&self.shown, stride);
            }
            let (_, pick) = g.frame(u, pointer, stride);
            u.w[U_PICK] = pick;
            let (n, over) = g.last();
            self.gpu_current = true;
            self.cpu_current = false;
            if over == 0 {
                (Ran::Gpu, n)
            } else {
                // a tile with more entries than a slab: the CPU kernel redraws it (it writes the same words)
                let mut tl = std::mem::take(&mut self.tiles);
                g.overflowed(&mut tl);
                self.shown = *u;
                self.cpu_catch_up();
                let g = self.gpu.as_mut().unwrap();
                self.scene.shade(u, &tl, g.out_mut(words), stride, &self.pool, 0);
                self.tiles = tl;
                (Ran::GpuFixed(over), n)
            }
        };
        if std::mem::take(&mut self.pending) {
            self.seen_set = tiles;
        }
        for s in 0..64 {
            if mask >> s & 1 != 0 {
                self.seen[s] = tiles;
            }
        }
        self.shown = *u;
        ran
    }
}
