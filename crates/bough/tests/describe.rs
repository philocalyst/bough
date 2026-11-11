//! Re-description: after random edits of plain fields (constants of the
//! description, `Option` and `Vec` parts, layout specs, texts, colours, positions) the retained records
//! equal a fresh mount of the same value, turn after turn; wired operands keep working alongside.
use bough::layout::prelude::*;
use bough::raw::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

struct Doc {
    title: String,
    items: Vec<(String, u32, f32)>,
    show: bool,
    x: f32,
    color: u32,
    gap: f32,
    marks: Vec<u32>,
    count: Signal<u32>,
}
impl Doc {
    fn copy(&self) -> Doc {
        Doc { title: self.title.clone(), items: self.items.clone(), show: self.show, x: self.x, color: self.color, gap: self.gap, marks: self.marks.clone(), count: Signal::new(*self.count) }
    }
}

impl View for Doc {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            column((
                label(self.title.clone()).font(14.0, 18.0).color(rgb(self.color)),
                row(each(self.items.iter().map(|(s, c, w)| column((label(s.clone()), block(*w, 6.0).bg(rgb(*c)))).px(2.0)))).gap(self.gap),
                if self.show { Some(block(40.0, 12.0).bg(rgb(0x00ff00))) } else { None },
                label(sig!(self.count).map(|c| format!("count {c}"))),
                block(10.0 + self.items.len() as f32, 8.0).bg(rgb(0x334455)),
            ))
            .w(380.0)
            .gap(2.0),
            quad(self.x, 300.0, 30.0, 20.0).bg(rgb(self.color)).rounded(if self.show { 4.0 } else { 0.0 }).children(text(format!("{} items", self.items.len())).at(2.0, 2.0).color(rgb(0xffffff))),
            each(self.marks.iter().enumerate().map(|(i, &c)| quad(10.0 + i as f32 * 12.0, 340.0, 10.0, 10.0).bg(rgb(c)))),
            clip(0.0, 360.0, 200.0 + self.x, 40.0, text(self.title.clone()).at(0.0, 362.0)),
        )
    }
}

type Item7 = (u8, [f32; 4], u32, u32, f32, f32, [u16; 2]);
type Dump = Vec<(u8, u16, [i16; 4], Vec<Item7>)>;
fn dump(app: &App<impl View>) -> Dump {
    app.rt
        .bough
        .records()
        .iter()
        .map(|r| {
            let items = if r.ink.is_empty() { Vec::new() } else { app.rt.expand_payload(r.payload) };
            (r.kind, r.depth, r.ink.0, items)
        })
        .collect()
}
fn same(ctx: &str, a: &Dump, b: &Dump) {
    assert_eq!(a.len(), b.len(), "{ctx}: record count");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!(x.0 == y.0 && x.1 == y.1, "{ctx}: record {i} kind/depth {:?} != {:?}", (x.0, x.1), (y.0, y.1));
        assert!(x.2 == y.2, "{ctx}: record {i} (kind {}) ink {:?} != fresh {:?}", x.0, x.2, y.2);
        assert!(x.3 == y.3, "{ctx}: record {i} payload {:?} != fresh {:?}", x.3, y.3);
    }
}

fn cfg() -> Config {
    Config::new(400.0, 420.0, 1.0, FONT.to_vec())
}

#[test]
fn redescription_equals_fresh_mount() {
    let words = ["a", "bb", "lorem", "ipsum dolor", "x y z", "wide label text"];
    for seed in 1..9u64 {
        let mut r = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D));
        let doc = Doc { title: "doc".into(), items: vec![], show: false, x: 5.0, color: 0x808080, gap: 3.0, marks: vec![], count: Signal::new(0) };
        let mut a = App::new(doc, cfg(), false);
        a.frame();
        for turn in 0..80 {
            let k = 1 + r.below(4);
            let ops: Vec<(usize, u64)> = (0..k).map(|_| (r.below(12), r.next())).collect();
            a.turn(|d| {
                for &(op, x) in &ops {
                    let n = d.items.len();
                    match op {
                        0 | 1 => d.items.push((words[x as usize % words.len()].to_string(), 0x101010 * (1 + x as u32 % 14), 4.0 + (x % 40) as f32)),
                        2 if n > 0 => {
                            d.items.pop();
                        }
                        3 if n > 0 => d.items[x as usize % n].0 = words[(x >> 8) as usize % words.len()].into(),
                        4 if n > 0 => d.items[x as usize % n].2 = 2.0 + (x % 50) as f32,
                        5 => d.show = !d.show,
                        6 => d.title = format!("title {}", x % 1000),
                        7 => d.x = (x % 120) as f32,
                        8 => d.color = (x as u32) & 0xffffff,
                        9 => d.gap = (x % 6) as f32,
                        10 => {
                            if x % 2 == 0 {
                                d.marks.push((x >> 3) as u32 & 0xffffff);
                            } else {
                                d.marks.truncate(d.marks.len().saturating_sub(2));
                            }
                        }
                        _ => *d.count += 1,
                    }
                }
            });
            a.frame();
            let mut f = App::new(a.owner.copy(), cfg(), false);
            f.frame();
            same(&format!("seed {seed} turn {turn}"), &dump(&a), &dump(&f));
        }
    }
}
