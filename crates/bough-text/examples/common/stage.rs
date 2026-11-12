//! Shared by `stage_bench` and `stage_parley`: the round-7 scenes' text generators and the counter harness.
#![allow(dead_code)]
use bough_mem::pmu;

// ---- the scenes' generators (r7/bench) ----------------------------------------------------------------------

pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const SYL: [&str; 28] = ["ka", "lo", "mi", "ra", "te", "su", "no", "vi", "da", "pe", "ru", "ha", "bo", "ni", "fe", "za", "qui", "tra", "len", "mar", "dor", "sel", "an", "or", "ex", "il", "pon", "ver"];

pub fn sentence(rng: &mut Rng, n: usize) -> String {
    let mut s = String::with_capacity(n * 7);
    for i in 0..n {
        if i > 0 {
            s.push(' ');
        }
        let at = s.len();
        for _ in 0..1 + rng.below(3) {
            s.push_str(SYL[rng.below(SYL.len())]);
        }
        if i == 0 {
            s[at..at + 1].make_ascii_uppercase();
        }
    }
    s.push('.');
    s
}

/// The chat scene's 2,000 message bodies (`Chat::build`, seed 0xc4a7), consuming the generator as it does.
pub fn chat_messages(n: usize) -> Vec<String> {
    let mut rng = Rng::new(0xc4a7);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let _author = rng.below(12);
        let words = 12 * (1 + rng.below(6)) - rng.below(8).min(9);
        out.push(sentence(&mut rng, words.max(3)));
        if i > 3 && rng.below(100) < 14 {
            let _ = rng.below(i.min(8));
        }
        if rng.below(100) < 20 {
            for _ in 0..1 + rng.below(3) {
                let _ = (rng.below(8), rng.below(9));
            }
        }
    }
    out
}

pub const VOCAB: [&str; 24] = ["value", "state", "index", "buffer", "result", "config", "item", "node", "count", "self", "len", "name", "path", "data", "text", "range", "cursor", "key", "next", "size", "offset", "event", "frame", "slot"];
pub const NUMS: [&str; 10] = ["0", "1", "2", "4", "8", "16", "42", "100", "1024", "0.5"];
pub const STRS: [&str; 6] = ["\"ok\"", "\"main\"", "\"id\"", "\"/tmp\"", "\"error\"", "\"title\""];
const CMT_A: [&str; 4] = ["// compute", "// TODO:", "// note:", "// fixme:"];
const CMT_B: [&str; 4] = [" the next", " flush the", " cached", " reset"];
const CMT_C: [&str; 4] = [" value", " buffer", " state", " index"];

/// The ide scene's code lines (`gen_line`, seed 0x1de): each line as (text, token class per byte).
pub fn ide_lines(n: usize) -> Vec<(String, Vec<u8>)> {
    let mut rng = Rng::new(0x1de);
    let ident = |rng: &mut Rng| (1u8, VOCAB[if rng.below(100) < 50 { 0 } else { 1 + rng.below(VOCAB.len() - 1) }]);
    (0..n)
        .map(|i| {
            let mut t: Vec<(u8, &str)> = Vec::new();
            let _indent = rng.below(4);
            if i % 16 == 3 {
                t.extend([(1, VOCAB[0]), (5, " += "), (3, NUMS[rng.below(10)]), (5, ";")]);
            } else {
                match rng.below(13) {
                    0 => t.extend([(0, "let "), ident(&mut rng), (5, " = "), ident(&mut rng), (5, "("), (3, NUMS[rng.below(10)]), (5, ");")]),
                    1 => t.extend([(0, "if "), ident(&mut rng), (5, " > "), (3, NUMS[rng.below(10)]), (5, " {")]),
                    2 => t.extend([(0, "fn "), ident(&mut rng), (5, "("), ident(&mut rng), (5, ": "), ident(&mut rng), (5, ") {")]),
                    3 => t.extend([(4, CMT_A[rng.below(4)]), (4, CMT_B[rng.below(4)]), (4, CMT_C[rng.below(4)])]),
                    4 => t.extend([(0, "return "), ident(&mut rng), (5, ";")]),
                    5 => t.extend([ident(&mut rng), (5, "."), ident(&mut rng), (5, "("), (2, STRS[rng.below(6)]), (5, ");")]),
                    6 => t.extend([(0, "match "), ident(&mut rng), (5, " {")]),
                    7 => t.extend([ident(&mut rng), (5, " => "), (2, STRS[rng.below(6)]), (5, ",")]),
                    8 => t.extend([(0, "use "), ident(&mut rng), (5, "::"), ident(&mut rng), (5, ";")]),
                    9 => t.extend([(0, "for "), ident(&mut rng), (0, " in "), (3, NUMS[rng.below(10)]), (5, ".."), ident(&mut rng), (5, " {")]),
                    10 => t.extend([(0, "while "), ident(&mut rng), (5, " < "), ident(&mut rng), (5, " {")]),
                    11 => t.extend([ident(&mut rng), (5, " += "), (3, NUMS[rng.below(10)]), (5, ";")]),
                    _ => t.extend([(0, "let mut "), ident(&mut rng), (5, " = "), (2, STRS[rng.below(6)]), (5, ";")]),
                }
            }
            let (mut s, mut c) = (String::new(), Vec::new());
            for (class, text) in t {
                s.push_str(text);
                c.extend(std::iter::repeat_n(class, text.len()));
            }
            (s, c)
        })
        .collect()
}

// ---- counters ----------------------------------------------------------------------------------------------

pub struct Meter {
    names: Vec<&'static str>,
}

impl Meter {
    pub fn new() -> Meter {
        let names: Vec<&'static str> = pmu::sample_events().iter().map(|e| e.name()).collect();
        let mut head = String::from("cell\tunit");
        for n in &names {
            head.push('\t');
            head.push_str(n);
        }
        println!("{head}\tnote");
        Meter { names }
    }
    /// `f` run `reps` times per batch, five batches: the median per call, per `div` units of work.
    pub fn cell(&self, name: &str, unit: &str, reps: usize, div: f64, note: &str, mut f: impl FnMut()) -> f64 {
        let n = self.names.len();
        let mut rows: Vec<Vec<f64>> = Vec::new();
        for _ in 0..5 {
            let o0 = pmu::sample();
            let o1 = pmu::sample();
            let a = pmu::sample();
            for _ in 0..reps {
                f();
            }
            let b = pmu::sample();
            let (d, o) = (b.since(&a), o1.since(&o0));
            rows.push((0..n).map(|i| d.v[i].saturating_sub(o.v[i]) as f64 / reps as f64 / div).collect());
        }
        let mut line = format!("{name}\t{unit}");
        let mut first = 0.0;
        for i in 0..n {
            let mut col: Vec<f64> = rows.iter().map(|r| r[i]).collect();
            col.sort_by(|a, b| a.partial_cmp(b).unwrap());
            if i == 0 {
                first = col[2];
            }
            line.push_str(&format!("\t{:.1}", col[2]));
        }
        println!("{line}\t{note}");
        first
    }
    pub fn fact(&self, name: &str, unit: &str, v: f64, note: &str) {
        let mut line = format!("{name}\t{unit}\t{v:.3}");
        for _ in 1..self.names.len() {
            line.push('\t');
        }
        println!("{line}\t{note}");
    }
}

