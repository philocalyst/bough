//! `App<V>`: owns the view value (the owner), the runtime, the optional Metal presenter, and drives
//! turns: handler -> settle wires -> text -> damage -> cull -> draw list -> present.
use crate::elem::{Element, Mounter};
use crate::repair::rects_from_plane;
use crate::rt::*;
use bough_core::{NRect, RecordInit};

/// A view: a plain struct whose `Signal` fields are the state. `view` describes it once at mount
/// (see RESULTS.md: re-description/diff for `FIXED = false` is not implemented in this slice).
pub trait View: Sized + 'static {
    const FIXED: bool = false;
    fn view(&self) -> impl Element<Self> + '_;
}

pub struct Config {
    pub width_pt: f32,
    pub height_pt: f32,
    pub scale: f32,
    pub font: Vec<u8>,
    /// Surfaces in the ring (age tracking is per surface). 4 = compositor holds two + one drawing + one spare.
    pub ring: usize,
}

pub const PH_UPDATE: usize = 0;
pub const PH_TEXT: usize = 1;
pub const PH_DAMAGE: usize = 2;
pub const PH_CULL: usize = 3;
pub const PH_PRESENT: usize = 4;
pub const PHASE_NAMES: [&str; 5] = ["update", "text", "damage", "cull+list", "present"];

/// Instruction-count phase profiler (proc_pid_rusage; off unless enabled).
pub struct Prof {
    pub on: bool,
    pub acc: [u64; 5],
    last: u64,
    ovh: u64,
}
impl Prof {
    fn new() -> Prof {
        Prof { on: false, acc: [0; 5], last: 0, ovh: 0 }
    }
    pub fn enable(&mut self) {
        self.on = true;
        self.ovh = bough_metal::perf::snap_overhead();
    }
    #[inline]
    pub fn reset(&mut self) {
        if self.on {
            self.last = bough_metal::perf::snap().instrs;
        }
    }
    #[inline]
    pub fn mark(&mut self, i: usize) {
        if self.on {
            let s = bough_metal::perf::snap().instrs;
            self.acc[i] += (s - self.last).saturating_sub(self.ovh);
            self.last = bough_metal::perf::snap().instrs;
        }
    }
}

pub struct App<V: View> {
    pub owner: Box<V>,
    pub rt: Rt,
    #[cfg(target_os = "macos")]
    pub gpu: Option<crate::backend::metal::Metal>,
    ages: Vec<Option<u32>>,
    next_surf: usize,
    rects: Vec<[i32; 4]>,
    rect_scratch: crate::repair::RectScratch,
    pub prof: Prof,
    pub frames: u64,
    pub last_surface: usize,
    /// Pixels the last repair re-drew (rect area), for reports.
    pub repaired_px: u64,
}

impl<V: View> App<V> {
    /// Mount `view`. `gpu`: create the Metal presenter (shared pool); otherwise the pool is heap
    /// memory and `frame` stops after the draw list (the RENDER=0 shape).
    pub fn new(view: V, cfg: Config, gpu: bool) -> App<V> {
        let (win_w, win_h) = ((cfg.width_pt * cfg.scale).round() as i32, (cfg.height_pt * cfg.scale).round() as i32);
        let cfg = Config { ring: if cfg.ring == 0 { 4 } else { cfg.ring }, ..cfg };
        #[cfg(target_os = "macos")]
        let mut metal = gpu.then(|| crate::backend::metal::Metal::new(win_w as usize, win_h as usize, cfg.scale as f64, cfg.ring));
        #[cfg(not(target_os = "macos"))]
        let _ = gpu;
        #[cfg(target_os = "macos")]
        let pool = match &metal {
            Some(m) => unsafe { Pool::external(m.pool_ptr()) },
            None => Pool::heap(),
        };
        #[cfg(not(target_os = "macos"))]
        let pool = Pool::heap();
        let mut rt = Rt::new(win_w, win_h, cfg.scale, &cfg.font, pool);
        let owner = Box::new(view);
        let mut recs: Vec<RecordInit> = Vec::with_capacity(4096);
        let mut scope = Scope::default();
        let ptr = &*owner as *const V;
        {
            let el = owner.view();
            let mut m = Mounter { rt: &mut rt, owner: ptr, recs: &mut recs, scope: &mut scope, depth: 0, ox: 0.0, oy: 0.0, space: 0, clip: 0 };
            el.mount(&mut m);
        }
        rt.bough.insert_run(0, &recs);
        #[cfg(target_os = "macos")]
        if let Some(m) = metal.as_mut() {
            m.sync_atlas(&mut rt.text.cache.atlas);
        }
        let ring = cfg.ring.max(1);
        App {
            owner,
            rt,
            #[cfg(target_os = "macos")]
            gpu: metal,
            ages: vec![None; ring],
            next_surf: 0,
            rects: Vec::with_capacity(MAX_RECTS),
            rect_scratch: Default::default(),
            prof: Prof::new(),
            frames: 0,
            last_surface: 0,
            repaired_px: 0,
        }
    }

    /// One turn: run the handler, settle the wires it queued, shape changed text.
    pub fn turn(&mut self, f: impl FnOnce(&mut V)) {
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut() {
            g.sync();
        }
        self.prof.reset();
        f(&mut self.owner);
        let owner = &*self.owner as *const V as *const u8;
        self.rt.settle(owner);
        self.prof.mark(PH_UPDATE);
        self.rt.flush_text();
        self.prof.mark(PH_TEXT);
    }

    /// The handler half of a turn alone: mutate the view's signals (no settling). Pair with `flush`.
    pub fn write(&mut self, f: impl FnOnce(&mut V)) {
        f(&mut self.owner);
    }

    /// The rest of a turn after `write`: settle the queued signals' wires and shape changed text.
    pub fn flush(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut() {
            g.sync();
        }
        let owner = &*self.owner as *const V as *const u8;
        self.rt.settle(owner);
        self.rt.flush_text();
    }

    /// Damage -> cull -> draw list -> present. Returns false (and does nothing at all) if the
    /// epoch has no damage: idle is zero work.
    pub fn frame(&mut self) -> bool {
        if self.rt.bough.plane(0).bbox.is_empty() {
            return false;
        }
        let n = self.ages.len();
        #[allow(unused_mut)]
        let mut idx = self.next_surf;
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_ref() {
            for k in 0..n {
                let c = (self.next_surf + k) % n;
                if !g.is_in_use(c) {
                    idx = c;
                    break;
                }
            }
        }
        self.next_surf = (idx + 1) % n;
        let epoch = self.rt.bough.epoch();
        let (w, h) = (self.rt.win_w, self.rt.win_h);
        let age = self.ages[idx].map(|l| epoch - l);
        let full = !matches!(age, Some(a) if a <= bough_core::damage::RING as u32);
        let bbox;
        if full {
            self.rects.clear();
            self.rects.push([0, 0, w, h]);
            bbox = NRect::new(0, 0, w, h);
        } else {
            let rt = &mut self.rt;
            rt.bough.repair_into(age.unwrap(), &mut rt.plane);
            rects_from_plane(&rt.plane, w, h, MAX_RECTS, &mut self.rect_scratch, &mut self.rects);
            // cull against the bounding box of the *tile-rounded* rects (the stencil repaints whole
            // tiles, so records whose ink lies in a repainted tile fringe must be drawn too)
            let mut u = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
            for r in &self.rects {
                u = [u[0].min(r[0]), u[1].min(r[1]), u[2].max(r[2]), u[3].max(r[3])];
            }
            bbox = NRect::new(u[0], u[1], u[2], u[3]);
        }
        self.repaired_px = self.rects.iter().map(|r| ((r[2] - r[0]) as u64) * ((r[3] - r[1]) as u64)).sum();
        self.prof.mark(PH_DAMAGE);
        let list_len = self.build_list(bbox);
        if self.rects.len() > 1 {
            let rp = self.rt.rects_ptr();
            for (k, r) in self.rects.iter().enumerate() {
                unsafe { *rp.add(k) = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32] };
            }
        }
        self.prof.mark(PH_CULL);
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut() {
            g.sync_atlas(&mut self.rt.text.cache.atlas);
            let tex = g.ring[idx].tex.clone();
            g.encode(&tex, full, &self.rects, list_len, Some(idx));
            g.last_idx = idx;
        }
        let _ = list_len;
        self.prof.mark(PH_PRESENT);
        self.rt.bough.next_epoch();
        self.ages[idx] = Some(epoch);
        self.last_surface = idx;
        self.frames += 1;
        true
    }

    /// Cull the bough against `bbox` and expand the surviving records' instance runs, in paint
    /// order, into the pool's draw list.
    fn build_list(&mut self, bbox: NRect) -> usize {
        let rt = &mut self.rt;
        let mut out = std::mem::take(&mut rt.cull_out);
        rt.bough.cull(bbox, None, &mut out);
        let list = rt.list_ptr();
        let mut k = 0usize;
        for &p in &out {
            let r = rt.runs[p as usize];
            for i in 0..r.len {
                unsafe { *list.add(k) = r.first + i };
                k += 1;
            }
        }
        rt.cull_out = out;
        k
    }

    pub fn debug_rects(&self) -> &[[i32; 4]] {
        &self.rects
    }

    /// Draw list of every record (a full redraw), for the reference render.
    pub fn full_list(&mut self) -> usize {
        let (w, h) = (self.rt.win_w, self.rt.win_h);
        self.build_list(NRect::new(0, 0, w, h))
    }

    /// Pixels (BGRA) of the surface the last frame drew into.
    #[cfg(target_os = "macos")]
    pub fn presented_pixels(&mut self) -> Vec<u8> {
        let i = self.last_surface;
        self.gpu.as_mut().unwrap().surface_pixels(i)
    }

    /// A from-scratch full redraw of the current retained state (all records, one unscissored pass).
    #[cfg(target_os = "macos")]
    pub fn reference_pixels(&mut self) -> Vec<u8> {
        let n = self.full_list();
        let g = self.gpu.as_mut().unwrap();
        g.sync_atlas(&mut self.rt.text.cache.atlas);
        g.reference_pixels(n)
    }
}
