//! zgui's `label-churn` probe on bough: thirty axis labels whose strings change on every frame
//! (`format!("{value:.2}")` of a new value each), the median wall time of one relabelled frame (median of
//! five rounds' medians, each of 48 samples after 12 warm-up frames, zgui's `sample::median_ns`), plus
//! retired instructions per frame.   label_churn [labels]   RENDER=1: GPU frames too
use bough::prelude::*;
use bough_zgui::*;

fn main() {
    let gpu = std::env::var("RENDER").is_ok_and(|v| v == "1");
    let mut app = App::new(Churn { zoom: Signal::new(0) }, config(CHURN_W, CHURN_H), gpu);
    app.frame();
    let mut step = 0usize;
    let mut rounds = Vec::new();
    let mut instr = Vec::new();
    for _ in 0..5 {
        let mut s = Samples::new();
        for k in 0..60 {
            step += 1;
            let st = step;
            let (us, i, _) = s.measure(|| {
                app.turn(|c| {
                    c.zoom.set(st);
                });
                app.frame()
            });
            if k >= 12 {
                s.push(us, i);
            }
        }
        rounds.push(spread(&s.us).0);
        instr.push(spread(&s.instr).0);
    }
    let mut sorted = rounds.clone();
    sorted.sort_by(f64::total_cmp);
    let mut si = instr.clone();
    si.sort_by(f64::total_cmp);
    println!(
        "label-churn: {LABELS} labels relabelled, {:.1} us/frame median of round medians ({}); {:.0} instructions/frame ({})",
        sorted[2],
        rounds.iter().map(|v| format!("{v:.1}")).collect::<Vec<_>>().join(" "),
        si[2],
        instr.iter().map(|v| format!("{v:.0}")).collect::<Vec<_>>().join(" ")
    );
}
