//! The pixel gate shared by the scene tests (as `comparisons/bough-scenes/tests/correctness.rs`): after every
//! `every`-th drawn frame the presented surface (read back from the IOSurface the ring drew into) equals a
//! from-scratch full redraw of the same retained state through the same kernels, for every ring age 1..=4
//! (a round-robin ring of `ring` surfaces repairs at age `ring`) and the windowed content-addressed ring;
//! at the end it equals a fresh mount at the same signal values.
#![allow(dead_code)]
use bough::prelude::*;

pub fn first_diff(a: &[u8], b: &[u8], w: usize) -> Option<(usize, usize, u8, u8)> {
    a.iter().zip(b).position(|(x, y)| x != y).map(|i| ((i / 4) % w, (i / 4) / w, a[i], b[i]))
}

/// Ring 0 is the windowed ring (4 surfaces, content-addressed); 1..=4 are round-robin.
pub const RINGS: [usize; 5] = [1, 2, 3, 4, 0];

pub fn app_ring<V: View>(v: V, mut c: Config, ring: usize) -> App<V> {
    c.ring = if ring == 0 { 4 } else { ring };
    let mut a = App::new(v, c, true);
    a.windowed = ring == 0;
    a
}

/// Run `steps` (each queues its writes; `step(app, t)` for t in 1..=steps), comparing every `every`-th drawn
/// frame; `fresh` mounts the view at the state the steps reached. Returns the retained app's last pixels.
pub fn check<V: View>(name: &str, mut app: App<V>, steps: usize, every: usize, mut step: impl FnMut(&mut App<V>, usize), fresh: impl Fn() -> App<V>) -> Vec<u8> {
    let w = app.rt.win_w as usize;
    app.frame();
    let name = format!("{name} ({})", if app.windowed { "windowed" } else { "rr" });
    let mut checked = 0;
    for t in 1..=steps {
        step(&mut app, t);
        let drew = app.frame();
        assert!(!app.bins_overflowed(), "{name}: row bin overflow at step {t}");
        if drew && t % every == 0 {
            let px = app.presented_pixels();
            let reference = app.reference_pixels();
            assert!(px == reference, "{name} step {t}: presented != full redraw at {:?}", first_diff(&px, &reference, w));
            checked += 1;
        }
    }
    assert!(checked > 0, "{name}: nothing was compared");
    let px = app.presented_pixels();
    let mut f = fresh();
    f.frame();
    let fp = f.presented_pixels();
    assert!(px == fp, "{name}: retained after {steps} steps != fresh mount, first diff {:?}", first_diff(&px, &fp, w));
    px
}

/// `c` with the ring a test runs at (0: the windowed ring's 4 surfaces).
pub fn with_ring(mut c: Config, ring: usize) -> Config {
    c.ring = if ring == 0 { 4 } else { ring };
    c
}
