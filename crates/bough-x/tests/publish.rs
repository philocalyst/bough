//! The published half (threads lane): a frame and every other reader see whole publishes only, and the turn
//! thread never waits for them.
//!
//! * `a_frame_in_flight_is_not_disturbed`: turns after a publish change nothing a frame reads.
//! * `readers_never_see_a_torn_scene`: a thread reads the published half through `Ar::read` while the turn
//!   thread rewrites every row and publishes; every record it sees carries one stamp. The feature
//!   `mutant_torn` (the reader skips the epoch check) must fail this.
use bough_x::arena::{Ar, Half};
use bough_x::prelude::*;
use bough_x::soft::Soft;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const W: i32 = 640;
const H: i32 = 640;
const ROW_H: i32 = 16;
const SHAPE: [Rect; 2] = [Rect::new(0, 0, 320, ROW_H), Rect::new(320, 0, 320, ROW_H)];

struct Stamps {
    u: U,
    rows: Rel<u32>,
}
fn tint(s: u32) -> Color {
    Color(0xff00_0000 | (s.wrapping_mul(0x9E37_79B9) >> 8))
}
impl View for Stamps {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.scope(Space { clip: Rect::new(0, 0, W, H), base: (0, 0), scroll: NO_WORD }, |s| {
            s.list(&List { n: self.rows.len() as u32, row_h: ROW_H, x: 0, y: 0, shape: &SHAPE, edits: &self.rows.journal }, |r| {
                let v = *self.rows.get(r);
                (Fill(rec(v).map(tint)), Num::<0, _, _>(rec(v as i32), Kc::<0x101010>))
            });
        });
    }
}
impl Stamps {
    fn new() -> Stamps {
        Stamps { u: U::default(), rows: Rel::new(vec![0; 200]) }
    }
    /// Every row gets the stamp, with no note taken: the next re-description compares and replaces.
    fn stamp(&mut self, s: u32) {
        self.rows.items_mut().iter_mut().for_each(|r| *r = s);
    }
}

/// The first record word of every live record of every list column of half `h`, by raw reads: what an
/// inspector or an accessibility walk does. Tolerates torn bytes: everything read is bounded before use.
fn stamps_of(ar: Ar, h: Half, out: &mut Vec<u32>) {
    out.clear();
    let n = (ar.ncols_racy() as usize).min(bough_x::arena::MAX_COLS);
    for c in 0..n {
        // SAFETY: entries below `MAX_COLS` are inside the half; an entry is plain words and a `&'static Desc`
        // that is either null (never sealed: skipped) or a descriptor of this binary.
        unsafe {
            let e = h.entry(c);
            let (cap, nrec, pages, live) = ((*e).cap, (*e).nrec, (*e).pages, (*e).live);
            let desc = (&raw const (*e).desc).cast::<*const bough_x::store::Desc>().read();
            if desc.is_null() || cap < 64 || cap > 4096 || nrec > cap || (live as usize) + (cap as usize / 64) > bough_x::arena::MAX_LIVE {
                continue;
            }
            let (off, size, psize) = ((*desc).rec_off, (*desc).rec_size, (*desc).page.size());
            if size < 4 || !ar.in_half(pages, (nrec as usize).div_ceil(64) * psize) {
                continue;
            }
            for slot in 0..nrec {
                if h.live(live + slot / 64).read_volatile() >> (slot & 63) & 1 != 0 {
                    let p = h.at(pages).add((slot as usize >> 6) * psize + off + (slot as usize & 63) * size);
                    out.push((p as *const u32).read_volatile());
                }
            }
        }
    }
}

#[test]
fn a_frame_in_flight_is_not_disturbed() {
    let mut v = Stamps::new();
    let mut world = World::<Soft>::new(W, H, 4);
    world.mount(&v);
    for k in 1..50u32 {
        v.stamp(k);
        world.redescribe(&v);
        world.publish();
        let u = world.shown;
        let mut a = Soft::new(W, H);
        world.paint_front(&u, &mut a);
        // the frame is "in flight": three more turns run and are not published
        for j in 0..3 {
            v.stamp(1000 * k + j);
            world.redescribe(&v);
        }
        let mut b = Soft::new(W, H);
        world.paint_front(&u, &mut b);
        assert!(a.px == b.px, "turn {k}: a turn changed what a frame in flight reads");
        let mut w = Soft::new(W, H);
        world.paint(&mut w);
        assert!(w.px != a.px, "turn {k}: the working half did not move on");
    }
}

#[test]
fn readers_never_see_a_torn_scene() {
    let mut v = Stamps::new();
    let mut world = World::<Soft>::new(W, H, 4);
    world.mount(&v);
    world.publish();
    let ar = world.ar;
    let (stop, reads, retries, torn) = (AtomicBool::new(false), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0));
    let turns: u32 = std::env::var("TURNS").ok().and_then(|t| t.parse().ok()).unwrap_or(200_000);
    std::thread::scope(|sc| {
        sc.spawn(|| {
            let mut got = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                let Some((_, tries)) = ar.read(|h, _| stamps_of(ar, h, &mut got)) else { continue };
                reads.fetch_add(1, Ordering::Relaxed);
                retries.fetch_add(tries as u64, Ordering::Relaxed);
                assert!(got.len() >= 80, "the reader found {} records", got.len());
                if got.iter().any(|&s| s != got[0]) {
                    torn.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
        for k in 1..=turns {
            v.stamp(k);
            world.redescribe(&v);
            world.tiles.clear();
            world.publish();
        }
        stop.store(true, Ordering::Relaxed);
    });
    let (r, t, x) = (reads.load(Ordering::Relaxed), retries.load(Ordering::Relaxed), torn.load(Ordering::Relaxed));
    println!("turns {turns}, reads {r}, retries {t} ({:.3} per read), torn {x}", t as f64 / r.max(1) as f64);
    assert!(r > 100, "the reader barely ran ({r} reads)");
    assert!(x == 0, "{x} of {r} reads saw records of two publishes");
}
