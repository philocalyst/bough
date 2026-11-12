//! Scrollable overflow contributions.

use crate::geometry::{Point, Rect, Size};
use crate::style::{Contain, Overflow};

/// The rectangle a child adds to its parent's scrollable overflow, in scroll-origin coordinates.
///
/// A scroll container or contained box contributes only its border box; `clip` clips its own
/// axis; `visible` propagates the child's own overflow. Boxes wholly before the scroll origin of a
/// scroll container are unreachable and contribute nothing.
#[inline]
pub fn scrollable_overflow_contribution(
    location: Point<f32>,
    size: Size<f32>,
    child_overflow: Rect<f32>,
    overflow: Point<Overflow>,
    contain: Contain,
    parent_is_scroll_container: bool,
) -> Rect<f32> {
    let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
    let contained = contain.contains_scrollable_overflow();
    let propagates_x = !is_scroll_container && !contained && overflow.x == Overflow::Visible;
    let propagates_y = !is_scroll_container && !contained && overflow.y == Overflow::Visible;
    let (start_x, end_x) = if propagates_x { contribution_extent(size.width, child_overflow.left, child_overflow.right) } else { (0.0, size.width) };
    let (start_y, end_y) = if propagates_y { contribution_extent(size.height, child_overflow.top, child_overflow.bottom) } else { (0.0, size.height) };
    if end_x <= 0.0 || end_y <= 0.0 {
        return Rect::ZERO;
    }
    let contribution = Rect {
        left: location.x + start_x,
        right: location.x + end_x,
        top: location.y + start_y,
        bottom: location.y + end_y,
    };
    if parent_is_scroll_container && (contribution.right <= 0.0 || contribution.bottom <= 0.0) {
        Rect::ZERO
    } else {
        contribution
    }
}

/// What [`scrollable_overflow_contribution`] reads of a child's overflow rectangle on an axis where it
/// propagates: the start clamped to the border box's start and the end to its end, from the size the
/// child was placed at on that axis and the rectangle's start and end. With the location and size
/// unchanged, two rectangles whose extents are equal bit for bit contribute the same rectangle, so the
/// parent cannot tell them apart (see [`crate::tree::LayoutOutput::same_contribution`]).
#[inline(always)]
pub fn contribution_extent(size: f32, start: f32, end: f32) -> (f32, f32) {
    (start.min(0.0), size.max(end))
}

/// Union of two overflow rects; both always contain the origin.
#[inline]
pub fn union(a: Rect<f32>, b: Rect<f32>) -> Rect<f32> {
    Rect { left: a.left.min(b.left), right: a.right.max(b.right), top: a.top.min(b.top), bottom: a.bottom.max(b.bottom) }
}
