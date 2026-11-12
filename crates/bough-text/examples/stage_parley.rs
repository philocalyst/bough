//! The same strings as `stage_bench`, through parley: unmodified upstream parley 0.11.1 (the engine under
//! gpui-ce's text system, which pins 0.12 and adds its own caches and clones on top) and bough's grafted
//! parley, by the twin driver of `tests/common/twin.rs`. DejaVu Sans at 26 px (run `stage_bench` with
//! `BOUGH_FONT` pointing at the same file to compare like with like).
//!
//!     cargo run --profile dist -p bough-text --example stage_parley
#[path = "common/stage.rs"]
mod common;
#[path = "../tests/common/twin.rs"]
mod twin;

use common::{chat_messages, ide_lines, Meter, VOCAB};
use std::hint::black_box;
use twin::Case;

const FAMILY: &str = "DejaVu Sans";
const SIZE: f32 = 26.0;
/// The chat bubble's content width at the 1280 pt window, device pixels (as `stage_bench`'s `units`).
const W: f32 = 1294.96;

macro_rules! bench {
    ($m:expr, $e:expr, $tag:literal, $msgs:expr, $code:expr) => {{
        let (m, e, msgs, code): (&Meter, _, &[String], &[(String, Vec<u8>)]) = ($m, &mut $e, $msgs, $code);
        let total: usize = msgs.iter().map(|s| s.len()).sum();
        let n = msgs.len() as f64;
        let mut ls: Vec<_> = msgs.iter().map(|t| e.layout(&Case::plain(t, FAMILY, SIZE, Some(W)))).collect();
        m.cell(concat!("p.", $tag, ".chat.layout"), "instr/byte", 2, total as f64, "shape and break every message into a retained layout (allocations reused)", || {
            for (l, t) in ls.iter_mut().zip(msgs) {
                e.layout_into(&Case::plain(t, FAMILY, SIZE, Some(W)), l);
            }
        });
        let mut k = 0;
        m.cell(concat!("p.", $tag, ".chat.rebreak"), "instr/para", 5, n, "break_all_lines at a new width, no shaping", || {
            k += 1;
            for l in ls.iter_mut() {
                l.break_all_lines(Some(W - ((k & 15) * 16) as f32));
            }
        });
        let exposed = &msgs[msgs.len() - 20..];
        m.cell(concat!("p.", $tag, ".chat.exposed20"), "instr", 10, 1.0, "20 messages laid out from nothing (stage_bench s1)", || {
            for t in exposed {
                black_box(e.layout(&Case::plain(t, FAMILY, SIZE, Some(W))));
            }
        });
        let stream: String = msgs.iter().take(600).flat_map(|s| [s.as_str(), " "]).collect();
        for kb in [1usize, 10] {
            let mut end = kb * 1024;
            let mut l = e.layout(&Case::plain(&stream[..end], FAMILY, SIZE, Some(W)));
            m.cell(&format!(concat!("p.", $tag, ".append.{}k"), kb), "instr/append", 20, 1.0, "6 bytes appended: the text laid out again (parley has no append)", || {
                end += 6;
                e.layout_into(&Case::plain(&stream[..end], FAMILY, SIZE, Some(W)), &mut l);
            });
        }
        let tree: Vec<String> = (0..40).map(|i| format!("{}_{}.rs", VOCAB[i % 24], VOCAB[(i * 7 + 3) % 24])).collect();
        let term: Vec<String> = (0..30).map(|i| format!("[{:>4}] build {} ok in 0.{}s", i * 17, VOCAB[i % 24], 10 + i)).collect();
        m.cell(concat!("p.", $tag, ".ide.lines130"), "instr", 20, 1.0, "130 one-line layouts from nothing (stage_bench s2)", || {
            for t in code[..60].iter().map(|c| &c.0).chain(&tree).chain(&term) {
                black_box(e.layout(&Case::plain(t, FAMILY, 24.0, None)));
            }
        });
    }};
}

fn main() {
    let fonts = twin::dejavu();
    let m = Meter::new();
    let msgs = chat_messages(2_000);
    let code = ide_lines(3_000);
    let mut u = twin::upstream::Eng::new(&fonts, false);
    bench!(&m, u, "upstream", &msgs, &code);
    let mut g = twin::grafted::Eng::new(&fonts, false);
    bench!(&m, g, "grafted", &msgs, &code);
}
