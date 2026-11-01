//! Rasterising strikes on several cores (round 3b, input agent: the cold first frame).
//!
//! A document known at mount time names its font sizes; the label path rasterises the 95 printable ASCII
//! tokens of every strike it meets, one glyph at a time on the mounting thread (1,150 glyphs, 3.6 ms for the
//! zgui gallery). Rasterisation is independent per glyph, so the glyphs of every requested strike are split
//! over scoped threads, each with its own scaler; the bitmaps are then placed in the atlas and entered in
//! one pass on the calling thread (allocation order: strike, then glyph id, so the atlas layout does not
//! depend on the thread count). Later lookups find the entries ready.
use super::*;

/// One rasterised glyph: (strike, glyph id, placement left, top, width, height, alpha bitmap).
type Raster = (StrikeId, u16, i32, i32, u32, u32, Vec<u8>);

impl GlyphCache {
    /// Rasterise glyphs `gids` at every strike of `strikes` that does not hold them yet, on up to `threads`
    /// threads, and enter them (as `entry` would one by one).
    pub fn prepare_parallel(&mut self, strikes: &[StrikeId], gids: &[u16], threads: usize) {
        let mut jobs: Vec<(StrikeId, u16, f32)> = Vec::new();
        for &s in strikes {
            let size = self.strikes[s].size;
            for &g in gids {
                let ready = self.strikes[s].entries.get(g as usize).is_some_and(|e| e.state != 0);
                if !ready && !jobs.iter().any(|j| j.0 == s && j.1 == g) {
                    jobs.push((s, g, size));
                }
            }
        }
        if jobs.is_empty() {
            return;
        }
        let t0 = std::time::Instant::now();
        let threads = threads.clamp(1, 16).min(jobs.len());
        let font: &[u8] = &self.font;
        let chunk = jobs.len().div_ceil(threads);
        let mut out: Vec<Raster> = Vec::with_capacity(jobs.len());
        std::thread::scope(|sc| {
            let handles: Vec<_> = jobs
                .chunks(chunk)
                .map(|part| {
                    sc.spawn(move || {
                        let mut ctx = ScaleContext::new();
                        let fr = FontRef::from_index(font, 0).expect("font");
                        let mut res = Vec::with_capacity(part.len());
                        let mut size = f32::NAN;
                        let mut scaler = None;
                        for &(s, g, sz) in part {
                            if sz != size || scaler.is_none() {
                                size = sz;
                                scaler = Some(ctx.builder(fr).size(sz).hint(false).build());
                            }
                            let sc = scaler.as_mut().unwrap();
                            let img = Render::new(&[Source::Outline]).format(Format::Alpha).render(sc, g);
                            match img {
                                Some(img) if img.placement.width > 0 && img.placement.height > 0 && img.content == Content::Mask => {
                                    res.push((s, g, img.placement.left, img.placement.top, img.placement.width, img.placement.height, img.data));
                                }
                                _ => res.push((s, g, 0, 0, 0, 0, Vec::new())),
                            }
                        }
                        res
                    })
                })
                .collect();
            for h in handles {
                out.extend(h.join().expect("raster thread"));
            }
        });
        out.sort_by_key(|r| (r.0, r.1));
        for (s, g, left, top, w, h, data) in out {
            let mut e = Entry { state: 1, ..Default::default() };
            if w > 0 {
                let (x, y) = self.atlas.alloc(w, h);
                self.atlas.blit(x, y, w, h, &data);
                e = Entry { geo: [left as f32, -(top as f32), w as f32, h as f32], uv: [x as u16, y as u16, w as u16, h as u16], state: 1, _pad: 0 };
            }
            let st = &mut self.strikes[s];
            if st.entries.len() <= g as usize {
                st.entries.resize(g as usize + 1, Entry::default());
            }
            st.entries[g as usize] = e;
            self.rasterized += 1;
        }
        self.raster_ns += t0.elapsed().as_nanos() as u64;
    }
}

// ---- Rasterising from process entry -------------------------------------------------------------------
//
// An application that knows its strikes before it mounts (the sizes are in its document) starts their
// rasterisation at process entry on worker threads, while its main thread builds the runtime, mounts,
// shapes and lays out. A strike created later takes its glyphs from here: done (entered with no
// rasterising), not started (the main thread rasterises that strike itself, so it never waits on an idle
// queue), or in progress (it waits for that one strike). Each strike's glyphs enter the atlas in glyph-id
// order when the strike is created, so the atlas layout does not depend on the thread count or timing.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// One rasterised glyph of a known strike: (glyph id, left, top, width, height, alpha bitmap).
type Glyph = (u16, i32, i32, u32, u32, Vec<u8>);

struct Pre {
    font: Vec<u8>,
    sizes: Vec<f32>,
    /// Per size: the glyphs to rasterise (sorted).
    gids: Vec<Vec<u16>>,
    /// Per size: 0 free, 1 claimed, 2 done.
    state: Vec<AtomicU8>,
    done: Mutex<Vec<Option<Vec<Glyph>>>>,
    cv: Condvar,
    /// Per worker: (strikes, glyphs, instructions retired, ns busy).
    stats: Mutex<Vec<[u64; 4]>>,
}

static PRE: Mutex<Option<Arc<Pre>>> = Mutex::new(None);
/// The absorbing thread: strikes it rasterised itself, strikes it waited for, ns waited.
static MAIN: Mutex<[u64; 3]> = Mutex::new([0; 3]);

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn thread_selfcounts(kind: i32, buffer: *mut u64, size: usize) -> i32;
}

/// The calling thread's retired instructions (0 where the counter is not available).
fn thread_instr() -> u64 {
    #[cfg(target_os = "macos")]
    {
        let mut c = [0u64; 2];
        // SAFETY: kind 1 fills two u64s (instructions, cycles); `c` has 16 bytes.
        unsafe { thread_selfcounts(1, c.as_mut_ptr(), 16) };
        c[0]
    }
    #[cfg(not(target_os = "macos"))]
    0
}

fn render_strike(ctx: &mut ScaleContext, font: &[u8], size: f32, gids: &[u16]) -> Vec<Glyph> {
    let fr = FontRef::from_index(font, 0).expect("font");
    let mut sc = ctx.builder(fr).size(size).hint(false).build();
    gids.iter()
        .map(|&g| match Render::new(&[Source::Outline]).format(Format::Alpha).render(&mut sc, g) {
            Some(img) if img.placement.width > 0 && img.placement.height > 0 && img.content == Content::Mask => {
                (g, img.placement.left, img.placement.top, img.placement.width, img.placement.height, img.data)
            }
            _ => (g, 0, 0, 0, 0, Vec::new()),
        })
        .collect()
}

impl Pre {
    fn claim(&self, i: usize) -> bool {
        self.state[i].compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }
    fn finish(&self, i: usize, g: Vec<Glyph>) {
        self.done.lock().unwrap()[i] = Some(g);
        self.state[i].store(2, Ordering::Release);
        self.cv.notify_all();
    }
    fn wait(&self, i: usize) -> Vec<Glyph> {
        let mut d = self.done.lock().unwrap();
        loop {
            if let Some(g) = d[i].take() {
                return g;
            }
            d = self.cv.wait(d).unwrap();
        }
    }
}

/// Start rasterising, for each `(pixel size, text)` of `strikes`, the glyphs of `font` that the text's
/// characters map to, on `threads` worker threads (call at process entry; strikes in the order the mount
/// meets them; a size given twice merges its texts). Strikes the next [`GlyphCache`] of the same font
/// creates at these sizes take their glyphs from here; glyphs not named (shaped ligatures, text the
/// document makes later) are rasterised when met, as before.
pub fn prerender_in_background(font: &[u8], strikes: &[(f32, &str)], threads: usize) {
    let face = ttf_parser::Face::parse(font, 0).expect("font");
    let mut sz: Vec<f32> = Vec::new();
    let mut gids: Vec<Vec<u16>> = Vec::new();
    for &(s, text) in strikes {
        let i = sz.iter().position(|&x| x == s).unwrap_or_else(|| {
            sz.push(s);
            gids.push(Vec::new());
            sz.len() - 1
        });
        gids[i].extend(text.chars().filter(|c| !c.is_control()).filter_map(|c| face.glyph_index(c)).map(|g| g.0));
    }
    for g in &mut gids {
        g.sort_unstable();
        g.dedup();
    }
    let n = sz.len();
    let pre = Arc::new(Pre {
        font: font.to_vec(),
        sizes: sz,
        gids,
        state: (0..n).map(|_| AtomicU8::new(0)).collect(),
        done: Mutex::new(vec![None; n]),
        cv: Condvar::new(),
        stats: Mutex::new(Vec::new()),
    });
    *PRE.lock().unwrap() = Some(pre.clone());
    for w in 0..threads.clamp(1, 16).min(n) {
        let p = pre.clone();
        std::thread::Builder::new()
            .name(format!("bough-raster-{w}"))
            .spawn(move || {
                let (i0, t0) = (thread_instr(), std::time::Instant::now());
                let mut ctx = ScaleContext::new();
                let (mut strikes, mut glyphs) = (0u64, 0u64);
                for i in 0..p.sizes.len() {
                    if p.claim(i) {
                        let g = render_strike(&mut ctx, &p.font, p.sizes[i], &p.gids[i]);
                        glyphs += g.len() as u64;
                        strikes += 1;
                        p.finish(i, g);
                    }
                }
                p.stats.lock().unwrap().push([strikes, glyphs, thread_instr() - i0, t0.elapsed().as_nanos() as u64]);
            })
            .expect("raster thread");
    }
}

/// Per worker of the last [`prerender_in_background`]: (strikes, glyphs, instructions, ns), for the
/// workers that have finished.
pub fn prerender_stats() -> Vec<[u64; 4]> {
    PRE.lock().unwrap().as_ref().map(|p| p.stats.lock().unwrap().clone()).unwrap_or_default()
}

/// The absorbing (main) thread's share: strikes it rasterised itself, strikes it waited for, ns waited.
pub fn prerender_main() -> [u64; 3] {
    *MAIN.lock().unwrap()
}

impl GlyphCache {
    /// Enter the glyphs [`prerender_in_background`] made for this strike's size (rasterising them here if
    /// no worker has started them). Returns whether the strike was known there.
    pub fn absorb_prerendered(&mut self, strike: StrikeId) -> bool {
        let Some(pre) = PRE.lock().unwrap().clone() else { return false };
        let size = self.strikes[strike].size;
        let Some(i) = pre.sizes.iter().position(|&s| s == size) else { return false };
        let head = self.font.len().min(4096);
        if pre.font.len() != self.font.len() || pre.font[..head] != self.font[..head] || pre.state[i].load(Ordering::Acquire) == 3 {
            return false;
        }
        let t0 = std::time::Instant::now();
        let glyphs = if pre.claim(i) {
            MAIN.lock().unwrap()[0] += 1;
            render_strike(&mut self.ctx, &pre.font, size, &pre.gids[i])
        } else {
            let w = std::time::Instant::now();
            let ready = pre.state[i].load(Ordering::Acquire) == 2;
            let g = pre.wait(i);
            if !ready {
                let mut m = MAIN.lock().unwrap();
                m[1] += 1;
                m[2] += w.elapsed().as_nanos() as u64;
            }
            g
        };
        // Taken: a later cache rasterises this size itself.
        pre.state[i].store(3, Ordering::Release);
        for (g, left, top, w, h, data) in glyphs {
            if self.strikes[strike].entries.get(g as usize).is_some_and(|e| e.state != 0) {
                continue;
            }
            let mut e = Entry { state: 1, ..Default::default() };
            if w > 0 {
                let (x, y) = self.atlas.alloc(w, h);
                self.atlas.blit(x, y, w, h, &data);
                e = Entry { geo: [left as f32, -(top as f32), w as f32, h as f32], uv: [x as u16, y as u16, w as u16, h as u16], state: 1, _pad: 0 };
            }
            let st = &mut self.strikes[strike];
            if st.entries.len() <= g as usize {
                st.entries.resize(g as usize + 1, Entry::default());
            }
            st.entries[g as usize] = e;
        }
        self.raster_ns += t0.elapsed().as_nanos() as u64;
        true
    }
}
