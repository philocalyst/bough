//! The pixel gates of the app, headless, on any platform: the host's ring and the span raster driven exactly as
//! the Wayland loop drives them, against a model compositor.
//!
//! After every commit: the buffer committed == a full redraw of the state. At every latch: the compositor's
//! screen (which copies only the damage it was told, accumulated over the commits it did not show) == a full
//! redraw; with a ring of three or more that is the *newest* state (commit on input: no latch is stale).
//! Every eighth step also == a fresh mount of the same state.
//!
//! Mutants (features): `mutant_straddle` (a glyph across a damaged and an undamaged tile is blended twice),
//! `mutant_age` (a repair forgets what an older buffer missed), `mutant_superseded` (a commit that supersedes
//! an unlatched one forgets the other's damage). Each must fail this file.
use bough_guibench_x::cli::Ticker;
use bough_guibench_x::scene::{Model, Scene};
use bough_guibench_x::{rows, search, spec, spinner, table};
use bough_host::App;
use bough_host::event::{Event, Mods, Sym};
use bough_host::pace::Ring;
use bough_span::Spans;
use bough_x::prelude::World;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A compositor as fakeland is: it shows the newest commit at each latch, holds the buffer it shows until a
/// newer one is shown, releases an unlatched commit when another supersedes it, and copies only what it was
/// told is damaged.
struct Comp {
    w: usize,
    h: usize,
    screen: Vec<u32>,
    bufs: Vec<Vec<u32>>,
    held: Option<usize>,
    pending: Option<usize>,
    damage: Vec<(i32, i32, i32, i32)>,
    released: Vec<usize>,
}
impl Comp {
    fn new(w: i32, h: i32) -> Comp {
        Comp { w: w as usize, h: h as usize, screen: vec![0x00dead00; (w * h) as usize], bufs: Vec::new(), held: None, pending: None, damage: Vec::new(), released: Vec::new() }
    }
    fn buf(&mut self, i: usize) -> *mut u32 {
        while self.bufs.len() <= i {
            self.bufs.push(vec![0x00bad000; self.w * self.h]);
        }
        self.bufs[i].as_mut_ptr()
    }
    fn commit(&mut self, i: usize) {
        if let Some(p) = self.pending.replace(i) {
            assert_ne!(p, i, "a buffer the compositor holds was committed again");
            self.released.push(p);
        }
        assert_ne!(self.held, Some(i), "the buffer on screen was drawn into");
    }
    fn latch(&mut self) -> bool {
        let Some(p) = self.pending.take() else { return false };
        for &(x, y, w, h) in &self.damage {
            for row in y..y + h {
                let a = row as usize * self.w + x as usize;
                self.screen[a..a + w as usize].copy_from_slice(&self.bufs[p][a..a + w as usize]);
            }
        }
        self.damage.clear();
        if let Some(old) = self.held.replace(p) {
            self.released.push(old);
        }
        true
    }
}

fn differing(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// A fresh mount of the same state, painted whole.
fn fresh<V: Model>(s: &Scene<V>, ms: u32) -> Vec<u32> {
    let (w, h) = (s.geo.w, s.geo.h);
    let mut world = World::<Spans<'static>>::new(w, h, V::NWORDS);
    // the words the world owns (focus, press, pick) are part of what is shown
    world.shown.w[bough_expr::APP_WORDS..].copy_from_slice(&s.world.shown.w[bough_expr::APP_WORDS..]);
    world.mount(&s.v);
    world.tick(ms);
    let mut px = vec![0u32; (w * h) as usize];
    let img = if s.geo.k >= 2 { &bough_glyph::DEJAVU_SANS_28 } else { &bough_glyph::DEJAVU_SANS_14 };
    let mut sp = Spans::new(unsafe { core::slice::from_raw_parts_mut(px.as_mut_ptr(), px.len()) }, w as usize, w, h, None, img);
    // SAFETY: the pass borrows `px` for this block only; the world needs a `'static` backend type.
    let sp2: &mut Spans<'static> = unsafe { core::mem::transmute(&mut sp) };
    world.paint(sp2);
    px
}

struct Rig<V: Model> {
    s: Scene<V>,
    ring: Ring,
    comp: Comp,
    max: usize,
    ms: u32,
    /// A full redraw as of the last commit.
    at_commit: Vec<u32>,
    commits: u64,
    latches: u64,
    starved: u64,
    repaired: u64,
}

impl<V: Model> Rig<V> {
    fn new(v: V, w: i32, h: i32, k: i32, max: usize) -> Rig<V> {
        let mut s = Scene::new(v);
        s.resize(w, h, k);
        let mut r = Rig { s, ring: Ring::new(w, h, 1, max), comp: Comp::new(w, h), max, ms: 0, at_commit: Vec::new(), commits: 0, latches: 0, starved: 0, repaired: 0 };
        r.draw("mount");
        r
    }
    fn resize(&mut self, w: i32, h: i32, k: i32) {
        self.s.resize(w, h, k);
        self.ring = Ring::new(w, h, 1, self.max);
        self.comp = Comp::new(w, h);
        self.draw("resize");
    }
    /// What the loop does after applying input: settle, and commit whatever changed, now.
    fn draw(&mut self, what: &str) {
        let d = self.s.settle();
        if d.lo < d.hi {
            self.ring.damage(d);
            self.s.painted();
        }
        if !self.ring.owed() {
            return;
        }
        let Some(f) = self.ring.begin() else {
            self.starved += 1;
            return;
        };
        let px = self.comp.buf(f.buf);
        let stride = self.s.geo.w as usize;
        // SAFETY: the model compositor's buffer `f.buf` is the window's size and the ring says it is free.
        unsafe { self.s.paint(px, stride, if f.full { None } else { Some(self.ring.repair()) }) };
        self.repaired += self.s.last.px_filled;
        let comp = &mut self.comp;
        self.ring.superseded_mutant_if(comp.pending.is_some());
        self.ring.commit(f.buf, |x, y, w, h| comp.damage.push((x, y, w, h)));
        comp.commit(f.buf);
        for i in comp.released.drain(..) {
            self.ring.release(i);
        }
        self.commits += 1;
        let full = self.s.full();
        let n = differing(&self.comp.bufs[f.buf], &full);
        assert_eq!(n, 0, "after {what} (commit {}): the committed buffer differs from a full redraw in {n} pixels", self.commits);
        self.at_commit = full;
    }
    fn latch(&mut self, what: &str) {
        if !self.comp.latch() {
            return;
        }
        for i in self.comp.released.drain(..) {
            self.ring.release(i);
        }
        self.latches += 1;
        let n = differing(&self.comp.screen, &self.at_commit);
        assert_eq!(n, 0, "after {what} (latch {}): the screen differs from a full redraw of the committed state in {n} pixels", self.latches);
        if self.max >= 3 {
            let newest = self.s.full();
            assert_eq!(differing(&self.comp.screen, &newest), 0, "after {what}: the frame latched is not the newest state");
        }
        // a frame that could not be drawn for want of a buffer goes out now
        self.draw("release");
    }
    fn input(&mut self, e: Event, what: &str) {
        self.s.input(&e);
        self.draw(what);
    }
    fn check_fresh(&mut self, what: &str) {
        let (a, b) = (self.s.full(), fresh(&self.s, self.ms));
        let n = differing(&a, &b);
        assert_eq!(n, 0, "after {what}: a full redraw differs from a fresh mount in {n} pixels");
    }
}

fn key(sym: Sym, ctrl: bool) -> Event {
    Event::Key { sym, pressed: true, mods: Mods { ctrl, ..Mods::default() } }
}

/// One random step of a scenario. `kind`: which inputs the scenario hears.
fn step<V: Model>(r: &mut Rig<V>, rng: &mut Rng, keys: &[Event]) -> String {
    let (w, h) = (r.s.geo.w, r.s.geo.h);
    let what;
    match rng.below(12) {
        0..=3 => {
            let (x, y) = (rng.below(w as u64) as f64, rng.below(h as u64) as f64);
            what = format!("motion {x} {y}");
            r.input(Event::Motion { x, y }, &what);
        }
        4..=5 => {
            let dy = [3.0, -3.0, 17.5, -40.0, 96.0, 700.0, -2000.0, 13.33][rng.below(8) as usize];
            what = format!("scroll {dy}");
            r.input(Event::Scroll { x: 640.0, y: 400.0, dx: 0.0, dy }, &what);
        }
        6 => {
            let (x, y) = (rng.below(w as u64) as f64, rng.below(h as u64) as f64);
            what = format!("click {x} {y}");
            r.input(Event::Button { x, y, button: 0, pressed: true }, &what);
            r.input(Event::Button { x, y, button: 0, pressed: false }, &what);
        }
        7..=9 if !keys.is_empty() => {
            let k = keys[rng.below(keys.len() as u64) as usize];
            what = format!("{k:?}");
            r.input(k, &what);
        }
        10 => {
            what = "clock".to_string();
            r.s.clock();
            r.draw(&what);
        }
        _ => {
            r.ms += [7, 16, 16, 250, 1000][rng.below(5) as usize];
            what = format!("time {}", r.ms);
            r.s.advance(r.ms);
            r.draw(&what);
        }
    }
    what
}

fn script<V: Model>(v: V, seed: u64, steps: usize, keys: &[Event], max: usize) -> (u64, u64, u64) {
    let (w, h) = (spec::WINDOW.0 as i32, spec::WINDOW.1 as i32);
    let mut r = Rig::new(v, w, h, 1, max);
    r.latch("mount");
    let mut rng = Rng(seed | 1);
    for n in 0..steps {
        let what = step(&mut r, &mut rng, keys);
        // bursts: several inputs inside one refresh, then a latch
        if rng.below(3) == 0 {
            r.latch(&what);
        }
        if n % 8 == 7 {
            r.check_fresh(&what);
        }
        if n % 97 == 96 {
            let (w2, h2, k2) = [(640, 480, 1), (1200, 800, 1), (1280, 800, 1), (2560, 1600, 2), (801, 333, 1)][rng.below(5) as usize];
            r.resize(w2, h2, k2);
            r.latch("resize");
            r.check_fresh("resize");
        }
    }
    r.latch("end");
    (r.commits, r.latches, r.starved)
}

fn fkeys() -> Vec<Event> {
    (1..=8).map(|n| key(Sym::F(n), false)).collect()
}
fn typing() -> Vec<Event> {
    let mut k: Vec<Event> = "plain red zqe r".chars().map(|c| key(Sym::Char(c), false)).collect();
    k.extend([key(Sym::Backspace, false), key(Sym::Backspace, false), key(Sym::Char('a'), true), key(Sym::Char('\u{e9}'), false), key(Sym::Char('f'), false), key(Sym::Char('i'), false)]);
    k
}

#[test]
fn rows_and_virtual() {
    for max in [2, 3, 4] {
        let c = script(rows::Rows::new(3000), 11 + max as u64, 400, &[], max);
        eprintln!("rows 3000, ring {max}: commits, latches, starved = {c:?}");
        assert!(c.2 == 0 || max == 2, "a ring of {max} starved");
    }
    script(rows::Rows::new(100_000), 5, 300, &[], 3);
    script(rows::Rows::new(7), 6, 150, &[], 3);
    script(rows::Rows::new(0), 7, 60, &[], 3);
}

#[test]
fn table_fine_and_coarse() {
    for max in [2, 3] {
        script(table::Table::new(1000, None), 21 + max as u64, 400, &fkeys(), max);
        script(table::Coarse::new(1000, None), 31 + max as u64, 300, &fkeys(), max);
    }
    script(table::Table::new(10, None), 23, 200, &fkeys(), 3);
}

#[test]
fn ticker() {
    for updates in [1, 100] {
        let t = Some(Ticker { updates, interval_ms: 16 });
        script(table::Table::new(1000, t), 41 + updates as u64, 300, &fkeys(), 3);
        script(table::Coarse::new(1000, t), 43 + updates as u64, 200, &fkeys(), 3);
    }
}

#[test]
fn typing_and_search() {
    for max in [2, 3] {
        script(search::Search::new(1000, false), 51 + max as u64, 400, &typing(), max);
        script(search::Search::new(1000, true), 61 + max as u64, 400, &typing(), max);
    }
}

#[test]
fn spinner_scene() {
    script(spinner::Spinner::new(10_000), 71, 300, &[], 3);
    script(spinner::Spinner::new(0), 72, 100, &[], 2);
}

/// The filter's refinement stack against the plain definition.
#[test]
fn filter_equals_the_definition() {
    let mut s = search::Search::new(1000, true);
    let mut rng = Rng(99);
    let alphabet = ['a', 'e', 'r', 'p', ' ', 'l', 'z', 'B', 'k'];
    for _ in 0..3000 {
        match rng.below(10) {
            0..=5 => {
                let c = alphabet[rng.below(alphabet.len() as u64) as usize];
                s.type_text(c.encode_utf8(&mut [0; 4]));
            }
            6..=8 => s.key(&bough_x::prelude::Ev { key: Some(bough_x::prelude::Key::Backspace), ..bough_x::prelude::Ev::at(0.0, 0.0) }),
            _ => {
                s.key(&bough_x::prelude::Ev { key: Some(bough_x::prelude::Key::Char('a')), mods: bough_x::prelude::Mods { ctrl: true, ..Default::default() }, ..bough_x::prelude::Ev::at(0.0, 0.0) });
            }
        }
        let q = s.query.get();
        let want: Vec<u32> = s.rows.iter().enumerate().filter(|(_, r)| spec::matches(&r.label, &q)).map(|(i, _)| i as u32).collect();
        assert_eq!(s.shown.items(), &want[..], "query {q:?}");
    }
}

/// Keys reach the handlers the harness's probes rely on: function keys the table, text and Backspace the field.
#[test]
fn keys_do_what_the_probes_expect() {
    let mut s = Scene::new(table::Table::new(1000, None));
    s.resize(1280, 800, 1);
    s.input(&key(Sym::F(3), false));
    assert_eq!(s.v.rows.len(), 999, "F3 removes a row");
    s.input(&key(Sym::F(4), false));
    assert_eq!(s.v.rows.len(), 1000, "F4 inserts one");
    s.input(&key(Sym::F(8), false));
    assert_eq!(s.v.rows.len(), 0, "F8 clears");
    let mut c = Scene::new(table::Coarse::new(1000, None));
    c.resize(1280, 800, 1);
    c.input(&key(Sym::F(6), false));
    assert_eq!(c.v.0.rows.len(), 2000, "F6 appends, coarse");
    c.input(&Event::Button { x: 640.0, y: 40.0 + 4.0 * 32.0 + 16.0, button: 0, pressed: true });
    c.input(&Event::Button { x: 640.0, y: 40.0 + 4.0 * 32.0 + 16.0, button: 0, pressed: false });
    assert_eq!(c.v.0.selected(), Some(4), "a click selects row 4");
    let mut f = Scene::new(search::Search::new(1000, true));
    f.resize(1280, 800, 1);
    f.input(&key(Sym::Char('z'), false));
    assert_eq!((f.v.query.get().as_str(), f.v.shown.len()), ("z", 0), "no label holds a z");
    f.input(&key(Sym::Backspace, false));
    assert_eq!((f.v.query.get().as_str(), f.v.shown.len()), ("", 1000));
    for ch in "red".chars() {
        f.input(&key(Sym::Char(ch), false));
    }
    assert!(f.v.shown.len() > 0 && f.v.shown.len() < 1000);
    f.input(&key(Sym::Char('a'), true));
    f.input(&key(Sym::Backspace, false));
    assert_eq!((f.v.query.get().as_str(), f.v.shown.len()), ("", 1000), "ctrl+a then Backspace clears the field");
}
