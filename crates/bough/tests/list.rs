//! `List<T>` against a `Vec` oracle, and list regions: after random splice / move / permutation / edit
//! sequences the retained records equal a fresh mount of the same items (no GPU: heap region; the pixel
//! gates are in comparisons/bough-suite).
use bough::layout::prelude::*;
use bough::list::{replay, Op};
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

// ------------------------------------------------------------------ data structure vs Vec

#[test]
fn list_ops_match_a_vec_oracle() {
    for seed in 1..40u64 {
        let mut r = Rng(seed * 0x9E37_79B9_7F4A_7C15);
        let mut l: List<u64> = List::new();
        let mut v: Vec<u64> = Vec::new();
        let mut slots: Vec<(Slot<u64>, u64)> = Vec::new();
        let mut taken: Vec<Slot<u64>> = Vec::new();
        for step in 0..400 {
            let k = r.below(12);
            let n = v.len();
            match k {
                0 | 1 => {
                    let at = r.below(n + 1);
                    let x = r.next();
                    let s = l.insert(at, x);
                    v.insert(at, x);
                    slots.push((s, x));
                }
                2 if n > 0 => {
                    let at = r.below(n);
                    let s = l.slot(at).unwrap();
                    assert_eq!(l.remove(at), v.remove(at));
                    taken.push(s);
                }
                3 if n > 1 => {
                    let (a, b) = (r.below(n), r.below(n));
                    l.move_to(a, b);
                    let x = v.remove(a);
                    v.insert(b, x);
                }
                4 if n > 1 => {
                    let (a, b) = (r.below(n), r.below(n));
                    l.swap(a, b);
                    v.swap(a, b);
                }
                5 => {
                    l.reverse();
                    v.reverse();
                }
                6 => {
                    l.sort_by_key(|x| x % 97);
                    v.sort_by_key(|x| x % 97);
                }
                7 if n > 0 => {
                    let at = r.below(n);
                    *l.edit(at) += 1;
                    v[at] += 1;
                }
                8 if !slots.is_empty() => {
                    let (s, _) = slots[r.below(slots.len())];
                    match l.take(s) {
                        Some(x) => {
                            let at = v.iter().position(|&y| y == x).unwrap();
                            v.remove(at);
                            taken.push(s);
                        }
                        None => assert!(l.position(s).is_none()),
                    }
                }
                9 if n > 2 => {
                    let keep = r.next();
                    l.retain(|x| (x ^ keep) % 3 != 0);
                    v.retain(|x| (x ^ keep) % 3 != 0);
                }
                10 if n > 0 => {
                    let at = r.below(n);
                    let (s, o) = (l.slot(at).unwrap(), l.slot(r.below(n)));
                    l.move_before(s, o);
                    // oracle: by value identity of the slots
                    let x = v.remove(at);
                    let to = match o {
                        Some(o) if o != s => v.iter().position(|&y| Some(&y) == l.get_slot(o)).unwrap(),
                        Some(_) => at,
                        None => v.len(),
                    };
                    v.insert(to, x);
                }
                _ => {
                    let x = r.next();
                    slots.push((l.push(x), x));
                    v.push(x);
                }
            }
            assert!(l.iter().copied().eq(v.iter().copied()), "seed {seed} step {step}: list != oracle");
            for &s in &taken {
                assert!(l.get_slot(s).is_none(), "a taken slot resolved");
            }
            for (i, s) in l.slots().enumerate() {
                assert_eq!(l.position(s), Some(i));
                assert_eq!(l.get_slot(s), l.get(i));
            }
        }
    }
}

// ------------------------------------------------------------------ regions

struct Item {
    label: Signal<String>,
    n: Signal<u32>,
    h: f32,
    c: u32,
}
impl Item {
    fn new(label: String, n: u32, h: f32, c: u32) -> Item {
        Item { label: Signal::new(label), n: Signal::new(n), h, c }
    }
    fn copy(&self) -> Item {
        Item::new((*self.label).clone(), *self.n, self.h, self.c)
    }
}
impl View for Item {
    fn view(&self) -> impl Element<Self> + '_ {
        row((
            label(sig!(self.label)).font(12.0, 16.0),
            block(20.0, self.h).bg(rgb(self.c)),
            label(sig!(self.n).map(|n| format!("#{n}"))).font(10.0, 14.0),
        ))
        .gap(4.0)
        .px(3.0)
    }
}

struct Root {
    items: List<Item>,
    title: Signal<String>,
}
impl View for Root {
    fn view(&self) -> impl Element<Self> + '_ {
        column((label(sig!(self.title)), list(sig!(self.items)).gap(2.0).w(300.0), block(50.0, 10.0).bg(rgb(0xff0000))))
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
    if std::env::var_os("LIST_DEBUG").is_some() {
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            if x.2 != y.2 {
                eprintln!("  rec {i} kind {} depth {} ink {:?} fresh {:?}", x.0, x.1, x.2, y.2);
            }
        }
    }
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!(x.0 == y.0 && x.1 == y.1, "{ctx}: record {i} kind/depth {:?} != {:?}", (x.0, x.1), (y.0, y.1));
        assert!(x.2 == y.2, "{ctx}: record {i} (kind {}) ink {:?} != fresh {:?}", x.0, x.2, y.2);
        assert!(x.3 == y.3, "{ctx}: record {i} payload {:?} != fresh {:?}", x.3, y.3);
    }
}

fn app(root: Root) -> App<Root> {
    App::new(root, Config::new(400.0, 1600.0, 1.0, FONT.to_vec()).once(), false)
}
fn fresh(a: &App<Root>) -> App<Root> {
    let items = List::from_iter_in(a.owner.items.iter().map(Item::copy));
    let mut f = app(Root { items, title: Signal::new((*a.owner.title).clone()) });
    f.frame();
    f
}

fn random_item(r: &mut Rng, id: u64) -> Item {
    let words = ["alpha", "beta", "gamma", "delta", "epsilon", "z", "a much longer label here"];
    Item::new(format!("{} {id}", words[r.below(words.len())]), r.below(1000) as u32, 4.0 + r.below(30) as f32, 0x203040 + (r.below(0xff) as u32) * 0x10101)
}

#[test]
fn list_region_equals_fresh_mount_after_random_deltas() {
    for seed in 1..13u64 {
        let mut r = Rng(seed.wrapping_mul(0xA24B_AED4_963E_E407));
        let mut id = 0;
        let mut items = List::new();
        for _ in 0..r.below(12) {
            id += 1;
            items.push(random_item(&mut r, id));
        }
        let mut a = app(Root { items, title: Signal::new("list".into()) });
        a.frame();
        let mut kept: Vec<Item> = Vec::new();
        for turn in 0..60 {
            let nops = 1 + r.below(5);
            let mut ops = Vec::new();
            for _ in 0..nops {
                ops.push((r.below(13), r.next(), r.next()));
            }
            let mut rr = Rng(r.next() | 1);
            if std::env::var_os("LIST_DEBUG").is_some() {
                eprintln!("seed {seed} turn {turn} n {} ops {:?}", a.owner.items.len(), ops.iter().map(|o| o.0).collect::<Vec<_>>());
            }
            a.turn(|v| {
                for &(k, x, y) in &ops {
                    let n = v.items.len();
                    let (x, y) = (x as usize, y as usize);
                    match k {
                        0 | 1 => {
                            id += 1;
                            v.items.insert(x % (n + 1), random_item(&mut rr, id));
                        }
                        2 if n > 0 => {
                            let it = v.items.remove(x % n);
                            if kept.len() < 4 {
                                kept.push(it);
                            }
                        }
                        3 if n > 1 => v.items.move_to(x % n, y % n),
                        4 if n > 1 => v.items.swap(x % n, y % n),
                        5 => v.items.reverse(),
                        6 => v.items.sort_by_key(|i| (*i.n % 7, i.c)),
                        7 | 8 if n > 0 => {
                            let it = v.items.edit(x % n);
                            *it.n = (y % 5000) as u32;
                            if k == 8 {
                                it.label.set(format!("edited {}", y % 1000));
                            }
                        }
                        9 if !kept.is_empty() => {
                            // a taken item comes back (its old wire heads must not be followed)
                            let it = kept.pop().unwrap();
                            v.items.insert(x % (n + 1), it);
                        }
                        10 if n > 3 => v.items.truncate(n - 2),
                        11 if n > 0 => {
                            let t = (y % 3) as u64;
                            v.items.retain(|i| (*i.n as u64) % 3 != t);
                        }
                        12 => {
                            v.title.set(format!("title {}", y % 100));
                        }
                        _ => {
                            id += 1;
                            v.items.push(random_item(&mut rr, id));
                        }
                    }
                }
            });
            a.frame();
            let f = fresh(&a);
            same(&format!("seed {seed} turn {turn}"), &dump(&a), &dump(&f));
        }
    }
}

#[test]
fn list_log_replays_to_the_final_order() {
    // the log a region replays reproduces the list's order (checked inside the region in debug builds too)
    struct R {
        items: List<Item>,
    }
    impl View for R {
        fn view(&self) -> impl Element<Self> + '_ {
            list(sig!(self.items))
        }
    }
    let mut a = App::new(R { items: List::new() }, Config::new(200.0, 200.0, 1.0, FONT.to_vec()).once(), false);
    let mut r = Rng(7);
    for t in 0..50 {
        let before: Vec<u32> = a.owner.items.slots().map(|s| s.index()).collect();
        a.write(|v| {
            for _ in 0..6 {
                let n = v.items.len();
                match r.below(5) {
                    0 | 1 => {
                        v.items.insert(r.below(n + 1), Item::new("x".into(), 0, 5.0, 0));
                    }
                    2 if n > 0 => {
                        v.items.remove(r.below(n));
                    }
                    3 if n > 1 => v.items.move_to(r.below(n), r.below(n)),
                    _ => v.items.reverse(),
                }
            }
        });
        let (ops, perm): (Vec<Op>, Vec<u32>) = {
            let (o, p) = bough::list::log_of(&a.owner.items);
            (o.to_vec(), p.to_vec())
        };
        let mut v = before;
        for op in ops {
            replay(&mut v, op, &perm, |s| s);
        }
        let after: Vec<u32> = a.owner.items.slots().map(|s| s.index()).collect();
        assert_eq!(v, after, "turn {t}");
        a.flush();
    }
}
