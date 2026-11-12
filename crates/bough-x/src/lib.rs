//! bough-x (round 8): a UI as a typed dataflow from state to damage.
//!
//! * [`expr`] (the `bough-expr` crate): expressions as types; the stage joined from the operands' stages; the
//!   uniform block.
//! * [`store`]: archetype columns in 64-record pages; the finite-difference derivative; tile damage.
//! * [`kinds`]: the paint kinds, generic over their operands' expressions.
//! * [`sink`]: `view` drives a sink; rows of a list are plain tuples; relations carry their own journal.
//! * [`world`]: the driver.
//! * [`soft`]: a reference raster and an immediate-mode sink, both oracles.
//!
//! Nothing here depends on the façade's runtime.
pub use bough_expr as expr;
pub mod kinds;
pub mod sink;
pub mod soft;
pub mod store;
pub mod world;

pub mod prelude {
    pub use crate::expr::{Color, Expr, K, Kc, Lens, LensArr, U, Words, lens, rec, un, un_at};
    pub use crate::kinds::{Fill, Label, Num, Outline, Txt};
    pub use crate::sink::{FIXED, Journal, List, Rel, Row, Sink, View, hover};
    pub use crate::store::{NO_WORD, Rect, Space};
    pub use crate::world::World;
}
