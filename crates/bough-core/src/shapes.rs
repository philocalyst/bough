//! Tree shapes resembling real UIs, as flat preorder specs both `Bough` and `ArenaTree` build from.

use crate::NRect;
use crate::arena::{ArenaTree, NodeId, Rect};
use crate::bough::{Bough, RecordInit};
use crate::rng::Rng;

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub depth: u16,
    /// Layout box, device px (x, y, w, h).
    pub b: [i32; 4],
    pub paints: bool,
    pub clips: bool,
    pub kind: u8,
}

impl Spec {
    pub fn ink(&self) -> NRect {
        if self.paints { NRect::new(self.b[0], self.b[1], self.b[0] + self.b[2], self.b[1] + self.b[3]) } else { NRect::EMPTY }
    }
    pub fn init(&self, payload: u32) -> RecordInit {
        RecordInit::new(self.depth, self.ink(), self.kind, payload)
    }
    pub fn rect(&self) -> Rect {
        Rect::new(self.b[0] as f32, self.b[1] as f32, self.b[2] as f32, self.b[3] as f32)
    }
}

pub struct ListInfo {
    /// Preorder index of the scroll container.
    pub container: usize,
    pub first_row: usize,
    pub rows: usize,
    pub row_len: usize,
    pub row_h: i32,
    /// Container box (x, y, w, h).
    pub area: [i32; 4],
}

pub struct Shape {
    pub name: &'static str,
    pub window: (i32, i32),
    pub specs: Vec<Spec>,
    /// Painting leaves (targets of scattered paint writes).
    pub leaves: Vec<usize>,
    /// A contiguous subtree of about 1,000 nodes, if the shape has one: (start, len).
    pub sub1k: Option<(usize, usize)>,
    pub list: Option<ListInfo>,
}

fn sp(depth: u16, x: i32, y: i32, w: i32, h: i32, paints: bool, kind: u8) -> Spec {
    Spec { depth, b: [x, y, w, h], paints, clips: false, kind }
}

fn finish(name: &'static str, window: (i32, i32), specs: Vec<Spec>, sub1k: Option<(usize, usize)>, list: Option<ListInfo>) -> Shape {
    let n = specs.len();
    // Painting leaves that are on screen (damage outside the window is dropped, so writes there
    // would measure nothing).
    let on_screen = |s: &Spec| s.b[0] < window.0 && s.b[1] < window.1 && s.b[0] + s.b[2] > 0 && s.b[1] + s.b[3] > 0;
    let leaves = (0..n).filter(|&i| specs[i].paints && on_screen(&specs[i]) && (i + 1 == n || specs[i + 1].depth <= specs[i].depth)).collect();
    Shape { name, window, specs, leaves, sub1k, list }
}

/// ~1,500 nodes: header chrome + two panes of rows of 12-node cards.
pub fn dashboard() -> Shape {
    let mut v = Vec::new();
    v.push(sp(0, 0, 0, 1280, 800, true, 0));
    // header: 21 nodes
    v.push(sp(1, 0, 0, 1280, 48, true, 1));
    v.push(sp(2, 8, 8, 32, 32, true, 2));
    for i in 0..8 {
        v.push(sp(2, 60 + i * 90, 8, 80, 32, true, 3));
        v.push(sp(3, 66 + i * 90, 16, 60, 14, true, 4));
    }
    v.push(sp(2, 900, 8, 300, 32, true, 5));
    v.push(sp(3, 908, 16, 120, 14, true, 4));
    v.push(sp(2, 1230, 8, 32, 32, true, 2));
    let card = |v: &mut Vec<Spec>, x: i32, y: i32, depth: u16| {
        v.push(sp(depth, x, y, 100, 68, true, 6)); // card
        v.push(sp(depth + 1, x, y, 100, 20, false, 7)); // header container
        v.push(sp(depth + 2, x + 6, y + 6, 60, 12, true, 4)); // title
        v.push(sp(depth + 2, x + 80, y + 6, 12, 12, true, 2)); // icon
        v.push(sp(depth + 1, x + 6, y + 22, 50, 20, true, 4)); // value
        v.push(sp(depth + 1, x + 6, y + 46, 88, 18, false, 7)); // bars container
        for i in 0..3 {
            v.push(sp(depth + 2, x + 6, y + 46 + i * 6, 88, 4, true, 8)); // track
            v.push(sp(depth + 3, x + 6, y + 46 + i * 6, 20 + 22 * i, 4, true, 9)); // fill
        }
    };
    let pane = |v: &mut Vec<Spec>, rows: std::ops::Range<i32>| -> (usize, usize) {
        let start = v.len();
        v.push(sp(1, 0, 56 + rows.start * 74, 1280, rows.len() as i32 * 74, false, 7));
        for r in rows {
            v.push(sp(2, 0, 56 + r * 74, 1280, 74, false, 7));
            for c in 0..12 {
                card(v, 4 + c * 106, 56 + r * 74, 3);
            }
        }
        (start, v.len() - start)
    };
    let a = pane(&mut v, 0..7);
    pane(&mut v, 7..10);
    finish("dashboard", (1280, 800), v, Some(a), None)
}

/// 20 mounted rows of 8 nodes under a clipping scroll container inside ~300 nodes of chrome.
pub fn list() -> Shape {
    let mut v = Vec::new();
    v.push(sp(0, 0, 0, 1280, 800, true, 0));
    // header: 61 nodes
    v.push(sp(1, 0, 0, 1280, 48, true, 1));
    for i in 0..30 {
        v.push(sp(2, 8 + i * 40, 8, 36, 32, true, 3));
        v.push(sp(3, 12 + i * 40, 16, 28, 14, true, 4));
    }
    // sidebar: 121 nodes
    v.push(sp(1, 0, 48, 220, 704, true, 1));
    for i in 0..40 {
        v.push(sp(2, 4, 52 + i * 17, 212, 16, true, 3));
        v.push(sp(3, 8, 54 + i * 17, 12, 12, true, 2));
        v.push(sp(3, 26, 54 + i * 17, 150, 12, true, 4));
    }
    // list container + rows
    let container = v.len();
    let mut c = sp(1, 220, 48, 1060, 704, true, 10);
    c.clips = true;
    v.push(c);
    let first_row = v.len();
    for r in 0..20 {
        v.extend(list_row(r * 32 + 48));
    }
    // footer: 115 nodes
    v.push(sp(1, 0, 752, 1280, 48, true, 1));
    for i in 0..38 {
        v.push(sp(2, 4 + i * 33, 756, 30, 40, true, 3));
        v.push(sp(3, 8 + i * 33, 760, 10, 10, true, 2));
        v.push(sp(3, 8 + i * 33, 776, 22, 10, true, 4));
    }
    let list = ListInfo { container, first_row, rows: 20, row_len: 8, row_h: 32, area: [220, 48, 1060, 704] };
    let mut shape = finish("list", (1280, 800), v, None, Some(list));
    // Rows churn in the scroll benchmark, so they are not stable write/query targets.
    let rows = first_row..first_row + 160;
    shape.leaves.retain(|i| !rows.contains(i));
    shape
}

/// One 8-node list row at vertical offset `y` (depth 2 root).
pub fn list_row(y: i32) -> Vec<Spec> {
    vec![
        sp(2, 224, y, 1052, 30, true, 6),
        sp(3, 230, y + 3, 24, 24, true, 2),
        sp(3, 262, y + 4, 300, 12, true, 4),
        sp(3, 262, y + 18, 200, 9, true, 4),
        sp(3, 900, y + 5, 70, 20, true, 8),
        sp(4, 906, y + 9, 50, 12, true, 4),
        sp(3, 1000, y + 7, 16, 16, true, 3),
        sp(3, 1100, y + 9, 60, 12, true, 4),
    ]
}

/// ~100k nodes: 100 groups of 999 leaves on a tall canvas.
pub fn flat100k() -> Shape {
    let mut v = Vec::with_capacity(100_001);
    v.push(sp(0, 0, 0, 1280, 31_500, false, 0));
    let mut leaf = 0i32;
    for g in 0..100 {
        let first_row = leaf / 32;
        v.push(sp(1, 0, first_row * 10, 1280, 320, false, 7));
        for _ in 0..999 {
            let (col, row) = (leaf % 32, leaf / 32);
            v.push(sp(2, col * 40, row * 10, 36, 8, true, 4));
            leaf += 1;
        }
        let _ = g;
    }
    finish("flat100k", (1280, 800), v, Some((1, 1000)), None)
}

/// Depth-40 nesting, three leaves per level.
pub fn deep40() -> Shape {
    let mut v = Vec::new();
    let mut leaves_after: Vec<Spec> = Vec::new();
    let _ = &mut leaves_after;
    // preorder: C0, a0, C1, a1, ..., C39, a39, then for each level upward: b, c
    let level_box = |i: i32| (4 * i, 4 * i, 1280 - 8 * i, 800 - 8 * i);
    for i in 0..40i32 {
        let (x, y, w, h) = level_box(i);
        v.push(sp(i as u16, x, y, w, h, true, 6));
        v.push(sp(i as u16 + 1, x + 2, y + 2, 30, 10, true, 4));
    }
    for i in (0..40i32).rev() {
        let (x, y, w, h) = level_box(i);
        v.push(sp(i as u16 + 1, x + w - 40, y + 2, 30, 10, true, 4));
        v.push(sp(i as u16 + 1, x + 2, y + h - 14, 30, 10, true, 4));
    }
    // subtree of the level-1 container: everything except C0, its first leaf and its two trailing leaves
    let n = v.len();
    finish("deep40", (1280, 800), v, Some((2, n - 4)), None)
}

pub fn all() -> Vec<Shape> {
    vec![dashboard(), list(), flat100k(), deep40()]
}

// -------------------------------------------------------------------------------- builders

pub fn build_bough(shape: &Shape) -> Bough {
    let mut b = Bough::new(shape.window.0, shape.window.1);
    let recs: Vec<RecordInit> = shape.specs.iter().enumerate().map(|(i, s)| s.init(i as u32)).collect();
    // mount in 4096-record chunks, like a view tree streaming in
    let mut at = 0;
    for chunk in recs.chunks(4096) {
        b.insert_run(at, chunk);
        at += chunk.len();
    }
    b.next_epoch();
    b
}

/// Build the arena tree; returns it with the NodeId of every spec (by index).
pub fn build_arena(shape: &Shape) -> (ArenaTree, Vec<NodeId>) {
    let mut t = ArenaTree::new(shape.window.0 as f32, shape.window.1 as f32);
    let mut ids: Vec<NodeId> = Vec::with_capacity(shape.specs.len());
    let mut stack: Vec<NodeId> = Vec::new();
    for (i, s) in shape.specs.iter().enumerate() {
        stack.truncate(s.depth as usize);
        let parent = stack.last().copied();
        let id = t.add(parent, None, s.rect(), s.paints, s.clips, i as u32);
        stack.push(id);
        ids.push(id);
    }
    t.end_frame();
    (t, ids)
}

/// Deterministic scatter targets.
pub fn scatter(shape: &Shape, n: usize, seed: u64) -> Vec<usize> {
    let mut rng = Rng::new(seed);
    (0..n).map(|_| shape.leaves[rng.usize_below(shape.leaves.len())]).collect()
}
