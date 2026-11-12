//! zgx-gpu `stream_memory`, native: resident memory while a plain text streams as in the comparison
//! benchmark: a 7-line window (105 bytes a line) sliding every tick, GPU-backed (offscreen), 1200 ticks,
//! 960 x 720 points at scale 2. The text is one `text(sig!(stream))` with explicit `\n` lines: zgx's lines
//! are fixed 105-byte chunks joined by `\n` before they reach the text box, so wrapping is not involved.
//! Prints RSS and the phys footprint at ticks 60 and 1199.
use bough::raw::*;
use bough_suite::*;
use std::fmt::Write;

struct Stream {
    stream: Signal<String>,
}
impl View for Stream {
    fn view(&self) -> impl Element<Self> + '_ {
        text(sig!(self.stream)).at(0.0, 0.0).size(16.0).line_height(normal(16.0))
    }
}

fn mb(b: u64) -> f64 {
    b as f64 / 1048576.0
}

fn main() {
    let mut a = App::new(Stream { stream: Signal::new(String::new()) }, Config::new(960.0, 720.0, 2.0, font()).once(), true);
    a.frame();
    let mut source = String::new();
    for tick in 0..1200 {
        write!(source, "{tick:06} The quick brown fox streams a token. ").unwrap();
        if source.len() > 8192 {
            source.drain(..source.len() - 8192);
        }
        let tail = &source.as_bytes()[source.len().saturating_sub(735)..];
        let visible = tail.chunks(105).map(|chunk| std::str::from_utf8(chunk).unwrap()).collect::<Vec<_>>().join("\n");
        a.turn(|v| _ = v.stream.set(visible));
        a.frame();
        if tick == 60 || tick == 1199 {
            a.wait_presented();
            let s = bough::sys::perf::snap();
            println!("tick {tick:4}: rss {:.0} MB, footprint {:.0} MB, glyph atlas {}x{}", mb(s.rss), mb(s.footprint), a.rt.text.cache.atlas.width, a.rt.text.cache.atlas.height);
        }
    }
}
