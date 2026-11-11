use crate::{
    model::{plain_row, striped_row},
    spec,
};
use bough::prelude::*;

pub struct Search {
    pub rows: Vec<spec::Row>,
    pub query: Signal<String>,
    pub y: Signal<f32>,
    pub filter: bool,
}
impl Search {
    pub fn new(count: usize, filter: bool) -> Self {
        Self {
            rows: spec::Ids::default().rows(count),
            query: Signal::new(String::new()),
            y: Signal::new(0.0),
            filter,
        }
    }
    pub fn snapshot(&self) -> Self {
        Self {
            rows: self.rows.clone(),
            query: Signal::new((*self.query).clone()),
            y: Signal::new(*self.y),
            filter: self.filter,
        }
    }
}
impl View for Search {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            column((
                row((
                    text(spec::status(self.rows.len())).color(rgb(spec::LABEL_COLOR)),
                    text_input(sig!(self.query)).size(spec::INPUT_WIDTH, spec::INPUT_HEIGHT),
                ))
                .w_pct(1.0)
                .h(spec::HEADER_HEIGHT - 1.0)
                .shrink(0.0)
                .px(spec::PADDING_X)
                .items(Align::Center)
                .justify(Justify::SpaceBetween),
                block(0.0, 1.0)
                    .w_pct(1.0)
                    .shrink(0.0)
                    .bg(rgb(spec::HEADER_BORDER)),
            ))
            .w_pct(1.0)
            .h(spec::HEADER_HEIGHT)
            .shrink(0.0)
            .bg(rgb(spec::HEADER_BG)),
            div()
                .w_pct(1.0)
                .grow(1.0)
                .min_h(0.0)
                .scroll(sig!(self.y))
                .child(each(
                    self.rows
                        .iter()
                        .filter(|r| !self.filter || spec::matches(&r.label, &self.query))
                        .map(plain_row),
                )),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(spec::BACKGROUND))
    }
}

pub struct Spinner {
    pub rows: Vec<spec::Row>,
    pub angle: Signal<f32>,
    pub y: Signal<f32>,
    pub ticks: u64,
}
impl Spinner {
    pub fn new(count: usize) -> Self {
        Self {
            rows: spec::Ids::default().rows(count),
            angle: Signal::new(0.0),
            y: Signal::new(0.0),
            ticks: 0,
        }
    }
    pub fn clock(&mut self) {
        self.ticks += 1;
        self.angle.set(
            ((self.ticks * 16) % spec::SPINNER_PERIOD_MS) as f32 / spec::SPINNER_PERIOD_MS as f32
                * std::f32::consts::TAU,
        );
    }
    pub fn snapshot(&self) -> Self {
        Self {
            rows: self.rows.clone(),
            angle: Signal::new(*self.angle),
            y: Signal::new(*self.y),
            ticks: self.ticks,
        }
    }
}
impl View for Spinner {
    fn view(&self) -> impl Element<Self> + '_ {
        // G8: the raw escape supplies only a rotating square; layout and all rows are prelude elements.
        const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><rect width="24" height="24" rx="3" fill="#2a78d6"/></svg>"##;
        let square = bough::raw::anchor(
            spec::SPINNER_SIZE,
            spec::SPINNER_SIZE,
            bough::raw::svg_image(0.0, 0.0, spec::SPINNER_SIZE, spec::SPINNER_SIZE, SVG)
                .angle(sig!(self.angle)),
        );
        column((
            column((
                row((
                    text(spec::status(self.rows.len())).color(rgb(spec::LABEL_COLOR)),
                    square,
                ))
                .w_pct(1.0)
                .h(spec::HEADER_HEIGHT - 1.0)
                .shrink(0.0)
                .px(spec::PADDING_X)
                .items(Align::Center)
                .justify(Justify::SpaceBetween),
                block(0.0, 1.0)
                    .w_pct(1.0)
                    .shrink(0.0)
                    .bg(rgb(spec::HEADER_BORDER)),
            ))
            .w_pct(1.0)
            .h(spec::HEADER_HEIGHT)
            .shrink(0.0)
            .bg(rgb(spec::HEADER_BG)),
            div()
                .w_pct(1.0)
                .grow(1.0)
                .min_h(0.0)
                .scroll(sig!(self.y))
                .child(each(self.rows.iter().map(plain_row))),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(spec::BACKGROUND))
    }
}

pub struct Virtual {
    pub count: usize,
    pub y: Signal<f32>,
}
impl Virtual {
    pub fn new(count: usize) -> Self {
        Self {
            count,
            y: Signal::new(0.0),
        }
    }
}
impl View for Virtual {
    fn view(&self) -> impl Element<Self> + '_ {
        fn row(v: &Virtual, i: usize, m: &mut bough::raw::Mounter<'_, Virtual>) {
            let _ = v;
            striped_row(i).mount(m);
        }
        bough::raw::index_list(self.count, row)
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(rgb(spec::BACKGROUND))
            .estimate(spec::ROW_HEIGHT)
            .scroll(sig!(self.y))
            .scrolls(
                sig!(self.y),
                15.0,
                (self.count as f32 * spec::ROW_HEIGHT - spec::WINDOW.1).max(0.0),
            )
    }
}
