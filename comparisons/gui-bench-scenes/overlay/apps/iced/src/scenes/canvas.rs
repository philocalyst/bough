//! The design canvas: a toolbar, a layers panel, a surface of 20 frames full of nodes, and an inspector.
//!
//! The surface is a stack of positioned containers, not a `canvas::Program`: the spec's drop shadows are
//! `container::Style::shadow` (a blurred shadow that iced's renderers draw), its gradients are
//! `Background::Gradient` and its clips are `Stack::clip`; iced's `canvas::Frame` has none of the first (no
//! blurred shadow). iced has no absolute positioning in its layout widgets other than `pin`, which places one
//! widget at a point of its parent, and `stack`, which layers widgets, so a node is `pin(container)` and a frame is
//! a `stack` of its nodes. The Elm loop is as in the sheet: hover over a component row, `t` and `m` are messages
//! that change the state, and `view` describes the whole scene again from it.

use iced::alignment::Vertical;
use iced::gradient::Linear;
use iced::widget::text::Wrapping;
use iced::widget::{
    Space, column, container, mouse_area, pin, row, scrollable, stack, text,
};
use iced::{Background, Border, Color, Element, Length, Radians, Shadow, Subscription, Theme, Vector, keyboard, theme};

use super::{argb, key_of, rgb};
use crate::rows::scrollbar;
use crate::rows::wheel::Wheel;
use crate::scenes::canvas::{self as spec, Kind, Node};
use crate::scenes::THEME_KEY;

/// The state of the canvas.
struct Canvas {
    /// The nodes of every frame, from the spec.
    frames: Vec<Vec<Node>>,
    /// 0 light, 1 dark.
    theme: usize,
    /// The component whose row the pointer is over.
    hot: Option<usize>,
    /// The selected node is at `MOVE_B`.
    moved: bool,
}

#[derive(Debug, Clone)]
enum Message {
    /// The pointer entered component row `k`.
    Entered(usize),
    /// The pointer left component row `k`.
    Left(usize),
    /// A key press that no widget took.
    Key(String),
}

/// Opens the window and runs the event loop until the window closes.
pub fn run(nodes: usize, title: &'static str) -> iced::Result {
    iced::application(move || Canvas::new(nodes), Canvas::update, Canvas::view)
        .subscription(Canvas::subscription)
        .title(title)
        .theme(Canvas::theme)
        .style(Canvas::style)
        .window(super::window())
        .run()
}

impl Canvas {
    fn new(nodes: usize) -> Self {
        let per_frame = nodes / spec::FRAMES;
        Self {
            frames: (0..spec::FRAMES)
                .map(|f| (0..per_frame).map(|i| spec::node(f, i, per_frame)).collect())
                .collect(),
            theme: 0,
            hot: None,
            moved: false,
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Entered(k) => self.hot = Some(k),
            // entering the next row may be heard before leaving the last one
            Message::Left(k) => {
                if self.hot == Some(k) {
                    self.hot = None;
                }
            }
            Message::Key(key) if key == THEME_KEY => self.theme = 1 - self.theme,
            Message::Key(key) if key == spec::MOVE_KEY => self.moved = !self.moved,
            Message::Key(_) => {}
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().filter_map(|event| key_of(event).map(Message::Key))
    }

    fn theme(&self) -> Theme {
        if self.theme == 0 { Theme::Light } else { Theme::Dark }
    }

    fn style(&self, _: &Theme) -> theme::Style {
        theme::Style {
            background_color: rgb(spec::CANVAS_BG[self.theme]),
            text_color: rgb(spec::INK[self.theme]),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        column![
            self.toolbar(),
            row![self.layers(), self.surface(), self.inspector()].height(Length::Fill),
        ]
        .into()
    }

    fn toolbar(&self) -> Element<'_, Message> {
        let t = self.theme;
        container(text("Canvas").size(14).color(rgb(spec::INK[t])))
            .width(Length::Fill)
            .height(spec::TOOLBAR_H)
            .padding([0.0, 12.0])
            .align_y(Vertical::Center)
            .style(fill(rgb(spec::CHROME[t])))
            .into()
    }

    /// The layers panel: the components, then every frame and its first layers, in a vertical scroll container.
    fn layers(&self) -> Element<'_, Message> {
        let t = self.theme;
        let ink = rgb(spec::INK[t]);
        let components = (0..spec::COMPONENTS).map(|k| {
            let background = if self.hot == Some(k) {
                Some(rgb(spec::ROW_HOT[t]))
            } else {
                None
            };
            let swatch = container(Space::new())
                .width(12)
                .height(12)
                .style(fill(rgb(spec::comp(t, k, false))));
            let line = container(
                row![swatch, text(format!("Component {}", k + 1)).size(12).color(ink)]
                    .spacing(8)
                    .align_y(Vertical::Center),
            )
            .width(Length::Fill)
            .height(spec::ROW_H)
            .padding([0.0, 12.0])
            .align_y(Vertical::Center)
            .style(move |_| container::Style {
                background: background.map(Background::Color),
                ..container::Style::default()
            });
            mouse_area(line)
                .on_enter(Message::Entered(k))
                .on_exit(Message::Left(k))
                .into()
        });
        let frames = self.frames.iter().enumerate().flat_map(|(f, nodes)| {
            let frame = layer_row(format!("Frame {}", f + 1), 12.0, ink);
            let layers = nodes
                .iter()
                .take(spec::LAYER_ROWS)
                .map(move |node| layer_row(format!("Layer {}", node.g), 24.0, ink));
            std::iter::once(frame).chain(layers)
        });
        let rows = iced::widget::Column::with_children(components.chain(frames)).width(Length::Fill);
        container(Wheel::new(
            scrollable(rows)
                .direction(scrollable::Direction::Vertical(scrollbar()))
                .width(Length::Fill)
                .height(Length::Fill),
        ))
        .width(spec::PANEL_W)
        .height(Length::Fill)
        .style(fill(rgb(spec::CHROME[t])))
        .into()
    }

    /// The surface: the frames and their titles, each frame a stack of its nodes.
    fn surface(&self) -> Element<'_, Message> {
        let t = self.theme;
        let mut layers: Vec<Element<'_, Message>> = vec![Space::new().width(Length::Fill).height(Length::Fill).into()];
        for f in 0..spec::FRAMES {
            let (x, y, w, h) = spec::frame_rect(f);
            // window coordinates to the surface's
            let (x, y) = (x - spec::PANEL_W, y - spec::TOOLBAR_H);
            layers.push(
                pin(container(text(format!("Frame {}", f + 1)).size(11).color(rgb(spec::MUTED)))
                    .height(16)
                    .align_y(Vertical::Center))
                .x(x)
                .y(y - 16.0)
                .into(),
            );
            let (dy, blur, color) = spec::FRAME_SHADOW;
            let frame = container(self.frame(f))
                .width(w)
                .height(h)
                .style(move |_| container::Style {
                    background: Some(Background::Color(rgb(spec::FRAME_BG[t]))),
                    border: Border {
                        radius: spec::FRAME_RADIUS.into(),
                        ..Border::default()
                    },
                    shadow: Shadow {
                        color: argb(color),
                        offset: Vector::new(0.0, dy),
                        blur_radius: blur,
                    },
                    ..container::Style::default()
                });
            layers.push(pin(frame).x(x).y(y).into());
        }
        container(stack(layers).clip(true))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(fill(rgb(spec::CANVAS_BG[t])))
            .into()
    }

    /// One frame: its nodes in index order, later on top, clipped to the frame. A stack draws its first layer
    /// without a layer of its own, so each stack's first layer is an empty base.
    fn frame(&self, f: usize) -> Element<'_, Message> {
        let mut layers: Vec<Element<'_, Message>> = vec![base()];
        for node in &self.frames[f] {
            // the selection is a highlight over the frame, drawn last (see DEVIATIONS-iced.md)
            if node.kind == Kind::Selected {
                continue;
            }
            layers.push(match node.group {
                None => self.node(node),
                Some(q) => {
                    let (gx, gy, gw, gh) = spec::group_rect(q);
                    let group = stack![base(), self.node(node)].width(gw).height(gh).clip(true);
                    pin(group).x(gx).y(gy).into()
                }
            });
        }
        layers.extend(self.frames[f].iter().filter(|node| node.kind == Kind::Selected).map(|node| self.node(node)));
        stack(layers).width(spec::FRAME_W).height(spec::FRAME_H).clip(true).into()
    }

    /// A node, pinned at its rectangle's corner.
    fn node(&self, node: &Node) -> Element<'_, Message> {
        let t = self.theme;
        let (mut x, y, w, h) = node.rect;
        let body: Element<'_, Message> = match node.kind {
            Kind::Shadowed { radius, fill: color, dy, blur } => shape(w, h, move |_| container::Style {
                background: Some(Background::Color(rgb(color))),
                border: Border { radius: radius.into(), ..Border::default() },
                shadow: Shadow { color: argb(spec::SHADOW), offset: Vector::new(0.0, dy), blur_radius: blur },
                ..container::Style::default()
            }),
            Kind::Gradient { top, bottom } => shape(w, h, move |_| container::Style {
                // iced's angle 0 points up; PI runs the gradient from the top edge to the bottom one
                background: Some(Background::Gradient(
                    Linear::new(Radians::PI).add_stop(0.0, rgb(top)).add_stop(1.0, rgb(bottom)).into(),
                )),
                ..container::Style::default()
            }),
            Kind::Bordered { fill: color, border } => shape(w, h, move |_| container::Style {
                background: Some(Background::Color(Color { a: 0x66 as f32 / 255.0, ..rgb(color) })),
                border: Border { color: rgb(border), width: 1.0, radius: 0.0.into() },
                ..container::Style::default()
            }),
            Kind::Label => text(format!("L{}", node.g))
                .size(10)
                .color(rgb(spec::INK[t]))
                .wrapping(Wrapping::None)
                .into(),
            Kind::Instance { k } => {
                let hot = self.hot == Some(k);
                shape(w, h, move |_| container::Style {
                    background: Some(Background::Color(rgb(spec::comp(t, k, hot)))),
                    border: Border { color: argb(0x33000000), width: 1.0, radius: 4.0.into() },
                    ..container::Style::default()
                })
            }
            Kind::Selected => {
                if self.moved {
                    x = spec::MOVE_B;
                }
                // the node, and its outline 2 px outside it
                let node = Space::new().width(w).height(h);
                let node = container(node).style(fill_rounded(rgb(spec::SELECT_FILL), 6.0));
                container(node)
                    .padding(2)
                    .style(move |_| container::Style {
                        border: Border { color: rgb(spec::SELECT_OUTLINE), width: 1.0, radius: 8.0.into() },
                        ..container::Style::default()
                    })
                    .into()
            }
        };
        let (px, py) = match node.kind {
            Kind::Selected => (x - 3.0, y - 3.0),
            _ => (x, y),
        };
        pin(body).x(px).y(py).into()
    }

    /// The inspector: the selected node's values.
    fn inspector(&self) -> Element<'_, Message> {
        let t = self.theme;
        let ink = rgb(spec::INK[t]);
        let x = if self.moved { spec::MOVE_B } else { spec::MOVE_A };
        let rows = spec::INSPECTOR.iter().zip(spec::inspector(x)).map(|(label, value)| {
            row![
                text(*label).size(12).color(rgb(spec::MUTED)).width(108),
                text(value).size(12).color(ink),
            ]
            .height(spec::ROW_H)
            .align_y(Vertical::Center)
            .into()
        });
        container(iced::widget::Column::with_children(rows).padding(iced::Padding { top: 12.0, left: 12.0, ..iced::Padding::ZERO }))
            .width(Length::Fixed(1280.0 - spec::INSPECTOR_X))
            .height(Length::Fill)
            .style(fill(rgb(spec::CHROME[t])))
            .into()
    }
}

/// An empty layer that fills its stack.
fn base<'a, Message: 'a>() -> Element<'a, Message> {
    Space::new().width(Length::Fill).height(Length::Fill).into()
}

/// A box of `w` x `h` with a style.
fn shape<'a, Message: 'a>(w: f32, h: f32, style: impl Fn(&Theme) -> container::Style + 'a) -> Element<'a, Message> {
    container(Space::new()).width(w).height(h).style(style).into()
}

/// A layers panel row: its text at `x`.
fn layer_row<'a, Message: 'a>(label: String, x: f32, ink: Color) -> Element<'a, Message> {
    container(text(label).size(12).color(ink))
        .width(Length::Fill)
        .height(spec::ROW_H)
        .padding(iced::Padding { left: x, ..iced::Padding::ZERO })
        .align_y(Vertical::Center)
        .into()
}

/// A container style: one solid fill.
fn fill(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::background(color)
}

/// A container style: a solid fill with rounded corners.
fn fill_rounded(color: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        border: Border { radius: radius.into(), ..Border::default() },
        ..container::Style::default()
    }
}
