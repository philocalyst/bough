//! The adapter between the host loop and a `bough-x` world.
//!
//! Input becomes calls on the world's event entry points (which run the handlers the view declared), the wheel
//! and the clock become calls on the model, a settle is the world's compare, and a paint is one pass of the
//! span raster over the world's columns restricted to the tiles the host's ring says need repair.
use crate::Geo;
use bough_host::event::{Event, Sym};
use bough_host::{App, TilesRef};
use bough_span::{Spans, TileSet};
use bough_x::event::{Button, Key, Mods};
use bough_x::prelude::*;
use core::time::Duration;

/// What a scenario's state provides besides its view.
pub trait Model: View + 'static {
    /// Words of the block the view uses.
    const NWORDS: usize;
    /// The state is opaque (plain collections, no journal): after every handler the exposed view is described
    /// again and compared with the columns (gui-bench's `coarse` variants).
    const REDESCRIBE: bool = false;
    /// The window is now this size and scale.
    fn layout(&mut self, geo: Geo);
    /// The wheel or a finger moved the content by `dy` buffer pixels at (x, y).
    fn wheel(&mut self, _x: i32, _y: i32, _dy: i32) {}
    fn period(&self) -> Option<Duration> {
        None
    }
    fn clock(&mut self) {}
    /// A point to press once after mount (what has the keyboard when the window opens).
    fn focus_at(&self) -> Option<(i32, i32)> {
        None
    }
}

pub struct Scene<V: Model> {
    pub v: V,
    pub world: World<Spans<'static>>,
    pub geo: Geo,
    mask: TileSet,
    scroll_acc: f64,
    /// The time of the last frame: a world made for a new size starts there, not at zero.
    ms: u32,
    /// Work counters of the last paint.
    pub last: bough_span::Stats,
}

fn image(k: i32) -> &'static bough_glyph::Image {
    if k >= 2 { &bough_glyph::DEJAVU_SANS_28 } else { &bough_glyph::DEJAVU_SANS_14 }
}

impl<V: Model> Scene<V> {
    /// A scene that has no size yet: [`App::resize`] mounts it.
    pub fn new(v: V) -> Scene<V> {
        Scene { v, world: World::new(1, 1, V::NWORDS), geo: Geo::new(0, 0, 1), mask: TileSet::new(1, 1), scroll_acc: 0.0, ms: 0, last: Default::default() }
    }
    fn after(&mut self) {
        // content may have moved under a still pointer: the pick follows the state, not the last motion event
        if V::REDESCRIBE {
            self.world.redescribe(&self.v);
        } else {
            self.world.settle(&self.v);
        }
    }
    /// Bring the world up to date and hover with it.
    pub fn sync(&mut self) {
        self.after();
        self.world.repick(&mut self.v);
        self.after();
    }
    /// Paint everything into a fresh buffer of the window's size (tests, dumps).
    pub fn full(&mut self) -> Vec<u32> {
        let (w, h) = (self.geo.w, self.geo.h);
        let mut px = vec![0u32; (w * h) as usize];
        // SAFETY: `px` is `h` rows of `w` pixels, owned here.
        unsafe { self.paint(px.as_mut_ptr(), w as usize, None) };
        px
    }
}

fn key(sym: Sym) -> Option<Key> {
    Some(match sym {
        Sym::Char(c) => Key::Char(c),
        Sym::Backspace => Key::Backspace,
        Sym::Tab => Key::Tab,
        Sym::Enter => Key::Enter,
        Sym::Escape => Key::Escape,
        Sym::Delete => Key::Delete,
        Sym::Left => Key::Left,
        Sym::Right => Key::Right,
        Sym::Up => Key::Up,
        Sym::Down => Key::Down,
        Sym::Home => Key::Home,
        Sym::End => Key::End,
        Sym::PageUp => Key::PageUp,
        Sym::PageDown => Key::PageDown,
        // function keys as the façade delivers them on every platform: AppKit's function-key code points
        Sym::F(n) => Key::Char(char::from_u32(0xF703 + n as u32)?),
    })
}

impl<V: Model> App for Scene<V> {
    fn resize(&mut self, w: i32, h: i32, scale: i32) {
        let geo = Geo::new(w, h, scale.max(1));
        if geo == self.geo {
            return;
        }
        self.geo = geo;
        self.v.layout(geo);
        self.world = World::new(w, h, V::NWORDS);
        self.world.tick(self.ms);
        self.world.mount(&self.v);
        self.mask = TileSet::new(w, h);
        if let Some((x, y)) = self.v.focus_at() {
            self.world.click(&mut self.v, x, y);
            self.world.pointer(&mut self.v, -1, -1);
            self.after();
        }
    }
    fn input(&mut self, e: &Event) {
        let (v, w) = (&mut self.v, &mut self.world);
        match *e {
            Event::Motion { x, y } => {
                w.pointer(v, x as i32, y as i32);
            }
            Event::Leave => {
                w.pointer(v, -1, -1);
            }
            Event::Button { x, y, button, pressed } => {
                let b = [Button::Primary, Button::Secondary, Button::Middle][button.min(2) as usize];
                if pressed {
                    w.pointer_down(v, x as i32, y as i32, b);
                } else {
                    w.pointer_up(v, x as i32, y as i32, b);
                }
            }
            Event::Scroll { x, y, dy, .. } => {
                // whole pixels now, the fraction kept for the next event
                self.scroll_acc += dy;
                let d = self.scroll_acc.trunc();
                self.scroll_acc -= d;
                if d != 0.0 {
                    v.wheel(x as i32, y as i32, d as i32);
                    self.sync();
                    return;
                }
            }
            Event::Key { sym, pressed: true, mods } => {
                let m = Mods { shift: mods.shift, ctrl: mods.ctrl, alt: mods.alt, cmd: mods.logo };
                match bough_host_typed(sym, mods) {
                    Some(c) => {
                        let mut b = [0u8; 4];
                        if !w.text(v, c.encode_utf8(&mut b)).consumed
                            && let Some(k) = key(sym)
                        {
                            w.key_down(v, k, m);
                        }
                    }
                    None => {
                        if let Some(k) = key(sym) {
                            w.key_down(v, k, m);
                        }
                    }
                }
            }
            Event::Key { .. } | Event::Focus(_) | Event::Close => {}
        }
        self.after();
    }
    fn period(&self) -> Option<Duration> {
        self.v.period()
    }
    fn clock(&mut self) {
        self.v.clock();
        self.after();
    }
    fn advance(&mut self, ms: u32) {
        self.ms = ms;
        self.world.tick(ms);
    }
    fn animating(&self) -> bool {
        !self.world.tiles.sweeps.is_empty()
    }
    fn settle(&mut self) -> TilesRef<'_> {
        self.after();
        let t = &self.world.tiles;
        TilesRef { bits: &t.bits, stride: t.stride, lo: t.lo, hi: t.hi }
    }
    unsafe fn paint(&mut self, px: *mut u32, stride: usize, repair: Option<TilesRef<'_>>) {
        let (w, h) = (self.geo.w, self.geo.h);
        let mask = match repair {
            Some(r) => {
                self.mask.clear();
                if r.lo < r.hi {
                    let (a, b) = (r.lo * r.stride, r.hi * r.stride);
                    self.mask.bits[a..b].copy_from_slice(&r.bits[a..b]);
                    (self.mask.lo, self.mask.hi) = (r.lo, r.hi);
                }
                Some(&self.mask)
            }
            None => None,
        };
        // SAFETY: the caller's contract for `px`; `self.mask` outlives the pass (it ends in this function).
        let mut sp = unsafe { Spans::erased(px, stride, w, h, mask, image(self.geo.k)) };
        self.world.paint(&mut sp);
        self.last = sp.st;
    }
    fn painted(&mut self) {
        self.world.tiles.clear();
    }
}

fn bough_host_typed(sym: Sym, mods: bough_host::event::Mods) -> Option<char> {
    match sym {
        Sym::Char(c) if !mods.ctrl && !mods.alt && !mods.logo && !c.is_control() => Some(c),
        _ => None,
    }
}
