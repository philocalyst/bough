//! 8. Stale-while-revalidate: a stats panel that shows its last answer while it asks again, every 30 s and
//! when the user presses Refresh, and keeps that answer (with a note) when a refresh fails.
//!
//! A re-ask of an unchanged question is quiet: nothing is held, nothing blanks, the old numbers stay until the
//! new ones replace them in one frame; `pending` on the numbers is true meanwhile (for a subtle indicator).
//! The 30 s interval is a property of the question, driven by the runtime's clock (a test advances a fake
//! one); the interval is dropped with the panel. A question that lands through a handler (`then`) names the
//! cell it is about (`about`), so `refresh` and `pending` on that cell reach it.
//!
//! Not written: a timer handle and its cleanup, a "refreshing" flag, a "has data" flag, copying the last
//! good value aside before a refresh, an error-or-data enum.
use crate::api::{self, Stats};
use bough::prelude::*;
use std::time::Duration;

pub struct Dash {
    pub stats: Signal<Option<Stats>>,
    pub stale: Signal<Option<String>>,
}

impl Dash {
    pub fn new() -> Dash {
        Dash { stats: Signal::new(None), stale: Signal::new(None) }
    }
    pub fn refresh(&mut self) {
        self.stats.refresh();
    }
    fn got(&mut self, r: Result<Stats, api::Error>) {
        match r {
            Ok(s) => {
                *self.stats = Some(s);
                *self.stale = None;
            }
            // keep the numbers; say they are stale
            Err(e) => *self.stale = Some(format!("Showing earlier numbers ({e})")),
        }
    }
}

const COL: Flex = Flex::column().gap(4.0);

impl View for Dash {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).child((
            match &*self.stats {
                None => Arm2::A(text("Loading\u{2026}")),
                Some(s) => Arm2::B(div().layout(COL).opacity(pending(sig!(self.stats)).map(|p| if p { 0.7 } else { 1.0 })).child((text(format!("{} visitors", s.visitors)), text(format!("${} revenue", s.revenue))))),
            },
            self.stale.as_ref().map(|m| text(m.clone())),
            button("Refresh").on_click(Dash::refresh),
            ask((), |()| api::stats()).every(Duration::from_secs(30)).then(Dash::got).about(sig!(self.stats)),
        ))
    }
}
