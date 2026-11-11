use gpui::{
    App, Application, Bounds, Context, IntoElement, Render, TitlebarOptions, Window,
    WindowBounds, WindowOptions, prelude::*, px, rgb, size,
};
use std::{cell::RefCell, rc::Rc};

const TITLE: &str = "GPUI Scroll 2000";

struct DashboardApp {
    refresh_count: usize,
    enabled: bool,
}

impl DashboardApp {
    fn refresh(&mut self, _: &gpui::ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_count += 1;
        cx.notify();
    }

    fn toggle(&mut self, _: &gpui::ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.enabled = !self.enabled;
        cx.notify();
    }
}

impl Render for DashboardApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // BUI change: margin-less <body> of the viewport's height around the root (see scroll.rs)
        body().m_0().css("height", "100vh").child(div()
            .size_full()
            .bg(rgb(0xffffff))
            .flex()
            .flex_col()
            .font_family("DejaVu Sans")
            .text_color(rgb(0x202124))
            .child(div().h(px(70.)).flex_shrink_0().px(px(24.)).flex().items_center().text_size(px(22.)).child("Project dashboard"))
            .child(
                div().h(px(56.)).flex_shrink_0().px(px(24.)).flex().items_center().gap(px(32.))
                    .child(div().w(px(360.)).h(px(40.)).child(input().id("notes").placeholder("Project notes").w(px(360.)).h(px(40.))))
                    .child(div().id("refresh").w(px(160.)).h(px(40.)).flex().items_center().justify_center().bg(rgb(0xe5e7eb)).cursor_pointer().on_click(cx.listener(Self::refresh)).child("Refresh"))
                    .child(div().id("toggle").ml(px(16.)).w(px(160.)).h(px(40.)).flex().items_center().justify_center().bg(rgb(if self.enabled { 0xc8e6c9 } else { 0xe5e7eb })).cursor_pointer().on_click(cx.listener(Self::toggle)).child(if self.enabled { "Enabled" } else { "Disabled" })),
            )
            .child(div().h(px(54.)).flex_shrink_0().px(px(24.)).flex().items_center().text_size(px(13.)).text_color(rgb(0x555555)).child(format!("Status: {} | Refresh count: {}", if self.enabled { "enabled" } else { "disabled" }, self.refresh_count)))
            .child(
                div().id("project-list").flex_1().min_h_0().overflow_y_scroll().px(px(24.))
                    .children((1..=1000).map(|number| {
                        div().h(px(92.)).w_full().flex_shrink_0().flex().items_center().gap(px(16.)).border_b_1().border_color(rgb(0xdddddd)).px(px(8.))
                            .child(img().src(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../assets/images/dashboard-{}.png", (number - 1) % 10)).to_string_lossy().to_string()).size(px(64.)))
                            .child(div().flex().flex_col().gap(px(6.)).child(div().text_size(px(16.)).child(format!("Project {number}"))).child(div().text_size(px(13.)).text_color(rgb(0x666666)).child(format!("Project {number} includes planning notes, recent activity, and team status."))))
                    }))
            ))
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
            |_, cx| {
                cx.new(|_| DashboardApp {
                    refresh_count: 0,
                    enabled: true,
                })
            },
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
