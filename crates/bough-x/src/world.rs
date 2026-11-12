//! The driver: one store tree, the block the raster last saw, the damage.
use crate::event::{Button, Dispatch, ENV_REDUCED_MOTION, Ev, Event, Key, Mods, NONE, PLACE_REC_BITS, REDUCED_MOTION_AHEAD, U_ENV, U_FOCUS, U_PRESS, U_PRESS_AT, kind};
use crate::expr::{APP_WORDS, HOT, U, U_PICK, U_PTR_X, U_PTR_Y, U_TIME};
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
    /// The world's columns in paint order: a place's column number indexes this. Rebuilt when columns appear.
    ctab: Vec<(Path, u32)>,
    ctab_at: (u32, usize),
    _b: PhantomData<B>,
}

impl<B: Backend> World<B> {
    pub fn new(w: i32, h: i32, nwords: usize) -> World<B> {
        let mut shown = U::default();
        for w in [U_PICK, U_FOCUS, U_PRESS] {
            shown.w[w] = NONE;
        }
        World { root: Box::new(Store::new(1, Space::root(w, h))), shown, tiles: Tiles::new(w, h), nwords, lists: 0, epoch: 0, reads: 0, col_reads: Vec::new(), by_word: [0; 65], readers: Vec::new(), cols: Vec::new(), cold: Vec::new(), spaces: Vec::new(), shape: u32::MAX, clock: 0, ptr: None, hov: Vec::new(), held: None, focus: None, ctab: Vec::new(), ctab_at: (0, 0), _b: PhantomData }
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
    /// order: what the raster writes into `Pick`.
    pub fn place(&mut self, path: &Path, col: u32, rec: u32) -> u32 {
        self.columns();
        match self.ctab.iter().position(|&(p, c)| p == *path && c == col) {
            Some(i) => (i as u32) << PLACE_REC_BITS | rec,
            None => NONE,
        }
    }
    /// The store, column and record a place names.
    pub fn at_place(&self, place: u32) -> Option<(&Store, u32, u32)> {
        let (path, col) = *self.ctab.get((place >> PLACE_REC_BITS) as usize)?;
        let st = self.root.at(&path)?;
        let rec = place & ((1 << PLACE_REC_BITS) - 1);
        (rec < st.cols.get(col as usize)?.nrec).then_some((st, col, rec))
    }
    /// The column table, rebuilt when the stores grew (cold: a walk of the store tree).
    fn columns(&mut self) {
        fn walk(st: &Store, path: &mut Path, out: &mut Vec<(Path, u32)>) {
            for p in &st.order {
                match *p {
                    crate::store::Pass::Col(j) => out.push((*path, j)),
                    crate::store::Pass::Sub(q) => {
                        path.at[path.n as usize] = q as u16;
                        path.n += 1;
                        walk(&st.subs[q as usize], path, out);
                        path.n -= 1;
                    }
                }
            }
        }
        fn count(st: &Store) -> (u32, usize) {
            st.subs.iter().map(count).fold((st.cols.len() as u32, st.subs.len()), |a, b| (a.0 + b.0, a.1 + b.1))
        }
        let n = count(&self.root);
        if n != self.ctab_at {
            self.ctab.clear();
            walk(&self.root, &mut Path::default(), &mut self.ctab);
            self.ctab_at = n;
        }
    }

    /// The places under (x, y) whose handlers hear one of `kinds`, bottom first (the last is the pick).
    fn chain(&mut self, x: i32, y: i32, kinds: u32) -> Vec<u32> {
        let mut v = Vec::new();
        self.root.chain(&self.shown, x, y, kinds, &mut Path::default(), &mut v);
        v.into_iter().map(|(p, c, r)| self.place(&p, c, r)).collect()
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
        let mut u = self.shown;
        u.w[slot] = val;
        if self.reads >> slot & 1 != 0 {
            self.delta(&u, 1 << slot);
        }
        self.shown = u;
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
        self.picked(v, all.last().copied().unwrap_or(NONE), &all)
    }
    /// A pick that arrived from the raster with a frame: `place` is the `Pick` word.
    pub fn accept_pick<V: View>(&mut self, v: &mut V, place: u32) -> Dispatch {
        let all = match self.ptr {
            Some((x, y)) if place != NONE => self.chain(x, y, kind::POINTER),
            _ => Vec::new(),
        };
        self.picked(v, place, &all)
    }
    fn picked<V: View>(&mut self, v: &mut V, pick: u32, all: &[u32]) -> Dispatch {
        self.core_word(U_PICK, pick);
        let hov: Vec<u32> = all.iter().copied().filter(|&p| self.kinds(p) & kind::HOVER != 0).collect();
        if hov == self.hov {
            return Dispatch { kind: kind::HOVER, place: pick, consumed: false };
        }
        let old = core::mem::replace(&mut self.hov, hov);
        let (x, y) = self.ptr.map_or((-1.0, -1.0), |(x, y)| (x as f32, y as f32));
        let mut out = Dispatch { kind: kind::HOVER, place: NONE, consumed: false };
        for &p in old.iter().rev().filter(|p| !self.hov.contains(p)) {
            self.call(v, p, &Event { kind: kind::HOVER, ev: Ev { on: false, ..Ev::at(x, y) }, text: "" });
        }
        let new = self.hov.clone();
        for &p in new.iter().rev().filter(|p| !old.contains(p)) {
            if self.call(v, p, &Event { kind: kind::HOVER, ev: Ev::at(x, y), text: "" }) && out.place == NONE {
                out = Dispatch { kind: kind::HOVER, place: p, consumed: true };
            }
        }
        out
    }
    fn kinds(&self, place: u32) -> u32 {
        self.at_place(place).and_then(|(st, c, _)| st.cols[c as usize].hover).map_or(0, |o| o.kinds)
    }

    /// A button went down at (x, y): focus moves to the nearest focusable record under it (or leaves, as the
    /// DOM blurs on the background), `Press` and `PressAt` are written, the press handler runs, and a drag
    /// handler captures the pointer.
    pub fn pointer_down<V: View>(&mut self, v: &mut V, x: i32, y: i32, button: Button) -> Dispatch {
        self.pointer(v, x, y);
        let chain = self.chain(x, y, kind::POINTER | kind::FOCUSABLE);
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
        self.bubble(v, chain.iter().rev().copied(), &e)
    }

    /// The button came up at (x, y): a drag ends (its handler sees `ev.on == false`); otherwise, if the pointer
    /// is still over the record pressed, it is activated (the click handler nearest to it under the pointer).
    pub fn pointer_up<V: View>(&mut self, v: &mut V, x: i32, y: i32, button: Button) -> Dispatch {
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
        self.pointer(v, x, y);
        if button != Button::Primary || h.place == NONE {
            return Dispatch::NOTHING;
        }
        let chain = self.chain(x, y, kind::POINTER);
        let Some(i) = chain.iter().position(|&p| p == h.place) else { return Dispatch::NOTHING };
        let e = Event { kind: kind::CLICK, ev: Ev::at(x as f32, y as f32), text: "" };
        self.bubble(v, chain[..=i].iter().rev().copied(), &e)
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
        if key == Key::Tab {
            return self.tab(v, mods.shift);
        }
        let e = Event { kind: kind::KEY, ev: Ev { key: Some(key), mods, ..Ev::at(0.0, 0.0) }, text: "" };
        let focused = self.shown.w[U_FOCUS];
        let (fx, fy) = self.center(focused).unwrap_or((0, 0));
        let mut chain = self.chain(fx, fy, kind::KEY);
        chain.retain(|&p| p != focused);
        self.bubble(v, core::iter::once(focused).filter(|&p| p != NONE).chain(chain.into_iter().rev()), &e)
    }

    /// Text typed (an IME commit, or a printable key's text) goes to the focused record's text handler.
    pub fn text<V: View>(&mut self, v: &mut V, s: &str) -> Dispatch {
        let focused = self.shown.w[U_FOCUS];
        let e = Event { kind: kind::TEXT, ev: Ev::at(0.0, 0.0), text: s };
        let consumed = focused != NONE && self.call(v, focused, &e);
        Dispatch { kind: kind::TEXT, place: if consumed { focused } else { NONE }, consumed }
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
        let mut out = Vec::new();
        for (i, &(path, col)) in self.ctab.iter().enumerate() {
            let Some(st) = self.root.at(&path) else { continue };
            let c = &st.cols[col as usize];
            if c.hover.is_none_or(|o| o.kinds & kind::FOCUSABLE == 0) {
                continue;
            }
            let live = crate::store::Live { cap: st.cap, rows: &st.row_live, nrec: c.nrec };
            let mut recs = Vec::new();
            for pg in 0..c.npages {
                let mut m = live.page(pg as usize);
                while m != 0 {
                    recs.push(pg * 64 + m.trailing_zeros());
                    m &= m - 1;
                }
            }
            // a list's records in row order, not ring order
            if st.cap > 1 {
                recs.sort_by_key(|&r| (row_of(st, r), r / st.cap));
            }
            out.extend(recs.into_iter().map(|r| (i as u32) << PLACE_REC_BITS | r));
        }
        out
    }
    /// Move focus to the next (or previous) focusable record in paint order, wrapping.
    pub fn tab<V: View>(&mut self, v: &mut V, back: bool) -> Dispatch {
        let all = self.focusables();
        if all.is_empty() {
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
        self.focus_place(v, Some(all[next]));
        Dispatch { kind: kind::FOCUS, place: all[next], consumed: true }
    }

    /// Focus a place (`None`: nothing). The record's focus handler hears it; so does the one that lost focus.
    pub fn focus_place<V: View>(&mut self, v: &mut V, place: Option<u32>) {
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
        let (path, col) = *self.ctab.get((place >> PLACE_REC_BITS) as usize)?;
        let st = self.root.at(&path)?;
        let rec = place & ((1 << PLACE_REC_BITS) - 1);
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
            let i = self.ctab.iter().position(|&(p, c)| p == f.path && c == s.col)?;
            Some((i as u32) << PLACE_REC_BITS | rec)
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
