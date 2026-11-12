//! The sheet scene: a toolbar, a header row, a body of `rows` rows of 10 cells in GPUI's `uniform_list`
//! (only the visible rows are built), and a footer with the column totals.
//!
//! State lives in the view: the theme, the hot column and whether the edit is applied. A hover on a header cell
//! sets the hot column and notifies the view; the view's next render builds the visible rows again and every cell
//! of the hot column reads the column from the view.

use gpui_kit::*;

use super::spec::{FONT_FAMILY, sheet as s};
use super::{CONTEXT, ToggleEdit, ToggleTheme, c};

pub struct Sheet {
    rows: usize,
    /// 0 light, 1 dark.
    theme: usize,
    /// The column under the pointer, if the pointer is over a header cell.
    hot: Option<usize>,
    /// Whether the `e` edit is applied to the cell `EDIT`.
    edited: bool,
    /// The sum of every column before the edit.
    totals: [f64; s::COLS],
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
}

impl Sheet {
    pub fn new(rows: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut totals = [0.0; s::COLS];
        for r in 0..rows {
            for (col, total) in totals.iter_mut().enumerate() {
                *total += s::value(r, col);
            }
        }
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            rows,
            theme: 0,
            hot: None,
            edited: false,
            totals,
            scroll: UniformListScrollHandle::new(),
            focus,
        }
    }

    /// The value shown in a cell: the edit adds `EDIT_DELTA` to its cell.
    fn shown(&self, row: usize, col: usize) -> f64 {
        let v = s::value(row, col);
        if self.edited && (row, col) == s::EDIT {
            v + s::EDIT_DELTA
        } else {
            v
        }
    }

    fn total(&self, col: usize) -> f64 {
        if self.edited && col == s::EDIT.1 {
            self.totals[col] + s::EDIT_DELTA
        } else {
            self.totals[col]
        }
    }

    /// The pointer entered or left header cell `col`.
    fn hover(&mut self, col: usize, hovered: bool, cx: &mut Context<Self>) {
        let hot = if hovered {
            Some(col)
        } else if self.hot == Some(col) {
            None
        } else {
            return;
        };
        if hot != self.hot {
            self.hot = hot;
            cx.notify();
        }
    }

    /// The toolbar: the title and the totals label.
    fn toolbar(&self) -> Div {
        let t = self.theme;
        div()
            .flex_none()
            .w_full()
            .h(px(s::TOOLBAR_H))
            .relative()
            .bg(c(s::CHROME[t]))
            .child(
                div()
                    .absolute()
                    .left(px(12.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .text_size(px(14.0))
                    .text_color(c(s::INK[t]))
                    .child("Sheet 1"),
            )
            .child(
                div()
                    .absolute()
                    .left(px(100.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .text_size(px(12.0))
                    .text_color(c(s::GUTTER_INK))
                    .child("Σ totals"),
            )
    }

    /// The header row: the gutter cell and one hoverable cell per column.
    fn header(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let cells = (0..s::COLS).map(|col| {
            let bg = if self.hot == Some(col) { s::HEADER_HOT } else { s::CHROME[t] };
            div()
                .id(("header", col))
                .flex_none()
                .w(px(s::CELL_W))
                .h_full()
                .flex()
                .items_center()
                .pl(px(s::TEXT_PAD))
                .text_size(px(12.0))
                .text_color(c(s::INK[t]))
                .bg(c(bg))
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| this.hover(col, *hovered, cx)))
                .child(s::COLUMN_NAMES[col])
        });
        div()
            .flex()
            .flex_row()
            .flex_none()
            .w_full()
            .h(px(s::HEADER_H))
            .bg(c(s::CHROME[t]))
            .child(div().flex_none().w(px(s::GUTTER_W)).h_full().bg(c(s::CHROME[t])))
            .children(cells)
    }

    /// One body row: the gutter and the cells.
    fn row(&self, r: usize) -> Div {
        let t = self.theme;
        let cells = (0..s::COLS).map(|col| {
            let hot = self.hot == Some(col);
            let cell = div()
                .flex_none()
                .w(px(s::CELL_W))
                .h(px(s::CELL_H))
                .flex()
                .items_center()
                .pl(px(s::TEXT_PAD))
                .text_size(px(12.0))
                .text_color(c(s::INK[t]))
                .bg(c(s::fill(t, hot, r)));
            let cell = if (r, col) == (0, 0) {
                // the selected cell: a border inside its rectangle, the text keeps its place
                cell.border(px(s::SELECT_BORDER)).border_color(c(s::SELECT)).pl(px(s::TEXT_PAD - s::SELECT_BORDER))
            } else {
                cell
            };
            cell.child(s::text(self.shown(r, col)))
        });
        div()
            .flex()
            .flex_row()
            .flex_none()
            .w_full()
            .h(px(s::CELL_H))
            .child(
                div()
                    .flex_none()
                    .w(px(s::GUTTER_W))
                    .h_full()
                    .flex()
                    .items_center()
                    .pl(px(s::TEXT_PAD))
                    .text_size(px(11.0))
                    .text_color(c(s::GUTTER_INK))
                    .bg(c(s::CHROME[t]))
                    .child((r + 1).to_string()),
            )
            .children(cells)
    }

    /// The footer: "Σ" in the gutter and the total of every column.
    fn footer(&self) -> Div {
        let t = self.theme;
        let cells = (0..s::COLS).map(|col| {
            div()
                .flex_none()
                .w(px(s::CELL_W))
                .h_full()
                .flex()
                .items_center()
                .pl(px(s::TEXT_PAD))
                .text_size(px(12.0))
                .text_color(c(s::INK[t]))
                .child(s::text(self.total(col)))
        });
        div()
            .flex()
            .flex_row()
            .flex_none()
            .w_full()
            .h(px(s::FOOTER_H))
            .bg(c(s::CHROME[t]))
            .child(
                div()
                    .flex_none()
                    .w(px(s::GUTTER_W))
                    .h_full()
                    .flex()
                    .items_center()
                    .pl(px(s::TEXT_PAD))
                    .text_size(px(11.0))
                    .text_color(c(s::GUTTER_INK))
                    .child("Σ"),
            )
            .children(cells)
    }
}

impl Render for Sheet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let body = uniform_list(
            "sheet-body",
            self.rows,
            cx.processor(|this, range: std::ops::Range<usize>, _, _| range.map(|r| this.row(r)).collect::<Vec<_>>()),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ToggleTheme, _, cx| {
                this.theme = 1 - this.theme;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleEdit, _, cx| {
                this.edited = !this.edited;
                cx.notify();
            }))
            .size_full()
            .flex()
            .flex_col()
            .font_family(FONT_FAMILY)
            .bg(c(s::PAGE[t]))
            .child(self.toolbar())
            .child(self.header(cx))
            .child(div().flex_1().min_h_0().w_full().bg(c(s::PAGE[t])).child(body))
            .child(self.footer())
    }
}
