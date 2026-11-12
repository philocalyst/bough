//! zgx's Sketch and Code example apps (`crates/zgx-script/examples/sketch`, `.../code_editor`, Luau) as native
//! bough views, driven by zgx's `apps_bench` interactions (`examples/support/apps.rs`: `sketch`, `code`).
//!
//! * [`sketch`]: a board of shapes you drag, pan and zoom, with a toolbar and an inspector. The board's
//!   drawing, as zgx's `ui.canvas`, is an instance run the app writes ([`canvas`]); the pan is one space
//!   write; a zoom is one pass over the shape column.
//! * [`code`]: a code editor over a 100,000-line file: a gap buffer with a line index ([`code::doc`]), zgx's
//!   Rust tokenizer ([`code::highlight`]), the lines in view as one multi-line text per colour (a mono font
//!   puts a span at its column, so colour is a projection of the line, not a run per span), find-all with
//!   the searches kept and refined ([`code::search`]).
//!
//! What is not on bough's public API is built here on top of it and listed as gaps in the port's report (canvas runs,
//! a wheel and pinch route, a text field change route).
#![feature(portable_simd)]
pub mod canvas;
pub mod code;
pub mod harness;
pub mod sketch;
pub mod theme;

/// zgx's UI font (DejaVu Sans: `ZGX_FONT_FILE`, else the workload's bundled copy).
pub fn font_sans() -> Vec<u8> {
    zgx_workload::font_file().unwrap_or_else(|| zgx_workload::FONT_BYTES.to_vec())
}

/// The code editor's font: DejaVu Sans Mono (`BOUGH_MONO_FONT`, else the system's DejaVu 2.37).
pub fn font_mono() -> Vec<u8> {
    let path = std::env::var("BOUGH_MONO_FONT")
        .unwrap_or_else(|_| "/Library/Fonts/Nix Fonts/vl9mbzk97wmh1s1yj3766b4zai8msjq6-dejavu-fonts-2.37/share/fonts/truetype/DejaVuSansMono.ttf".into());
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e} (set BOUGH_MONO_FONT to DejaVuSansMono.ttf)"))
}

/// A monospace font's advance and units per em (`hmtx`'s common advance, `head`'s upem).
pub fn mono_metrics(font: &[u8]) -> (u32, u32) {
    let be16 = |o: usize| u16::from_be_bytes([font[o], font[o + 1]]) as u32;
    let be32 = |o: usize| u32::from_be_bytes([font[o], font[o + 1], font[o + 2], font[o + 3]]) as usize;
    let n = be16(4) as usize;
    let table = |tag: &[u8; 4]| (0..n).map(|i| 12 + 16 * i).find(|&r| &font[r..r + 4] == tag).map(|r| be32(r + 8)).expect("font table");
    let (head, hhea, hmtx) = (table(b"head"), table(b"hhea"), table(b"hmtx"));
    let upem = be16(head + 18);
    let metrics = be16(hhea + 34) as usize;
    // the advance most glyphs share (a monospace font: all but a few)
    let mut counts: Vec<(u32, u32)> = Vec::new();
    for g in 0..metrics.min(200) {
        let a = be16(hmtx + 4 * g);
        match counts.iter_mut().find(|c| c.0 == a) {
            Some(c) => c.1 += 1,
            None => counts.push((a, 1)),
        }
    }
    let adv = counts.iter().max_by_key(|c| c.1).map(|c| c.0).unwrap_or(0);
    (adv, upem)
}

/// Instructions retired by this process so far (`proc_pid_rusage` `ri_instructions`).
#[inline]
pub fn instructions() -> u64 {
    bough::sys::perf::snap().instrs
}

/// A size from the environment, else `default`.
pub fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
