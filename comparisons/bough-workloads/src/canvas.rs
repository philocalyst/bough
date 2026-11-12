//! Placeholder: the canvas scene is being written (bench lane, round 7). It mounts one box so the crate builds.
use crate::Scene;
use bough::prelude::*;
use bough::raw::{App, Caps};

pub struct Canvas {
    theme: Signal<u32>,
}

impl View for Canvas {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w(100.0).h(100.0).bg(sig!(self.theme).map(|t: &u32| if *t == 0 { rgb(0xffffff) } else { rgb(0x000000) }))
    }
}

impl Scene for Canvas {
    const NAME: &'static str = "canvas";
    fn build(_scale: f32) -> Self {
        Canvas { theme: Signal::new(0) }
    }
    fn caps(&self) -> Caps {
        Caps::DEFAULT
    }
    fn describe(&self) -> String {
        "placeholder".into()
    }
    fn fanout(&self) -> usize {
        1
    }
    fn steady(app: &mut App<Self>, _k: usize) {
        app.turn(|_| {});
    }
    fn hover(app: &mut App<Self>, _k: usize) {
        app.turn(|_| {});
    }
    fn theme(app: &mut App<Self>, _k: usize) {
        app.turn(|s| {
            let t = *s.theme ^ 1;
            s.theme.set(t);
        });
    }
    fn animate(_app: &mut App<Self>, _k: usize) {}
}
