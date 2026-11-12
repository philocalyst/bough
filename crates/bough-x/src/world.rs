//! The driver: one store tree, the block the raster last saw, the damage.
use crate::expr::{APP_WORDS, HOT, U, U_PTR_X, U_PTR_Y, U_TIME};
use crate::sink::{Cx, Patch, REL_EPOCH, View, Write};
use core::sync::atomic::Ordering;
use crate::arena::{Ar, Arena};
use crate::store::{self, Backend, Path, Space, Store, Tiles};
use core::marker::PhantomData;

#[derive(Clone, Copy, PartialEq, Debug)]
struct Hot {
    path: Path,
    col: u32,
    rec: u32,
}

pub struct World<B> {
    /// The scene's memory; `root` and `ar` are handles into it.
    _arena: Arena,
    pub ar: Ar,
    pub root: Store,
    /// The block as of the last settle: what the pixels on screen were shaded with.
    pub shown: U,
    pub tiles: Tiles,
    /// The words the application uses; the compare stops there.
    nwords: usize,
    /// The block words that move some list's exposure (scroll offsets), and the relation epoch as of the last
    /// pass: the two things besides the block that can make the view worth driving.
    lists: u64,
    epoch: u32,
    hot: Option<Hot>,
    _b: PhantomData<B>,
}

/// Address space reserved for each half of a scene. Memory is committed as pages are touched.
pub const RESERVE: usize = 64 << 20;

impl<B: Backend> World<B> {
    pub fn new(w: i32, h: i32, nwords: usize) -> World<B> {
        let arena = Arena::new(RESERVE);
        let ar = arena.ar();
        World { _arena: arena, ar, root: Store::new(ar, 1, Space::root(w, h)), shown: U::default(), tiles: Tiles::new(w, h), nwords, lists: 0, epoch: 0, hot: None, _b: PhantomData }
    }

    /// The application's block with the words the raster owns (time, pointer, pick) as the world has them.
    #[inline(always)]
    fn block<V: View>(&self, v: &V) -> U {
        let mut u = *v.block();
        u.w[APP_WORDS..].copy_from_slice(&self.shown.w[APP_WORDS..]);
        u
    }
    /// The time of the frame being prepared (the raster's word; handlers read it to start a motion).
    #[inline(always)]
    pub fn now(&self) -> u32 {
        self.shown.w[U_TIME]
    }

    /// Describe the view for the first time. Only what is exposed comes to exist.
    pub fn mount<V: View>(&mut self, v: &V) {
        self.shown = self.block(v);
        self.tiles.mark_all();
        self.root.live_one();
        self.epoch = REL_EPOCH.load(Ordering::Relaxed);
        let mut cx = Cx { ar: self.ar, tiles: &mut self.tiles, u: &self.shown, all: true, lists: 0 };
        v.view(&mut Write::<V, B>::new(&mut self.root, &mut cx, true));
        self.lists = cx.lists;
        self.root.resweep(&self.shown, &mut self.tiles);
        self.ordered();
    }

    /// A frame at time `now`. Time advancing is not a turn: nothing is compared, no record is evaluated and no
    /// store is visited. Each sweep (a column with records in motion) is one mark.
    #[inline]
    pub fn tick(&mut self, now: u32) {
        self.shown.w[U_TIME] = now;
        if !self.tiles.sweeps.is_empty() {
            self.tiles.tick(now, &self.shown);
        }
    }

    /// The pointer is at (x, y), for the records whose paint reads the pointer words. If none does (the
    /// usual case: hover goes through the pick and a handler) this is two stores.
    #[inline]
    pub fn pointer_words(&mut self, x: i32, y: i32) {
        let ch = ((self.shown.w[U_PTR_X] != x as u32) as u64) << U_PTR_X | ((self.shown.w[U_PTR_Y] != y as u32) as u64) << U_PTR_Y;
        if ch & self.ar.head().reads != 0 {
            let mut u = self.shown;
            (u.w[U_PTR_X], u.w[U_PTR_Y]) = (x as u32, y as u32);
            store::delta(self.ar, &self.shown, &u, ch, &mut self.tiles);
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
            store::delta(self.ar, &self.shown, u, ch, &mut self.tiles);
        }
        if (epoch != self.epoch) | (ch & self.lists != 0) {
            self.epoch = epoch;
            let mut cx = Cx { ar: self.ar, tiles: &mut self.tiles, u, all: false, lists: 0 };
            v.view(&mut Patch::<V, B> { st: &mut self.root, cx: &mut cx, sub: 0, _p: PhantomData });
            self.lists |= cx.lists;
            self.ordered();
        }
        if ch != 0 {
            self.shown = *u;
        }
    }

    /// Re-describing mode: the state is opaque, so the whole exposed view is described again and compared
    /// with the columns as it goes. The same `view` code as mount.
    pub fn redescribe<V: View>(&mut self, v: &V) {
        let u = &self.block(v);
        let ch = self.shown.diff(u, self.nwords) & !HOT;
        if ch != 0 {
            store::delta(self.ar, &self.shown, u, ch, &mut self.tiles);
        }
        self.epoch = REL_EPOCH.load(Ordering::Relaxed);
        let mut cx = Cx { ar: self.ar, tiles: &mut self.tiles, u, all: true, lists: 0 };
        v.view(&mut Write::<V, B>::new(&mut self.root, &mut cx, false));
        self.lists |= cx.lists;
        if self.root.timed {
            self.root.resweep(u, &mut self.tiles);
        }
        self.ordered();
        if ch != 0 {
            self.shown = *u;
        }
    }

    /// The pointer is at (x, y): tell the records it left and entered. Returns whether a handler ran.
    pub fn pointer<V: View>(&mut self, v: &mut V, x: i32, y: i32) -> bool {
        let mut best = None;
        self.root.hit(&self.shown, x, y, &mut Path::default(), &mut best);
        let now = best.map(|(path, col, rec)| Hot { path, col, rec });
        if now == self.hot {
            return false;
        }
        for (h, on) in [(self.hot.take(), false), (now, true)] {
            let Some(h) = h else { continue };
            let Some(c) = self.root.at(&h.path).and_then(|s| s.cols.get(h.col as usize)) else { continue };
            if let Some((f, _)) = c.hover {
                // SAFETY: `f` was registered by `emit_hover::<A, F>` on this column for the view type `V` this
                // world is driven with; `rec_ptr` is a record of that column.
                unsafe { f(v as *mut V as *mut (), c.rec_ptr(self.ar, h.rec), on) }
            }
        }
        self.hot = now;
        true
    }

    /// Keep the paint order: a pass that created a column leaves it to be written.
    #[inline(always)]
    fn ordered(&mut self) {
        if self.ar.head().unordered != 0 {
            self.root.reorder();
        }
    }

    /// Paint everything through the backend with the block as last settled, from the working half: what the
    /// turn thread itself sees.
    pub fn paint(&self, b: &mut B) {
        // SAFETY: every column of this world was described with backend type `B` (`World<B>`'s sinks are the
        // only ones that seal entries); the turn thread reads its own half.
        unsafe { store::paint(self.ar.work(), self.ar.head().ncols as usize, &self.shown, b) }
    }

    /// Publish the turn thread's writes: the published half becomes what the working half is. Call between
    /// frames (no frame is reading the published half). Returns the lines copied.
    pub fn publish(&mut self) -> u32 {
        self.ar.publish()
    }

    /// Paint from the published half with block `u` (the block that was current at the publish): what a
    /// frame does.
    pub fn paint_front(&self, u: &crate::expr::U, b: &mut B) {
        // SAFETY: as `paint`; the published half is written only by `publish`, which takes `&mut self`.
        unsafe { store::paint(self.ar.front(), self.ar.head().ncols as usize, u, b) }
    }

    /// Write the scene to `path` as an image a later run of this binary can map (`World::open`). Publishes
    /// first: the image is the published scene and the block it was shaded with.
    pub fn save(&mut self, path: &std::path::Path) -> std::io::Result<usize> {
        self.publish();
        self._arena.save(path, &self.shown, [self.tiles.w, self.tiles.h])
    }

    /// Map an image as this world's scene. What comes back can be painted at once (`paint_front`, with
    /// `shown`): nothing was mounted, nothing was read but the image's structural part, and no page of it was
    /// written. It cannot take a turn until [`World::adopt`] has run.
    #[cfg(unix)]
    pub fn open(path: &std::path::Path) -> Result<World<B>, crate::arena::Refused> {
        let arena = Arena::open(path, RESERVE)?;
        let ar = arena.ar();
        let h = ar.head();
        let (shown, size) = (crate::expr::U { w: h.shown }, h.size);
        // The store tree is found again by `adopt`; until then it is a placeholder that owns nothing.
        Ok(World { _arena: arena, ar, root: Store::placeholder(ar), shown, tiles: Tiles::new(size[0], size[1]), nwords: 0, lists: 0, epoch: 0, hot: None, _b: PhantomData })
    }

    /// Make a mapped scene this view's: the view is described against the image, with the same code as a
    /// re-description. The store tree is found again (the same requests get the same columns), and every
    /// operand and rect the view gives is compared with what the image holds; what differs is written and
    /// damaged like any change. So an image of another state is repaired, not trusted.
    ///
    /// `Err` when the view's shape is not the image's (another view, another window size class): the caller
    /// drops this world and mounts.
    pub fn adopt<V: View>(&mut self, v: &V, nwords: usize) -> Result<(), crate::arena::Refused> {
        self.nwords = nwords;
        self.ar.adopt_begin();
        self.root = Store::new(self.ar, 1, Space::root(self.tiles.w, self.tiles.h));
        self.root.live_one();
        let u = &self.block(v);
        let ch = self.shown.diff(u, self.nwords) & !HOT;
        self.epoch = REL_EPOCH.load(Ordering::Relaxed);
        let mut cx = Cx { ar: self.ar, tiles: &mut self.tiles, u, all: true, lists: 0 };
        v.view(&mut Write::<V, B>::new(&mut self.root, &mut cx, false));
        self.lists = cx.lists;
        if !(self.ar.adopt_end() && self.root.same_as_image()) {
            return Err(crate::arena::Refused::Bounds);
        }
        // records compared equal under the image's block; what the block's change shows is the delta's
        if ch != 0 {
            store::delta(self.ar, &self.shown, u, ch, &mut self.tiles);
            self.shown = *u;
        }
        self.tiles.sweeps.clear();
        self.resweep_all(u);
        Ok(())
    }
    fn resweep_all(&mut self, u: &crate::expr::U) {
        fn go(s: &Store, u: &crate::expr::U, t: &mut Tiles) {
            if s.timed {
                s.resweep(u, t);
            }
            s.subs.iter().for_each(|q| go(q, u, t));
        }
        go(&self.root, u, &mut self.tiles);
    }

    /// Records, pages and bytes of columns.
    pub fn stats(&self) -> (usize, usize, usize) {
        store::stats(self.ar)
    }
}
