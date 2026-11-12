//! Derived geometry settles in rank order (round 7). A clip's absolute rect is a function of its own rect,
//! the origin of the space it is in and its parent clip's rect; whatever order the writes of a turn come in,
//! the retained clips must be what a fresh mount at the same values computes. The first case is the bench
//! lane's repro (`comparisons/bough-workloads/tests/defects.rs` on r7/bench `0dd77a69b2`): two nested
//! `clip_by`s and an `offset`, all wired to one signal, whose wires ran newest first, so the inner clip was
//! cut against the outer clip's old rect and against the offset's old origin.
use bough::layout::prelude::*;
use bough::raw::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

/// Every record with its ink and the rect of its clip: what the kernels cut it to.
fn dump(app: &App<impl View>) -> Vec<(u8, u16, [i16; 4], [i16; 4])> {
    app.rt.bough.records().iter().map(|r| (r.kind, r.depth, r.ink.0, app.rt.bough.clip_rect(r.clip).0)).collect()
}

fn cfg() -> Config {
    Config::new(400.0, 300.0, 1.0, FONT.to_vec()).once()
}

struct Lcg(u64);
impl Lcg {
    fn f(&mut self, lo: f32, hi: f32) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        lo + (hi - lo) * ((self.0 >> 40) as f32 / (1u64 << 24) as f32)
    }
}

// ---------------------------------------------------------------- one signal: clip in clip around an offset
struct Nested {
    pan: Signal<(f32, f32)>,
}
impl View for Nested {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, 400.0, 300.0).bg(rgb(0xffffff)),
            clip_by(
                sig!(self.pan),
                |p: &(f32, f32)| [20.0 + p.0, 20.0 + p.1, 200.0, 200.0],
                clip_by(
                    sig!(self.pan),
                    |p: &(f32, f32)| [60.0 + p.0, 60.0 + p.1, 100.0, 100.0],
                    offset(sig!(self.pan), quad(0.0, 0.0, 400.0, 300.0).bg(rgb(0x2f6bff))),
                ),
            ),
        )
    }
}

#[test]
fn nested_clip_by_retained_equals_fresh() {
    let mut a = App::new(Nested { pan: Signal::new((0.0, 0.0)) }, cfg(), false);
    a.frame();
    let mut r = Lcg(7);
    let mut pans = vec![(30.0f32, 0.0f32), (30.0, 40.0), (-15.0, 10.0)];
    pans.extend((0..60).map(|_| (r.f(-80.0, 120.0).round(), r.f(-80.0, 120.0).round())));
    for (k, p) in pans.into_iter().enumerate() {
        a.turn(|v| {
            v.pan.set(p);
        });
        a.frame();
        let f = App::new(Nested { pan: Signal::new(p) }, cfg(), false);
        assert_eq!(dump(&a), dump(&f), "step {k}, pan {p:?}: retained != fresh mount");
    }
}

// ---------------------------------------------------------------- separate signals: who moved is not who is cut
struct Split {
    outer: Signal<(f32, f32)>,
    inner: Signal<(f32, f32)>,
    off: Signal<(f32, f32)>,
}
impl View for Split {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, 400.0, 300.0).bg(rgb(0xffffff)),
            // a clip inside an offset: the offset's write moves the space the clip's rect is in
            offset(
                sig!(self.off),
                clip_by(sig!(self.inner), |p: &(f32, f32)| [10.0 + p.0, 10.0 + p.1, 120.0, 90.0], quad(0.0, 0.0, 300.0, 200.0).bg(rgb(0x2f6bff))),
            ),
            // a clip inside a clip: the outer's write changes what the inner is cut to
            clip_by(
                sig!(self.outer),
                |p: &(f32, f32)| [200.0 + p.0, 100.0 + p.1, 150.0, 150.0],
                clip_by(sig!(self.inner), |p: &(f32, f32)| [220.0 + p.0, 90.0 + p.1, 100.0, 100.0], quad(180.0, 80.0, 220.0, 220.0).bg(rgb(0xff8800))),
            ),
        )
    }
}

#[test]
fn a_clip_follows_its_space_and_its_parent_clip() {
    let mut a = App::new(Split { outer: Signal::new((0.0, 0.0)), inner: Signal::new((0.0, 0.0)), off: Signal::new((0.0, 0.0)) }, cfg(), false);
    a.frame();
    let mut r = Lcg(99);
    let mut cur = [(0.0f32, 0.0f32); 3];
    for step in 0..150 {
        // one, two or all three written in a turn, in varying order
        let which = (r.f(1.0, 7.99)) as u32;
        for k in 0..3 {
            if which >> k & 1 != 0 {
                cur[k] = (r.f(-60.0, 60.0).round(), r.f(-60.0, 60.0).round());
            }
        }
        let c = cur;
        a.turn(|v| {
            if step % 2 == 0 {
                v.inner.set(c[1]);
                v.outer.set(c[0]);
                v.off.set(c[2]);
            } else {
                v.off.set(c[2]);
                v.outer.set(c[0]);
                v.inner.set(c[1]);
            }
        });
        a.frame();
        let f = App::new(Split { outer: Signal::new(c[0]), inner: Signal::new(c[1]), off: Signal::new(c[2]) }, cfg(), false);
        assert_eq!(dump(&a), dump(&f), "step {step}, values {c:?}: retained != fresh mount");
    }
}

// ---------------------------------------------------------------- a wired clip inside an anchor
struct Anchored {
    pan: Signal<(f32, f32)>,
}
impl View for Anchored {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            block(100.0, 50.0).bg(rgb(0x808080)),
            anchor(200.0, 100.0, clip_by(sig!(self.pan), |p: &(f32, f32)| [10.0 + p.0, 10.0 + p.1, 80.0, 60.0], quad(0.0, 0.0, 200.0, 100.0).bg(rgb(0x2f6bff)))),
        ))
        .w(300.0)
    }
}

#[test]
fn a_wired_clip_in_an_anchor_is_placed_with_it() {
    let mut a = App::new(Anchored { pan: Signal::new((0.0, 0.0)) }, cfg(), false);
    a.frame();
    // the anchor's box is below the 50-point block: its content's clip is there from the first frame
    let clipped: Vec<[i16; 4]> = a.rt.bough.records().iter().filter(|r| r.clip != 0).map(|r| a.rt.bough.clip_rect(r.clip).0).collect();
    assert_eq!(clipped, vec![[10, 60, -90, -120]], "the clip is at the anchor's origin before any write");
    for p in [(5.0f32, 5.0f32), (0.0, 0.0), (-20.0, 30.0)] {
        a.turn(|v| {
            v.pan.set(p);
        });
        a.frame();
        let f = App::new(Anchored { pan: Signal::new(p) }, cfg(), false);
        assert_eq!(dump(&a), dump(&f), "pan {p:?}: retained != fresh mount");
    }
}

// ---------------------------------------------------------------- a constant clip under a moving parent
struct Static {
    pan: Signal<(f32, f32)>,
}
impl View for Static {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, 400.0, 300.0).bg(rgb(0xffffff)),
            // constant in its space, but the space moves
            offset(sig!(self.pan), clip(10.0, 10.0, 100.0, 80.0, quad(0.0, 0.0, 300.0, 200.0).bg(rgb(0x2f6bff)))),
            // constant, but cut to a clip that moves
            clip_by(sig!(self.pan), |p: &(f32, f32)| [200.0 + p.0, 100.0 + p.1, 150.0, 150.0], clip(220.0, 90.0, 100.0, 100.0, quad(180.0, 80.0, 220.0, 220.0).bg(rgb(0xff8800)))),
        )
    }
}

#[test]
fn a_constant_clip_follows_its_space_and_its_parent_clip() {
    let mut a = App::new(Static { pan: Signal::new((0.0, 0.0)) }, cfg(), false);
    a.frame();
    for p in [(30.0f32, 0.0f32), (30.0, 40.0), (-15.0, 10.0), (0.0, 0.0), (55.0, -35.0)] {
        a.turn(|v| {
            v.pan.set(p);
        });
        a.frame();
        let f = App::new(Static { pan: Signal::new(p) }, cfg(), false);
        assert_eq!(dump(&a), dump(&f), "pan {p:?}: retained != fresh mount");
    }
}

// ---------------------------------------------------------------- gaps: rows, ports, tooltips
fn clipped_row(_: &Rows, i: usize, m: &mut Mounter<'_, Rows>) {
    // a constant clip inside a row: it must move with the row
    m.mount(clip(4.0, 4.0, 120.0, 12.0, quad(0.0, 0.0, 300.0, 24.0).bg(rgb(if i % 2 == 0 { 0x2f6bff } else { 0xff8800 }))));
}
fn vis(scroll: f32) -> std::ops::Range<usize> {
    let first = (scroll / 24.0) as usize;
    first..first + 9
}
struct Rows {
    scroll: Signal<f32>,
    pan: Signal<(f32, f32)>,
    cut: Signal<(f32, f32)>,
}
impl View for Rows {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, 400.0, 300.0).bg(rgb(0xffffff)),
            // a virtual list whose rows hold a constant clip
            virtual_list(sig!(self.scroll), 24.0, [10.0, 10.0, 150.0, 120.0], vis, clipped_row),
            // a virtual list inside an offset: its port and its rows move with the offset
            offset(sig!(self.pan), virtual_list(sig!(self.scroll), 24.0, [200.0, 10.0, 150.0, 96.0], vis, clipped_row)),
            // a virtual list inside a wired clip: its port is cut to the clip
            clip_by(sig!(self.cut), |p: &(f32, f32)| [20.0 + p.0, 150.0 + p.1, 200.0, 100.0], virtual_list(sig!(self.scroll), 24.0, [10.0, 160.0, 300.0, 120.0], vis, clipped_row)),
        )
    }
}

#[test]
fn clips_in_rows_and_ports_follow_scrolls_offsets_and_parent_clips() {
    let mk = |s: f32, p: (f32, f32), c: (f32, f32)| App::new(Rows { scroll: Signal::new(s), pan: Signal::new(p), cut: Signal::new(c) }, cfg(), false);
    let mut a = mk(0.0, (0.0, 0.0), (0.0, 0.0));
    a.frame();
    let mut r = Lcg(5);
    let (mut s, mut p, mut c) = (0.0f32, (0.0f32, 0.0f32), (0.0f32, 0.0f32));
    for step in 0..120 {
        match step % 3 {
            0 => s = r.f(0.0, 600.0).round(),
            1 => p = (r.f(-40.0, 40.0).round(), r.f(-40.0, 40.0).round()),
            _ => c = (r.f(-40.0, 40.0).round(), r.f(-40.0, 40.0).round()),
        }
        a.turn(|v| {
            v.scroll.set(s);
            v.pan.set(p);
            v.cut.set(c);
        });
        a.frame();
        let f = mk(s, p, c);
        assert_eq!(dump(&a), dump(&f), "step {step}: scroll {s}, pan {p:?}, cut {c:?}: retained != fresh mount");
    }
}

struct Tipped {
    pan: Signal<(f32, f32)>,
}
impl View for Tipped {
    fn view(&self) -> impl Element<Self> + '_ {
        (quad(0.0, 0.0, 400.0, 300.0).bg(rgb(0xffffff)), offset(sig!(self.pan), tooltip(quad(20.0, 20.0, 60.0, 24.0).bg(rgb(0x2f6bff)), "a tooltip", [20.0, 50.0, 120.0, 28.0])))
    }
}

#[test]
fn a_tooltip_under_an_offset_shows_where_its_trigger_is() {
    let mk = |p: (f32, f32)| App::new(Tipped { pan: Signal::new(p) }, cfg(), false);
    let mut a = mk((0.0, 0.0));
    a.frame();
    for p in [(30.0f32, 0.0f32), (100.0, 80.0), (-10.0, 5.0)] {
        a.pointer_leave();
        a.turn(|v| {
            v.pan.set(p);
        });
        a.pointer_move(30.0 + p.0, 30.0 + p.1);
        a.frame();
        let mut f = mk(p);
        f.frame();
        f.pointer_move(30.0 + p.0, 30.0 + p.1);
        f.frame();
        let shown: Vec<[i16; 4]> = a.rt.bough.records().iter().filter(|r| r.clip != 0).map(|r| a.rt.bough.clip_rect(r.clip).0).collect();
        assert!(shown.iter().any(|c| c[0] != i16::MAX), "pan {p:?}: the tooltip is showing");
        assert_eq!(dump(&a), dump(&f), "pan {p:?}: retained != fresh mount");
    }
}

// ---------------------------------------------------------------- a keyed list's port under moving parents
struct Cell {
    c: u32,
}
impl View for Cell {
    fn view(&self) -> impl Element<Self> + '_ {
        block(120.0, 20.0).bg(rgb(self.c))
    }
}
struct Listed {
    a: List<Cell>,
    b: List<Cell>,
    scroll: Signal<f32>,
    pan: Signal<(f32, f32)>,
    cut: Signal<(f32, f32)>,
}
impl View for Listed {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, 400.0, 300.0).bg(rgb(0xffffff)),
            offset(sig!(self.pan), column(list(sig!(self.a)).w(140.0).h(90.0).scroll(sig!(self.scroll)).estimate(20.0))),
            clip_by(sig!(self.cut), |p: &(f32, f32)| [180.0 + p.0, 20.0 + p.1, 120.0, 60.0], at(170.0, 10.0, column(list(sig!(self.b)).w(140.0).h(90.0).scroll(sig!(self.scroll)).estimate(20.0)))),
        )
    }
}

#[test]
fn a_keyed_list_port_follows_an_offset_and_a_parent_clip() {
    let cells = || {
        let mut l = List::new();
        for i in 0..40u32 {
            l.push(Cell { c: 0x203040 + i * 0x050301 });
        }
        l
    };
    let mk = |s: f32, p: (f32, f32), c: (f32, f32)| {
        let mut a = App::new(Listed { a: cells(), b: cells(), scroll: Signal::new(s), pan: Signal::new(p), cut: Signal::new(c) }, cfg(), false);
        a.frame();
        a
    };
    let mut a = mk(0.0, (0.0, 0.0), (0.0, 0.0));
    let mut r = Lcg(11);
    let (mut s, mut p, mut c) = (0.0f32, (0.0f32, 0.0f32), (0.0f32, 0.0f32));
    let mut before_turn = dump(&a);
    for step in 0..90 {
        match step % 3 {
            0 => p = (r.f(-40.0, 40.0).round(), r.f(-40.0, 40.0).round()),
            1 => c = (r.f(-40.0, 40.0).round(), r.f(-40.0, 40.0).round()),
            _ => s = r.f(0.0, 500.0).round(),
        }
        a.turn(|v| {
            v.scroll.set(s);
            v.pan.set(p);
            v.cut.set(c);
        });
        let before = dump(&a);
        a.frame();
        let f = mk(s, p, c);
        assert_eq!(dump(&a), dump(&f), "step {step}: scroll {s}, pan {p:?}, cut {c:?}: retained != fresh mount");
        // the case is live: each of the three inputs changes what the records are cut to
        if step % 3 != 2 {
            assert_ne!(dump(&a), before_turn, "step {step}: the write changed nothing");
        }
        before_turn = dump(&a);
        let _ = before;
    }
}

// ---------------------------------------------------------------- scroll views under moving parents
struct Scrolled {
    pan: Signal<(f32, f32)>,
    s1: Signal<f32>,
    s2: Signal<f32>,
}
impl View for Scrolled {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, 400.0, 300.0).bg(rgb(0xffffff)),
            // a raw scroll view inside an offset, with a constant clip and a nested scroll view in its content
            offset(
                sig!(self.pan),
                scroll_y(
                    sig!(self.s1),
                    [20.0, 20.0, 150.0, 100.0],
                    400.0,
                    (
                        quad(0.0, 0.0, 150.0, 500.0).bg(rgb(0xeeeeee)),
                        clip(10.0, 40.0, 100.0, 30.0, quad(0.0, 30.0, 150.0, 60.0).bg(rgb(0x2f6bff))),
                        scroll_y(sig!(self.s2), [10.0, 120.0, 120.0, 60.0], 200.0, quad(0.0, 0.0, 120.0, 260.0).bg(rgb(0xff8800))),
                    ),
                ),
            ),
            // a laid-out scroll box inside an offset
            offset(sig!(self.pan), at(200.0, 150.0, column((block(120.0, 60.0).bg(rgb(0x00aa55)), block(120.0, 60.0).bg(rgb(0xaa0055)), block(120.0, 60.0).bg(rgb(0x5500aa)))).w(140.0).h(100.0).scroll(sig!(self.s2)))),
        )
    }
}

#[test]
fn a_scroll_view_under_an_offset_keeps_its_place() {
    let mk = |p: (f32, f32), s1: f32, s2: f32| {
        let mut a = App::new(Scrolled { pan: Signal::new(p), s1: Signal::new(s1), s2: Signal::new(s2) }, cfg(), false);
        a.frame();
        a
    };
    let mut a = mk((0.0, 0.0), 0.0, 0.0);
    let mut r = Lcg(31);
    let (mut p, mut s1, mut s2) = ((0.0f32, 0.0f32), 0.0f32, 0.0f32);
    for step in 0..120 {
        match step % 3 {
            0 => p = (r.f(-50.0, 50.0).round(), r.f(-50.0, 50.0).round()),
            1 => s1 = r.f(0.0, 380.0).round(),
            _ => s2 = r.f(0.0, 70.0).round(),
        }
        a.turn(|v| {
            v.pan.set(p);
            v.s1.set(s1);
            v.s2.set(s2);
        });
        a.frame();
        let f = mk(p, s1, s2);
        assert_eq!(dump(&a), dump(&f), "step {step}: pan {p:?}, scrolls {s1} {s2}: retained != fresh mount");
    }
}
