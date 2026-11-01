//! The overlapped cold start (`bough::app::prepare`: Metal device, pipelines, window resources and the
//! document's glyphs built on other threads from process entry while the main thread mounts into heap
//! memory that the presenter then wraps without a copy) presents the same pixels as the serial one, and
//! both equal a from-scratch full redraw. Its own test binary: `prepare` is process-wide.
use bough::prelude::*;
use bough_zgui::gallery::{Doc, GalleryL};
use bough_zgui::*;

#[test]
fn overlapped_cold_start_presents_the_serial_pixels() {
    let cfg = config(WIDTH, HEIGHT);
    bough::app::prepare(&cfg, &GalleryL::strikes(), &GalleryL::svgs(), 3);
    let mut a = App::new(GalleryL::fresh(), cfg, true);
    let st = a.startup;
    assert!(st.gpu_prep_ns > 0, "the presenter came from the preparation thread: {st:?}");
    let workers: u64 = bough::app::raster_stats().iter().map(|r| r[1]).sum::<u64>();
    let here = bough::app::raster_main();
    assert!(a.frame());
    let px = a.presented_pixels();
    let rf = a.reference_pixels();
    assert!(px == rf, "overlapped: presented != full redraw");
    // the serial start (nothing prepared now: the device and glyphs are made on this thread)
    let mut b = App::new(GalleryL::fresh(), config(WIDTH, HEIGHT), true);
    assert_eq!(b.startup.gpu_prep_ns, 0);
    assert!(b.frame());
    let pb = b.presented_pixels();
    let first = px.iter().zip(&pb).position(|(x, y)| x != y).map(|i| ((i / 4) % WIDTH as usize, (i / 4) / WIDTH as usize));
    assert!(px == pb, "overlapped != serial at {first:?}");
    assert!(workers + here[0] > 0, "glyphs came from the prepared strikes ({workers} on workers, {here:?} here)");
    eprintln!("overlapped start: {st:?}; glyphs on workers {workers}, main {here:?}; main-thread misses {}", a.rt.text.cache.rasterized);
}
