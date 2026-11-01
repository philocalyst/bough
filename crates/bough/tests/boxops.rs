//! Box operands (`opacity`, `translate`, `w_op`, `h_op`, a label's colour operand): after random writes the
//! retained records equal a fresh mount at the same values.
use bough::layout::prelude::*;
use bough::prelude::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct S {
    a: Signal<f32>,
    t: Signal<(f32, f32)>,
    w: Signal<f32>,
    h: Signal<f32>,
    c: Signal<u32>,
}
impl S {
    fn copy(&self) -> S {
        S { a: Signal::new(*self.a), t: Signal::new(*self.t), w: Signal::new(*self.w), h: Signal::new(*self.h), c: Signal::new(*self.c) }
    }
}
impl View for S {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            block(30.0, 10.0).bg(rgb(0x3366ff)).opacity(sig!(self.a)),
            block(20.0, 20.0).bg(rgb(0xff8800)).rounded(4.0).translate(sig!(self.t)),
            row((block(10.0, 10.0).bg(rgb(0x00ff00)).w_op(sig!(self.w)).h_op(sig!(self.h)), label("after").font(12.0, 16.0))).gap(4.0),
            label("colour").font(14.0, 18.0).color(sig!(self.c).map(|c| Color(*c | 0xff00_0000))),
            block(50.0, 6.0).bg(rgb(0x808080)),
        ))
        .gap(3.0)
        .w(200.0)
    }
}

type Item7 = (u8, [f32; 4], u32, u32, f32, f32, [u16; 2]);
fn dump(app: &App<impl View>) -> Vec<(u8, u16, [i16; 4], Vec<Item7>)> {
    app.rt.bough.records().iter().map(|r| (r.kind, r.depth, r.ink.0, if r.ink.is_empty() { Vec::new() } else { app.rt.expand_payload(r.payload) })).collect()
}

#[test]
fn box_operands_equal_fresh_mount() {
    let cfg = || Config::new(220.0, 200.0, 1.0, FONT.to_vec());
    let mut a = App::new(S { a: Signal::new(1.0), t: Signal::new((0.0, 0.0)), w: Signal::new(10.0), h: Signal::new(10.0), c: Signal::new(0xffffff) }, cfg(), false);
    a.frame();
    let mut r = 0x1234_5678_9abc_def1u64;
    for step in 0..120 {
        r ^= r << 13;
        r ^= r >> 7;
        r ^= r << 17;
        let x = r;
        a.turn(|v| match x % 5 {
            0 => *v.a = (x % 101) as f32 / 100.0,
            1 => *v.t = (((x >> 8) % 40) as f32 - 20.0, ((x >> 16) % 30) as f32 - 15.0),
            2 => *v.w = 4.0 + ((x >> 8) % 60) as f32,
            3 => *v.h = 4.0 + ((x >> 8) % 30) as f32,
            _ => *v.c = (x >> 8) as u32 & 0xffffff,
        });
        a.frame();
        let mut f = App::new(a.owner.copy(), cfg(), false);
        f.frame();
        assert!(dump(&a) == dump(&f), "step {step}: retained != fresh mount");
    }
}
