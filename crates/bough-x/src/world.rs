//! The driver: one store tree, the block the raster last saw, the damage.
use crate::event::{Button, Dispatch, ENV_REDUCED_MOTION, Ev, Event, Key, Mods, NONE, PLACE_REC_BITS, REDUCED_MOTION_AHEAD, kind};
use crate::expr::{APP_WORDS, HOT, U, U_ENV, U_FOCUS, U_PICK, U_PRESS, U_PRESS_AT, U_PTR_X, U_PTR_Y, U_TIME};
use crate::sink::{Cx, Patch, REL_EPOCH, View, Write};
use core::sync::atomic::Ordering;
use crate::store::{Backend, ColEnt, Path, SHAPE, Space, Store, Tiles};
use core::marker::PhantomData;

/// A held button: where it went down, the record it went down on, and the record whose drag handler captured
/// the pointer (`NONE`: none).
#[derive(Clone, Copy, Debug)]
struct Held {
    place: u32,
    drag: u32,
    at: (i32, i32),
    button: Button,
}

/// The focused record by what it is (store, site, row), so that focus survives its row leaving exposure and
/// coming back; its place is recomputed whenever a window may have moved.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Focused {
    path: Path,
    site: u32,
    row: u32,
}

pub struct World<B> {
    /// Boxed: the column table points into the tree, and the world itself may be moved.
    pub root: Box<Store>,
    /// The block as of the last settle: what the pixels on screen were shaded with.
    pub shown: U,
    pub tiles: Tiles,
    /// The words the application uses; the compare stops there.
    nwords: usize,
    /// The block words that move some list's exposure (scroll offsets), and the relation epoch as of the last
    /// pass: the two things besides the block that can make the view worth driving.
    lists: u64,
    epoch: u32,
    /// The block words some column reads: a pointer event that no record reads costs nothing.
    reads: u64,
    /// Every column of the world in paint order: the words each reads (dense, scanned on a block change), its
    /// entry (reached only on a match), and where it came from (cold). Rebuilt when the tree changes shape.
    col_reads: Vec<u64>,
    /// Per block word, the columns that read it (`by_word[w]..by_word[w + 1]` in `readers`): a write to one
    /// word visits its readers and no other column.
    by_word: [u16; 65],
    readers: Vec<u16>,
    cols: Vec<ColEnt>,
    cold: Vec<(*const Store, u32)>,
    spaces: Vec<(u64, *const Store)>,
    shape: u32,
    /// The clock (`tick`'s argument). The raster's `Time` runs ahead of it under reduced motion.
    clock: u32,
    /// The pointer, and the places under it whose handlers hear hover, bottom first.
    ptr: Option<(i32, i32)>,
    hov: Vec<u32>,
    held: Option<Held>,
    focus: Option<Focused>,
    /// The columns whose records have handlers: index in the table and the kinds they hear. The pointer path
    /// visits these and no other column.
    handlers: Vec<(u32, u32)>,
    /// Scratch for the event path, reserved once: an event allocates nothing.
    buf: Vec<u32>,
    pool: Vec<Vec<u32>>,
    _b: PhantomData<B>,
}

impl<B: Backend> World<B> {
    pub fn new(w: i32, h: i32, nwords: usize) -> World<B> {
        let mut shown = U::default();
        for w in [U_PICK, U_FOCUS, U_PRESS] {
            shown.w[w] = NONE;
        }
        World { root: Box::new(Store::new(1, Space::root(w, h))), shown, tiles: Tiles::new(w, h), nwords, lists: 0, epoch: 0, reads: 0, col_reads: Vec::new(), by_word: [0; 65], readers: Vec::new(), cols: Vec::new(), cold: Vec::new(), spaces: Vec::new(), shape: u32::MAX, clock: 0, ptr: None, hov: Vec::with_capacity(64), held: None, focus: None, handlers: Vec::new(), buf: Vec::with_capacity(64), pool: (0..4).map(|_| Vec::with_capacity(64)).collect(), _b: PhantomData }
    }

    /// The application's block with the words it does not own (the surface's and the raster's) as the world
    /// has them.
    #[inline(always)]
    fn block<V: View>(&self, v: &V) -> U {
        let mut u = *v.block();
        u.w[APP_WORDS..].copy_from_slice(&self.shown.w[APP_WORDS..]);
        u
    }
    /// The time of the frame being prepared, by the clock: what a handler stamps a motion's start with.
    #[inline(always)]
    pub fn now(&self) -> u32 {
        self.clock
    }

    /// Describe the view for the first time. Only what is exposed comes to exist.
    pub fn mount<V: View>(&mut self, v: &V) {
        self.shown = self.block(v);
        self.tiles.mark_all();
        self.root.row_live[0] = 1;
        self.epoch = REL_EPOCH.load(Ordering::Relaxed);
        let mut cx = Cx { tiles: &mut self.tiles, u: &self.shown, all: true, lists: 0 };
        v.view(&mut Write::<V, B>::new(&mut self.root, &mut cx, true));
        self.lists = cx.lists;
        self.root.resweep(&self.shown, &mut self.tiles);
        self.retable();
        self.refocus();
    }

    /// Bring the column table up to date with the store tree. Cold: the tree changes shape when a column or a
    /// child store is first described.
    #[cold]
    #[inline(never)]
    fn retable(&mut self) {
        self.shape = SHAPE.load(Ordering::Relaxed);
        self.cols.clear();
        self.cold.clear();
        self.spaces.clear();
        self.root.table(&mut self.cols, &mut self.cold);
        self.root.moving_spaces(&mut self.spaces);
        self.handlers.clear();
        for (k, &(st, j)) in self.cold.iter().enumerate() {
            // SAFETY: the table was just built from this tree, so `st` is one of its stores.
            let st = unsafe { &*st };
            if let Some(o) = st.cols[j as usize].hover {
                self.handlers.push((k as u32, o.kinds));
            }
        }
        self.col_reads.clear();
        self.col_reads.extend(self.cols.iter().map(|e| e.reads));
        assert!(self.cols.len() < 1 << 16);
        self.readers.clear();
        for w in 0..64 {
            self.by_word[w] = self.readers.len() as u16;
            self.readers.extend((0..self.cols.len()).filter(|&k| self.col_reads[k] >> w & 1 != 0).map(|k| k as u16));
            assert!(self.readers.len() < 1 << 16);
        }
        self.by_word[64] = self.readers.len() as u16;
        // what a column reads, and what moves a space
        self.reads = self.spaces.iter().fold(self.root.reads_all(), |m, s| m | s.0);
    }

    /// The damage of the block going from what is shown to `u` (`ch`: the changed words). The path from here
    /// to a page is this world, the column's entry, the page: the store tree is not on it.
    #[inline]
    fn delta(&mut self, u: &U, ch: u64) {
        if self.shape != SHAPE.load(Ordering::Relaxed) {
            self.retable();
        }
        if ch & self.reads == 0 {
            return;
        }
        for &(reads, st) in &self.spaces {
            if reads & ch != 0 {
                // SAFETY: the table is current, so `st` is a store of this world's tree.
                let st = unsafe { &*st };
                // the space moved: everything it shows is somewhere else
                self.tiles.mark(st.space.clip);
                if st.timed & (st.reads & ch != 0) {
                    st.resweep(u, &mut self.tiles);
                }
            }
        }
        if ch & (ch - 1) == 0 {
            // one word changed (a hover, a theme, a scroll): its readers, by the list
            let w = ch.trailing_zeros() as usize;
            for i in self.by_word[w] as usize..self.by_word[w + 1] as usize {
                self.delta_one(self.readers[i] as usize, u, ch);
            }
        } else {
            for k in 0..self.col_reads.len() {
                if self.col_reads[k] & ch != 0 {
                    self.delta_one(k, u, ch);
                }
            }
        }
    }
    #[inline(always)]
    fn delta_one(&mut self, k: usize, u: &U, ch: u64) {
        let e = &self.cols[k];
        if e.skip & ch != 0 {
            return;
        }
        // SAFETY: the table is current; `delta` was instantiated for the entry's archetype.
        unsafe { (e.delta)(e, &self.shown, u, &mut self.tiles) };
        if e.timed != 0 {
            let (st, j) = self.cold[k];
            // SAFETY: as above: `st` is a store of this tree and `j` one of its columns.
            unsafe { (*st).resweep_col(&(&(*st).cols)[j as usize], u, &mut self.tiles) };
        }
    }

    /// A frame at time `now`. Time advancing is not a turn: nothing is compared, no record is evaluated and no
    /// store is visited. Each sweep (a column with records in motion) is one mark.
    #[inline]
    pub fn tick(&mut self, now: u32) {
        self.clock = now;
        // Reduced motion: the raster shades every one-shot transition at its end by running `Time` far ahead of
        // the clock (loops read the `Env` bit instead). Nothing else knows.
        self.shown.w[U_TIME] = now.wrapping_add(if self.shown.w[U_ENV] & ENV_REDUCED_MOTION != 0 { REDUCED_MOTION_AHEAD } else { 0 });
        if !self.tiles.sweeps.is_empty() {
            self.tiles.tick(self.shown.w[U_TIME], &self.shown);
        }
    }

    /// The pointer is at (x, y), for the records whose paint reads the pointer words. If none does (the
    /// usual case: hover goes through the pick and a handler) this is two stores.
    #[inline]
    pub fn pointer_words(&mut self, x: i32, y: i32) {
        let ch = ((self.shown.w[U_PTR_X] != x as u32) as u64) << U_PTR_X | ((self.shown.w[U_PTR_Y] != y as u32) as u64) << U_PTR_Y;
        if ch & self.reads != 0 {
            let mut u = self.shown;
            (u.w[U_PTR_X], u.w[U_PTR_Y]) = (x as u32, y as u32);
            self.delta(&u, ch);
        }
        (self.shown.w[U_PTR_X], self.shown.w[U_PTR_Y]) = (x as u32, y as u32);
    }

    /// After handlers ran: find what they changed and damage it.
    ///
    /// The block is compared with the copy the pixels were shaded with. If nothing differs and no relation was
    /// edited, that compare is the whole turn. Otherwise each column that reads a changed word differentiates
    /// its exposed pages, and the view is driven (with a sink that enters only lists) only if a relation was
    /// edited or a word that scrolls a list changed.
    #[inline]
    pub fn settle<V: View>(&mut self, v: &V) {
        let ch = self.shown.diff(v.block(), self.nwords) & !HOT;
        let epoch = REL_EPOCH.load(Ordering::Relaxed);
        if (ch == 0) & (epoch == self.epoch) {
            return;
        }
        self.changed(v, ch, epoch);
    }
    #[inline(never)]
    fn changed<V: View>(&mut self, v: &V, ch: u64, epoch: u32) {
        let u = &self.block(v);
        if ch != 0 {
            self.delta(u, ch);
        }
        if (epoch != self.epoch) | (ch & self.lists != 0) {
            self.epoch = epoch;
            let mut cx = Cx { tiles: &mut self.tiles, u, all: false, lists: 0 };
            v.view(&mut Patch::<V, B> { st: &mut self.root, cx: &mut cx, sub: 0, _p: PhantomData });
            self.lists |= cx.lists;
            if ch != 0 {
                self.shown = *u;
            }
            // windows may have moved: the focused row's place (or that it has none)
            self.refocus();
        } else if ch != 0 {
            self.shown = *u;
        }
    }

    /// Re-describing mode: the state is opaque, so the whole exposed view is described again and compared
    /// with the columns as it goes. The same `view` code as mount.
    pub fn redescribe<V: View>(&mut self, v: &V) {
        let u = &self.block(v);
        let ch = self.shown.diff(u, self.nwords) & !HOT;
        if ch != 0 {
            self.delta(u, ch);
        }
        self.epoch = REL_EPOCH.load(Ordering::Relaxed);
        let mut cx = Cx { tiles: &mut self.tiles, u, all: true, lists: 0 };
        v.view(&mut Write::<V, B>::new(&mut self.root, &mut cx, false));
        self.lists |= cx.lists;
        if self.root.timed {
            self.root.resweep(u, &mut self.tiles);
        }
        if ch != 0 {
            self.shown = *u;
        }
        self.refocus();
    }

    // ---- events: the dataflow run backwards (`event.rs`) -----------------------------------------------------

    /// The place of record `rec` of column `col` of the store at `path`, numbering the world's columns in paint
    /// order (the column table's order): what the raster writes into `Pick`.
    pub fn place(&mut self, path: &Path, col: u32, rec: u32) -> u32 {
        self.columns();
        let Some(st) = self.root.at(path) else { return NONE };
        match self.col_index(st, col) {
            Some(i) => i << PLACE_REC_BITS | rec,
            None => NONE,
        }
    }
    fn col_index(&self, st: &Store, col: u32) -> Option<u32> {
        let st = st as *const Store;
        self.cold.iter().position(|&(s, c)| core::ptr::eq(s, st) && c == col).map(|i| i as u32)
    }
    /// The store, column and record a place names (the column table holds the store: no walk).
    ///
    /// The table is current for this world: every entry point brings it up to date before it dispatches, and
    /// nothing a handler does changes this world's store tree (only a settle does). The shape counter is
    /// process-wide, so another world growing must not make this one's places unreadable.
    pub fn at_place(&self, place: u32) -> Option<(&Store, u32, u32)> {
        let &(st, col) = self.cold.get((place >> PLACE_REC_BITS) as usize)?;
        // SAFETY: the table is current (see above), so `st` is a store of this world's tree.
        let st = unsafe { &*st };
        let rec = place & ((1 << PLACE_REC_BITS) - 1);
        (rec < st.cols.get(col as usize)?.nrec).then_some((st, col, rec))
    }
    /// The column table, current (cold when the tree changed shape).
    fn columns(&mut self) {
        if self.shape != SHAPE.load(Ordering::Relaxed) {
            self.retable();
        }
    }
    /// The path of a store of this world (cold: a walk).
    fn path_of(&self, want: *const Store) -> Option<Path> {
        fn go(st: &Store, want: *const Store, p: &mut Path) -> bool {
            if core::ptr::eq(st, want) {
                return true;
            }
            for (q, s) in st.subs.iter().enumerate() {
                p.at[p.n as usize] = q as u16;
                p.n += 1;
                if go(s, want, p) {
                    return true;
                }
                p.n -= 1;
            }
            false
        }
        let mut p = Path::default();
        go(&self.root, want, &mut p).then_some(p)
    }

    /// The places under (x, y) whose handlers hear one of `kinds`, bottom first (the last is the pick).
    fn chain(&mut self, x: i32, y: i32, kinds: u32) -> Vec<u32> {
        let mut out = self.take();
        self.chain_into(x, y, kinds, &mut out);
        out
    }
    /// A scratch buffer from the world's pool (allocated once, at the world's creation, for every event after).
    fn take(&mut self) -> Vec<u32> {
        let mut v = self.pool.pop().unwrap_or_default();
        v.clear();
        v
    }
    fn chain_into(&mut self, x: i32, y: i32, kinds: u32, out: &mut Vec<u32>) {
        self.columns();
        out.clear();
        let mut recs = core::mem::take(&mut self.buf);
        for &(k, hears) in &self.handlers {
            if hears & kinds == 0 {
                continue;
            }
            let (st, j) = self.cold[k as usize];
            // SAFETY: the table is current, so `st` is a store of this world's tree.
            let st = unsafe { &*st };
            let c = &st.cols[j as usize];
            let Some(o) = c.hover else { continue };
            let sv = crate::store::StoreView { space: &st.space, live: crate::store::Live { cap: st.cap, rows: &st.row_live, nrec: c.nrec } };
            recs.clear();
            // SAFETY: `hit_all` was instantiated for the archetype the column was created with.
            unsafe { (o.hit_all)(c, &sv, &self.shown, x, y, &mut recs) };
            out.extend(recs.iter().map(|&r| k << PLACE_REC_BITS | r));
        }
        self.buf = recs;
    }
    /// Give a scratch buffer back to the pool.
    fn done(&mut self, v: Vec<u32>) {
        self.pool.push(v);
    }

    /// Run the handler of `place` that hears `e`, if it has one.
    fn call<V: View>(&self, v: &mut V, place: u32, e: &Event) -> bool {
        let Some((st, col, rec)) = self.at_place(place) else { return false };
        let c = &st.cols[col as usize];
        let Some(o) = c.hover else { return false };
        if o.kinds & e.kind == 0 {
            return false;
        }
        // SAFETY: `call` was instantiated for the record and handler type this column was created with, and for
        // the view type `V` this world is driven with; `rec` is a record of the column and the captures its own.
        unsafe { (o.call)(v as *mut V as *mut (), c.caps_ptr(rec), c.rec_ptr(rec), e) }
    }
    /// The first place of `chain` (topmost first) whose handler consumes `e`.
    fn bubble<V: View>(&self, v: &mut V, chain: impl Iterator<Item = u32>, e: &Event) -> Dispatch {
        for p in chain {
            if self.call(v, p, e) {
                return Dispatch { kind: e.kind, place: p, consumed: true };
            }
        }
        Dispatch { kind: e.kind, place: NONE, consumed: false }
    }

    /// Write a core word (focus, press, environment) as the raster would see it, damaging what reads it.
    fn core_word(&mut self, slot: usize, val: u32) {
        if self.shown.w[slot] == val {
            return;
        }
        if self.reads >> slot & 1 != 0 {
            let mut u = self.shown;
            u.w[slot] = val;
            self.delta(&u, 1 << slot);
        }
        self.shown.w[slot] = val;
    }

    /// The platform's settings changed (`event::ENV_*` bits). Reduced motion takes effect at the next frame.
    pub fn set_env(&mut self, bits: u32) {
        self.core_word(U_ENV, bits);
    }

    /// The pointer moved to (x, y): the pick, then hover left and entered (the chain's symmetric difference),
    /// or, while a drag handler holds the pointer, the drag.
    pub fn pointer<V: View>(&mut self, v: &mut V, x: i32, y: i32) -> Dispatch {
        self.ptr = Some((x, y));
        if let Some(h) = self.held
            && h.drag != NONE
        {
            let e = Event { kind: kind::DRAG, ev: Ev { dx: (x - h.at.0) as f32, dy: (y - h.at.1) as f32, ..Ev::at(x as f32, y as f32) }, text: "" };
            let consumed = self.call(v, h.drag, &e);
            return Dispatch { kind: kind::DRAG, place: h.drag, consumed };
        }
        self.repick(v)
    }
    /// The pick at the pointer as it stands, and hover brought up to date: after a move, or after a turn that
    /// moved content under a still pointer (what the raster's pick in the frame being drawn delivers).
    pub fn repick<V: View>(&mut self, v: &mut V) -> Dispatch {
        let Some((x, y)) = self.ptr else { return Dispatch::NOTHING };
        let all = self.chain(x, y, kind::POINTER);
        let d = self.picked(v, all.last().copied().unwrap_or(NONE), &all);
        self.done(all);
        d
    }
    /// A pick that arrived from the raster with a frame: `place` is the `Pick` word.
    pub fn accept_pick<V: View>(&mut self, v: &mut V, place: u32) -> Dispatch {
        let all = match self.ptr {
            Some((x, y)) if place != NONE => self.chain(x, y, kind::POINTER),
            _ => self.take(),
        };
        let d = self.picked(v, place, &all);
        self.done(all);
        d
    }
    fn picked<V: View>(&mut self, v: &mut V, pick: u32, all: &[u32]) -> Dispatch {
        self.core_word(U_PICK, pick);
        let mut next = self.take();
        for &p in all {
            if self.kinds(p) & kind::HOVER != 0 {
                next.push(p);
            }
        }
        if next == self.hov {
            // the usual move: the hovered set is what it was
            self.done(next);
            return Dispatch { kind: kind::HOVER, place: pick, consumed: false };
        }
        let old = core::mem::replace(&mut self.hov, next);
        let (x, y) = self.ptr.map_or((-1.0, -1.0), |(x, y)| (x as f32, y as f32));
        let mut out = Dispatch { kind: kind::HOVER, place: NONE, consumed: false };
        // left: the old set's records not under the pointer now, topmost first
        for i in (0..old.len()).rev() {
            let p = old[i];
            if !self.hov.contains(&p) {
                self.call(v, p, &Event { kind: kind::HOVER, ev: Ev { on: false, ..Ev::at(x, y) }, text: "" });
            }
        }
        // entered: the new set's records that were not under it, topmost first
        for i in (0..self.hov.len()).rev() {
            let p = self.hov[i];
            if !old.contains(&p) && self.call(v, p, &Event { kind: kind::HOVER, ev: Ev::at(x, y), text: "" }) && out.place == NONE {
                out = Dispatch { kind: kind::HOVER, place: p, consumed: true };
            }
        }
        self.done(old);
        out
    }
    fn kinds(&self, place: u32) -> u32 {
        self.at_place(place).and_then(|(st, c, _)| st.cols[c as usize].hover).map_or(0, |o| o.kinds)
    }

    /// A button went down at (x, y): focus moves to the nearest focusable record under it (or leaves, as the
    /// DOM blurs on the background), `Press` and `PressAt` are written, the press handler runs, and a drag
    /// handler captures the pointer.
    pub fn pointer_down<V: View>(&mut self, v: &mut V, x: i32, y: i32, button: Button) -> Dispatch {
        // one walk under the pointer serves the pick, hover, focus, the press and the drag
        self.ptr = Some((x, y));
        let chain = self.chain(x, y, kind::POINTER | kind::FOCUSABLE);
        let mut ptr = self.take();
        for &p in &chain {
            if self.kinds(p) & kind::POINTER != 0 {
                ptr.push(p);
            }
        }
        self.picked(v, ptr.last().copied().unwrap_or(NONE), &ptr);
        self.done(ptr);
        if button == Button::Primary {
            let f = chain.iter().rev().copied().find(|&p| self.kinds(p) & kind::FOCUSABLE != 0);
            self.focus_place(v, f);
        }
        let top = chain.iter().rev().copied().find(|&p| self.kinds(p) & kind::POINTER != 0).unwrap_or(NONE);
        let drag = chain.iter().rev().copied().find(|&p| self.kinds(p) & kind::DRAG != 0).unwrap_or(NONE);
        self.held = Some(Held { place: top, drag, at: (x, y), button });
        if button == Button::Primary {
            self.core_word(U_PRESS, top);
            self.core_word(U_PRESS_AT, (x as u32 & 0xffff) << 16 | (y as u32 & 0xffff));
        }
        let e = Event { kind: kind::PRESS, ev: Ev { button, ..Ev::at(x as f32, y as f32) }, text: "" };
        let d = self.bubble(v, chain.iter().rev().copied(), &e);
        self.done(chain);
        d
    }

    /// The button came up at (x, y): a drag ends (its handler sees `ev.on == false`); otherwise, if the pointer
    /// is still over the record pressed, it is activated (the click handler nearest to it under the pointer).
    pub fn pointer_up<V: View>(&mut self, v: &mut V, x: i32, y: i32, button: Button) -> Dispatch {
        self.columns();
        let Some(h) = self.held.take() else { return Dispatch::NOTHING };
        if h.button == Button::Primary {
            self.core_word(U_PRESS, NONE);
        }
        if h.drag != NONE {
            let e = Event { kind: kind::DRAG, ev: Ev { dx: (x - h.at.0) as f32, dy: (y - h.at.1) as f32, on: false, ..Ev::at(x as f32, y as f32) }, text: "" };
            let consumed = self.call(v, h.drag, &e);
            self.pointer(v, x, y);
            return Dispatch { kind: kind::DRAG, place: h.drag, consumed };
        }
        // one walk under the pointer serves the pick, hover and the click
        self.ptr = Some((x, y));
        let chain = self.chain(x, y, kind::POINTER);
        self.picked(v, chain.last().copied().unwrap_or(NONE), &chain);
        let d = match chain.iter().position(|&p| p == h.place) {
            Some(i) if button == Button::Primary && h.place != NONE => {
                let e = Event { kind: kind::CLICK, ev: Ev::at(x as f32, y as f32), text: "" };
                self.bubble(v, chain[..=i].iter().rev().copied(), &e)
            }
            _ => Dispatch::NOTHING,
        };
        self.done(chain);
        d
    }

    /// A press and a release at (x, y).
    pub fn click<V: View>(&mut self, v: &mut V, x: i32, y: i32) -> Dispatch {
        self.pointer_down(v, x, y, Button::Primary);
        self.pointer_up(v, x, y, Button::Primary)
    }

    /// A key went down. Tab and Shift+Tab move focus in paint order; otherwise the focused record's key handler,
    /// then those of the records with key handlers it lies inside, innermost first (with nothing focused: those
    /// that hold the window's origin, the root's included).
    pub fn key_down<V: View>(&mut self, v: &mut V, key: Key, mods: Mods) -> Dispatch {
        self.columns();
        if key == Key::Tab {
            return self.tab(v, mods.shift);
        }
        let e = Event { kind: kind::KEY, ev: Ev { key: Some(key), mods, ..Ev::at(0.0, 0.0) }, text: "" };
        let focused = self.shown.w[U_FOCUS];
        if focused != NONE {
            if self.call(v, focused, &e) {
                return Dispatch { kind: kind::KEY, place: focused, consumed: true };
            }
            // Enter or Space on a focused record that a click activates: its own behaviour, before any record it
            // lies inside hears the key
            if matches!(key, Key::Enter | Key::Char(' ')) {
                let c = Event { kind: kind::CLICK, ..e };
                if self.call(v, focused, &c) {
                    return Dispatch { kind: kind::CLICK, place: focused, consumed: true };
                }
            }
        }
        let (fx, fy) = self.center(focused).unwrap_or((0, 0));
        let mut chain = self.chain(fx, fy, kind::KEY);
        chain.retain(|&p| p != focused);
        let d = self.bubble(v, chain.iter().rev().copied(), &e);
        self.done(chain);
        d
    }

    /// Text typed (an IME commit, or a printable key's text) goes to the focused record's text handler.
    pub fn text<V: View>(&mut self, v: &mut V, s: &str) -> Dispatch {
        self.columns();
        let focused = self.shown.w[U_FOCUS];
        let e = Event { kind: kind::TEXT, ev: Ev::at(0.0, 0.0), text: s };
        let consumed = focused != NONE && self.call(v, focused, &e);
        Dispatch { kind: kind::TEXT, place: if consumed { focused } else { NONE }, consumed }
    }

    /// The wheel turned by (dx, dy) lines at (x, y): the wheel handlers under the pointer first (topmost first,
    /// one that returns `true` consumes it); otherwise the innermost space under the pointer whose origin reads a
    /// scroll word moves, within its lists' extent: the space is its own scroller. One line is a row of its
    /// first list.
    pub fn wheel<V: View>(&mut self, v: &mut V, x: i32, y: i32, dx: f32, dy: f32) -> Dispatch {
        self.ptr = Some((x, y));
        let chain = self.chain(x, y, kind::WHEEL);
        let e = Event { kind: kind::WHEEL, ev: Ev { dx, dy, ..Ev::at(x as f32, y as f32) }, text: "" };
        let d = self.bubble(v, chain.iter().rev().copied(), &e);
        self.done(chain);
        if d.consumed {
            return d;
        }
        fn scroller(st: &Store, x: i32, y: i32) -> Option<&Store> {
            let c = st.space.clip;
            if x < c.x0 || x >= c.x1 || y < c.y0 || y >= c.y1 {
                return None;
            }
            let inner = st.subs.iter().find_map(|s| scroller(s, x, y));
            // a list inherits its scope's space; the scope is the scroller
            inner.or((st.space.scroll != crate::store::NO_WORD && st.src.is_none()).then_some(st))
        }
        let Some(st) = scroller(&self.root, x, y) else { return Dispatch { kind: kind::WHEEL, place: NONE, consumed: false } };
        let word = st.space.scroll as usize;
        let lists = st.subs.iter().filter(|s| s.src.is_some());
        let (extent, line) = lists.fold((0i32, 0i32), |(e, l), s| (e.max(s.geo.1 + s.n as i32 * s.geo.2), if l == 0 { s.geo.2 } else { l }));
        let max = (extent - (st.space.clip.y1 - st.space.clip.y0)).max(0);
        let Some(u) = v.block_mut() else { return Dispatch { kind: kind::WHEEL, place: NONE, consumed: false } };
        let at = u.w[word] as i32;
        let to = (at + (dy * line.max(1) as f32) as i32).clamp(0, max);
        u.w[word] = to as u32;
        Dispatch { kind: kind::WHEEL, place: NONE, consumed: to != at }
    }

    /// Activate a place as a click would, with no pointer: what an assistive technology's "press" does. The
    /// place's own click handler runs, or the nearest under its centre.
    pub fn activate<V: View>(&mut self, v: &mut V, place: u32) -> Dispatch {
        self.columns();
        let e = Event { kind: kind::CLICK, ev: Ev::at(0.0, 0.0), text: "" };
        if self.call(v, place, &e) {
            return Dispatch { kind: kind::CLICK, place, consumed: true };
        }
        let Some((x, y)) = self.center(place) else { return Dispatch::NOTHING };
        let chain = self.chain(x, y, kind::CLICK);
        let d = self.bubble(v, chain.iter().rev().copied(), &e);
        self.done(chain);
        d
    }

    /// Scroll the list at `path` so that row `r` is exposed (as little as needed): what an assistive
    /// technology's "scroll into view" does. Writes the scroll word of the list's space; the next settle
    /// exposes the row. Returns whether the word changed.
    pub fn scroll_into_view<V: View>(&mut self, v: &mut V, path: &Path, r: u32) -> bool {
        let Some(st) = self.root.at(path) else { return false };
        let (sp, (_, y, h)) = (st.space, st.geo);
        if sp.scroll == crate::store::NO_WORD || h == 0 {
            return false;
        }
        let Some(u) = v.block_mut() else { return false };
        let at = u.w[sp.scroll as usize] as i32;
        let view = sp.clip.y1 - sp.clip.y0;
        let (top, bot) = (y + r as i32 * h, y + (r as i32 + 1) * h);
        let to = if top < at { top } else if bot > at + view { bot - view } else { at };
        u.w[sp.scroll as usize] = to as u32;
        to != at
    }

    /// The centre of a place's record on screen.
    fn center(&self, place: u32) -> Option<(i32, i32)> {
        let (st, col, rec) = self.at_place(place)?;
        let r = rect_of(st, col, rec)?.shift(st.space.origin(&self.shown));
        Some(((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2))
    }

    /// The focusable places in paint order, exposed records only.
    fn focusables(&mut self) -> Vec<u32> {
        self.columns();
        let mut out = self.take();
        let mut recs = self.take();
        for (i, &(st, col)) in self.cold.iter().enumerate() {
            // SAFETY: the table is current, so `st` is a store of this world's tree.
            let st = unsafe { &*st };
            let c = &st.cols[col as usize];
            if c.hover.is_none_or(|o| o.kinds & kind::FOCUSABLE == 0) {
                continue;
            }
            let live = crate::store::Live { cap: st.cap, rows: &st.row_live, nrec: c.nrec };
            recs.clear();
            for pg in 0..c.npages {
                let mut m = live.page(pg as usize);
                while m != 0 {
                    recs.push(pg * 64 + m.trailing_zeros());
                    m &= m - 1;
                }
            }
            // a list's records in row order, not ring order
            if st.cap > 1 {
                recs.sort_unstable_by_key(|&r| (row_of(st, r), r / st.cap));
            }
            out.extend(recs.iter().map(|&r| (i as u32) << PLACE_REC_BITS | r));
        }
        self.done(recs);
        out
    }
    /// Move focus to the next (or previous) focusable record in paint order, wrapping.
    pub fn tab<V: View>(&mut self, v: &mut V, back: bool) -> Dispatch {
        let all = self.focusables();
        if all.is_empty() {
            self.done(all);
            return Dispatch { kind: kind::FOCUS, place: NONE, consumed: false };
        }
        let cur = all.iter().position(|&p| p == self.shown.w[U_FOCUS]);
        let n = all.len();
        let next = match (cur, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        let p = all[next];
        self.done(all);
        self.focus_place(v, Some(p));
        Dispatch { kind: kind::FOCUS, place: p, consumed: true }
    }

    /// Focus a place (`None`: nothing). The record's focus handler hears it; so does the one that lost focus.
    pub fn focus_place<V: View>(&mut self, v: &mut V, place: Option<u32>) {
        self.columns();
        let old = self.shown.w[U_FOCUS];
        let new = place.unwrap_or(NONE);
        if old == new {
            return;
        }
        self.focus = place.and_then(|p| self.logical(p));
        self.core_word(U_FOCUS, new);
        if old != NONE {
            self.call(v, old, &Event { kind: kind::FOCUS, ev: Ev { on: false, ..Ev::at(0.0, 0.0) }, text: "" });
        }
        if new != NONE {
            self.call(v, new, &Event { kind: kind::FOCUS, ev: Ev::at(0.0, 0.0), text: "" });
        }
    }
    /// The focused record as (store, site, row), or `None` when nothing is focused.
    pub fn focused(&self) -> Option<(Path, u32, u32)> {
        self.focus.map(|f| (f.path, f.site, f.row))
    }
    fn logical(&self, place: u32) -> Option<Focused> {
        let (st, col, rec) = self.at_place(place)?;
        let path = self.path_of(st)?;
        let k = rec / st.cap;
        let site = st.sites.iter().enumerate().filter(|(_, s)| s.col == col).nth(k as usize)?.0 as u32;
        Some(Focused { path, site, row: row_of(st, rec) })
    }
    /// After a pass that may have moved a window: where the focused row is now, or that it is not exposed
    /// (`Focus` holds `NONE` until it comes back). Nothing to do when nothing is focused.
    fn refocus(&mut self) {
        let Some(f) = self.focus else { return };
        self.columns();
        let place = (|| {
            let st = self.root.at(&f.path)?;
            let s = st.sites.get(f.site as usize)?;
            let exposed = st.cap == 1 || (f.row >= st.lo && f.row < st.hi);
            if !exposed {
                return None;
            }
            let rec = s.base + f.row % st.cap;
            let i = self.col_index(st, s.col)?;
            Some(i << PLACE_REC_BITS | rec)
        })()
        .unwrap_or(NONE);
        self.core_word(U_FOCUS, place);
    }

    /// Paint everything through the backend with the block as last settled.
    pub fn paint(&self, b: &mut B) {
        self.root.paint(&self.shown, b);
    }
}

/// The row a record of store `st` shows: 0 in a fixed shape; in a list, the row of the window at its slot.
fn row_of(st: &Store, rec: u32) -> u32 {
    if st.cap == 1 {
        return 0;
    }
    let slot = rec % st.cap;
    st.lo + (slot + st.cap - st.lo % st.cap) % st.cap
}
/// A record's rect in its store's space.
fn rect_of(st: &Store, col: u32, rec: u32) -> Option<crate::store::Rect> {
    let base = rec / st.cap * st.cap;
    let s = st.sites.iter().find(|s| s.col == col && s.base == base)?;
    // SAFETY: a site of this store; the slot is under its capacity; site pointers are current.
    Some(unsafe { s.rect(rec % st.cap) })
}
