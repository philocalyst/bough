//! bough: the public façade over bough-core (records, damage), bough-text (shaping into the glyph pool),
//! cephal (layout) and bough-metal's fused presenter. One region holds everything the GPU reads
//! (`rt.rs`); a frame's CPU work ends at the tile list (`app.rs`).
pub mod app;
pub mod backend;
pub mod changed;
pub mod color;
pub mod describe;
pub mod elem;
pub mod entity;
pub mod input;
pub mod lanes;
pub mod lay_deep;
pub mod layout;
pub mod list;
pub mod listview;
pub mod mlist;
pub mod reader;
pub mod rt;
pub mod select;
pub mod signal;
pub mod template;
pub mod text;

pub use bough_text::numeric::NumFmt;

pub mod prelude {
    pub use crate::app::{App, Config, View};
    pub use crate::color::{rgb, Color};
    pub use crate::elem::{at, bars, clip, each, quad, text, virtual_list, Element, IntoProp, Mounter, Quad, TextSrc};
    pub use crate::changed::Changed;
    pub use crate::entity::{entity_view, Entities, Entity};
    pub use crate::input::{no_hit, text_field, tooltip, Button, Ev, Interact, Key, Mods, TipStyle};
    pub use crate::lanes::{lanes, Lane};
    pub use crate::list::{List, Slot};
    pub use crate::listview::{list, list_with};
    pub use crate::reader::{latest, produce, Reader, Request};
    pub use crate::template::{clip_by, offset, port_list, scroll_y, template, wired_clip, Hooked, TNode, T_CLIP, T_FOCUS, T_HOOK};
    pub use crate::mlist::measured_list;
    pub use crate::rt::Caps;
    pub use crate::sig;
    pub use crate::signal::{Signal, Wired};
    pub use bough_text::numeric::NumFmt;
}
