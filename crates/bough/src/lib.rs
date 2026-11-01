//! bough: the public façade over bough-core (damage/cull), bough-text (shaping) and the Metal backend.
pub mod app;
pub mod backend;
pub mod color;
pub mod elem;
pub mod repair;
pub mod rt;
pub mod signal;
pub mod text;

pub mod prelude {
    pub use crate::app::{App, Config, View};
    pub use crate::color::{rgb, Color};
    pub use crate::elem::{at, clip, bars, each, quad, text, virtual_list, Element, IntoProp, Mounter, Quad};
    pub use crate::signal::Signal;
}
