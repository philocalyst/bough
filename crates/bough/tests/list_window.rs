//! The one list mechanism with a viewport (virtual list regions) against simple oracles (no GPU: heap region;
//! the pixel gates are in comparisons/bough-suite and comparisons/bough-vlist):
//!  1. the prefix index (`HeightIndex`) agrees with a naive vector under random point updates, splices and
//!     suffix rebuilds: `prefix`, `row_at`, `height`, `total`;
//!  2. after random splices, moves, swaps, reversals, sorts, edits that change row heights (inside and
//!     outside the window), scrolls, jumps and sub-row steps, the mounted window equals what a fresh mount of
//!     the same list at the same scroll offset with the same row heights shows, record for record (kind,
//!     depth, ink, drawn payload), and the window is exactly the rows meeting the viewport ± overscan;
//!  3. anchoring: rows measured above the viewport move the offset, not the first visible row;
//!  4. the same random deltas with moves animating: every frame of the motion keeps the records equal to
//!     their own spaces, and the settled state equals a fresh mount.
use bough::layout::prelude::*;
use bough::mlist::HeightIndex;
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

// ------------------------------------------------------------------ the index

fn naive_row_at(v: &[i32], y: i64) -> usize {
    if v.is_empty() {
        return 0;
    }
    let (mut acc, mut k) = (0i64, 0usize);
    // the last k with prefix(k) <= y
    while k < v.len() && acc + v[k] as i64 <= y.max(0) {
        acc += v[k] as i64;
        k += 1;
    }
    k.min(v.len() - 1)
}

#[test]
fn height_index_matches_naive_under_splices() {
    for seed in 1..24u64 {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let n0 = r.below(700);
        let est = 1 + r.below(40) as i32;
        let mut idx = HeightIndex::new(n0, est);
        let mut v = vec![est; n0];
        for step in 0..1500 {
            let n = v.len();
            match r.below(7) {
                0 | 1 if n > 0 => {
                    let k = r.below(n);
                    let h = r.below(300) as i32 * (r.below(8) != 0) as i32;
                    assert_eq!(idx.set(k, h), h - v[k]);
                    v[k] = h;
                }
                2 => {
                    let (at, cnt, h) = (r.below(n + 1), r.below(150), r.below(90) as i32);
                    idx.insert(at, cnt, h);
                    v.splice(at..at, std::iter::repeat_n(h, cnt));
                }
                3 if n > 0 => {
                    let at = r.below(n);
                    let cnt = r.below(n - at + 1);
                    idx.remove(at, cnt);
                    v.drain(at..at + cnt);
                }
                4 => {
                    let from = r.below(n + 1);
                    let len = from + r.below(200);
                    let salt = r.next();
                    let f = |p: usize| ((p as u64).wrapping_mul(salt) >> 40) as i32 % 120;
                    idx.assign_from(from, len, f);
                    v.truncate(from);
                    v.extend((from..len).map(f));
                }
                _ => {}
            }
            assert_eq!(idx.len(), v.len(), "seed {seed} step {step}");
            assert_eq!(idx.total(), v.iter().map(|&x| x as i64).sum::<i64>(), "seed {seed} step {step}: total");
            for _ in 0..6 {
                let q = r.below(v.len() + 1);
                assert_eq!(idx.prefix(q), v[..q].iter().map(|&x| x as i64).sum::<i64>(), "seed {seed} step {step}: prefix({q})");
                let y = r.below(idx.total() as usize + 60) as i64 - 20;
                assert_eq!(idx.row_at(y), naive_row_at(&v, y), "seed {seed} step {step}: row_at({y})");
                if !v.is_empty() {
                    let k = r.below(v.len());
                    assert_eq!(idx.height(k), v[k]);
                }
            }
        }
    }
}

// ------------------------------------------------------------------ virtual regions

struct Item {
    text: Signal<String>,
    c: u32,
}
impl Item {
    fn new(text: String, c: u32) -> Item {
        Item { text: Signal::new(text), c }
    }
    fn copy(&self) -> Item {
        Item::new((*self.text).clone(), self.c)
    }
}
impl View for Item {
    fn view(&self) -> impl Element<Self> + '_ {
        column(para(sig!(self.text)).font(12.0, 16.0).w(150.0)).py(2.0).px(4.0).bg(rgb(self.c))
    }
}

/// A list of wrapping rows (1-4 lines) in a 300-point viewport between a header and a footer.
struct Root {
    items: List<Item>,
    scroll: Signal<f32>,
    animate: bool,
}
impl View for Root {
    fn view(&self) -> impl Element<Self> + '_ {
        let l = list(sig!(self.items)).w(200.0).h(300.0).scroll(sig!(self.scroll)).estimate(30.0).gap(2.0).px(3.0).py(5.0).bg(rgb(0x101820));
        let l = if self.animate { l.animate_moves() } else { l };
        column((label("head").font(14.0, 18.0), l, block(50.0, 10.0).bg(rgb(0xff0000)))).w(220.0)
    }
}

fn words(r: &mut Rng, id: u64) -> String {
    let w = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "a much longer word", "x"];
    let n = 1 + r.below(14);
    let mut s = format!("{id}");
    for _ in 0..n {
        s.push(' ');
        s.push_str(w[r.below(w.len())]);
    }
    s
}
fn random_item(r: &mut Rng, id: u64) -> Item {
    Item::new(words(r, id), 0x203040 + (r.below(0xff) as u32) * 0x10101)
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

fn app(root: Root) -> App<Root> {
    App::new(root, Config::new(400.0, 600.0, 1.0, FONT.to_vec()).once(), false)
}

/// A fresh mount of the same items, given the retained region's heights, scrolled to its offset.
fn fresh(a: &App<Root>) -> App<Root> {
    let items = List::from_iter_in(a.owner.items.iter().map(Item::copy));
    let mut f = app(Root { items, scroll: Signal::new(0.0), animate: a.owner.animate });
    f.frame();
    let h = a.rt.lists.regs()[0].idx.heights().to_vec();
    f.rt.list_set_heights(0, &h);
    let s = a.rt.list_scroll_pt(0);
    f.turn(|v| {
        v.scroll.set(s);
    });
    let at = f.rt.list_scroll_pt(0);
    f.owner.scroll.set_silent(at);
    f.frame();
    f
}

/// The window is the rows meeting the viewport (`y - pad .. y - pad + 300`), widened by the overscan (2).
fn check_window(ctx: &str, a: &App<Root>) {
    let r = &a.rt.lists.regs()[0];
    let n = r.idx.len();
    let s = r.s;
    let c = s - 5;
    let want = if n == 0 {
        0..0
    } else {
        let (f, l) = (r.idx.row_at(c.max(0)), r.idx.row_at((c + 300 - 1).max(0)));
        f.saturating_sub(2)..(l + 3).min(n)
    };
    assert_eq!(a.rt.list_window(0), want, "{ctx}: window");
    assert!(r.rows.iter().all(|x| x.mounted), "{ctx}: a hole survived placement");
    let max = (r.idx.total() - if n > 0 { 2 } else { 0 } + 10 - 300).max(0);
    assert!(s >= 0 && s <= max, "{ctx}: offset {s} outside 0..={max}");
}

fn random_turn(a: &mut App<Root>, r: &mut Rng, id: &mut u64) {
    let nops = 1 + r.below(4);
    let mut ops = Vec::new();
    for _ in 0..nops {
        ops.push((r.below(12), r.next(), r.next()));
    }
    let mut rr = Rng(r.next() | 1);
    let content = a.rt.list_content_height_pt(0);
    let row_off: Vec<f32> = (0..a.owner.items.len().min(400)).map(|p| a.rt.list_row_offset_pt(0, p)).collect();
    a.turn(|v| {
        for &(k, x, y) in &ops {
            let n = v.items.len();
            let (x, y) = (x as usize, y as usize);
            match k {
                0 | 1 => {
                    *id += 1;
                    v.items.insert(x % (n + 1), random_item(&mut rr, *id));
                }
                2 if n > 0 => {
                    v.items.remove(x % n);
                }
                3 if n > 1 => v.items.move_to(x % n, y % n),
                4 if n > 1 => v.items.swap(x % n, y % n),
                5 => v.items.reverse(),
                6 => v.items.sort_by_key(|i| i.c),
                7 | 8 if n > 0 => {
                    let t = words(&mut rr, y as u64 % 1000);
                    v.items.edit(x % n).text.set(t);
                }
                9 => {
                    // scroll anywhere (sometimes past the end)
                    v.scroll.set((y % (content as usize + 400)) as f32);
                }
                10 if !row_off.is_empty() => {
                    // a jump: a row at the top by its (estimated) offset
                    v.scroll.set(row_off[x % row_off.len()]);
                }
                _ => {
                    let d = (y % 200) as f32 - 100.0;
                    let at = (*v.scroll + d).max(0.0);
                    v.scroll.set(at);
                }
            }
        }
    });
    let at = a.rt.list_scroll_pt(0);
    a.owner.scroll.set_silent(at);
}

#[test]
fn virtual_region_equals_fresh_mount_after_random_deltas() {
    for seed in 1..11u64 {
        let mut r = Rng(seed.wrapping_mul(0xA24B_AED4_963E_E407) | 1);
        let mut id = 0;
        let mut items = List::new();
        for _ in 0..r.below(120) {
            id += 1;
            items.push(random_item(&mut r, id));
        }
        let mut a = app(Root { items, scroll: Signal::new(0.0), animate: false });
        a.frame();
        for turn in 0..50 {
            random_turn(&mut a, &mut r, &mut id);
            a.frame();
            let ctx = format!("seed {seed} turn {turn}");
            check_window(&ctx, &a);
            let f = fresh(&a);
            assert_eq!(f.rt.lists.regs()[0].s, a.rt.lists.regs()[0].s, "{ctx}: fresh offset");
            assert_eq!(f.rt.list_window(0), a.rt.list_window(0), "{ctx}: fresh window");
            same(&ctx, &dump(&a), &dump(&f));
        }
    }
}

#[test]
fn anchoring_holds_the_first_visible_row() {
    // rows of 1-4 lines under a 30-point estimate: scrolling down measures rows; jumping back up measures rows
    // above the viewport, which must move the offset, not the first visible row
    let mut r = Rng(99);
    let items = List::from_iter_in((0..3000u64).map(|i| random_item(&mut r, i)));
    let mut a = app(Root { items, scroll: Signal::new(0.0), animate: false });
    a.frame();
    let target = 2000usize;
    let off = a.rt.list_row_offset_pt(0, target);
    a.turn(|v| {
        v.scroll.set(off);
    });
    let at = a.rt.list_scroll_pt(0);
    a.owner.scroll.set_silent(at);
    // the jump converged inside its turn: the target row is at the top
    assert_eq!(a.rt.list_row_offset_pt(0, target), at, "jump lands on its row");
    let (h0, s0) = (a.rt.lists.regs()[0].stats.height_changes, at);
    for step in 0..60 {
        // up through unmeasured rows, by less than a row
        let first = {
            let rg = &a.rt.lists.regs()[0];
            rg.idx.row_at(rg.s - 5)
        };
        let before = a.rt.list_row_offset_pt(0, first) - a.rt.list_scroll_pt(0);
        let s = (*a.owner.scroll - 7.0).max(0.0);
        a.turn(|v| {
            v.scroll.set(s);
        });
        let at = a.rt.list_scroll_pt(0);
        a.owner.scroll.set_silent(at);
        // the row that was first visible moved by the scroll step exactly (whatever was measured above it)
        let after = a.rt.list_row_offset_pt(0, first) - at;
        assert_eq!(after, before + 7.0, "step {step}: first visible row {first} moved by {} instead of the step", after - before);
    }
    // rows above the viewport were measured on the way up, and the offset absorbed their corrections
    assert!(a.rt.lists.regs()[0].stats.height_changes >= h0 + 5, "rows above were measured");
    assert!(a.rt.list_scroll_pt(0) != s0 - 420.0, "the offset was corrected by anchoring");
}

/// Every mounted row's records follow its space: its first record (the row's background quad at local
/// (0, 0)) has its ink at the same offset from the space in every row, whatever the row's motion, except by
/// the displacement a moving row out of sight has not given its ink yet (`pend`): such a row is outside the
/// viewport both where it is drawn and where its ink is.
fn ink_follows_space(ctx: &str, a: &App<Root>) {
    // records: the root column, the header label, the list's container, then the rows
    let mut pos = 3;
    let mut off = None;
    let vp = a.rt.lists.regs()[0].vpr.to_xyxy();
    for (k, row) in a.rt.lists.regs()[0].rows.iter().enumerate() {
        let sp = a.rt.glyphs.spaces[row.space as usize];
        let [_, y0, _, y1] = a.rt.bough.ink_at(pos).to_xyxy();
        let o = y0 - (sp[1] - row.pend);
        assert_eq!(*off.get_or_insert(o), o, "{ctx}: row {k}'s records do not follow its space (cur {}, pend {})", row.cur, row.pend);
        if row.pend != 0 {
            let out = |a: i32, b: i32| b <= vp[1] || a >= vp[3];
            assert!(out(y0, y1) && out(y0 + row.pend, y1 + row.pend), "{ctx}: row {k} is in sight with its ink behind (pend {})", row.pend);
        }
        pos += row.nrec as usize;
    }
}

#[test]
fn animated_region_settles_to_a_fresh_mount() {
    for seed in 1..6u64 {
        let mut r = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
        let mut id = 0;
        let mut items = List::new();
        for _ in 0..40 + r.below(60) {
            id += 1;
            items.push(random_item(&mut r, id));
        }
        let mut a = app(Root { items, scroll: Signal::new(0.0), animate: true });
        a.frame();
        for turn in 0..30 {
            random_turn(&mut a, &mut r, &mut id);
            a.frame();
            // a few frames of the motion (sometimes interrupted by the next change)
            let frames = r.below(80);
            for _ in 0..frames {
                if !a.animating() {
                    break;
                }
                a.tick(1.0 / 120.0);
                a.frame();
                ink_follows_space(&format!("seed {seed} turn {turn}"), &a);
            }
            let ctx = format!("seed {seed} turn {turn}");
            check_window(&ctx, &a);
        }
        while a.animating() {
            a.tick(1.0 / 120.0);
            a.frame();
        }
        assert!(a.rt.lists.regs()[0].rows.iter().all(|x| x.cur == 0), "settled");
        let f = fresh(&a);
        same(&format!("seed {seed} settled"), &dump(&a), &dump(&f));
    }
}
