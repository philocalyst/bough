//! The canvas scene: a toolbar, a layers panel (GPUI's `uniform_list`, built rows only), the design surface and
//! the inspector.
//!
//! The design surface is a tree of absolutely positioned divs, which is how GPUI's own examples draw boxes with
//! shadows and gradients (`examples/shadow.rs`, `examples/gradient.rs`): `.shadow(..)` for a drop shadow,
//! `linear_gradient` as a background, `.border_1()` for a border and `.overflow_hidden()` for a clip. GPUI's
//! `canvas` element (low-level paint calls) is what its `painting.rs` example uses for free-form paths; nothing here
//! needs it. The view is one entity; a state change notifies it and its whole tree is described again.

use gpui_kit::*;

use super::spec::{FONT_FAMILY, canvas as s};
use super::{CONTEXT, ToggleMove, ToggleTheme, c, ca};

/// GPUI's `blur_radius` is the standard deviation of the shadow's Gaussian (the wgpu shader blurs with it as sigma
/// and reaches 3 sigma); the spec's blur radius is the CSS one, whose sigma is half of it.
fn sigma(blur: f32) -> f32 {
    blur / 2.0
}

pub struct Canvas {
    per_frame: usize,
    /// The nodes of every frame, in drawing order.
    frames: Vec<Vec<s::Node>>,
    /// 0 light, 1 dark.
    theme: usize,
    /// The component under the pointer in the layers panel.
    hot: Option<usize>,
    /// Whether the selected node is at `MOVE_B` (else `MOVE_A`).
    moved: bool,
    layers: UniformListScrollHandle,
    focus: FocusHandle,
}

impl Canvas {
    pub fn new(nodes: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let per_frame = nodes / s::FRAMES;
        let frames = (0..s::FRAMES).map(|f| (0..per_frame).map(|i| s::node(f, i, per_frame)).collect()).collect();
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self { per_frame, frames, theme: 0, hot: None, moved: false, layers: UniformListScrollHandle::new(), focus }
    }

    /// Layer rows listed under each frame.
    fn layer_rows(&self) -> usize {
        s::LAYER_ROWS.min(self.per_frame)
    }

    fn sel_x(&self) -> f32 {
        if self.moved { s::MOVE_B } else { s::MOVE_A }
    }

    /// The pointer entered or left component row `k`.
    fn hover(&mut self, k: usize, hovered: bool, cx: &mut Context<Self>) {
        let hot = if hovered {
            Some(k)
        } else if self.hot == Some(k) {
            None
        } else {
            return;
        };
        if hot != self.hot {
            self.hot = hot;
            cx.notify();
        }
    }

    /// Row `i` of the layers panel: the components, then each frame and its first layers.
    fn layer_row(&self, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let row = div().flex().flex_row().items_center().flex_none().w_full().h(px(s::ROW_H)).text_size(px(12.0)).text_color(c(s::INK[t]));
        if i < s::COMPONENTS {
            let bg = if self.hot == Some(i) { c(s::ROW_HOT[t]) } else { c(s::CHROME[t]) };
            return row
                .id(("component", i))
                .bg(bg)
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| this.hover(i, *hovered, cx)))
                .child(div().flex_none().ml(px(12.0)).w(px(12.0)).h(px(12.0)).bg(c(s::comp(t, i, false))))
                .child(div().flex_none().ml(px(8.0)).child(format!("Component {}", i + 1)))
                .into_any_element();
        }
        let j = i - s::COMPONENTS;
        let per = 1 + self.layer_rows();
        let (f, k) = (j / per, j % per);
        if k == 0 {
            row.pl(px(12.0)).child(format!("Frame {}", f + 1)).into_any_element()
        } else {
            row.pl(px(24.0)).child(format!("Layer {}", self.frames[f][k - 1].g)).into_any_element()
        }
    }

    /// The layers panel.
    fn layers(&self, cx: &mut Context<Self>) -> Div {
        let count = s::COMPONENTS + s::FRAMES * (1 + self.layer_rows());
        let list = uniform_list(
            "layers",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| range.map(|i| this.layer_row(i, cx)).collect::<Vec<_>>()),
        )
        .track_scroll(&self.layers)
        .size_full();
        div().flex_none().w(px(s::PANEL_W)).h_full().bg(c(s::CHROME[self.theme])).child(list)
    }

    /// The inspector: the selected node's values.
    fn inspector(&self) -> Div {
        let t = self.theme;
        let values = s::inspector(self.sel_x());
        div()
            .flex_none()
            .w(px(1280.0 - s::INSPECTOR_X))
            .h_full()
            .bg(c(s::CHROME[t]))
            .pt(px(56.0 - s::TOOLBAR_H))
            .children(s::INSPECTOR.iter().zip(values).map(|(label, v)| {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h(px(s::ROW_H))
                    .pl(px(12.0))
                    .text_size(px(12.0))
                    .child(div().flex_none().w(px(108.0)).text_color(c(s::MUTED)).child(*label))
                    .child(div().text_color(c(s::INK[t])).child(v.to_string()))
            }))
    }

    /// One node, positioned in its frame (or in its group's clip).
    fn node(&self, n: &s::Node) -> AnyElement {
        let t = self.theme;
        let (x, y, w, h) = if matches!(n.kind, s::Kind::Selected) { (self.sel_x(), n.rect.1, n.rect.2, n.rect.3) } else { n.rect };
        let at = || div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h));
        let el = match n.kind {
            s::Kind::Shadowed { radius, fill, dy, blur } => at()
                .rounded(px(radius))
                .bg(c(fill))
                .shadow(vec![BoxShadow::new(px(0.0), px(dy), ca(s::SHADOW).into()).blur_radius(px(sigma(blur)))])
                .into_any_element(),
            s::Kind::Gradient { top, bottom } => {
                at().bg(linear_gradient(180.0, linear_color_stop(c(top), 0.0), linear_color_stop(c(bottom), 1.0))).into_any_element()
            }
            s::Kind::Bordered { fill, border } => {
                at().bg(rgba(fill << 8 | 0x66)).border_1().border_color(c(border)).into_any_element()
            }
            s::Kind::Label => at()
                .whitespace_nowrap()
                .text_size(px(10.0))
                .text_color(c(s::INK[t]))
                .child(format!("L{}", n.g))
                .into_any_element(),
            s::Kind::Instance { k } => at()
                .rounded(px(4.0))
                .bg(c(s::comp(t, k, self.hot == Some(k))))
                .border_1()
                .border_color(ca(0x33000000))
                .into_any_element(),
            s::Kind::Selected => div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .rounded(px(6.0))
                .bg(c(s::SELECT_FILL))
                // the outline: 1 px wide, 2 px outside the node
                .child(
                    div()
                        .absolute()
                        .left(px(-3.0))
                        .top(px(-3.0))
                        .w(px(w + 6.0))
                        .h(px(h + 6.0))
                        .rounded(px(9.0))
                        .border_1()
                        .border_color(c(s::SELECT_OUTLINE)),
                )
                .into_any_element(),
        };
        match n.group {
            // a group's nodes are clipped to the group's rectangle inside the frame
            Some(q) => {
                let (gx, gy, gw, gh) = s::group_rect(q);
                div().absolute().left(px(gx)).top(px(gy)).w(px(gw)).h(px(gh)).overflow_hidden().child(el).into_any_element()
            }
            None => el,
        }
    }

    /// Frame `f`: its title, and the frame with its shadow, clipping its nodes.
    fn frame(&self, f: usize) -> Div {
        let t = self.theme;
        let (x, y, w, h) = s::frame_rect(f);
        let (dy, blur, color) = s::FRAME_SHADOW;
        // Index order, later on top; except that the selected node (index 0 of frame 0) goes last: under the other
        // 49 to 249 nodes of its frame it is hidden and the `m` probe sees nothing change (DEVIATIONS-gpui-kit.md).
        let mut nodes: Vec<AnyElement> = self.frames[f].iter().map(|n| self.node(n)).collect();
        if f == 0 {
            let selected = nodes.remove(0);
            nodes.push(selected);
        }
        div()
            .absolute()
            .left(px(x - s::PANEL_W))
            .top(px(y - s::TOOLBAR_H - 16.0))
            .w(px(w))
            .child(div().h(px(16.0)).text_size(px(11.0)).text_color(c(s::MUTED)).child(format!("Frame {}", f + 1)))
            .child(
                div()
                    .relative()
                    .w(px(w))
                    .h(px(h))
                    .rounded(px(s::FRAME_RADIUS))
                    .bg(c(s::FRAME_BG[t]))
                    .shadow(vec![BoxShadow::new(px(0.0), px(dy), ca(color).into()).blur_radius(px(sigma(blur)))])
                    .overflow_hidden()
                    .children(nodes),
            )
    }
}

impl Render for Canvas {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let surface = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(c(s::CANVAS_BG[t]))
            .overflow_hidden()
            .children((0..s::FRAMES).map(|f| self.frame(f)));
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ToggleTheme, _, cx| {
                this.theme = 1 - this.theme;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleMove, _, cx| {
                this.moved = !this.moved;
                cx.notify();
            }))
            .size_full()
            .flex()
            .flex_col()
            .font_family(FONT_FAMILY)
            .child(
                div()
                    .flex_none()
                    .w_full()
                    .h(px(s::TOOLBAR_H))
                    .pl(px(12.0))
                    .flex()
                    .items_center()
                    .bg(c(s::CHROME[t]))
                    .text_size(px(14.0))
                    .text_color(c(s::INK[t]))
                    .child("Canvas"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.layers(cx))
                    .child(surface)
                    .child(self.inspector()),
            )
    }
}
