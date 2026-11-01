//! bough: the public façade over bough-core (records, damage), bough-text (shaping into the glyph pool),
//! cephal (layout) and bough-metal's fused presenter. One region holds everything the GPU reads
//! (`rt.rs`); a frame's CPU work ends at the tile list (`app.rs`).
pub mod app;
pub mod backend;
pub mod color;
pub mod elem;
pub mod input;
pub mod lay_deep;
pub mod layout;
pub mod ltemplate;
pub mod mlist;
pub mod rt;
pub mod signal;
pub mod template;
pub mod text;

pub use bough_text::numeric::NumFmt;

pub mod prelude {
    pub use crate::app::{App, Config, View};
    pub use crate::color::{rgb, Color};
    pub use crate::elem::{at, bars, clip, each, quad, text, virtual_list, Element, IntoProp, Mounter, Quad, TextSrc};
    pub use crate::input::{no_hit, text_field, tooltip, Button, Ev, Interact, Key, Mods, TipStyle};
    pub use crate::ltemplate::{ltemplate, wire_restyle, Decls, LCtx, LHooked, LNode, P};
    pub use crate::template::{anchor, clip_by, offset, port_list, port_list_in, scroll_y, template, wired_clip, Hooked, TNode, T_CLIP, T_FOCUS, T_HOOK};
    pub use crate::mlist::measured_list;
    pub use crate::rt::Caps;
    pub use crate::sig;
    pub use crate::signal::{Signal, Wired};
    pub use bough_text::numeric::NumFmt;
}
