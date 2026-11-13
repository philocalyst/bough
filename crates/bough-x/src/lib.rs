//! bough-x (round 8): a UI as a typed dataflow from state to damage.
//!
//! * [`expr`] (the `bough-expr` crate): expressions as types; the stage joined from the operands' stages; the
//!   uniform block.
//! * [`store`]: archetype columns in 64-record pages; the finite-difference derivative; tile damage.
//! * [`kinds`]: the paint kinds, generic over their operands' expressions.
//! * [`sink`]: `view` drives a sink; rows of a list are plain tuples; relations carry their own journal.
//! * [`world`]: the driver.
//! * [`reflect`]: the typed walk every tool reads (tests, theming, accessibility, events, an inspector).
//! * [`soft`]: a reference raster and an immediate-mode sink, both oracles.
//!
//! Nothing here depends on the façade's runtime.
#![feature(trusted_random_access, auto_traits, negative_impls)]
#![allow(internal_features)]
pub use bough_expr as expr;
pub mod el;
pub mod event;
pub mod kinds;
pub mod reflect;
pub mod sink;
pub mod soft;
pub mod store;
pub mod world;

pub mod prelude {
    pub use crate::expr::{Color, Cx, Expr, K, Kc, Lens, LensArr, Pick, PtrX, PtrY, Time, U, U_TIME, Words, lens, rec, un, un_at};
    pub use crate::kinds::{Fill, Glide, Label, Num, Outline, Tween, Txt};
    pub use crate::event::{Button, Ev, Interact, Key, Mods, hover};
    pub use crate::sink::{FIXED, Journal, List, Rel, Row, Sink, View};
    pub use crate::store::{NO_WORD, Rect, Space};
    pub use crate::world::World;
}
