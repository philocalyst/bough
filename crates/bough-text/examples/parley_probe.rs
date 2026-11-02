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
    let tb = {
        let words = ["the", "renderer", "keeps", "every", "glyph", "retained", "while", "streaming", "tokens", "arrive"];
        (0..64).map(|i| words[(i * 7 + 3) % words.len()]).collect::<Vec<_>>().join(" ")
    };
    let cases: Vec<(&str, Case)> = vec![
        ("tb para", Case::plain(&tb, "DejaVu Sans", 14.0, Some(736.0))),
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
    if std::env::var("SPIN_RETEXT").is_ok() {
        let c = Case::plain(&log, "DejaVu Sans", 12.0, None);
        let mut l = g.layout(&c);
        let t = std::time::Instant::now();
        while t.elapsed().as_secs() < 14 {
            for _ in 0..1000 {
                assert!(l.retext(&mut g.lcx, &log));
            }
        }
        return;
    }
    if std::env::var("RETEXT").is_ok() {
        for (t, sz) in [("-12.34", 11.0), ("17.85", 11.0), (log.as_str(), 12.0)] {
            let c = Case::plain(t, "DejaVu Sans", sz, None);
            let mut l = g.layout(&c);
            let r = meter::count(30, || {
                assert!(l.retext(&mut g.lcx, t));
            });
            let b = meter::count(30, || l.break_all_lines(None));
            let a = meter::count(30, || l.align(parley::Alignment::Start, parley::AlignmentOptions::default()));
            println!("{:>8} B: retext {:.0}, break {:.0}, align {:.0}", t.len(), r.instr, b.instr, a.instr);
        }
        return;
    }
    if std::env::var("PATHS").is_ok() {
        for t in ["12.34", "-5.67", "12,345 req/s", "Throughput", "45%", &log] {
            let (a0, s0) = (bough_text::parley::bough::analyses(), bough_text::parley::bough::segments());
            let _ = g.layout(&Case::plain(t, "DejaVu Sans", 12.0, None));
            let (a1, s1) = (bough_text::parley::bough::analyses(), bough_text::parley::bough::segments());
            println!("{t:?}: analyses icu {} simple {}; segments harfrust {} table {}", a1[0] - a0[0], a1[1] - a0[1], s1[0] - s0[0], s1[1] - s0[1]);
        }
        return;
    }
    if let Ok(which) = std::env::var("SPIN") {
        let c = &cases.iter().find(|c| c.0.starts_with(which.as_str())).expect("case").1;
        let up = std::env::var("UPSTREAM").is_ok();
        let t = std::time::Instant::now();
        while t.elapsed().as_secs() < 14 {
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
        if std::env::var("PHASES").is_ok() {
            let b = meter::count(30, || {
                std::hint::black_box(g.build(c));
            });
            let mut l = g.build(c);
            let br = meter::count(30, || {
                l.break_all_lines(c.width);
            });
            let al = meter::count(30, || {
                l.align(parley::Alignment::Start, parley::AlignmentOptions::default());
            });
            let bu = meter::count(30, || {
                std::hint::black_box(u.build(c));
            });
            let mut lg = g.layout(c);
            let ri = meter::count(30, || g.layout_into(c, &mut lg));
            let mut lu = u.layout(c);
            let ru = meter::count(30, || u.layout_into(c, &mut lu));
            println!(
                "  {name}: grafted build {:.0} (upstream {:.0}), break {:.0}, align {:.0}; retained layout (build_into+break+align): grafted {:.0}, upstream {:.0}",
                b.instr, bu.instr, br.instr, al.instr, ri.instr, ru.instr
            );
        }
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
