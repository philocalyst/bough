#![feature(type_info, ptr_metadata, rustc_attrs, const_trait_impl, const_cmp, arbitrary_self_types_pointers, f16)]
#![cfg_attr(target_arch = "aarch64", feature(stdarch_neon_f16))]
#![allow(internal_features)]
//! bough: the public façade over bough-core (records, damage), bough-text (shaping into the glyph pool),
//! cephal (layout) and bough-metal's fused presenter. One region holds everything the GPU reads
//! (`rt.rs`); a frame's CPU work ends at the tile list (`app.rs`).
pub mod app;
pub mod backend;
pub mod boxops;
pub mod changed;
pub mod color;
pub mod cpu_raster;
pub mod deco;
pub mod describe;
pub mod elem;
pub mod entity;
pub mod input;
pub mod inspect;
pub mod lanes;
pub mod lay_deep;
pub mod layout;
pub mod ledger;
pub mod list;
pub mod listview;
pub mod ltemplate;
pub mod mlist;
pub mod reader;
pub mod reflect;
pub mod repair;
pub mod rt;
pub mod seal;
pub mod select;
pub mod signal;
pub mod template;
pub mod text;
pub mod transition;
pub mod ui;

pub use bough_text::numeric::NumFmt;
/// The OS counters (instructions, memory) and the frame contract, for harnesses and benchmarks on every platform.
pub use bough_frame as frame;
pub use bough_sys as sys;

/// The application surface (`designs/reactivity/final`): views, state, elements in flow (laid out by cephal),
/// routes, and `Application::run`. No coordinates: `use bough::raw::*;`.
pub mod prelude {
    pub use crate::app::View;
    pub use crate::changed::Changed;
    pub use crate::color::{rgb, Color};
    pub use crate::elem::{each, Element, IntoProp};
    pub use crate::entity::{Entities, Entity};
    pub use crate::list::{List, Slot};
    pub use crate::listview::list;
    pub use crate::reader::{boundary, Reader, Request};
    pub use crate::input::{text_field, tooltip, Interact, Key, Mods};
    pub use crate::layout::{block, column, para, row, Align, Justify, Styled};
    pub use crate::sig;
    pub use crate::signal::Signal;
    pub use crate::transition::{Curve, Ease};
    pub use crate::ui::{button, div, image, text, text_input, Application, Flex, Window};
}

/// The scene-level surface the benchmark ports are written in: absolutely placed quads and text, templates,
/// measured lists, the `App` driver. `use bough::raw::*;`.
pub mod raw {
    pub use crate::transition::{Curve, Ease, Lerp};
    pub use crate::deco::{deco_box, svg_image, Deco, Grad, Shadow, Xf};
    pub use crate::app::{App, Config, View};
    pub use crate::color::{rgb, Color};
    pub use crate::elem::{at, bars, clip, each, quad, text, virtual_list, Element, IntoProp, Mounter, Quad, TextSrc};
    pub use crate::changed::Changed;
    pub use crate::entity::{entity_view, Entities, Entity};
    pub use crate::input::{no_hit, text_field, tooltip, Button, Ev, Interact, Key, Mods, TipStyle};
    pub use crate::ltemplate::{ltemplate, wire_restyle, Decls, LCtx, LHooked, LNode, P};
    pub use crate::template::{anchor, clip_by, offset, port_list, port_list_in, scroll_y, template, wired_clip, Hooked, TNode, T_CLIP, T_FOCUS, T_HOOK};
    pub use crate::lanes::{lanes, Lane};
    pub use crate::list::{List, Slot};
    pub use crate::listview::{index_list, list, list_with};
    pub use crate::reader::{boundary, latest, produce, Reader, Request};
    pub use crate::rt::Caps;
    pub use crate::sig;
    pub use crate::signal::{Signal, Wired};
    pub use bough_text::numeric::NumFmt;
}
