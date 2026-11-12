//! The driver: one store tree, the block the raster last saw, the damage.
use crate::expr::{APP_WORDS, U, U_TIME};
use crate::sink::{Cx, Patch, REL_EPOCH, View, Write};
use core::sync::atomic::Ordering;
use crate::store::{Backend, Path, Space, Store, Tiles};
use core::marker::PhantomData;

#[derive(Clone, Copy, PartialEq, Debug)]
struct Hot {
    path: Path,
    col: u32,
    rec: u32,
}

pub struct World<B> {
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
    /// Some column is in motion: frames damage without turns until it rests.
    moving: bool,
    hot: Option<Hot>,
    _b: PhantomData<B>,
}

impl<B: Backend> World<B> {
    pub fn new(w: i32, h: i32, nwords: usize) -> World<B> {
        World { root: Store::new(1, Space::root(w, h)), shown: U::default(), tiles: Tiles::new(w, h), nwords, lists: 0, epoch: 0, moving: false, hot: None, _b: PhantomData }
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
        self.root.row_live[0] = 1;
        self.epoch = REL_EPOCH.load(Ordering::Relaxed);
        let mut cx = Cx { tiles: &mut self.tiles, u: &self.shown, all: true, lists: 0 };
        v.view(&mut Write::<V, B>::new(&mut self.root, &mut cx, true));
        self.lists = cx.lists;
    }

    /// A frame at time `now`. Time advancing is not a turn: nothing is compared or evaluated. If some record
    /// is in motion, the box it moves in is damaged (one mark per column in motion).
    #[inline]
    pub fn tick(&mut self, now: u32) {
        self.shown.w[U_TIME] = now;
        if self.moving {
            self.moving = self.root.tick(&self.shown, now, &mut self.tiles);
        }
    }

    /// After handlers ran: find what they changed and damage it.
    ///
    /// The block is compared with the copy the pixels were shaded with. If nothing differs and no relation was
    /// edited, that compare is the whole turn. Otherwise each column that reads a changed word differentiates
    /// its exposed pages, and the view is driven (with a sink that enters only lists) only if a relation was
    /// edited or a word that scrolls a list changed.
    #[inline]
    pub fn settle<V: View>(&mut self, v: &V) {
        let ch = self.shown.diff(v.block(), self.nwords);
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
            self.moving |= self.root.delta(&self.shown, u, ch, &mut self.tiles);
        }
        if (epoch != self.epoch) | (ch & self.lists != 0) {
            self.epoch = epoch;
            let mut cx = Cx { tiles: &mut self.tiles, u, all: false, lists: 0 };
            v.view(&mut Patch::<V, B> { st: &mut self.root, cx: &mut cx, sub: 0, _p: PhantomData });
            self.lists |= cx.lists;
        }
        if ch != 0 {
            self.shown = *u;
        }
    }

    /// Re-describing mode: the state is opaque, so the whole exposed view is described again and compared
    /// with the columns as it goes. The same `view` code as mount.
    pub fn redescribe<V: View>(&mut self, v: &V) {
        let u = &self.block(v);
        let ch = self.shown.diff(u, self.nwords);
        if ch != 0 {
            self.moving |= self.root.delta(&self.shown, u, ch, &mut self.tiles);
        }
        self.epoch = REL_EPOCH.load(Ordering::Relaxed);
        let mut cx = Cx { tiles: &mut self.tiles, u, all: true, lists: 0 };
        v.view(&mut Write::<V, B>::new(&mut self.root, &mut cx, false));
        self.lists |= cx.lists;
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
                unsafe { f(v as *mut V as *mut (), c.rec_ptr(h.rec), on) }
            }
        }
        self.hot = now;
        true
    }

    /// Paint everything through the backend with the block as last settled.
    pub fn paint(&self, b: &mut B) {
        self.root.paint(&self.shown, b);
    }
}
