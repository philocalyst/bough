//! The canvas to the gui-bench spec (`spec::canvas`): placeholder until it is written.
use bough::prelude::*;
use bough::raw::Caps;

pub struct Canvas {
    nodes: usize,
    theme: Signal<u32>,
}

impl Canvas {
    pub fn new(nodes: usize) -> Canvas {
        Canvas { nodes, theme: Signal::new(0) }
    }
    pub fn caps(&self) -> Caps {
        let _ = self.nodes;
        Caps::DEFAULT
    }
}

impl View for Canvas {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(sig!(self.theme).map(|t: &u32| if *t == 0 { rgb(0xe5e5e5) } else { rgb(0x1e1e1e) }))
    }
}
