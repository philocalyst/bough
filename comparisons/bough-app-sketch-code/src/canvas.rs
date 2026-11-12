//! A canvas: one record owning a run of GPU instances that the app writes itself (zgx's `ui.canvas` with
//! `draw` / `draw_item`), with exact damage.
//!
//! bough has no such element on its public API (a custom element needs `Mounter`'s crate-private fields), so
//! a canvas is allocated by mounting `lanes(cap, ..)` (one record, one instance run, one handle, in the
//! current space and clip) and located after the mount by its capacity; from then on the app's paint step
//! writes whole instances (kind, radius, border, both colours: what `lanes`' `Lane` cannot carry) through
//! `App::rt`, between a turn and its frame (the GPU is idle then: every turn syncs first). Damage is exact:
//! the old and the new rect of every instance that changed go to the core plane; the record's ink is kept a
//! superset of its instances' (the GPU culls by it) without damaging it.
use bough::rt::{Rt, NK_BARS};
use bough_core::{Handle, NRect};
use bough::frame::Instance;

pub struct Canvas {
    pub first: u32,
    pub cap: u32,
    pub handle: Handle,
    pub space: u16,
    /// Union of the instances' rects (space-local device px), as set on the record.
    ink: [f32; 4],
    ink_set: bool,
}

/// Antialiasing margin of an rrect/border instance (device px).
const MARGIN: f32 = 1.0;

impl Canvas {
    /// The canvas mounted as `lanes(cap, ..)`: the one live lanes record whose run holds `cap` instances.
    pub fn locate(rt: &Rt, cap: u32) -> Canvas {
        let mut found = None;
        for n in rt.nodes.iter() {
            if n.live && n.kind == NK_BARS && rt.runs[n.run as usize].len == cap {
                assert!(found.is_none(), "two canvases of capacity {cap}");
                found = Some(Canvas { first: rt.runs[n.run as usize].first, cap, handle: n.handle, space: n.space, ink: [0.0; 4], ink_set: false });
            }
        }
        found.unwrap_or_else(|| panic!("no canvas of capacity {cap}"))
    }

    #[inline]
    pub fn get(&self, rt: &Rt, i: u32) -> Instance {
        rt.inst[(self.first + i) as usize]
    }

    /// Write instance `i`; damage its old and new rects when it changed. Returns whether it changed.
    #[inline]
    pub fn set(&mut self, rt: &mut Rt, i: u32, ins: Instance) -> bool {
        debug_assert!(i < self.cap);
        let p = rt.inst_mut(self.first + i);
        let old = *p;
        if same(&old, &ins) {
            return false;
        }
        *p = Instance { space: self.space, ..ins };
        if old.size[0] > 0.0 && old.size[1] > 0.0 {
            let r = rt.ink_rect(self.space, old.pos[0], old.pos[1], old.size[0], old.size[1], MARGIN);
            rt.bough.damage_rect(r.to_nrect());
        }
        if ins.size[0] > 0.0 && ins.size[1] > 0.0 {
            let r = rt.ink_rect(self.space, ins.pos[0], ins.pos[1], ins.size[0], ins.size[1], MARGIN);
            rt.bough.damage_rect(r.to_nrect());
            self.grow(ins.pos[0], ins.pos[1], ins.size[0], ins.size[1]);
        }
        true
    }

    /// Hide instance `i` (zero size).
    #[inline]
    pub fn clear(&mut self, rt: &mut Rt, i: u32) -> bool {
        let old = self.get(rt, i);
        if old.size == [0.0, 0.0] {
            return false;
        }
        self.set(rt, i, Instance { kind: old.kind, ..Default::default() })
    }

    fn grow(&mut self, x: f32, y: f32, w: f32, h: f32) {
        if !self.ink_set {
            self.ink = [x, y, x + w, y + h];
            self.ink_set = true;
            return;
        }
        let k = &mut self.ink;
        k[0] = k[0].min(x);
        k[1] = k[1].min(y);
        k[2] = k[2].max(x + w);
        k[3] = k[3].max(y + h);
    }

    /// The instances' union, known by construction (space-local device px: x0, y0, x1, y1).
    pub fn set_ink_local(&mut self, r: [f32; 4]) {
        self.ink = r;
        self.ink_set = true;
    }

    /// Recompute the record's ink from every instance (after a full repaint: it may shrink).
    pub fn reset_ink(&mut self, rt: &Rt) {
        self.ink_set = false;
        for i in 0..self.cap {
            let p = rt.inst[(self.first + i) as usize];
            if p.size[0] > 0.0 && p.size[1] > 0.0 {
                self.grow(p.pos[0], p.pos[1], p.size[0], p.size[1]);
            }
        }
    }

    /// Put the instances' union on the record (no damage: every changed instance damaged its own rects).
    pub fn commit(&self, rt: &mut Rt) {
        let ink = if self.ink_set {
            let k = self.ink;
            rt.ink_rect(self.space, k[0], k[1], k[2] - k[0], k[3] - k[1], MARGIN)
        } else {
            bough_core::nrect::WRect::EMPTY
        };
        rt.bough.set_ink_handle_nodamage_wide(self.handle, ink);
    }
}

#[inline]
fn same(a: &Instance, b: &Instance) -> bool {
    a.pos == b.pos && a.size == b.size && a.color == b.color && a.color2 == b.color2 && a.radius == b.radius && a.bw == b.bw && a.kind == b.kind
}
