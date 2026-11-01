//! Layout semantics and incremental correctness (no GPU: the pool is heap memory).
//!
//! * hand-computed flex / percentage / absolute / wrapped-text geometry at scale 1 (points = px);
//! * an edit sequence on reactive text (wrapping, growing, shrinking) after which, at every step,
//!   (a) the engine's incremental layout equals a cold layout of a copy of its tree, bit for bit,
//!   (b) the retained records (ink) and instances equal a fresh mount of the view at the same values.
use bough::layout::prelude::*;
use bough::prelude::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

fn cfg(w: f32, h: f32) -> Config {
    Config { width_pt: w, height_pt: h, scale: 1.0, font: FONT.to_vec(), ring: 3 }
}

/// Every record's (kind, depth, ink) and its instances, in preorder. The atlas position of a glyph depends on
/// the order glyphs were first rasterized, so `uv`'s x, y are zeroed (its size stays): pixels are compared by
/// the pixel tests in comparisons/bough-vlist.
type Dump = Vec<(u8, u16, [i16; 4], Vec<bough_text::atlas::Instance>)>;
fn dump(app: &App<impl View>) -> Dump {
    app.rt
        .bough
        .records()
        .iter()
        .map(|r| {
            let run = app.rt.runs[r.payload as usize];
            let ins = unsafe { std::slice::from_raw_parts(app.rt.inst_ptr(run.first), run.len as usize) }
                .iter()
                .map(|i| {
                    let mut i = *i;
                    i.uv[0] = 0;
                    i.uv[1] = 0;
                    i
                })
                .collect();
            (r.kind, r.depth, r.ink.0, ins)
        })
        .collect()
}

fn same(step: usize, a: &Dump, b: &Dump) {
    assert_eq!(a.len(), b.len(), "step {step}: record count");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!(x.0 == y.0 && x.1 == y.1, "step {step}: record {i} kind/depth");
        assert!(x.2 == y.2, "step {step}: record {i} (kind {}) ink {:?} != fresh {:?}", x.0, x.2, y.2);
        assert!(x.3.len() == y.3.len(), "step {step}: record {i} instance count {} != {}", x.3.len(), y.3.len());
        for (k, (p, q)) in x.3.iter().zip(&y.3).enumerate() {
            assert!(p == q, "step {step}: record {i} instance {k}: {p:?} != {q:?}");
        }
    }
}

/// rect (x, y, w, h) of every layout node of the group at `root`, depth first.
fn rects(app: &App<impl View>, root: u32) -> Vec<(u32, (f32, f32, f32, f32))> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        out.push((n, app.rt.lay.eng.rect(n)));
        app.rt.lay.eng.children_into(&app.rt.bough, n, &mut stack);
    }
    out.sort_by_key(|e| e.0);
    out
}

// ------------------------------------------------------------------ geometry

struct Flex;
impl View for Flex {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            // 0: a row of fixed boxes with gap and padding
            row((block(30.0, 20.0).bg(rgb(0xff0000)), block(40.0, 20.0).bg(rgb(0x00ff00)), block(10.0, 20.0).bg(rgb(0x0000ff)))).gap(5.0).p(7.0),
            // 1: grow: two flexible boxes share the remaining width (100 - 2*0)
            row((block(20.0, 10.0).bg(rgb(0xffff00)).w(20.0).grow(1.0), block(20.0, 10.0).bg(rgb(0x00ffff)).grow(3.0).w(20.0))).w(100.0),
            // 2: percentages and centre alignment
            row((block(0.0, 10.0).w_pct(0.5).bg(rgb(0xff00ff)), block(10.0, 10.0).bg(rgb(0x808080)))).w(200.0).h(40.0).items(Align::Center),
            // 3: space-between
            row((block(10.0, 10.0).bg(rgb(0x111111)), block(10.0, 10.0).bg(rgb(0x222222)), block(10.0, 10.0).bg(rgb(0x333333)))).w(100.0).justify(Justify::SpaceBetween),
            // 4: absolute inside a sized box (no flow effect)
            column((block(10.0, 10.0).bg(rgb(0x444444)).abs(30.0, 12.0),)).w(60.0).h(30.0).relative(),
        ))
        .w(300.0)
        .h(300.0)
        .bg(rgb(0x202020))
    }
}

#[test]
fn flex_geometry_matches_hand_computation() {
    let app = App::new(Flex, cfg(300.0, 300.0), false);
    let root = app.rt.lay.first_root();
    let r = rects(&app, root);
    let by_depth_first: Vec<(f32, f32, f32, f32)> = r.iter().map(|e| e.1).collect();
    // root 300x300 at 0,0; row0: padding 7 + (30+5+40+5+10)=90 wide -> 104 x 34 (as flex item, stretch: 300)
    let rect = |i: usize| by_depth_first[i];
    assert_eq!(rect(0), (0.0, 0.0, 300.0, 300.0));
    // row 0 is the first child: x 0, y 0, stretched to 300 wide, 20 + 2*7 = 34 tall
    let row0 = app.rt.lay_child_rect(root, 0);
    assert_eq!(row0, (0.0, 0.0, 300.0, 34.0));
    let b0 = app.rt.lay_child_rect(app.rt.lay_child(root, 0), 0);
    let b1 = app.rt.lay_child_rect(app.rt.lay_child(root, 0), 1);
    let b2 = app.rt.lay_child_rect(app.rt.lay_child(root, 0), 2);
    assert_eq!(b0, (7.0, 7.0, 30.0, 20.0));
    assert_eq!(b1, (42.0, 7.0, 40.0, 20.0));
    assert_eq!(b2, (87.0, 7.0, 10.0, 20.0));
    // row 1: 100 wide, two items w=20 grow 1:3 -> free 60 -> 35 and 65
    let row1 = app.rt.lay_child(root, 1);
    assert_eq!(app.rt.lay_child_rect(root, 1), (0.0, 34.0, 100.0, 10.0));
    assert_eq!(app.rt.lay_child_rect(row1, 0), (0.0, 0.0, 35.0, 10.0));
    assert_eq!(app.rt.lay_child_rect(row1, 1), (35.0, 0.0, 65.0, 10.0));
    // row 2: 200 x 40, a 50% (100) wide box centred vertically, then a 10 box
    let row2 = app.rt.lay_child(root, 2);
    assert_eq!(app.rt.lay_child_rect(root, 2), (0.0, 44.0, 200.0, 40.0));
    assert_eq!(app.rt.lay_child_rect(row2, 0), (0.0, 15.0, 100.0, 10.0));
    assert_eq!(app.rt.lay_child_rect(row2, 1), (100.0, 15.0, 10.0, 10.0));
    // row 3: space-between over 100: boxes at 0, 45, 90
    let row3 = app.rt.lay_child(root, 3);
    assert_eq!(app.rt.lay_child_rect(row3, 0).0, 0.0);
    assert_eq!(app.rt.lay_child_rect(row3, 1).0, 45.0);
    assert_eq!(app.rt.lay_child_rect(row3, 2).0, 90.0);
    // absolute child: at its inset inside the 60 x 30 box
    let col4 = app.rt.lay_child(root, 4);
    assert_eq!(app.rt.lay_child_rect(col4, 0), (30.0, 12.0, 10.0, 10.0));
}

// ------------------------------------------------------------------ text

struct Wrap {
    a: Signal<String>,
    b: Signal<String>,
    c: Signal<String>,
    w: f32,
}
const A: &str = "the build passed on main after we moved layout into its own pass and a list of rows now measures each height once text wraps at the width so scrolling stays smooth";
impl View for Wrap {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            para(&self.a).font(14.0, 20.0).w(self.w).color(rgb(0xe5edf7)),
            row((label(&self.b).font(12.0, 16.0).color(rgb(0x5eb1ff)), para(&self.c).font(12.0, 16.0).grow(1.0).color(rgb(0x8b98ab)))).gap(6.0),
            block(40.0, 12.0).bg(rgb(0x2b3a52)),
        ))
        .w(self.w + 20.0)
        .p(10.0)
        .gap(4.0)
        .bg(rgb(0x161c26))
    }
}

fn wrap_app(a: &str, b: &str, c: &str) -> App<Wrap> {
    App::new(Wrap { a: Signal::new(a.to_string()), b: Signal::new(b.to_string()), c: Signal::new(c.to_string()), w: 300.0 }, cfg(400.0, 700.0), false)
}

#[test]
fn paragraph_wraps_and_measures() {
    let app = wrap_app(A, "user-0001", "short");
    let root = app.rt.lay.first_root();
    // 300 px wide paragraph at 14 px: 160 chars ~ 1,000+ px of text -> several lines of 20
    let (_, _, w, h) = app.rt.lay_child_rect(root, 0);
    assert_eq!(w, 300.0);
    assert!(h >= 60.0 && h <= 200.0 && h % 20.0 == 0.0, "para height {h}");
    // the same text at the same width laid out by bough-text's own wrap helper
    let mut shaper_run = bough_text::GlyphRun::default();
    let sh = bough_text::Shaper::new(bough_text::SimpleScript::build(FONT, 0).unwrap());
    assert!(sh.shape_into(A, 14.0, &mut shaper_run));
    let mut starts = Vec::new();
    let max_units = (300.0 / 14.0 * shaper_run.upem as f32).floor() as i32;
    let lines = bough_text::wrap::wrap_greedy(&shaper_run, A.as_bytes(), max_units, &mut starts);
    assert_eq!(h, lines as f32 * 20.0);
}

#[test]
fn incremental_edits_equal_cold_layout_and_fresh_mount() {
    let mut app = wrap_app(A, "user-0001", "a short reply");
    let mut st = 0x9E3779B97F4A7C15u64;
    let mut rnd = move || {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        st
    };
    let words = ["the", "build", "passed", "on", "main", "after", "we", "moved", "layout", "wraps", "everyone", "smooth", "a"];
    let sentence = |r: &mut dyn FnMut() -> u64, n: usize| -> String {
        (0..n).map(|_| words[(r() % words.len() as u64) as usize]).collect::<Vec<_>>().join(" ")
    };
    for step in 0..60 {
        let which = rnd() % 3;
        let n = (rnd() % 40) as usize;
        let s = sentence(&mut rnd, n);
        app.turn(|v| match which {
            0 => {
                v.a.set(s.clone());
            }
            1 => {
                v.b.set(s.chars().take(14).collect());
            }
            _ => {
                v.c.set(s.clone());
            }
        });
        app.frame();
        // (a) incremental == cold
        let root = app.rt.lay.first_root();
        let avail = cephal::Size { width: cephal::AvailableSpace::Definite(400.0), height: cephal::AvailableSpace::Definite(700.0) };
        let cold = app.rt.lay.eng.cold_rects(&app.rt.bough, root, avail);
        let now = rects(&app, root);
        assert_eq!(now.len(), cold.len());
        for (a, b) in now.iter().zip(&cold) {
            assert_eq!(a.1, b.1, "step {step}: node {} incremental {:?} != cold {:?}", a.0, a.1, b.1);
        }
        // (b) retained == fresh mount at the same signal values
        let fresh = wrap_app(&app.owner.a, &app.owner.b, &app.owner.c);
        same(step, &dump(&app), &dump(&fresh));
    }
}
