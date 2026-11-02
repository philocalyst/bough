//! zgui's thread-resize document (`scenario/thread.rs`): a chat thread of 250 messages, each a fixed-width
//! header (name 96 px, time 48 px) and 1–3 wrapped paragraphs of deterministic prose (zgui's word list and
//! seeds), every tenth with a 320 x 80 card, in a flex column (padding 12, gap 10) inside a scroll view the
//! window's size. Laid out by cephal; a width step re-asks every row and re-wraps every paragraph.
//!
//! Not reproduced: zgui's paragraphs are inline spans in three faces (bold, mono with a background,
//! italic); here each paragraph is one run with the same characters in one face (the same wrapping work).
use crate::*;
use bough::layout::prelude::*;

pub const MESSAGES: usize = 250;

const WORDS: &[&str] = &[
    "the", "resize", "asks", "every", "paragraph", "its", "question", "again", "and", "what", "one", "step", "costs", "is", "decided", "by", "how", "much", "of", "last", "width's", "answer", "was", "kept", "a", "line", "that", "wraps", "carries", "words", "over", "to", "where", "they", "fit", "now", "while", "fixed", "header", "stays",
];

/// zgui's `prose(seed, count)`.
pub fn prose(seed: usize, count: usize) -> String {
    let mut text = String::new();
    for step in 0..count {
        if step > 0 {
            text.push(' ');
        }
        text.push_str(WORDS[seed.wrapping_mul(31).wrapping_add(step.wrapping_mul(7)) % WORDS.len()]);
    }
    text
}

/// One paragraph's characters: zgui's six spans in order.
pub fn paragraph(seed: usize) -> String {
    format!("{} {} {} frame_budget({seed}) {} {}", prose(seed, 9), prose(seed + 1, 3), prose(seed + 2, 11), prose(seed + 3, 8), prose(seed + 4, 4))
}

pub struct Thread {
    pub scroll: Signal<f32>,
}

const BG: Color = rgb(0x14161a);
const FG: Color = rgb(0xe7ecf5);
const WHEN: Color = rgb(0x8b93a7);

fn message(i: usize) -> impl Element<Thread> {
    let count = 1 + i % 3;
    let card = (i % 10 == 0).then(|| block(320.0, 80.0).bg(rgb(0x1b202b)).border(1.0, rgb(0x232833)));
    column((
        row((label(format!("agent-{}", i % 7)).font(13.0, 16.0).color(FG).w(96.0), label(format!("{:02}:{:02}", i / 60 % 24, i % 60)).font(13.0, 16.0).color(WHEN).w(48.0))).gap(8.0).items(Align::Center),
        each((0..count).map(move |p| para(paragraph(i * 131 + p * 17)).font(13.0, 18.85).color(FG))),
        each(card.into_iter()),
    ))
    .gap(4.0)
}

impl View for Thread {
    fn view(&self) -> impl Element<Self> + '_ {
        scroll_y(sig!(self.scroll), [0.0, 0.0, WIDTH, HEIGHT], 0.0, column(each((0..MESSAGES).map(message))).w_pct(1.0).p(12.0).gap(10.0).bg(BG)).bg(BG).line(19.5 * 3.0)
    }
}

/// The thread's text is ~100 K glyphs (paragraphs with "fi" / "fl" go through the glyph-instance path).
pub fn thread_config(w: f32, h: f32) -> Config {
    let mut c = config(w, h);
    c.caps.instances = 1 << 18;
    c.caps.glyphs = 1 << 18;
    c.caps.runs = 1 << 15;
    c.caps.pages = 1024;
    c
}

/// zgui `scenario/thread.rs`'s step phase: one warm-up width (1,800), then 32 widths
/// (1,600 + (step % 24) * 8), each delivered and settled.
pub fn run(gpu: bool) {
    let mut app = App::new(Thread { scroll: Signal::new(0.0) }, thread_config(WIDTH, HEIGHT), gpu);
    app.frame();
    let wraps0 = bough::layout::WRAP_RUNS.load(std::sync::atomic::Ordering::Relaxed);
    app.resize(WIDTH + 200.0, HEIGHT);
    app.frame();
    let mut s = Samples::new();
    let (mut relaid, mut wraps) = (0u64, 0u64);
    for step in 0..32 {
        let w = WIDTH + (step % 24) as f32 * 8.0;
        let (v0, r0) = (app.rt.lay.stats.nodes_written, bough::layout::WRAP_RUNS.load(std::sync::atomic::Ordering::Relaxed));
        s.time(|| {
            app.resize(w, HEIGHT);
            app.frame()
        });
        relaid += app.rt.lay.stats.nodes_written - v0;
        wraps += bough::layout::WRAP_RUNS.load(std::sync::atomic::Ordering::Relaxed) - r0;
    }
    let _ = wraps0;
    let paras: usize = (0..MESSAGES).map(|i| 1 + i % 3).sum();
    println!("SCENARIO thread-resize {MESSAGES} messages, {paras} paragraphs, {} records; 32 width steps", app.rt.bough.len());
    report("thread-resize", "thread.resize", "ms", &s, 1e-3);
    count("thread-resize", "thread.nodes_written_per_step", "nodes", relaid as f64 / 32.0);
    count("thread-resize", "thread.wraps_per_step", "paragraphs", wraps as f64 / 32.0);
}
