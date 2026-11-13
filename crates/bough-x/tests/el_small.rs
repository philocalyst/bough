//! The element layer on a small view (a painted root, a header, a scrolled list of rows with a hover handler, a
//! transitioning outline), held to the oracles: retained == full redraw == immediate, in both turn modes.
use bough_x::el::App;
use bough_x::el::prelude::*;
use bough_x::soft::{Immediate, Soft};

const W: i32 = 640;
const H: i32 = 480;
const ROW_H: f32 = 20.0;

struct Small {
    n: usize,
    look: Signal<u32>,
    sel: Signal<u32>,
    y: Signal<f32>,
    vals: Vec<f32>,
}
fn shade(look: u32, r: u32) -> Color {
    rgb(if look & 0xff == r { 0x99bbff } else if look & 0x100 != 0 { 0x202428 + (r & 1) * 0x080808 } else { 0xf0f0f0 - (r & 1) * 0x080808 })
}
const FMT: NumFmt = NumFmt::new("", Some(2), "");
impl View for Small {
    fn view(&self) -> impl Element<Self> + '_ {
        let line = move |r: usize| {
            let k = r as u32;
            row((
                div().w(60.0).h(ROW_H).bg(sig!(self.look).map(move |l: &u32| shade(*l, k))).on_hover(move |s: &mut Small, e: &Ev| {
                    let l = (*s.look & !0xff) | if e.on { k } else { 0xff };
                    s.look.set(l);
                }),
                text(FMT.of(self.vals[r])).font(12.0, ROW_H).color(sig!(self.look).map(|l: &u32| rgb(if l & 0x100 != 0 { 0xffffff } else { 0x101010 }))),
            ))
        };
        let outline = block(60.0, ROW_H).border(2.0, rgb(0x2f6bff)).abs(0.0, 0.0).translate(sig!(self.sel).map(|i: &u32| (0.0, *i as f32 * ROW_H)).transition(Curve::out(0.2)));
        column((
            div().h(30.0).bg(sig!(self.look).map(|l: &u32| rgb(if l & 0x100 != 0 { 0x303030 } else { 0xd0d0d0 }))).child(text("small").font(12.0, 30.0)),
            column((column(each((0..self.n).map(line))), outline)).grow(1.0).scroll(sig!(self.y)),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(0x808080))
    }
}

fn check(tag: &str, step: usize, app: &mut App<Small, Soft>, retained: &mut Soft) {
    let world = &mut app.world;
    retained.repair(&world.tiles, |s| world.paint(s));
    world.tiles.clear();
    let mut full = Soft::new(W, H);
    world.paint(&mut full);
    let diff = |a: &[u32], b: &[u32]| a.iter().zip(b).filter(|(x, y)| x != y).count();
    assert!(retained.px == full.px, "{tag} step {step}: retained != full redraw ({} pixels)", diff(&retained.px, &full.px));
    let mut now = Soft::new(W, H);
    bough_x::sink::View::view(&app.v, &mut Immediate::new(&mut now, &world.shown));
    assert!(full.px == now.px, "{tag} step {step}: stores != immediate ({} pixels)", diff(&full.px, &now.px));
}

fn script(once: bool) {
    let n = 5_000;
    let v = Small { n, look: Signal::new(0xff), sel: Signal::new(0), y: Signal::new(0.0), vals: (0..n).map(|i| i as f32 * 1.25).collect() };
    let mut app = App::<Small, Soft>::new(v, W, H, 1.0);
    app.once = once;
    let mut retained = Soft::new(W, H);
    let tag = if once { "once" } else { "redescribe" };
    check(tag, 0, &mut app, &mut retained);
    let (records, _, _) = app.world.root.stats();
    assert!(records < 80, "{records} records exist for 5,000 rows: exposure is not what is paid for");
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut rnd = |m: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % m
    };
    let mut t = 1u32;
    for step in 1..=250 {
        t += if rnd(4) == 0 { 500 } else { 16 + rnd(30) as u32 };
        app.frame(t);
        match rnd(6) {
            0 => app.turn(|s| {
                let l = *s.look ^ 0x100;
                s.look.set(l);
            }),
            1 => {
                let dy = rnd(900) as f32 - 300.0;
                app.turn(|s| {
                    let y = (*s.y + dy).clamp(0.0, 5_000.0 * ROW_H - 450.0);
                    s.y.set(y);
                })
            }
            2 => {
                let i = (*app.v.y / ROW_H) as u32 + rnd(20) as u32;
                app.turn(|s| {
                    s.sel.set(i);
                })
            }
            3 => app.pointer_move(30.0, 40.0 + rnd(400) as f32),
            4 => {
                // state the view reads that is neither a Signal nor in the block: found by re-description
                let r = (*app.v.y / ROW_H) as usize + rnd(20) as usize;
                app.turn(|s| s.vals[r] += 1.0);
                if once {
                    // mount-once has no way to see a plain field change: the author asks for the description
                    app.world.redescribe(&app.v);
                }
            }
            _ => {}
        }
        check(tag, step, &mut app, &mut retained);
    }
}
#[test]
fn small_once() {
    script(true);
}
#[test]
fn small_redescribing() {
    script(false);
}
