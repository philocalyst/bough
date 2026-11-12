//! gui-bench's app for bough on the round-8 core.
//!
//! * `spec`, `cli`: gui-bench's shared files, byte for byte (the same files the façade's port includes).
//! * `scene`: the adapter between the host loop and a `bough-x` world: input to handlers, settle, paint by spans.
//! * `rows`, `table`, `search`, `spinner`: the scenarios. State is plain structs and handlers are methods; the
//!   views are written against the core (rows as tuples of records, layout by arithmetic) until the element
//!   layer in the façade's idiom lands (ROUND8 Amendment 2), which replaces the `view` bodies and nothing else.
#[path = "../../bough-guibench/src/cli.rs"]
pub mod cli;
#[path = "../../bough-guibench/src/spec.rs"]
pub mod spec;

pub mod rows;
pub mod scene;
pub mod search;
pub mod spinner;
pub mod table;

use bough_x::prelude::*;

/// The window's geometry: buffer pixels and the scale they were multiplied by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geo {
    pub w: i32,
    pub h: i32,
    pub k: i32,
}
impl Geo {
    pub const fn new(w: i32, h: i32, k: i32) -> Geo {
        Geo { w, h, k }
    }
    pub fn row_h(&self) -> i32 {
        spec::ROW_HEIGHT as i32 * self.k
    }
    pub fn header_h(&self) -> i32 {
        spec::HEADER_HEIGHT as i32 * self.k
    }
    pub fn pad(&self) -> i32 {
        spec::PADDING_X as i32 * self.k
    }
    /// One row of a list, relative to the row: background (the whole row), bottom border, label, value.
    pub fn row_shape(&self) -> [Rect; 4] {
        let (w, k, h) = (self.w, self.k, self.row_h());
        let p = self.pad();
        [Rect::new(0, 0, w, h), Rect::new(0, h - k, w, k), Rect::new(p, 0, w / 2 - p, h - k), Rect::new(w / 2, 0, w - w / 2 - p, h - k)]
    }
    /// The header: background, bottom border, status text.
    pub fn header_shape(&self) -> [Rect; 3] {
        let (w, k, h) = (self.w, self.k, self.header_h());
        [Rect::new(0, 0, w, h), Rect::new(0, h - k, w, k), Rect::new(self.pad(), 0, w / 2 - self.pad(), h - k)]
    }
    /// The list's space under a header of `top` pixels.
    pub fn body(&self, top: i32) -> Rect {
        Rect { x0: 0, y0: top, x1: self.w, y1: self.h.max(top) }
    }
    /// The furthest a list of `n` rows scrolls under a header of `top` pixels.
    pub fn max_scroll(&self, n: usize, top: i32) -> i32 {
        (n as i64 * self.row_h() as i64 - (self.h - top) as i64).clamp(0, i32::MAX as i64) as i32
    }
}

pub const NONE: u32 = u32::MAX;
