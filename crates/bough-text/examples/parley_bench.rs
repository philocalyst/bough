//! bough-text's workloads through the parley path: unmodified upstream parley vs the grafted parley, same input,
//! same driver (`tests/common/twin.rs`), retained layouts (`build_into`, allocations reused), instructions per
//! phase (`proc_pid_rusage`). The pre-parley bough numbers are in RESULTS.md Part III (same generators).
//!
//! * text_bench (zgx-gpu): 1000 rich paragraphs (DejaVu Sans / Bold / Mono runs, 14 px) laid out at 736 px, again at
//!   736, at 700, and one paragraph streamed to 3000 bytes 6 bytes a step, laid out at 736 and 968 each step;
//! * label churn (zgui): 30 labels relabelled per frame (`{value:.2}`), 11 px, unwrapped;
//! * heavy (zgx): per tick, one new log line and the ~80 numeric labels, unwrapped;
//! * i18n (zgx): 1000 plural messages (`{n} messages`) laid out once, and after a locale switch (`reçus`).
#[path = "common/meter.rs"]
mod meter;
#[path = "common/corpus.rs"]
mod corpus;
#[path = "../tests/common/twin.rs"]
mod twin;

use twin::{Case, Prop};

const WORDS: &[&str] = &[
    "the", "renderer", "keeps", "every", "glyph", "retained", "while", "streaming", "tokens", "arrive", "layout", "measures", "paragraphs",
    "at", "two", "widths", "per", "pass", "and", "virtualized", "transcripts", "need", "exact", "heights", "before", "mounting", "views",
    "fonts", "shape", "once", "wrapping", "is", "arithmetic", "over", "cached", "advances",
];

fn paragraph(seed: usize, chars: usize) -> (String, Vec<(usize, u8)>) {
    let (mut text, mut runs) = (String::new(), Vec::<(usize, u8)>::new());
    let mut i = seed;
    while text.len() < chars {
        let font = match i % 11 {
            3 => 1,
            7 => 2,
            _ => 0,
        };
        if runs.last().is_none_or(|r| r.1 != font) {
            runs.push((text.len(), font));
        }
        for _ in 0..(1 + i % 4) {
            text.push_str(WORDS[(i * 7 + seed) % WORDS.len()]);
            text.push(' ');
            i += 1;
        }
    }
    (text, runs)
}

fn rich<'a>(text: &'a str, runs: &[(usize, u8)], width: Option<f32>) -> Case<'a> {
    let mut c = Case::plain(text, "DejaVu Sans", 14.0, width);
    for (k, &(start, font)) in runs.iter().enumerate() {
        let end = runs.get(k + 1).map_or(text.len(), |r| r.0).min(text.len());
        if start >= end {
            continue;
        }
        match font {
            1 => c.spans.push((start..end, Prop::Weight(700.0))),
            2 => c.spans.push((start..end, Prop::Family("DejaVu Sans Mono"))),
            _ => {}
        }
    }
    c
}

fn label(zoom: usize, index: usize) -> String {
    let value = ((zoom * 7_919 + index * 104_729) % 200_000) as f64 / 1_000.0 - 100.0;
    format!("{value:.2}")
}

/// Relayout a retained one-style layout with a new text: graft 6's `retext` where the engine has it, a
/// `build_into` otherwise (upstream parley's only way).
macro_rules! relabel {
    (grafted, $e:expr, $c:expr, $l:expr) => {{
        if $l.retext(&mut $e.lcx, $c.text) {
            $l.break_all_lines($c.width);
            $l.align(bough_text::parley::Alignment::Start, bough_text::parley::AlignmentOptions::default());
        } else {
            $e.layout_into(&$c, $l);
        }
    }};
    (upstream, $e:expr, $c:expr, $l:expr) => {{
        $e.layout_into(&$c, $l);
    }};
}

macro_rules! bench {
    ($eng:expr, $name:tt) => {{
        let e = &mut $eng;
        let mut out = Vec::new();
        // text_bench
        let paragraphs: Vec<(String, Vec<(usize, u8)>)> = (0..1000).map(|i| paragraph(i, 200 + (i * 37) % 400)).collect();
        let mut ls: Vec<_> = paragraphs.iter().map(|(t, r)| e.layout(&rich(t, r, Some(736.0)))).collect();
        let i0 = meter::instructions();
        for (l, (t, r)) in ls.iter_mut().zip(&paragraphs) {
            e.layout_into(&rich(t, r, Some(736.0)), l);
        }
        let i1 = meter::instructions();
        for l in ls.iter_mut() {
            l.break_all_lines(Some(736.0));
        }
        let i2 = meter::instructions();
        for l in ls.iter_mut() {
            l.break_all_lines(Some(700.0));
        }
        let i3 = meter::instructions();
        let (st, sr) = paragraph(7, 3000);
        let mut sl = e.layout(&rich(&st[..6], &sr, Some(736.0)));
        let i4 = meter::instructions();
        let mut end = 6;
        while end <= st.len() {
            let c = rich(&st[..end], &sr, Some(736.0));
            e.layout_into(&c, &mut sl);
            sl.break_all_lines(Some(968.0));
            end += 6;
        }
        let i5 = meter::instructions();
        out.push(("text_bench first measure, 1000 paragraphs", i1 - i0));
        out.push(("text_bench same width", i2 - i1));
        out.push(("text_bench new width", i3 - i2));
        out.push(("text_bench stream 3000 B, 6 B/step, 2 widths", i5 - i4));
        // label churn: 30 labels x 200 frames
        let mut labels: Vec<String> = (0..30).map(|i| label(0, i)).collect();
        let mut ll: Vec<_> = labels.iter().map(|t| e.layout(&Case::plain(t, "DejaVu Sans", 11.0, None))).collect();
        let i6 = meter::instructions();
        for z in 1..=200 {
            for (i, (s, l)) in labels.iter_mut().zip(ll.iter_mut()).enumerate() {
                *s = label(z, i);
                relabel!($name, e, Case::plain(s, "DejaVu Sans", 11.0, None), l);
            }
        }
        let i7 = meter::instructions();
        out.push(("label churn, per frame (30 labels)", (i7 - i6) / 200));
        // heavy: per tick, one log line + the numeric labels
        let logs = corpus::heavy_log_lines(300);
        let nums: Vec<Vec<(String, f32)>> = (0..300u64).map(|t| corpus::heavy_numeric(t)).collect();
        let mut log_l = e.layout(&Case::plain(&logs[0], "DejaVu Sans", 12.0, None));
        let mut num_l: Vec<_> = nums[0].iter().map(|(s, z)| e.layout(&Case::plain(s, "DejaVu Sans", *z, None))).collect();
        let i8 = meter::instructions();
        for t in 0..300 {
            relabel!($name, e, Case::plain(&logs[t], "DejaVu Sans", 12.0, None), &mut log_l);
            for ((s, z), l) in nums[t].iter().zip(num_l.iter_mut()) {
                relabel!($name, e, Case::plain(s, "DejaVu Sans", *z, None), l);
            }
        }
        let i9 = meter::instructions();
        let mut log_only = 0;
        for t in 0..300 {
            let a = meter::instructions();
            relabel!($name, e, Case::plain(&logs[t], "DejaVu Sans", 12.0, None), &mut log_l);
            log_only += meter::instructions() - a;
        }
        out.push(("heavy text per tick (log line + numeric labels)", (i9 - i8) / 300));
        out.push(("  of which the log line", log_only / 300));
        // i18n
        let msgs: Vec<String> = (0..1000u32).map(|n| if n == 1 { format!("{n} message") } else { format!("{n} messages") }).collect();
        let fr: Vec<String> = (0..1000u32).map(|n| if n == 1 { format!("{n} message") } else { format!("{n} messages reçus") }).collect();
        let mut ml: Vec<_> = msgs.iter().map(|s| e.layout(&Case::plain(s, "DejaVu Sans", 13.0, None))).collect();
        let ia = meter::instructions();
        for (s, l) in msgs.iter().zip(ml.iter_mut()) {
            e.layout_into(&Case::plain(s, "DejaVu Sans", 13.0, None), l);
        }
        let ib = meter::instructions();
        for (s, l) in fr.iter().zip(ml.iter_mut()) {
            relabel!($name, e, Case::plain(s, "DejaVu Sans", 13.0, None), l);
        }
        let ic = meter::instructions();
        out.push(("i18n 1000 messages", ib - ia));
        out.push(("i18n locale switch (1000 messages)", ic - ib));
        out
    }};
}

fn main() {
    let fonts = twin::dejavu();
    let mut g = twin::grafted::Eng::new(&fonts, false);
    let mut u = twin::upstream::Eng::new(&fonts, false);
    let rg = bench!(g, grafted);
    let ru = bench!(u, upstream);
    println!("{:50} {:>14} {:>14} {:>7}", "phase (instructions)", "upstream", "grafted", "x");
    for ((name, a), (_, b)) in ru.iter().zip(&rg) {
        println!("{name:50} {a:14} {b:14} {:7.2}", *a as f64 / (*b).max(1) as f64);
    }
}
