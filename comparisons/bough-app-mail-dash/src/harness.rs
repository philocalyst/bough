//! zgx's `apps.rs` `Bench`, on bough: an app mounted at the window's size, a frame after each interaction
//! (`App::frame`: damage, tile list, and with `RENDER=1` the GPU's repair of the offscreen surface at twice
//! the points), and `measure`: two interactions to warm up, each followed by its frame, the GPU caught up
//! (zgx's `finish`: a readback), then `REPS` interactions each followed by its frame and one more `finish`,
//! timed and counted (`proc_pid_rusage` instructions, the whole process) and printed as zgx prints them.
use crate::instructions;
use bough::app::{PHASE_NAMES, View};
use bough::raw::*;
use std::time::Instant;

pub struct Bench<V: View> {
    pub app: App<V>,
    /// Drawn on the GPU (`RENDER=1`, zgx's `apps_bench` default) or not (`RENDER=0`: the frame ends at the
    /// tile list, zgx's `Shaped`).
    pub render: bool,
    pub reps: usize,
    /// `BOUGH_PROFILE=1`: instructions by phase (update / layout / damage / tile list / present).
    pub profile: bool,
}

/// `RENDER` (default 1) and the device scale the apps are drawn at (zgx: twice the points; `SCALE` overrides).
pub fn render_from_env() -> bool {
    std::env::var("RENDER").map_or(true, |v| v != "0")
}
pub fn scale_from_env() -> f32 {
    std::env::var("SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(2.0)
}

impl<V: View> Bench<V> {
    pub fn new(app: App<V>, render: bool) -> Bench<V> {
        let profile = std::env::var_os("BOUGH_PROFILE").is_some();
        let mut b = Bench { app, render, reps: crate::env_usize("REPS", 20), profile };
        if profile {
            b.app.prof.enable();
        }
        b.frames(3);
        b
    }
    /// A frame: what the interaction changed, its damage drawn (zgx `Bench::frame`).
    pub fn frame(&mut self) {
        self.app.frame();
    }
    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }
    /// zgx's `Drawing::finish`: the GPU caught up and the frame read back.
    pub fn finish(&mut self) {
        #[cfg(target_os = "macos")]
        if self.render {
            std::hint::black_box(self.app.presented_pixels());
        }
    }
    /// zgx's `Bench::measure` (`ONLY` / `SKIP` as zgx's).
    pub fn measure(&mut self, name: &str, mut interaction: impl FnMut(&mut Self, usize)) {
        if std::env::var("ONLY").is_ok_and(|only| !name.contains(only.as_str())) || std::env::var("SKIP").is_ok_and(|skip| name.contains(skip.as_str())) {
            return;
        }
        for k in 0..2 {
            interaction(self, k);
            self.frame();
        }
        self.finish();
        self.app.prof.acc = [0; 5];
        let (start, before) = (Instant::now(), instructions());
        for k in 0..self.reps {
            interaction(self, k + 2);
            self.frame();
        }
        self.finish();
        let reps = self.reps as f64;
        let ms = start.elapsed().as_secs_f64() * 1e3 / reps;
        let millions = (instructions() - before) as f64 / 1e6 / reps;
        let mut line = format!("{name}: {ms:.2} ms, {millions:.2} M instructions");
        if self.profile {
            for (i, n) in PHASE_NAMES.iter().enumerate() {
                line += &format!(" | {n} {:.3}", self.app.prof.acc[i] as f64 / 1e6 / reps);
            }
        }
        println!("{line}");
    }
    /// A primary press and release at (x, y) points (zgx `driver::click_at`): hit test, focus, routes.
    pub fn click_at(&mut self, x: f32, y: f32) {
        self.app.pointer_down(x, y, Button::Primary);
        self.app.pointer_up(x, y, Button::Primary);
    }
    /// The middle of a rect (points).
    pub fn click_mid(&mut self, r: [f32; 4]) {
        self.click_at(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    }
    pub fn key(&mut self, k: Key) {
        self.app.key_down(k, Mods::default());
    }
}
