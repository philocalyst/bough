//! Rasterising a document's strikes on other cores from process entry (round 3b, input agent: the cold
//! first frame overlapped across cores).
use super::*;

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
    fonts: Vec<Vec<u8>>,
    /// Per strike: pixel size and font index.
    sizes: Vec<f32>,
    font: Vec<usize>,
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
pub fn prerender_in_background(fonts: &[&[u8]], strikes: &[(f32, &str, usize)], threads: usize) {
    let faces: Vec<ttf_parser::Face> = fonts.iter().map(|f| ttf_parser::Face::parse(f, 0).expect("font")).collect();
    let mut sz: Vec<f32> = Vec::new();
    let mut fi: Vec<usize> = Vec::new();
    let mut gids: Vec<Vec<u16>> = Vec::new();
    for &(s, text, f) in strikes {
        let i = (0..sz.len()).find(|&i| sz[i] == s && fi[i] == f).unwrap_or_else(|| {
            sz.push(s);
            fi.push(f);
            gids.push(Vec::new());
            sz.len() - 1
        });
        gids[i].extend(text.chars().filter(|c| !c.is_control()).filter_map(|c| faces[f].glyph_index(c)).map(|g| g.0));
    }
    for g in &mut gids {
        g.sort_unstable();
        g.dedup();
    }
    let n = sz.len();
    let pre = Arc::new(Pre {
        fonts: fonts.iter().map(|f| f.to_vec()).collect(),
        sizes: sz,
        font: fi,
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
                        let g = render_strike(&mut ctx, &p.fonts[p.font[i]], p.sizes[i], &p.gids[i]);
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
        let font = self.face_font(self.strikes[strike].face);
        let head = font.len().min(4096);
        let same = |f: &Vec<u8>| f.len() == font.len() && f[..head] == font[..head];
        let Some(i) = (0..pre.sizes.len()).find(|&i| pre.sizes[i] == size && same(&pre.fonts[pre.font[i]])) else { return false };
        if pre.state[i].load(Ordering::Acquire) == 3 {
            return false;
        }
        let t0 = std::time::Instant::now();
        let glyphs = if pre.claim(i) {
            MAIN.lock().unwrap()[0] += 1;
            render_strike(&mut self.ctx, &pre.fonts[pre.font[i]], size, &pre.gids[i])
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
