//! GPU bounds safety by construction (bough-metal RESULTS Part IV): every index the fused kernels read from
//! region data is ANDed with its column's power-of-two capacity mask and every loop bound is min-ed with a
//! capacity, so a corrupted Bough (random payloads, page ids, clip ids, run headers, glyph entries, spaces)
//! renders garbage but never makes the GPU read outside its columns: the frame completes with no GPU
//! address fault, and in bounded time (a corrupted run length cannot make a kernel loop). Out-of-column
//! reads inside the VA space do not fault on this GPU (measured: opening the index masks renders garbage
//! without an error, and Metal's shader validation did not flag it), so the masks are the guarantee by
//! construction; what a test can observe is the loop bound. The sabotage test (ignored by default: it keeps
//! the GPU busy for 5 s; run it alone: `cargo test -p bough-heavy --test gpu_bounds -- --ignored`) opens the
//! glyph mask and the frame must not finish.
use bough::raw::*;
use bough_heavy::*;
use zgx_workload::heavy::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Byte offset of the mask block in the kernels' header (`FHdr`: 2 f32 + 16 u32 before it).
const MASKS_OFF: usize = 72;

fn corrupt_and_draw(open_masks: bool) {
    let work = Heavy::from_env();
    let mut cfg = config(2.0);
    cfg.ring = 2;
    let mut app = App::new(Dash::new(&work), cfg, true);
    app.frame();
    app.presented_pixels(); // the GPU is idle: the region may be written
    let l = app.rt.layout;
    let base = app.rt.region_base();
    // the directory is blocks of 64 entries, live pages first and holes after (EMPTY ink, page id 0): walk
    // every entry the kernels scan and skip the holes
    let dir = app.rt.bough.dir_len();
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    // SAFETY: every write below lands inside a column carved for it (offsets and capacities from the
    // layout); the GPU is idle (synced above).
    unsafe {
        for d in 0..dir {
            if *(base.add(l.dir_ink_off as usize + 8 * d) as *const i16) == i16::MAX {
                continue; // a hole (NRect::EMPTY)
            }
            let id = *(base.add(l.dir_id_off as usize + 4 * d) as *const u32) as usize;
            let pg = base.add(l.pages_off as usize + id * 1472);
            for s in 0..64 {
                *(pg.add(768 + 4 * s) as *mut u32) = rng.next() as u32; // payload: any instance / run / irun
                *(pg.add(1280 + 2 * s) as *mut u16) = rng.next() as u16; // clip id
            }
            if d % 3 == 0 {
                *(base.add(l.dir_id_off as usize + 4 * d) as *mut u32) = rng.next() as u32; // page id
            }
        }
        for r in 0..256 {
            let rh = base.add(l.runs_off as usize + 32 * r);
            *(rh.add(16) as *mut u16) = rng.next() as u16; // space
            *(rh.add(18) as *mut u16) = rng.next() as u16; // strike
            *(rh.add(20) as *mut u32) = rng.next() as u32; // base
            *(rh.add(24) as *mut u32) = rng.next() as u32 | 0x0100_0000; // len: huge
        }
        for g in 0..4096 {
            *(base.add(l.gent_off as usize + 2 * g) as *mut u16) = rng.next() as u16;
        }
        for i in 0..2048 {
            *(base.add(l.inst_off as usize + 48 * i + 40) as *mut u16) = rng.next() as u16; // space
        }
        // the decoration kinds (input 3b: image, gradient, shadow; bough deco.rs): random kind, texel origin
        // and size, margin, angle / sigma / ramp words, box (texel reads are clamped to the image atlas, or
        // to the glyph atlas standing in for it; the shadow's loop is fixed)
        for i in (0..2048).step_by(3) {
            let ins = base.add(l.inst_off as usize + 48 * i);
            *(ins.add(42) as *mut u8) = 4 + (rng.next() % 3) as u8; // kind
            *(ins.add(43) as *mut u8) = rng.next() as u8; // ink margin
            *(ins.add(32) as *mut u64) = rng.next(); // uv: texel origin / ramp row / gradient type
            *(ins.add(24) as *mut u32) = rng.next() as u32; // radius / angle (any bits: NaN, inf)
            *(ins.add(28) as *mut u32) = rng.next() as u32; // bw: sigma / gradient angle
            *(ins.add(16) as *mut u64) = rng.next(); // colours / radial centre
            *(ins.add(8) as *mut f32) = (rng.next() % 4096) as f32; // size
            *(ins.add(12) as *mut f32) = (rng.next() % 4096) as f32;
        }
        if open_masks {
            // open the glyph mask: it also bounds the run loops, so the corrupted lengths (> 2^24 glyphs per
            // run) now loop for seconds and the frame cannot finish
            *(base.add(MASKS_OFF + 4 * 2) as *mut u32) = 0x0FFF_FFFF;
        }
    }
    // everything damaged: every tile row is binned over the corrupted records
    let win = app.rt.bough.window();
    app.rt.bough.damage_rect(win);
    assert!(app.frame());
    // a GPU address fault aborts the frame's command buffer; its event never signals and `sync` panics
    // with the command buffer's error after 5 s
    app.presented_pixels();
}

#[test]
fn corrupted_region_renders_without_reading_outside_its_columns() {
    corrupt_and_draw(false);
}

#[test]
#[ignore]
#[should_panic(expected = "did not finish")]
fn sabotage_open_loop_bound_never_finishes() {
    corrupt_and_draw(true);
}
