//! Wire segments (round 7): the wires of one cell whose appliers commute (a colour) are kept per kind of
//! operand and run as one loop; a projection that captures nothing is evaluated once per run for all of them
//! and, when its value did not change, none run. The observable state must be what a fresh mount at the
//! same values gives, whatever was mounted, written and unmounted on the way.
use bough::layout::prelude::*;
use bough::raw::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;
const N: usize = 40;

struct Grid {
    n: usize,
    /// theme bit 0x100 | hovered column
    look: Signal<u32>,
    vals: [Signal<u32>; N],
}

fn ink(l: u32) -> Color {
    if l & 0x100 != 0 { rgb(0xe6e9ef) } else { rgb(0x1d2129) }
}
fn fill(l: u32, col: u32, row: u32) -> Color {
    match (l & 0x100 != 0, l & 0xff == col, row % 2 == 0) {
        (false, true, _) => rgb(0xdbe8ff),
        (false, false, true) => rgb(0xffffff),
        (false, false, false) => rgb(0xf6f7f9),
        (true, true, _) => rgb(0x26406b),
        (true, false, true) => rgb(0x15171c),
        (true, false, false) => rgb(0x1b1e24),
    }
}

impl View for Grid {
    fn view(&self) -> impl Element<Self> + '_ {
        let cell = move |i: usize| {
            let (row, col) = ((i / 4) as u32, (i % 4) as u32);
            block(60.0, 18.0)
                // a projection with captures: one evaluation per member
                .bg(sig!(self.look).map(move |l: &u32| fill(*l, col, row)))
                // a projection without: one evaluation per run
                .child(label(sig!(self.vals).at(i).map(|v: &u32| format!("{v}"))).font(11.0, 14.0).color(sig!(self.look).map(|l: &u32| ink(*l))))
        };
        column(each((0..self.n).map(cell))).w(300.0)
    }
}

type Item7 = (u8, [f32; 4], u32, u32, f32, f32, [u16; 2]);
fn dump(app: &App<impl View>) -> Vec<(u8, u16, [i16; 4], Vec<Item7>)> {
    app.rt.bough.records().iter().map(|r| (r.kind, r.depth, r.ink.0, if r.ink.is_empty() { Vec::new() } else { app.rt.expand_payload(r.payload) })).collect()
}

fn grid(n: usize, look: u32) -> Grid {
    Grid { n, look: Signal::new(look), vals: std::array::from_fn(|i| Signal::new(i as u32)) }
}

#[test]
fn segments_equal_a_fresh_mount_through_hover_theme_and_row_churn() {
    // re-describing: the row count changes, so members mount and unmount between runs
    let cfg = || Config::new(320.0, 800.0, 1.0, FONT.to_vec());
    let mut a = App::new(grid(12, 0xff), cfg(), false);
    a.frame();
    let mut r = 0x9e37_79b9_7f4a_7c15u64;
    let mut rnd = move |n: u32| {
        r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((r >> 33) as u32) % n
    };
    let (mut n, mut look) = (12usize, 0xffu32);
    for step in 0..200 {
        match rnd(4) {
            0 => look = (look & 0x100) | [0, 1, 2, 3, 0xff][rnd(5) as usize], // hover
            1 => look ^= 0x100,                                              // theme
            2 => n = 1 + rnd(N as u32 - 1) as usize,                         // rows come and go
            _ => {}                                                          // a turn that writes the same value
        }
        a.turn(|v| {
            v.n = n;
            v.look.set(look);
        });
        a.frame();
        let mut f = App::new(grid(n, look), cfg(), false);
        f.frame();
        assert_eq!(dump(&a), dump(&f), "step {step}: n {n}, look {look:#x}: retained != fresh mount");
    }
}

#[test]
fn a_captureless_projection_runs_once_and_not_at_all_when_unchanged() {
    let cfg = || Config::new(320.0, 800.0, 1.0, FONT.to_vec()).once();
    let mut a = App::new(grid(N, 0xff), cfg(), false);
    a.frame();
    // first write after the mount: both segments run (the cached value is not trusted across a mount)
    a.turn(|v| drop(v.look.set(0x01)));
    a.frame();
    let c0 = a.rt.ctr.wires_run;
    // a hover that does not change the theme: the fill segment runs (one dispatch), the text colour
    // segment's one evaluation finds its value unchanged; nothing else is dispatched
    a.turn(|v| drop(v.look.set(0x02)));
    assert_eq!(a.rt.ctr.wires_run - c0, 2, "two segment dispatches for {N} cells");
    a.frame();
    let mut f = App::new(grid(N, 0x02), cfg(), false);
    f.frame();
    assert_eq!(dump(&a), dump(&f));
    // the theme: both change everything
    a.turn(|v| drop(v.look.set(0x102)));
    a.frame();
    let mut f = App::new(grid(N, 0x102), cfg(), false);
    f.frame();
    assert_eq!(dump(&a), dump(&f));
}
