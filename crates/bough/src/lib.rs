//! bough: the public façade over bough-core (records, damage), bough-text (shaping into the glyph pool),
//! cephal (layout) and bough-metal's fused presenter. One region holds everything the GPU reads
//! (`rt.rs`); a frame's CPU work ends at the tile list (`app.rs`).
pub mod app;
pub mod backend;
pub mod color;
pub mod describe;
pub mod elem;
pub mod lay_deep;
pub mod layout;
pub mod list;
pub mod listview;
pub mod mlist;
pub mod rt;
pub mod signal;
pub mod text;

pub use bough_text::numeric::NumFmt;

pub mod prelude {
    pub use crate::app::{App, Config, View};
    pub use crate::color::{rgb, Color};
    pub use crate::elem::{at, bars, clip, each, quad, text, virtual_list, Element, IntoProp, Mounter, Quad, TextSrc};
    pub use crate::list::{List, Slot};
    pub use crate::listview::{list, list_with};
    pub use crate::mlist::measured_list;
    pub use crate::rt::Caps;
    pub use crate::sig;
    pub use crate::signal::{Signal, Wired};
    pub use bough_text::numeric::NumFmt;
}
