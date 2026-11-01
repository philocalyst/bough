//! The parley path's cost per operation (instructions): unmodified upstream parley vs the grafted parley, same
//! input, same driver (`tests/common/twin.rs`). `SPIN=label|log|para|rich [UPSTREAM=1]` loops one case for `sample`.
#[path = "common/meter.rs"]
mod meter;
#[path = "common/corpus.rs"]
mod corpus;
#[path = "../tests/common/twin.rs"]
mod twin;

use twin::{Case, Prop};

fn main() {
    let fonts = twin::dejavu();
    let mut g = twin::grafted::Eng::new(&fonts, false);
    let mut u = twin::upstream::Eng::new(&fonts, false);
    let label = "12,345 req/s".to_string();
    let log = corpus::heavy_log_lines(3).pop().unwrap();
    let para = "The quick brown fox jumps over the lazy dog while streaming tokens across the wire; \
        every paragraph is measured at two widths, wrapped greedily, and drawn once per frame. "
        .repeat(2);
    let cases: Vec<(&str, Case)> = vec![
        ("label", Case::plain(&label, "DejaVu Sans", 12.0, None)),
        ("log line", Case::plain(&log, "DejaVu Sans", 12.0, None)),
        ("paragraph", Case::plain(&para, "DejaVu Sans", 14.0, Some(400.0))),
        ("rich para", {
            let mut c = Case::plain(&para, "DejaVu Sans", 14.0, Some(400.0));
            c.spans.push((10..30, Prop::Weight(700.0)));
            c.spans.push((60..90, Prop::Family("DejaVu Sans Mono")));
            c
        }),
    ];
    if let Ok(which) = std::env::var("SPIN") {
        let c = &cases.iter().find(|c| c.0.starts_with(which.as_str())).expect("case").1;
        let up = std::env::var("UPSTREAM").is_ok();
        let t = std::time::Instant::now();
        while t.elapsed().as_secs() < 6 {
            for _ in 0..100 {
                if up {
                    std::hint::black_box(u.layout(c));
                } else {
                    std::hint::black_box(g.layout(c));
                }
            }
        }
        return;
    }
    println!("{:10} {:>5}  {:>10} {:>10} {:>7}  {:>8} {:>8}", "case", "bytes", "upstream", "grafted", "x", "up/B", "graft/B");
    for (name, c) in &cases {
        let _ = (g.layout(c), u.layout(c));
        let cu = meter::count(30, || {
            std::hint::black_box(u.layout(c));
        });
        let cg = meter::count(30, || {
            std::hint::black_box(g.layout(c));
        });
        let n = c.text.len() as f64;
        println!(
            "{name:10} {:5}  {:10.0} {:10.0} {:7.2}  {:8.1} {:8.1}",
            c.text.len(),
            cu.instr,
            cg.instr,
            cu.instr / cg.instr,
            cu.instr / n,
            cg.instr / n
        );
    }
}
