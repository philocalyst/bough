//! The site's dashboard workload: a toolbar with a text input, a Refresh button with a visible counter and an
//! enabled toggle, over a scrollable list of 1,000 project records with text and images, in a 1000 x 700
//! window. Geometry matches the other apps' (egui's) toolbar: input at (24, 70) 360 x 40, Refresh at (416, 70)
//! and the toggle at (624, 70), both 160 x 40, the count at (24, 124), the list from y = 180 — reached by
//! layout (padding, gaps, sizes), not coordinates.
use bough::prelude::*;

const IMAGES: [&[u8]; 10] = [
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-0.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-1.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-2.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-3.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-4.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-5.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-6.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-7.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-8.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-9.png"),
];

const TOOLBAR: Flex = Flex::row().align(Align::Center).gap(32.0);

pub struct Dashboard {
    notes: Signal<String>,
    refreshes: Signal<u32>,
    enabled: Signal<bool>,
    y: Signal<f32>,
}

impl Dashboard {
    fn refresh(&mut self) {
        *self.refreshes += 1;
    }
    fn toggle(&mut self) {
        let on = *self.enabled;
        self.enabled.set(!on);
    }
}

fn record(i: usize) -> impl Element<Dashboard> {
    column((
        row((
            image(IMAGES[i % IMAGES.len()]).w(64.0).h(64.0),
            column((text(format!("Project {}", i + 1)), text("A project record with descriptive dashboard information."))).gap(4.0),
        ))
        .gap(8.0)
        .items(Align::Center),
        block(0.0, 1.0).w_pct(1.0).bg(rgb(0xdcdcdc)),
    ))
    .gap(6.0)
    .px(8.0)
    .py(4.0)
}

impl View for Dashboard {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            div().h(70.0).pad(24.0, 16.0, 0.0, 0.0).child(text("Project dashboard").font(24.0, 32.0)),
            div()
                .layout(TOOLBAR)
                .h(40.0)
                .pad(24.0, 0.0, 0.0, 0.0)
                .child(text_input(sig!(self.notes)).size(360.0, 40.0))
                .child(button("Refresh").w(160.0).h(40.0).on_click(Dashboard::refresh))
                .child(button(sig!(self.enabled).map(|on| if *on { "Enabled".to_string() } else { "Disabled".to_string() })).w(160.0).h(40.0).margin(16.0, 0.0, 0.0, 0.0).on_click(Dashboard::toggle)),
            div().h(70.0).pad(24.0, 14.0, 0.0, 0.0).child(text(sig!(self.refreshes).map(|n| format!("Refresh count: {n}")))),
            div().w_pct(1.0).grow(1.0).min_h(0.0).scroll(sig!(self.y)).child(each((0..1000).map(record))),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(0xffffff))
    }
}

pub fn main() {
    let app = Dashboard { notes: Signal::new("Project notes".to_string()), refreshes: Signal::new(0), enabled: Signal::new(false), y: Signal::new(0.0) };
    Application::run(Window::document(app).title("bough Dashboard").size(1000.0, 700.0));
}

pub fn make() -> Dashboard { Dashboard { notes: Signal::new("Project notes".into()), refreshes: Signal::new(0), enabled: Signal::new(false), y: Signal::new(0.0) } }
pub fn step(v: &mut Dashboard) { v.refresh(); }
