//! An empty App's construction cost (round 6, lane `store`): `bough_mem::Region::heap` used to be the
//! allocator's zeroed block, cleared whole on every `App::new` (12 M of an empty App's 13.3 M instructions,
//! and resident from the start). It is a mapping now, and the façade's own tables are columns carved from
//! another: an empty App is ~1.3 M instructions. The bound keeps an eager clear (or any per-capacity work at
//! construction) from coming back unnoticed.
use bough::prelude::*;
use bough::raw::{App, Caps, Config};
use bough_metal::perf::tinstr;

struct Empty;
impl View for Empty {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
    }
}

#[test]
fn an_empty_app_stays_under_two_million_instructions() {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let one = || {
        let mut cfg = Config::new(1280.0, 800.0, 1.0, font.clone());
        // an application's capacities: half a million records, a million glyph slots
        cfg.caps = Caps { instances: 1 << 17, pages: 1 << 13, glyphs: 1 << 20, runs: 1 << 17, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
        let t0 = tinstr();
        let mut a = App::new(Empty, cfg, false);
        a.turn(|_| {});
        let d = tinstr() - t0;
        drop(a);
        d
    };
    one(); // the process's first App builds the font's tables
    let n = one();
    assert!(n < 2_000_000, "an empty App took {n} instructions (bound 2,000,000; ~1.3 M when this was written)");
}
