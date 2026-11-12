//! The sheet: a toolbar and a header (top panels), a footer (bottom panel) and the body in the central
//! panel, in `ScrollArea::show_rows` (egui's own virtualisation: only the rows in the viewport are laid out).

use eframe::egui::{
    self, Align, Color32, Context, Frame, Key, Layout, Panel, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, vec2,
};

use super::{color, pressed, theme_index, toggle_theme};
use crate::heavy::sheet as s;
use crate::scroll::Scroll;

/// The sheet's data (owned by the app, as in any immediate mode app) and its interaction state.
pub struct Sheet {
    rows: usize,
    /// The values, row-major.
    values: Vec<f64>,
    /// The total of each column.
    totals: [f64; s::COLS],
    /// Whether the `e` edit is applied.
    edited: bool,
    /// The column under the pointer in the header, set at the top of each pass.
    hot: Option<usize>,
    scroll: Scroll,
}

impl Sheet {
    pub fn new(rows: usize) -> Self {
        let values: Vec<f64> = (0..rows * s::COLS).map(|i| s::value(i / s::COLS, i % s::COLS)).collect();
        let mut sheet = Self { rows, values, totals: [0.0; s::COLS], edited: false, hot: None, scroll: Scroll::default() };
        for c in 0..s::COLS {
            sheet.totals[c] = sheet.total(c);
        }
        sheet
    }

    fn total(&self, col: usize) -> f64 {
        (0..self.rows).map(|r| self.values[r * s::COLS + col]).sum()
    }

    /// `e`: toggles the edited cell between its value and its value plus the delta, and its column's total follows.
    fn toggle_edit(&mut self) {
        let (r, c) = s::EDIT;
        if r >= self.rows {
            return;
        }
        self.edited = !self.edited;
        let base = s::value(r, c);
        self.values[r * s::COLS + c] = if self.edited { base + s::EDIT_DELTA } else { base };
        self.totals[c] = self.total(c);
    }
}

/// A text label of `size` px and `ink`, left aligned and centred vertically in the cell `ui` is a slot of.
fn text(ui: &mut Ui, text: impl Into<String>, size: f32, ink: Color32) {
    ui.label(RichText::new(text).size(size).color(ink));
}

/// A cell of `w` x `h` with a `fill` and its text at `s::TEXT_PAD`, centred vertically.
fn cell(ui: &mut Ui, w: f32, h: f32, fill: Color32, add: impl FnOnce(&mut Ui)) -> Rect {
    ui.allocate_ui_with_layout(vec2(w, h), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(vec2(w, h));
        ui.painter().rect_filled(ui.max_rect(), 0.0, fill);
        ui.add_space(s::TEXT_PAD);
        add(ui);
    })
    .response
    .rect
}

impl eframe::App for Sheet {
    fn raw_input_hook(&mut self, _: &Context, input: &mut egui::RawInput) {
        self.scroll.take(input);
    }

    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if pressed(&ctx, Key::T) {
            toggle_theme(&ctx);
        }
        if pressed(&ctx, Key::E) {
            self.toggle_edit();
        }
        let t = theme_index(&ctx);
        let (chrome, ink) = (color(s::CHROME[t]), color(s::INK[t]));
        let gutter_ink = color(s::GUTTER_INK);
        let bare = Frame::new().fill(chrome);

        Panel::top("toolbar").exact_size(s::TOOLBAR_H).frame(bare).show_separator_line(false).show(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add_space(12.0);
                ui.allocate_ui_with_layout(vec2(100.0 - 12.0, s::TOOLBAR_H), Layout::left_to_right(Align::Center), |ui| {
                    ui.set_min_size(vec2(100.0 - 12.0, s::TOOLBAR_H));
                    text(ui, "Sheet 1", 14.0, ink)
                });
                text(ui, "\u{3a3} totals", 12.0, gutter_ink);
            });
        });

        // the header: the pointer over a cell makes its column hot, read through `Response::hovered`
        self.hot = None;
        Panel::top("header").exact_size(s::HEADER_H).frame(bare).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                cell(ui, s::GUTTER_W, s::HEADER_H, chrome, |_| {});
                for (c, name) in s::COLUMN_NAMES.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(vec2(s::CELL_W, s::HEADER_H), Sense::hover());
                    let hot = response.hovered();
                    if hot {
                        self.hot = Some(c);
                    }
                    ui.painter().rect_filled(rect, 0.0, if hot { color(s::HEADER_HOT) } else { chrome });
                    // the label sits in a child of the cell's rectangle (a child does not advance the row)
                    let mut label = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
                    label.add_space(s::TEXT_PAD);
                    text(&mut label, *name, 12.0, ink);
                }
            });
        });

        Panel::bottom("footer").exact_size(s::FOOTER_H).frame(bare).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                cell(ui, s::GUTTER_W, s::FOOTER_H, chrome, |ui| text(ui, "\u{3a3}", 12.0, ink));
                for total in self.totals {
                    cell(ui, s::CELL_W, s::FOOTER_H, chrome, |ui| text(ui, s::text(total), 12.0, ink));
                }
            });
        });

        // the body: only the rows of the viewport are built; right of the last cell it is the page colour
        egui::CentralPanel::no_frame().show(ui, |ui| {
            ui.painter().rect_filled(ui.max_rect(), 0.0, color(s::PAGE[t]));
            let output = self.scroll.area().show_rows(ui, s::CELL_H, self.rows, |ui, range| {
                for r in range {
                    ui.horizontal(|ui| {
                        cell(ui, s::GUTTER_W, s::CELL_H, chrome, |ui| text(ui, (r + 1).to_string(), 11.0, gutter_ink));
                        for c in 0..s::COLS {
                            let hot = self.hot == Some(c);
                            let rect = cell(ui, s::CELL_W, s::CELL_H, color(s::fill(t, hot, r)), |ui| {
                                text(ui, s::text(self.values[r * s::COLS + c]), 12.0, ink);
                            });
                            if (r, c) == (0, 0) {
                                ui.painter().rect_stroke(rect, 0.0, Stroke::new(s::SELECT_BORDER, color(s::SELECT)), StrokeKind::Inside);
                            }
                        }
                    });
                }
            });
            self.scroll.end(&output);
        });
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}
