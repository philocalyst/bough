//! Round 6 (lane memory): what a plain `each` of N rows in a scroll box costs, by N. One process per N
//! (footprint is per process): records mounted, instructions to mount (`App::new`, calling thread),
//! instructions per scroll frame (a 2 pt step: turn + frame, headless, slope over two repeat counts),
//! instructions of a jump to the middle, and the footprint growth of the mount. Default `Caps`.
//!
//!     cargo run --release -p bough --example dormant_scale -- 100 10000 1000000
//!     EAGER=1 ...   every item mounted (the path before dormant runs; needs raised caps: RAISED=1)
use bough::prelude::*;
use bough::raw::{App, Caps, Config};
use bough_metal::perf::{snap, tinstr};

struct Rows {
    n: usize,
    y: Signal<f32>,
    selected: usize,
}
impl View for Rows {
    fn view(&self) -> impl Element<Self> + '_ {
        let line = move |i: usize| {
            let bg = if i == self.selected { rgb(0x2f6bff) } else if i % 2 == 0 { rgb(0xf5f5f5) } else { rgb(0xe6e6e6) };
            row((text(i).w(64.0), text("Row label"))).w(960.0).h(32.0).bg(bg)
        };
        column((text("rows"), div().w(1000.0).h(700.0).scroll(sig!(self.y)).child(each((0..self.n).map(line))))).bg(rgb(0xffffff))
    }
}

fn one(n: usize) {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut cfg = Config::new(1280.0, 800.0, 1.0, font);
    cfg.eager_parts = std::env::var_os("EAGER").is_some();
    if std::env::var_os("RAISED").is_some() {
        cfg.caps = Caps { instances: 1 << 17, pages: 1 << 13, glyphs: 1 << 20, runs: 1 << 17, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
    }
    let f0 = snap().footprint;
    let t0 = tinstr();
    let mut a = App::new(Rows { n, y: Signal::new(0.0), selected: usize::MAX }, cfg, false);
    let mount = tinstr() - t0;
    a.frame();
    let foot = snap().footprint.saturating_sub(f0);
    let records = a.record_count();
    let mut y = 0.0f32;
    let mut scroll = |a: &mut App<Rows>, reps: u32| {
        let t0 = tinstr();
        for _ in 0..reps {
            y += 2.0;
            a.turn(|r| {
                r.y.set(y);
            });
            a.frame();
        }
        tinstr() - t0
    };
    scroll(&mut a, 64);
    let (lo, hi) = (scroll(&mut a, 128), scroll(&mut a, 384));
    let per_frame = (hi - lo) / 256;
    let t0 = tinstr();
    let mid = (n as f32 * 16.0).max(0.0);
    a.turn(|r| {
        r.y.set(mid);
    });
    a.frame();
    let jump = tinstr() - t0;
    // a plain field changed (a selection): one re-description
    let mut sel = |a: &mut App<Rows>, reps: usize| {
        let t0 = tinstr();
        for k in 0..reps {
            a.turn(|r| r.selected = k);
        }
        tinstr() - t0
    };
    sel(&mut a, 8);
    let (lo, hi) = (sel(&mut a, 16), sel(&mut a, 48));
    let sel_turn = (hi - lo) / 32;
    // the same write aimed at a row that is dormant (the last row): the re-description never reaches it
    let last = n - 1;
    let mut dorm = |a: &mut App<Rows>, reps: usize| {
        let t0 = tinstr();
        for k in 0..reps {
            a.turn(|r| r.selected = if k % 2 == 0 { last } else { usize::MAX });
        }
        tinstr() - t0
    };
    dorm(&mut a, 8);
    let (lo, hi) = (dorm(&mut a, 16), dorm(&mut a, 48));
    let dorm_turn = (hi - lo) / 32;
    // wake: a jump to somewhere else realises a screenful; instructions per realised item (description,
    // records, instances, shaping, layout, the walk and the scans included)
    let r0 = a.rt.dorm.stats.realised;
    let t0 = tinstr();
    a.turn(|r| {
        r.y.set(mid * 0.5 + 3.0);
    });
    let wake = (tinstr() - t0) / (a.rt.dorm.stats.realised - r0).max(1);
    let woke = a.rt.dorm.stats.realised - r0;
    let st = a.rt.dorm.stats;
    println!("{n},{records},{mount},{per_frame},{jump},{sel_turn},{dorm_turn},{wake},{woke},{},{},{}", foot / 1024, a.record_count(), st.unconverged);
}

fn main() {
    eprintln!("bytes: entry (DynItem) {}, part state (Dorm) {}", std::mem::size_of::<bough::describe::DynItem>(), std::mem::size_of::<bough::dormant::Dorm>());
    println!("rows,records_at_mount,mount_instr,scroll_frame_instr,jump_instr,select_turn_instr,dormant_row_turn_instr,wake_instr_per_item,items_woken,mount_footprint_kib,records_after,unconverged");
    for n in std::env::args().skip(1).filter_map(|s| s.parse::<usize>().ok()) {
        one(n);
    }
}
