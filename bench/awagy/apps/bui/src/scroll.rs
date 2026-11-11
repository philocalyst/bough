#[path = "../../scroll_config.rs"]
mod scroll_config;

use gpui::{
    App, Application, Bounds, Context, IntoElement, Render, TitlebarOptions, Window, WindowBounds,
    WindowOptions, prelude::*, px, rgb, size,
};
use std::{cell::RefCell, rc::Rc};

const TITLE: &str = "GPUI Scroll";

struct ScrollApp;

impl Render for ScrollApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // BUI change: gpui's root fills the window; BUI's is an HTML document whose <body> has an 8 px margin and
        // grows with its content (so nothing would overflow), so the root is wrapped in a margin-less <body> of
        // the viewport's height (BUI's own examples use `body { margin: 0 } .app { height: 100vh }`), for
        // `size_full()` and the scroll container to mean the window.
        body().m_0().css("height", "100vh").child(
            div()
                .id("rows")
                .size_full()
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .font_family("DejaVu Sans")
                .text_size(px(14.0))
                .line_height(px(24.0))
                .text_color(rgb(0x000000))
                .children((1..=scroll_config::row_count()).map(|number| {
                    div()
                        .w(px(960.0))
                        .h(px(48.0))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .p(px(12.0))
                        .bg(rgb(if number % 2 == 1 { 0xf5f5f5 } else { 0xe6e6e6 }))
                        .child(format!("Item {number}"))
                }))
        )
    }
}

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let failure = Rc::new(RefCell::new(None));
    let launch_failure = failure.clone();
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1000.0), px(700.0)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(TITLE.into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|_| ScrollApp),
        ) {
            *launch_failure.borrow_mut() = Some(error.to_string());
            cx.quit();
            return;
        }
        cx.activate(true);
    });
    if let Some(error) = failure.borrow_mut().take() {
        return Err(std::io::Error::other(error).into());
    }
    Ok(())
}
