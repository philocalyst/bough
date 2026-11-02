//! Where a vlist row's text mount goes (instructions per row, 300 vlist paragraphs): a fresh layout leaf
//! (`LeafCtx::new` + `set_text` + one wrap + line starts + release), the paragraph alone (`Para::set` + `wrap`),
//! and the round-2 leaf's path rebuilt from its parts (block + byte-scan word table + word wrap).
use bough::layout::LeafCtx;
use bough::text::TextSys;
use bough_text::para::Para;
use bough_text::pool::GlyphPool;
use bough_text_bench::instrs;

/// The round-2 leaf (git 9c61b75 `LeafCtx`, fast path), verbatim but for paths, as the baseline.
#[allow(dead_code)]
#[derive(Clone, Default)]
pub struct OldLeaf {
    pub fast: bool,
    pub words: Vec<bough::layout::Word>,
    pub line_w: i32,
    pub run: bough_text::GlyphRun,
    pub text: Vec<u8>,
    pub starts: Vec<u32>,
    pub wrapped_at: i32,
    pub widest: i32,
    pub min_word: i32,
    pub wrap: bool,
    pub ascii: bool,
    pub upem: f32,
    pub size_px: f32,
    pub lh_px: f32,
    pub block: Option<bough_text::window::Shaped>,
}
use bough::layout::Word;
use bough_text::window::Shaped;
use bough_text::wrap::wrap_greedy;
#[allow(dead_code)]
impl OldLeaf {
    pub fn new(size_px: f32, lh_px: f32, wrap: bool) -> OldLeaf {
        OldLeaf { wrapped_at: i32::MIN, min_word: -1, wrap, size_px, lh_px, ..Default::default() }
    }
    pub fn release(&mut self, pool: &mut GlyphPool) {
        if let Some(b) = self.block.take() {
            pool.free_shaped(b);
        }
    }
    fn line_width(&self) -> i32 { self.line_w }
    pub fn set_text(&mut self, ts: &mut bough::text::TextSys, pool: &mut GlyphPool, text: &str) {
        self.release(pool);
        self.text.clear();
        self.text.extend_from_slice(text.as_bytes());
        self.ascii = text.is_ascii();
        self.upem = ts.shaper.script.upem as f32;
        self.wrapped_at = i32::MIN;
        self.min_word = -1;
        self.run.gid.clear();
        self.run.x.clear();
        // the slot kernel straight into the glyph pool (ASCII, no GSUB/GPOS trigger: glyph i is byte i), or
        // the general shaper into a run (drawn as glyph instances)
        let mut blk = Shaped::default();
        self.fast = pool.shape_block(&ts.shaper, text.as_bytes(), &mut blk);
        if !self.fast {
            ts.shape_run(text, self.size_px, &mut self.run);
            return;
        }
        self.block = Some(blk);
        // word table from the pool's pens: spaces found 8 bytes at a time (SWAR zero-byte mask)
        let b = &self.text;
        let n = b.len();
        let xs = &pool.x[blk.base as usize..blk.base as usize + n];
        let width = blk.width;
        let words = &mut self.words;
        words.clear();
        let mut start = 0usize;
        let push = |start: usize, end: usize, words: &mut Vec<Word>| {
            let x1 = if end < n { xs[end] } else { width };
            words.push(Word { ws: start as u32, we: end as u32, x0: xs[start], x1 });
        };
        let mut base = 0usize;
        const LO7: u64 = 0x7f7f_7f7f_7f7f_7f7f;
        while base + 8 <= n {
            let chunk = u64::from_le_bytes(b[base..base + 8].try_into().unwrap());
            let t = chunk ^ 0x2020_2020_2020_2020;
            // 0x80 in exactly the bytes of `t` that are zero (the spaces)
            let mut m = !(((t & LO7) + LO7) | t | LO7);
            while m != 0 {
                let p = base + (m.trailing_zeros() >> 3) as usize;
                m &= m - 1;
                if p > start {
                    push(start, p, words);
                }
                start = p + 1;
            }
            base += 8;
        }
        for p in base..n {
            if b[p] == b' ' {
                if p > start {
                    push(start, p, words);
                }
                start = p + 1;
            }
        }
        if n > start {
            push(start, n, words);
        }
        self.line_w = self.words.last().map_or(0, |w| w.x1);
    }

    #[inline]
    fn k(&self) -> f32 {
        self.size_px / self.upem.max(1.0)
    }

    /// Does this text wrap at all (a paragraph, whose slow path additionally needs glyph i = byte i)?
    #[inline]
    fn wraps(&self) -> bool {
        self.wrap && self.ascii && (self.fast || self.run.len() == self.text.len())
    }

    /// Wrap at `max_units`; returns the line count (`starts` holds the first byte / glyph of each line).
    pub fn wrap_at(&mut self, max_units: i32) -> usize {
        if self.wrapped_at == max_units && !self.starts.is_empty() {
            return self.starts.len();
        }
        self.wrapped_at = max_units;
        
        let empty = if self.fast { self.text.is_empty() } else { self.run.is_empty() };
        if empty {
            self.starts.clear();
            self.starts.push(0);
            self.widest = 0;
            return 1;
        }
        if !self.wraps() {
            self.starts.clear();
            self.starts.push(0);
            self.widest = if self.fast { self.line_w } else { self.line_width() };
            return 1;
        }
        if self.fast {
            // wrap_greedy over the word table: break before a word whose right edge is past the line
            self.starts.clear();
            self.starts.push(0);
            let (mut line_x0, mut has_word, mut widest, mut last_x1) = (0i32, false, 0i32, 0i32);
            for w in &self.words {
                if has_word && w.x1 - line_x0 > max_units {
                    widest = widest.max(last_x1 - line_x0);
                    self.starts.push(w.ws);
                    line_x0 = w.x0;
                }
                has_word = true;
                last_x1 = w.x1;
            }
            self.widest = widest.max(last_x1 - line_x0);
            return self.starts.len();
        }
        let n = self.run.len();
        let lines = wrap_greedy(&self.run, &self.text, max_units, &mut self.starts);
        let mut widest = 0;
        for i in 0..lines {
            let a = self.starts[i] as usize;
            let mut b = if i + 1 < lines { self.starts[i + 1] as usize } else { n };
            while b > a && self.text[b - 1] == b' ' {
                b -= 1;
            }
            if b == a {
                continue;
            }
            let right = self.run.x[b - 1] + self.run.advance(b - 1);
            widest = widest.max(right - self.run.x[a]);
        }
        self.widest = widest;
        lines
    }

}


fn main() {
    {
        use bough::raw::*;
        println!("size_of LeafCtx {} B, Para {} B", size_of::<LeafCtx>(), size_of::<Para>());
        let mut app = App::new(bough_vlist::VList::new(100_000), bough_vlist::config(2.0), false);
        app.frame();
        let c0 = bough_text::para::COLD_HALVES.load(std::sync::atomic::Ordering::Relaxed);
        for tick in 1..=600u64 {
            bough_vlist::step(&mut app, zgx_workload::vlist::Mode::Jump, tick);
            app.frame();
        }
        println!("vlist jump 600 ticks: {} cold halves, {} rows mounted", bough_text::para::COLD_HALVES.load(std::sync::atomic::Ordering::Relaxed) - c0, app.rt.ctr.rows_mounted);
    }
    let mut ts = TextSys::new(&bough_text_bench::font_bytes(), bough_mem::Col::owned("toks"));
    let mut pool = GlyphPool::new();
    let texts: Vec<String> = (0..300).map(zgx_workload::vlist::text).collect();
    let mu = 100_649;
    let n = texts.len() as f64;
    let mut spans = Vec::new();
    for round in 0..3 {
        let i0 = instrs();
        for t in &texts {
            let mut c = LeafCtx::new(28.0, 40.0, true);
            c.set_text(&mut ts, &mut pool, t);
            c.wrap_at(mu);
            c.line_spans(&mut spans);
            std::hint::black_box((&c, &spans));
            c.release(&mut pool);
        }
        let i1 = instrs();
        for t in &texts {
            let mut p = Para::default();
            p.set(&mut pool, &[&ts.shaper], t, &[]).unwrap();
            std::hint::black_box(p.wrap(mu).len());
            p.release(&mut pool);
        }
        let i2 = instrs();
        for t in &texts {
            let mut c = OldLeaf::new(28.0, 40.0, true);
            c.set_text(&mut ts, &mut pool, t);
            let l = c.wrap_at(mu);
            spans.clear();
            for i in 0..l {
                spans.push((c.starts[i], if i + 1 < l { c.starts[i + 1] } else { t.len() as u32 }));
            }
            std::hint::black_box(&spans);
            std::hint::black_box(&c);
            c.release(&mut pool);
        }
        let i3 = instrs();
        for t in &texts {
            let mut p = Para::default();
            p.set(&mut pool, &[&ts.shaper], t, &[]).unwrap();
            std::hint::black_box(&p);
            p.release(&mut pool);
        }
        let i4 = instrs();
        for t in &texts {
            let mut c = OldLeaf::new(28.0, 40.0, true);
            c.set_text(&mut ts, &mut pool, t);
            std::hint::black_box(&c);
            c.release(&mut pool);
        }
        let i5 = instrs();
        if round == 2 {
            println!("set only: para {:.0}, round-2 leaf {:.0}", (i4 - i3) as f64 / n, (i5 - i4) as f64 / n);
        }
        if round == 2 {
            println!("per row ({:.0} bytes): leaf {:.0}, para {:.0}, round-2 leaf {:.0}", texts.iter().map(|t| t.len()).sum::<usize>() as f64 / n, (i1 - i0) as f64 / n, (i2 - i1) as f64 / n, (i3 - i2) as f64 / n);
        }
    }
}
