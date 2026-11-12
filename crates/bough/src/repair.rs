//! The repair backends' choice and the CPU backend's self-check (lane memory, round 6, card
//! `2026-11-11-repair-backends`). The CPU backend itself is `cpu_raster.rs`. The self-check compares it with a
//! GPU executor, so it exists where one does (macOS); elsewhere the CPU executor is the only one.
/// The CPU backend is exact only for the GPU and shader compiler it was established on (the kernel's arithmetic is
/// written out, but the store's rounding and the texture unit's conversions are the hardware's): before its first
/// use on a machine, a fixed probe scene of every modelled kind is drawn by both backends and compared byte for
/// byte. The verdict is cached per CPU model, OS build and shader source (a file in `$TMPDIR`, or
/// `BOUGH_CHECK_DIR`), so later processes never submit GPU work to decide. Any mismatch disables the CPU backend.
#[cfg(target_os = "macos")]
pub fn cpu_exact(font: &'static [u8]) -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let path = check_path();
        if let Ok(s) = std::fs::read_to_string(&path) {
            match s.trim() {
                "exact" => return true,
                "mismatch" => return false,
                _ => {}
            }
        }
        let v = probe(font);
        let _ = std::fs::write(&path, if v { "exact\n" } else { "mismatch\n" });
        v
    })
}

#[cfg(target_os = "macos")]
fn check_path() -> std::path::PathBuf {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let key = format!("{}|{}|{}", bough_sys::machine_key(), MODEL_VERSION, bough_metal::gpu::SHADER_SRC);
    for b in key.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let dir = std::env::var_os("BOUGH_CHECK_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join(format!("bough-cpu-repair-{h:016x}"))
}

/// Bumped whenever the model's arithmetic changes (invalidates every cached verdict).
#[cfg(target_os = "macos")]
const MODEL_VERSION: u32 = 1;

/// The probe: translucent solid boxes at quarter-pixel positions, rounded boxes and borders of several radii and
/// widths, and translucent text, overlapping; drawn by the CPU backend and compared with the GPU's full redraw.
#[cfg(target_os = "macos")]
fn probe(font: &'static [u8]) -> bool {
    use crate::app::{App, Config, View};
    use crate::raw::*;
    struct Probe;
    impl View for Probe {
        fn view(&self) -> impl Element<Self> + '_ {
            (
                quad(0.0, 0.0, 96.0, 64.0).bg(rgb(0xf0f0ec)),
                each((0..24usize).map(|i| {
                    let (x, y) = ((i % 6) as f32 * 15.25 + 0.25 * (i % 4) as f32, (i / 6) as f32 * 15.0 + 1.5);
                    let c = Color(0x40ff_ffff & (0x9e37_79b9u32.wrapping_mul(i as u32 + 1)) | (0x30 + 9 * i as u32) << 24);
                    match i % 4 {
                        0 => quad(x, y, 21.0, 13.0).bg(c).rounded(0.0).border(0.0, c),
                        1 => quad(x, y, 21.0, 13.0).bg(c).rounded(1.5 + i as f32 * 0.5).border(0.0, c),
                        2 => quad(x, y, 21.0, 13.0).bg(c).rounded(i as f32 * 0.25).border(0.5 + (i % 3) as f32, Color(c.0 ^ 0x00ff_00ff)),
                        _ => quad(x, y, 21.0, 13.0).bg(c).rounded(0.0).border(1.0, Color(c.0 ^ 0x0000_ffff)),
                    }
                })),
                text("Wg probe 0123 &%").at(3.0, 20.0).size(13.0).line_height(16.0).color(Color(0xb0_80_30_c0)),
                text("fx").at(60.25, 44.0).size(17.0).line_height(20.0).color(Color(0x60_00_a0_30)),
            )
        }
    }
    let mut app = App::new(Probe, Config::new(96.0, 64.0, 2.0, font.to_vec()).once(), true);
    app.repair = crate::app::Repair::Cpu;
    app.frame();
    app.surf_stats.cpu == 1 && app.presented_pixels() == app.reference_pixels()
}
