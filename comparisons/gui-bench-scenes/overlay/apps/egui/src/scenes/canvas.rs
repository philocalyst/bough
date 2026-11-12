//! The canvas: a toolbar (top panel), the layers panel (left panel, rows in `ScrollArea::show_rows`), the
//! inspector (right panel) and the drawing in the central panel, painted with `Painter` shapes: `RectShape`
//! with rounding and stroke, `Shadow` for drop shadows, a vertex-coloured `Mesh` for gradients and
//! `Painter::with_clip_rect` for the clips.

use eframe::egui::{
    self, Align, Align2, CentralPanel, Color32, Context, FontId, Frame, Key, Layout, Painter, Panel, Rect, Sense, Shape, Stroke, StrokeKind, Ui,
    epaint::{Mesh, Shadow},
    pos2, vec2,
};

use super::{color, color_argb, pressed, theme_index, toggle_theme};
use crate::heavy::canvas as c;
use crate::scroll::Scroll;

/// The canvas's data (owned by the app) and its interaction state.
pub struct Canvas {
    per_frame: usize,
    /// Every node, frame by frame, in index order.
    nodes: Vec<c::Node>,
    /// The layers panel's rows per frame: the frame's row and its layer rows.
    layer_rows: usize,
    /// Whether the selected node is at `MOVE_B`.
    moved: bool,
    /// The component whose row is under the pointer, set at the top of each pass.
    hot: Option<usize>,
    scroll: Scroll,
}

impl Canvas {
    pub fn new(nodes: usize) -> Self {
        let per_frame = nodes / c::FRAMES;
        let nodes = (0..c::FRAMES).flat_map(|f| (0..per_frame).map(move |i| c::node(f, i, per_frame))).collect();
        Self { per_frame, nodes, layer_rows: 1 + c::LAYER_ROWS.min(per_frame), moved: false, hot: None, scroll: Scroll::default() }
    }

    fn x(&self) -> f32 {
        if self.moved { c::MOVE_B } else { c::MOVE_A }
    }

    /// The layers panel: component rows (hover makes a component hot), then each frame's row and layer rows.
    fn layers(&mut self, ui: &mut Ui, t: usize) {
        let ink = color(c::INK[t]);
        let rows = c::COMPONENTS + c::FRAMES * self.layer_rows;
        let output = self.scroll.area().id_salt("layers").show_rows(ui, c::ROW_H, rows, |ui, range| {
            for row in range {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), c::ROW_H), Sense::hover());
                let painter = ui.painter();
                let mid = rect.left_center();
                if row < c::COMPONENTS {
                    if response.hovered() {
                        self.hot = Some(row);
                        painter.rect_filled(rect, 0.0, color(c::ROW_HOT[t]));
                    }
                    let swatch = Rect::from_center_size(mid + vec2(12.0 + 6.0, 0.0), vec2(12.0, 12.0));
                    painter.rect_filled(swatch, 0.0, color(c::comp(t, row, false)));
                    painter.text(mid + vec2(32.0, 0.0), Align2::LEFT_CENTER, format!("Component {}", row + 1), FontId::proportional(12.0), ink);
                } else {
                    let (f, i) = ((row - c::COMPONENTS) / self.layer_rows, (row - c::COMPONENTS) % self.layer_rows);
                    if i == 0 {
                        painter.text(mid + vec2(12.0, 0.0), Align2::LEFT_CENTER, format!("Frame {}", f + 1), FontId::proportional(12.0), ink);
                    } else {
                        let g = f * self.per_frame + i - 1;
                        painter.text(mid + vec2(24.0, 0.0), Align2::LEFT_CENTER, format!("Layer {g}"), FontId::proportional(12.0), ink);
                    }
                }
            }
        });
        self.scroll.end(&output);
    }

    /// The inspector's rows of the selected node.
    fn inspector(&self, ui: &mut Ui, t: usize) {
        let ink = color(c::INK[t]);
        ui.add_space(56.0 - c::TOOLBAR_H);
        for (label, value) in c::INSPECTOR.iter().zip(c::inspector(self.x())) {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), c::ROW_H), Sense::hover());
            let mid = rect.left_center();
            ui.painter().text(mid + vec2(12.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(12.0), color(c::MUTED));
            ui.painter().text(mid + vec2(120.0, 0.0), Align2::LEFT_CENTER, value.to_string(), FontId::proportional(12.0), ink);
        }
    }

    /// The frames and their nodes.
    fn draw(&self, painter: &Painter, t: usize) {
        let ink = color(c::INK[t]);
        let (_, dy, shadow) = c::FRAME_SHADOW;
        for f in 0..c::FRAMES {
            let (x, y, w, h) = c::frame_rect(f);
            let frame = Rect::from_min_size(pos2(x, y), vec2(w, h));
            painter.text(pos2(x, y - 16.0), Align2::LEFT_TOP, format!("Frame {}", f + 1), FontId::proportional(11.0), color(c::MUTED));
            let shadow = Shadow { offset: [0, dy as i8], blur: c::FRAME_SHADOW.1 as u8, spread: 0, color: color_argb(shadow) };
            painter.add(shadow.as_shape(frame, c::FRAME_RADIUS));
            painter.rect_filled(frame, c::FRAME_RADIUS, color(c::FRAME_BG[t]));
            // the nodes are clipped to the frame (a rectangle: egui clips to rectangles only), group nodes also to their group
            let clip = frame.intersect(painter.clip_rect());
            let in_frame = painter.with_clip_rect(clip);
            let groups: [Painter; 4] = std::array::from_fn(|q| {
                let (gx, gy, gw, gh) = c::group_rect(q);
                in_frame.with_clip_rect(Rect::from_min_size(frame.min + vec2(gx, gy), vec2(gw, gh)).intersect(clip))
            });
            // index order, later on top; the selected node (node 0 of frame 0) last, on top of the frame (the spec)
            let nodes = &self.nodes[f * self.per_frame..(f + 1) * self.per_frame];
            for node in nodes.iter().filter(|n| n.kind != c::Kind::Selected).chain(nodes.iter().filter(|n| n.kind == c::Kind::Selected)) {
                let (nx, ny, nw, nh) = node.rect;
                let (painter, origin) = match node.group {
                    Some(q) => (&groups[q], frame.min + vec2(c::group_rect(q).0, c::group_rect(q).1)),
                    None => (&in_frame, frame.min),
                };
                let nx = if node.kind == c::Kind::Selected { self.x() } else { nx };
                let rect = Rect::from_min_size(origin + vec2(nx, ny), vec2(nw, nh));
                self.node(painter, node, rect, t, ink);
            }
        }
    }

    fn node(&self, painter: &Painter, node: &c::Node, rect: Rect, t: usize, ink: Color32) {
        match node.kind {
            c::Kind::Shadowed { radius, fill, dy, blur } => {
                let shadow = Shadow { offset: [0, dy as i8], blur: blur as u8, spread: 0, color: color_argb(c::SHADOW) };
                painter.add(shadow.as_shape(rect, radius));
                painter.rect_filled(rect, radius, color(fill));
            }
            c::Kind::Gradient { top, bottom } => {
                let (top, bottom) = (color(top), color(bottom));
                let mut mesh = Mesh::default();
                mesh.colored_vertex(rect.left_top(), top);
                mesh.colored_vertex(rect.right_top(), top);
                mesh.colored_vertex(rect.right_bottom(), bottom);
                mesh.colored_vertex(rect.left_bottom(), bottom);
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                painter.add(Shape::mesh(mesh));
            }
            c::Kind::Bordered { fill, border } => {
                let (r, g, b, _) = color(fill).to_tuple();
                painter.rect(rect, 0.0, Color32::from_rgba_unmultiplied(r, g, b, 0x66), Stroke::new(1.0, color(border)), StrokeKind::Inside);
            }
            c::Kind::Label => {
                painter.text(rect.min, Align2::LEFT_TOP, format!("L{}", node.g), FontId::proportional(10.0), ink);
            }
            c::Kind::Instance { k } => {
                let hot = self.hot == Some(k);
                painter.rect(rect, 4.0, color(c::comp(t, k, hot)), Stroke::new(1.0, Color32::from_black_alpha(0x33)), StrokeKind::Inside);
            }
            c::Kind::Selected => {
                painter.rect_filled(rect, 6.0, color(c::SELECT_FILL));
                painter.rect_stroke(rect.expand(2.0), 8.0, Stroke::new(1.0, color(c::SELECT_OUTLINE)), StrokeKind::Middle);
            }
        }
    }
}

impl eframe::App for Canvas {
    fn raw_input_hook(&mut self, _: &Context, input: &mut egui::RawInput) {
        self.scroll.take(input);
    }

    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if pressed(&ctx, Key::T) {
            toggle_theme(&ctx);
        }
        if pressed(&ctx, Key::M) {
            self.moved = !self.moved;
        }
        let t = theme_index(&ctx);
        let bare = Frame::new().fill(color(c::CHROME[t]));

        Panel::top("toolbar").exact_size(c::TOOLBAR_H).frame(bare).show_separator_line(false).show(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add_space(12.0);
                ui.label(egui::RichText::new("Canvas").size(14.0).color(color(c::INK[t])));
            });
        });

        // the layers panel sets the hot component; the canvas below reads it in the same pass
        self.hot = None;
        Panel::left("layers").exact_size(c::PANEL_W).frame(bare).show_separator_line(false).show(ui, |ui| self.layers(ui, t));
        Panel::right("inspector")
            .exact_size(ui.max_rect().right() - c::INSPECTOR_X)
            .frame(bare)
            .show_separator_line(false)
            .show(ui, |ui| self.inspector(ui, t));

        CentralPanel::no_frame().show(ui, |ui| {
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, color(c::CANVAS_BG[t]));
            self.draw(&painter, t);
        });
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}
