//! (scratch, not part of zgx) The vlist scene (zgx_workload::vlist) headless: the windowed
//! `vlist_workload`'s list and per-tick offset logic, ticked as fast as it goes, so instructions per
//! frame can be taken by the slope method (as heavy_headless). ZGX_MODE=scroll|fling|jump|probe,
//! TICKS (900), SCALE (2), RENDER (1: offscreen GpuRenderer; 0: layout + damage only).
use zgx::{compose::prelude::*, widgets::Ui};
use zgx_gpu::{GpuRenderer, TextServices};
use zgx_workload::vlist::{self, Mode};

#[path = "support/headless_font.rs"]
mod headless_font;
#[path = "support/vlist_view.rs"]
mod vlist_view;

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn main() {
    let ticks: usize = env("TICKS", 900);
    let scale: f32 = env("SCALE", 2.);
    let render = env("RENDER", 1) != 0;
    let mode = Mode::from_env();
    let (width, height) = (vlist::WIDTH, vlist::HEIGHT);
    let mut ui = Ui::new(width, height);
    let text = TextServices::new();
    let mut gpu = render.then(|| {
        let mut gpu = GpuRenderer::new((width * scale) as u32, (height * scale) as u32).expect("GPU");
        gpu.set_scale_factor(scale);
        headless_font::font().install(&mut gpu.text_system().borrow_mut());
        gpu.install_text(&mut ui.scene.borrow_mut());
        gpu
    });
    if gpu.is_none() {
        headless_font::font().install(&mut text.text_system().borrow_mut());
        text.install(&mut ui.scene.borrow_mut());
    }
    let offset = ui.signal(0_f32);
    let heights = VariableHeights::new(&ui.runtime, vlist::ROWS, vlist_view::ESTIMATE);
    let list = measured_virtual_list(offset.clone(), heights.clone(), 2, |i| i, |_, index, _| vlist_view::chat_row(index))
        .w_full()
        .h_full();
    ui.mount(vlist_view::frame(list));
    let mut frame = |ui: &mut Ui, gpu: &mut Option<GpuRenderer>| {
        ui.prepare_frame();
        let report = ui.scene.borrow_mut().flush();
        if let Some(gpu) = gpu {
            gpu.render(&ui.scene.borrow(), &report.damage).expect("render");
        }
    };
    frame(&mut ui, &mut gpu);
    if mode == Mode::Probe {
        let mut measured = Vec::new();
        let rows: Vec<usize> = match std::env::var("PROBE_SPREAD").ok().and_then(|v| v.parse::<usize>().ok()) {
            Some(n) => (0..n).map(|i| i * 97 % vlist::ROWS).collect(),
            None => vlist::probe_rows(),
        };
        for row in rows {
            offset.set(heights.row_offset(row));
            for _ in 0..6 {
                frame(&mut ui, &mut gpu);
            }
            offset.set(heights.row_offset(row));
            for _ in 0..6 {
                frame(&mut ui, &mut gpu);
            }
            measured.push((row, heights.row_height(row).unwrap_or(0.)));
        }
        eprintln!("{}", vlist::probe_report(&measured, vlist::WIDTH));
        return;
    }
    let started = std::time::Instant::now();
    for tick in 1..=ticks as u64 {
        match vlist::jump(tick) {
            Some(row) if mode == Mode::Jump => {
                offset.set(heights.row_offset(row));
            }
            _ => {
                let max = (heights.content_height() - vlist::HEIGHT).max(0.);
                let moved = offset.get() + vlist::delta(mode, tick);
                offset.set(if moved > max { 0. } else { moved });
            }
        }
        frame(&mut ui, &mut gpu);
        if tick % 60 == 0 {
            if let Some(gpu) = &mut gpu {
                gpu.readback().expect("readback");
            }
        }
    }
    if let Some(gpu) = &mut gpu {
        gpu.readback().expect("readback");
    }
    eprintln!("vlist_headless {mode:?} render={render} ticks={ticks} {:.4} ms/tick", started.elapsed().as_secs_f64() * 1e3 / ticks as f64);
}
