//! What an inspector reads from a running app (round 4, input agent): the public, read-only view of the
//! runtime the inspector app is built on (INSPECTOR.md logs why each accessor exists). Reflection of the
//! owner's state comes from the api agent's typed reflection, not from here.
use crate::app::{App, View};
use bough_core::NRect;

/// One record as the inspector shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecordView {
    pub pos: usize,
    pub depth: u16,
    /// The kind byte (low bits: quad / text / box; high bits: interactive, focusable, pointer-transparent).
    pub kind: u8,
    /// Absolute ink (device px; a transformed record's: its transformed bounding box).
    pub ink: NRect,
    pub handle: u32,
    pub clip: u16,
    /// The layout box (absolute device px: x, y, w, h), for records that are layout nodes.
    pub layout: Option<[f32; 4]>,
    /// Whether the record is in a transformed subtree.
    pub transformed: bool,
}

impl RecordView {
    pub fn kind_name(&self) -> &'static str {
        match self.kind & 0x1F {
            crate::rt::REC_QUAD => "quad",
            crate::rt::REC_TEXT => "text",
            crate::rt::REC_BOX => "box",
            _ => "record",
        }
    }
}

impl<V: View> App<V> {
    /// Records in the tree (preorder).
    pub fn record_count(&self) -> usize {
        self.rt.bough.len()
    }

    /// Record `pos` with its layout box.
    pub fn record_view(&self, pos: usize) -> RecordView {
        let r = self.rt.bough.record(pos);
        let layout = (r.handle != 0)
            .then(|| self.rt.lay.info.get(r.handle as usize))
            .flatten()
            .filter(|li| li.live && li.written)
            .map(|li| {
                let o = self.rt.space(li.space);
                [o[0] + li.ax, o[1] + li.ay, li.aw, li.ah]
            });
        RecordView { pos, depth: r.depth, kind: r.kind, ink: r.ink, handle: r.handle, clip: r.clip, layout, transformed: self.rt.bough.xform_at(pos).is_some() }
    }

    /// The topmost pickable record at window point (x, y) (points), as hit testing answers it.
    pub fn pick_record(&self, x: f32, y: f32) -> Option<usize> {
        let s = self.rt.scale;
        self.rt.bough.hit_top((x * s) as i32, (y * s) as i32)
    }

    /// The ancestors of `pos` (nearest first).
    pub fn record_ancestors(&self, pos: usize) -> Vec<usize> {
        let mut v = Vec::new();
        self.rt.bough.ancestors(pos, &mut v);
        v
    }

    /// The tiles the last drawn frame repaired: per 32-px tile row, a bitmask of tile columns (every tile of
    /// the window when the frame was a full repaint).
    pub fn last_repair(&self) -> Vec<u128> {
        let (rows, full) = self.repaired_rows();
        if !full {
            return rows.to_vec();
        }
        let tx = (self.rt.win_w as usize).div_ceil(32);
        let all = if tx >= 128 { u128::MAX } else { (1u128 << tx) - 1 };
        vec![all; (self.rt.win_h as usize).div_ceil(32)]
    }
}
