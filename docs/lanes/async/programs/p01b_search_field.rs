//! 1b. Search as you type, the question as a field (an alternative to p01, for comparison).
//!
//! In p01 the answer is an ordinary `Signal<Vec<Hit>>` and the relation is a view element,
//! `ask(sig!(self.query), search).into(sig!(self.hits))`: the author names a cell that only carries the answer
//! and wires it by hand. Here the cell *is* the question: `hits: Answer<Vec<Hit>>` says once, beside the state,
//! that hits is the answer to `search(query)`. It reads like a `Signal` (`*self.hits`, `sig!(self.hits)`), it
//! knows whether it is `pending()`, and `refresh()` asks it again; there is no line in `view` for it.
//!
//! What it costs: a field question lives as long as its owner, not as long as a subtree of the view, so a
//! question that should exist only while a pane is open belongs in the pane's owner (a list row, an entity) or
//! stays a view element (p01's form). The source is named with `field!(Search.query)` (the compiler proves the
//! offset as `sig!` does, from the type instead of a value).
//!
//! One concept for both? An `Answer` is triggered by its sources and keeps the latest; an `Action` is triggered by
//! `run` and keeps every run. Both are fields found at mount; what differs is the trigger and the policy, and
//! both knobs are independent (an autosave is source-triggered and keeps every run: `Action::on(field!(..))`).
//! See FOR-USER.md, question 1.
use crate::api::{self, Hit};
use bough::prelude::*;
use std::time::Duration;

pub struct Search {
    pub query: Signal<String>,
    pub hits: Answer<Vec<Hit>>,
}

impl Search {
    pub fn new() -> Search {
        Search { query: Signal::new(String::new()), hits: Answer::to(field!(Search.query), search) }
    }
}

pub async fn search(q: String) -> Vec<Hit> {
    if q.trim().is_empty() {
        return Vec::new();
    }
    sleep(Duration::from_millis(200)).await;
    api::search(q).await
}

const COL: Flex = Flex::column().gap(8.0);

impl View for Search {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).p(16.0).child((
            text_input(sig!(self.query)),
            text(sig!(self.query).map(|q| format!("Results for \u{201c}{q}\u{201d}"))),
            text(pending(sig!(self.query)).map(|p| if p { "searching\u{2026}".to_string() } else { String::new() })),
            each(self.hits.iter().map(|h| text(h.title.clone()))),
        ))
    }
}
