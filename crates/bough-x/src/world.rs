//! The driver: one store tree, the block the raster last saw, the damage.
use crate::expr::U;
use crate::sink::{Cx, Patch, View, Write};
use crate::store::{Backend, Space, Store, Tiles};
use core::marker::PhantomData;

#[derive(Clone, PartialEq, Debug)]
struct Hot {
    path: Vec<u16>,
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
    hot: Option<Hot>,
    _b: PhantomData<B>,
}

impl<B: Backend> World<B> {
    pub fn new(w: i32, h: i32, nwords: usize) -> World<B> {
        World { root: Store::new(1, Space::root(w, h)), shown: U::default(), tiles: Tiles::new(w, h), nwords, hot: None, _b: PhantomData }
    }

    /// Describe the view for the first time. Only what is exposed comes to exist.
    pub fn mount<V: View>(&mut self, v: &V) {
        self.shown = *v.block();
        self.tiles.mark_all();
        self.root.row_live[0] = 1;
        let mut cx = Cx { tiles: &mut self.tiles, u: &self.shown, all: true };
        v.view(&mut Write::<V, B>::new(&mut self.root, &mut cx, true));
    }

    /// After handlers ran: find what they changed and damage it.
    ///
    /// The block is compared with the copy the pixels were shaded with; each column that reads a changed word
    /// differentiates its exposed pages. Then the view is driven once with a sink that enters only lists.
    #[inline]
    pub fn settle<V: View>(&mut self, v: &V) {
        let u = v.block();
        let ch = self.shown.diff(u, self.nwords);
        if ch != 0 {
            self.root.delta(&self.shown, u, ch, &mut self.tiles);
        }
        let mut cx = Cx { tiles: &mut self.tiles, u, all: false };
        v.view(&mut Patch::<V, B> { st: &mut self.root, cx: &mut cx, sub: 0, _p: PhantomData });
        if ch != 0 {
            self.shown = *u;
        }
    }

    /// Re-describing mode: the state is opaque, so the whole exposed view is described again and compared
    /// with the columns as it goes. The same `view` code as mount.
    pub fn redescribe<V: View>(&mut self, v: &V) {
        let u = v.block();
        let ch = self.shown.diff(u, self.nwords);
        if ch != 0 {
            self.root.delta(&self.shown, u, ch, &mut self.tiles);
        }
        let mut cx = Cx { tiles: &mut self.tiles, u, all: true };
        v.view(&mut Write::<V, B>::new(&mut self.root, &mut cx, false));
        if ch != 0 {
            self.shown = *u;
        }
    }

    /// The pointer is at (x, y): tell the records it left and entered. Returns whether a handler ran.
    pub fn pointer<V: View>(&mut self, v: &mut V, x: i32, y: i32) -> bool {
        let mut best = None;
        self.root.hit(&self.shown, x, y, &mut Vec::new(), &mut best);
        let now = best.map(|(path, col, rec)| Hot { path, col, rec });
        if now == self.hot {
            return false;
        }
        for (h, on) in [(self.hot.take(), false), (now.clone(), true)] {
            let Some(h) = h else { continue };
            let Some(c) = self.root.at(&h.path).and_then(|s| s.cols.get(h.col as usize)) else { continue };
            if let Some(f) = c.hover {
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
