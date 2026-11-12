//! Foreign content: a rectangle of the tree that bough lays out, clips, scrolls, orders, hit-tests and
//! reflects, and whose pixels someone else produces (a web view, a video layer, a native control, another
//! renderer's surface). `docs/lanes/composition/DESIGN.md` §1.
//!
//! A foreign element is **one record** of kind [`REC_FOREIGN`](crate::rt::REC_FOREIGN) in the paint preorder,
//! with a layout node like any box. Nothing else is special: its ink, clip, space and position in the order are
//! the record's, so scroll (by copy too), transforms, damage, hit testing and reflection treat it as they treat
//! a quad. What differs is the record's *payload*, by [`Mode`]:
//!
//! * [`Mode::Under`]: the host composites the content **beneath** bough's surface, and the record paints a
//!   *hole*: an instance of kind [`KIND_PUNCH`] that the tile kernel applies as destination-out (rounded,
//!   anti-aliased, clamped to the record's clip). Everything painted before the record is removed inside the
//!   hole; everything painted after it lies over the hole with its own alpha. So the content is ordered
//!   between bough's records with one surface, and clipping, rounded corners and translucent overlays cost the
//!   host nothing: the foreign layer needs only its frame. Right for opaque content.
//! * [`Mode::Over`]: the host composites the content **above** the surface; the record has geometry and no
//!   pixels ([`KIND_NONE`], rejected when the kernel stages it). Right when nothing is painted over the
//!   element (`Placement::covered` says whether something is) and for content with its own transparency.
//! * [`Mode::Sampled`]: bough paints the content itself, from pixels the host hands over, through the image
//!   kernel. Fully general (any order, any blending, snapshots include it); costs an upload and a repaint
//!   of its tiles per content change.
//!
//! The host learns where the content goes from [`ForeignHost::place`], called at the start of a frame for each
//! foreign element whose [`Placement`] differs from the last one it was given. A content change on the
//! foreign side is signalled through a [`ForeignSignal`] (any thread): in `Sampled` mode it is a paint write on
//! the record (damage of its visible ink, content addressed by the serial); in the layer modes bough's pixels
//! do not depend on it and nothing is repainted.
//!
//! Cost when unused: `Rt::foreign.slots` is empty, and each frame and pointer event tests that once.
use crate::app::{App, View};
use crate::elem::{Element, Mounter};
use crate::input::Ev;
use crate::layout::{Spec, Styled, attach};
use crate::rt::*;
use bough_core::{Handle, KF_NOHIT, NRect, RecordInit};
use bough_metal::instance::Instance;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Instance kind of a foreign record's hole (the tile kernel's destination-out case, `shader.metal` kind 7).
pub const KIND_PUNCH: u8 = 7;
/// Instance kind of a record with geometry and no pixels (`shader.metal`: never staged).
pub const KIND_NONE: u8 = 8;

/// How the content reaches the screen (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Under,
    Over,
    Sampled,
}

/// Who answers a pointer over the element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The record is an ordinary hit target: it stops the pointer, and routes on it (`.on_click`) run.
    Bough,
    /// Pointer-transparent: whatever bough paints beneath it answers.
    Pass,
    /// The host answers: pointer presses, releases and moves over the element go to
    /// [`ForeignHost::pointer`] and to no route.
    Capture,
}

/// Where the compositor puts the content relative to bough's surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    /// A layer under the surface (the surface is transparent in the element's hole).
    Below,
    /// A layer over the surface.
    Above,
    /// No layer: bough paints the pixels.
    Inline,
}

/// What the host needs to composite one foreign element. Device px, window coordinates, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub id: u64,
    /// The laid-out box `[x0, y0, x1, y1]` (before clipping).
    pub rect: [f32; 4],
    /// The record's clip ∩ the window: the content may show only inside it. In `Under` mode bough's own
    /// pixels already enforce it (the hole is cut to the clip).
    pub clip: [i32; 4],
    /// Corner radius of the box.
    pub radius: f32,
    /// Device px per point.
    pub scale: f32,
    pub plane: Plane,
    /// Rank among the window's foreign elements in paint order (0 is painted first): layers of one plane
    /// stack in this order.
    pub order: u32,
    /// Position in the paint preorder (what the inspector shows; changes when earlier records are added).
    pub pos: u32,
    /// A bough record painted after the element meets its visible rectangle. `Under` and `Sampled` handle
    /// that; an `Over` layer would hide that content.
    pub covered: bool,
    /// The affine of the transformed subtree the element is in (`[a, b, c, d]`, pivot), if any.
    pub xform: Option<([f32; 4], [f32; 2])>,
    /// Content changes signalled so far.
    pub serial: u64,
}

impl Placement {
    /// `rect ∩ clip`, or `None` when nothing of the element can show.
    pub fn visible(&self) -> Option<[i32; 4]> {
        let r = [self.rect[0].floor() as i32, self.rect[1].floor() as i32, self.rect[2].ceil() as i32, self.rect[3].ceil() as i32];
        let v = [r[0].max(self.clip[0]), r[1].max(self.clip[1]), r[2].min(self.clip[2]), r[3].min(self.clip[3])];
        (v[0] < v[2] && v[1] < v[3]).then_some(v)
    }
}

/// A pointer event bough forwards to a `Hit::Capture` element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    Down,
    Up,
    Move,
}

/// The compositor side: a trait object at the boundary (one per window).
pub trait ForeignHost {
    /// The element's placement changed (or it was just mounted). Called at the start of the frame that shows
    /// the change, on the thread that drives the app.
    fn place(&mut self, p: &Placement);
    /// The element was unmounted.
    fn remove(&mut self, id: u64);
    /// `Mode::Sampled`: write the content's current pixels, premultiplied RGBA8, `w * h * 4` bytes, top row
    /// first. Return `false` to leave what is there.
    fn pixels(&mut self, _id: u64, _w: u32, _h: u32, _out: &mut [u8]) -> bool {
        false
    }
    /// A pointer event over a `Hit::Capture` element (`ev` in points, window coordinates).
    fn pointer(&mut self, _id: u64, _what: Pointer, _ev: &Ev) {}
}

/// The foreign side's "my content changed": cloneable, `Send + Sync`, one atomic add.
#[derive(Clone)]
pub struct ForeignSignal(Arc<AtomicU64>);
impl ForeignSignal {
    pub fn content_changed(&self) {
        self.0.fetch_add(1, Ordering::Release);
    }
}

pub(crate) struct Slot {
    pub id: u64,
    pub handle: Handle,
    pub node: u32,
    pub mode: Mode,
    pub hit: Hit,
    pub radius: f32,
    /// Content serial: bumped by the signal, read at the frame.
    pub signal: Arc<AtomicU64>,
    pub seen: u64,
    /// `Sampled`: the atlas rect (x, y, w, h) holding the pixels.
    pub uv: [u16; 4],
    pub last: Option<Placement>,
    pub live: bool,
}

/// The window's foreign elements and its host.
#[derive(Default)]
pub struct Foreigns {
    pub(crate) slots: Vec<Slot>,
    pub(crate) host: Option<Box<dyn ForeignHost>>,
    /// Live slots.
    pub(crate) live: u32,
    order: Vec<(u32, u32)>,
    buf: Vec<u8>,
}

/// A foreign element in flow: sized and placed by layout like any box (`.w()`, `.h()`, `.grow()`, ...).
pub struct Foreign {
    id: u64,
    spec: Spec,
    mode: Mode,
    hit: Hit,
    radius: f32,
    at: &'static std::panic::Location<'static>,
}

/// A foreign element identified by `id` (the host's name for the content: it comes back in every
/// [`Placement`]). Defaults: `Mode::Under`, `Hit::Bough`.
#[track_caller]
pub fn foreign(id: u64) -> Foreign {
    let mut spec = Spec::new(false, false);
    spec.shrink = 0.0;
    Foreign { id, spec, mode: Mode::Under, hit: Hit::Bough, radius: 0.0, at: std::panic::Location::caller() }
}

impl Foreign {
    pub fn mode(mut self, m: Mode) -> Self {
        self.mode = m;
        self
    }
    pub fn hit(mut self, h: Hit) -> Self {
        self.hit = h;
        self
    }
    /// Corner radius (points). `Under`: the hole is rounded; `Sampled` ignores it; `Over`: reported to the host.
    pub fn rounded(mut self, r: f32) -> Self {
        self.radius = r;
        self
    }
}

impl Styled for Foreign {
    fn spec_mut(&mut self) -> &mut Spec {
        &mut self.spec
    }
}

impl<V: 'static> Element<V> for Foreign {
    fn mount(self, m: &mut Mounter<'_, V>) {
        // its state is the host's and the layout's: a re-description steps over it
        crate::describe::mount_opaque(m, move |m: &mut Mounter<'_, V>| {
            let s = m.rt.scale;
            let hd = m.rt.bough.new_handle();
            let lid = m.rt.lay.eng.add(hd, &self.spec, s, None);
            attach(m, lid);
            let node = m.rt.alloc_node();
            let run = m.rt.alloc_run(1);
            m.rt.runs[run as usize].len = 1;
            let inst = m.rt.runs[run as usize].first;
            let radius = snap(self.radius * s);
            *m.rt.inst_mut(inst) = match self.mode {
                // the hole: full strength (the colour's alpha scales it)
                Mode::Under => Instance { color: u32::MAX, radius, space: m.space, kind: KIND_PUNCH, ..Default::default() },
                Mode::Over => Instance { radius, space: m.space, kind: KIND_NONE, ..Default::default() },
                // the image kernel, drawing an empty atlas rect until the first pixels arrive
                Mode::Sampled => Instance { color: u32::MAX, space: m.space, kind: crate::deco::KIND_IMAGE, ..Default::default() },
            };
            {
                let n = &mut m.rt.nodes[node as usize];
                n.handle = hd;
                n.run = run;
                n.space = m.space;
                n.clip = m.clip;
                n.kind = crate::deco::NK_DECO;
                n.margin = 1.0;
                n.radius = radius;
                n.lnode = lid;
            }
            m.scope.nodes.push(node);
            m.scope.runs.push(run);
            // a painted layout box (`LInfo::kind` 1): layout places it through `Rt::quad_geom`, whose damage is
            // the bands of the edges that moved, as for any quad
            m.rt.lay.info[lid as usize].rtn = node;
            m.rt.lay.info[lid as usize].kind = 1;
            let flags = if self.hit == Hit::Pass { KF_NOHIT } else { 0 };
            m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_FOREIGN | flags, payload: inst, handle: hd.idx, clip: m.clip });
            m.rt.foreign.add(Slot { id: self.id, handle: hd, node, mode: self.mode, hit: self.hit, radius, signal: Arc::new(AtomicU64::new(0)), seen: 0, uv: [0; 4], last: None, live: true });
        })
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
    fn probe(self, p: &mut crate::probe::Probe<'_, V>) {
        let mark = p.mark();
        p.leaf(crate::probe::Family::Foreign, std::any::type_name::<Self>(), "foreign");
        p.at(mark, self.at);
    }
}

impl Foreigns {
    fn add(&mut self, s: Slot) {
        self.live += 1;
        match self.slots.iter().position(|x| !x.live) {
            Some(i) => self.slots[i] = s,
            None => self.slots.push(s),
        }
    }
    #[inline]
    pub(crate) fn slot_of(&self, h: Handle) -> Option<usize> {
        self.slots.iter().position(|s| s.live && s.handle == h)
    }
}

impl Rt {
    /// Bring the host up to date with the foreign elements (the start of a frame): unmounted elements are
    /// removed, content changes are taken (a paint write in `Sampled` mode), and every element whose
    /// placement differs from the last one handed over is placed.
    pub(crate) fn foreign_sync(&mut self) {
        let core_dirty = !self.bough.plane(0).is_clear();
        let mut order = std::mem::take(&mut self.foreign.order);
        order.clear();
        for i in 0..self.foreign.slots.len() {
            if !self.foreign.slots[i].live {
                continue;
            }
            match self.bough.pos_of(self.foreign.slots[i].handle) {
                Some(p) => order.push((p as u32, i as u32)),
                None => {
                    // unmounted (its record left the tree): the host drops the content
                    let s = &mut self.foreign.slots[i];
                    s.live = false;
                    self.foreign.live -= 1;
                    let id = s.id;
                    if let Some(h) = self.foreign.host.as_mut() {
                        h.remove(id);
                    }
                }
            }
        }
        order.sort_unstable();
        let win = self.bough.window();
        for (rank, &(pos, i)) in order.iter().enumerate() {
            let (i, pos) = (i as usize, pos as usize);
            let (id, node, mode, radius, hd) = {
                let s = &self.foreign.slots[i];
                (s.id, s.node, s.mode, s.radius, s.handle)
            };
            let n = &self.nodes[node as usize];
            let o = self.space(n.space);
            // the gates' sabotage: a placement that ignores the element's space (so: scroll) must fail them
            #[cfg(feature = "sabotage_foreign")]
            let o = [0.0f32, 0.0];
            let rect = [o[0] + n.x + n.tx, o[1] + n.y + n.ty, o[0] + n.x + n.tx + n.w, o[1] + n.y + n.ty + n.h];
            let clip = self.bough.clip_rect(self.bough.record(pos).clip).intersect(win).canon();
            let clip = if clip.is_empty() { [0; 4] } else { clip.to_xyxy() };
            // content: the signal's count
            let serial = self.foreign.slots[i].signal.load(Ordering::Acquire);
            let changed = serial != self.foreign.slots[i].seen;
            let (w, h) = ((rect[2] - rect[0]).round().max(0.0) as u32, (rect[3] - rect[1]).round().max(0.0) as u32);
            if mode == Mode::Sampled && w > 0 && h > 0 {
                let uv = self.foreign.slots[i].uv;
                let resized = uv[2] as u32 != w || uv[3] as u32 != h;
                if resized || changed {
                    self.foreign_pixels(i, w, h, resized);
                }
                if resized {
                    // another atlas rect: the record's pixels changed for a reason that is not its paint state
                    self.bough.touch_handle(hd);
                } else if changed {
                    // the record's paint state is its content serial (content addressing)
                    self.paint_write(hd, self.foreign.slots[i].seen, serial);
                }
            }
            self.foreign.slots[i].seen = serial;
            let last = self.foreign.slots[i].last;
            let mut p = Placement {
                id,
                rect,
                clip,
                radius,
                scale: self.scale,
                plane: match mode {
                    Mode::Under => Plane::Below,
                    Mode::Over => Plane::Above,
                    Mode::Sampled => Plane::Inline,
                },
                order: rank as u32,
                pos: pos as u32,
                covered: last.is_some_and(|l| l.covered),
                xform: self.bough.xform_at(pos),
                serial,
            };
            // what is painted over it can change only with core damage (geometry, structure, clips) or its own
            // move: asked then, by the summaries (`Bough::first_over`)
            if core_dirty || last.is_none_or(|l| l.rect != p.rect || l.clip != p.clip || l.pos != p.pos) {
                p.covered = match p.visible() {
                    Some(v) => self.bough.first_over(pos, NRect::new(v[0], v[1], v[2], v[3])).is_some(),
                    None => false,
                };
            }
            if last != Some(p) {
                self.foreign.slots[i].last = Some(p);
                if let Some(h) = self.foreign.host.as_mut() {
                    h.place(&p);
                }
            }
        }
        self.foreign.order = order;
    }

    /// `Sampled`: take the host's pixels for slot `i` into its atlas rect (allocated at the content's device
    /// size; a resize takes a new rect).
    fn foreign_pixels(&mut self, i: usize, w: u32, h: u32, resized: bool) {
        let mut buf = std::mem::take(&mut self.foreign.buf);
        buf.clear();
        buf.resize((w * h * 4) as usize, 0);
        let id = self.foreign.slots[i].id;
        let got = self.foreign.host.as_mut().is_some_and(|host| host.pixels(id, w, h, &mut buf));
        if resized {
            let uv = self.images.reserve(w, h);
            self.foreign.slots[i].uv = uv;
            let inst = self.runs[self.nodes[self.foreign.slots[i].node as usize].run as usize].first;
            self.inst_mut(inst).uv = uv;
        }
        if got || resized {
            self.images.write(self.foreign.slots[i].uv, &buf);
        }
        self.foreign.buf = buf;
    }
}

/// One foreign element as reflection shows it.
#[derive(Clone, Debug)]
pub struct ForeignView {
    pub id: u64,
    pub handle: u32,
    pub mode: Mode,
    pub hit: Hit,
    /// `file:line:column` of the `foreign(..)` call.
    pub source: String,
    /// The last placement handed to the host (none before the first frame).
    pub placement: Option<Placement>,
}

impl<V: View> App<V> {
    /// Install the window's foreign host. Every live element is placed again at the next frame.
    pub fn set_foreign_host(&mut self, host: Box<dyn ForeignHost>) {
        self.rt.foreign.host = Some(host);
        for s in &mut self.rt.foreign.slots {
            s.last = None;
        }
    }

    /// The content-changed signal of foreign element `id` (hand it to the thread that produces the content).
    pub fn foreign_signal(&self, id: u64) -> Option<ForeignSignal> {
        self.rt.foreign.slots.iter().find(|s| s.live && s.id == id).map(|s| ForeignSignal(s.signal.clone()))
    }

    /// The foreign element whose record is the topmost hit at (x, y) points, with its hit policy.
    pub fn foreign_at(&self, x: f32, y: f32) -> Option<(u64, Hit)> {
        if self.rt.foreign.live == 0 {
            return None;
        }
        let s = self.rt.scale;
        let top = self.rt.bough.hit_top((x * s).floor() as i32, (y * s).floor() as i32)?;
        if self.rt.bough.kind_at(top) & 0x1F != REC_FOREIGN {
            return None;
        }
        let h = self.rt.bough.handle_at(top)?;
        self.rt.foreign.slot_of(h).map(|i| (self.rt.foreign.slots[i].id, self.rt.foreign.slots[i].hit))
    }

    /// A pointer event at (x, y): if a `Hit::Capture` element is the topmost hit, forward it to the host and
    /// report that bough must not dispatch it.
    pub(crate) fn foreign_capture(&mut self, what: Pointer, x: f32, y: f32) -> bool {
        if self.rt.foreign.live == 0 {
            return false;
        }
        match self.foreign_at(x, y) {
            Some((id, Hit::Capture)) => {
                let e = Ev { x, y, dx: 0.0, dy: 0.0, key: None, mods: Default::default(), button: crate::input::Button::Primary, on: true };
                if let Some(h) = self.rt.foreign.host.as_mut() {
                    h.pointer(id, what, &e);
                }
                true
            }
            _ => false,
        }
    }

    /// Every live foreign element in paint order (reflection: the inspector lists them with the records).
    pub fn reflect_foreign(&self) -> Vec<ForeignView> {
        let f = &self.rt.foreign;
        // provenance from the description, walked again (nothing is kept per element for it)
        let w = (f.live != 0).then(|| crate::probe::walk_root(&self.rt, &*self.owner));
        let source = |pos: usize| w.as_ref().and_then(|w| w.at_rec.get(pos).filter(|&&i| i != u32::MAX).and_then(|&i| w.seen[i as usize].at)).map_or_else(String::new, |a| a.to_string());
        let mut v: Vec<(usize, ForeignView)> = f
            .slots
            .iter()
            .filter(|s| s.live)
            .filter_map(|s| {
                let pos = self.rt.bough.pos_of(s.handle)?;
                Some((pos, ForeignView { id: s.id, handle: s.handle.idx, mode: s.mode, hit: s.hit, source: source(pos), placement: s.last }))
            })
            .collect();
        v.sort_by_key(|x| x.0);
        v.into_iter().map(|x| x.1).collect()
    }
}
