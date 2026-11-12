//! The region's capacity is address space (lane memory, round 6, card `2026-11-11-region-views`): a mount past
//! what `Caps` used to allow (APP_CAPS: 65,536 instances; DEFAULT: 8,192) works, and the GPU sees every column
//! through views that grew with it.
//!
//! The window first draws 1,000 quads, then a turn raises it to 70,000: the columns' views grow in a live window.
//! The oracle is independent of the views: the quads lie on a 4 px grid, each a distinct colour, and the pixel
//! at the centre of each quad must be the colour the CPU gave that quad. A view that did not grow masks a quad's
//! instance index into a smaller capacity and draws another quad's colour or position, so the negative control
//! (`--features sabotage_views`: views never grow past their first size) fails this test.
use bough::raw::*;

const N: usize = 70_000;
const COLS: usize = 300;
const PITCH: f32 = 4.0;

/// Distinct colours: an odd multiplier is a bijection on 24 bits.
fn colour(i: usize) -> u32 {
    ((i as u32).wrapping_mul(2_654_435_761) >> 4) & 0xff_ffff
}

/// `n` quads: a plain field, so a turn that raises it re-describes the view and mounts the new quads into the
/// live window (the columns' views grow between frames, through the pre-armed stream's drain).
struct Grid {
    n: usize,
}

impl View for Grid {
    fn view(&self) -> impl Element<Self> + '_ {
        each((0..self.n).map(|i| quad((i % COLS) as f32 * PITCH, (i / COLS) as f32 * PITCH, 3.0, 3.0).bg(rgb(colour(i)))))
    }
}

#[test]
fn seventy_thousand_quads_past_the_old_caps_draw_exactly() {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let rows = N.div_ceil(COLS);
    let (w, h) = (COLS as f32 * PITCH, rows as f32 * PITCH);
    // the configured capacities are the old defaults: the region reserves the format's limits anyway
    let mut app = App::new(Grid { n: 1000 }, Config::new(w, h, 1.0, font), true);
    app.frame();
    app.turn(|g| g.n = N);
    app.frame();
    let px = app.presented_pixels();
    let stride = w as usize * 4;
    let mut wrong = 0;
    for i in 0..N {
        let (x, y) = ((i % COLS) * PITCH as usize + 1, (i / COLS) * PITCH as usize + 1);
        let o = y * stride + x * 4;
        let c = colour(i);
        let want = [(c & 0xff) as u8, (c >> 8 & 0xff) as u8, (c >> 16) as u8, 255]; // BGRA
        if px[o..o + 4] != want {
            if wrong < 5 {
                eprintln!("quad {i}: pixel {:?}, want {want:?}", &px[o..o + 4]);
            }
            wrong += 1;
        }
    }
    assert_eq!(wrong, 0, "{wrong} of {N} quads drawn wrong");
    let r = app.reference_pixels();
    let diff = px.chunks_exact(4).zip(r.chunks_exact(4)).enumerate().filter(|(_, (a, b))| a != b).map(|(i, (a, b))| (i % w as usize, i / w as usize, a.to_vec(), b.to_vec())).collect::<Vec<_>>();
    assert!(diff.is_empty(), "the presented frame equals a full redraw: {} pixels differ, first {:?} (repair {:?}, {:?})", diff.len(), &diff[..diff.len().min(4)], app.repair, app.surf_stats);
}
