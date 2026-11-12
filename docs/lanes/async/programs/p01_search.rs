//! 1. Search as you type, with cancellation.
//!
//! What the author did not write: a request id or ticket, a cancellation token, a debounce timer handle, a
//! "loading" flag, an "is this answer still for the current query?" check, or any code to keep the heading
//! and the list in step. Typing is never delayed (the input shows what was typed); the heading and the list
//! change together, when the answer for the heading's query is there.
use crate::api::{self, Hit};
use bough::prelude::*;
use std::time::Duration;

pub struct Search {
    pub query: Signal<String>,
    pub hits: Signal<Vec<Hit>>,
}

impl Search {
    pub fn new() -> Search {
        Search { query: Signal::new(String::new()), hits: Signal::new(Vec::new()) }
    }
}

/// The question a keystroke asks. A newer keystroke drops this future wherever it is: inside the debounce
/// (no request is ever sent) or while the request is out (the request is dropped with it).
pub async fn search(q: String) -> Vec<Hit> {
    if q.trim().is_empty() {
        return Vec::new(); // ready on its first poll: published in the same turn, no pending frame
    }
    sleep(Duration::from_millis(200)).await;
    api::search(q).await
}

const COL: Flex = Flex::column().gap(8.0);

impl View for Search {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).p(16.0).child((
            // an input shows what was typed (inputs read intent)
            text_input(sig!(self.query)),
            // held while the answer is out: the heading names the query whose hits are listed under it
            text(sig!(self.query).map(|q| format!("Results for \u{201c}{q}\u{201d}"))),
            text(pending(sig!(self.query)).map(|p| if p { "searching\u{2026}".to_string() } else { String::new() })),
            each(self.hits.iter().map(|h| text(h.title.clone()))),
            // the question: hits = search(query), latest wins, superseded futures are dropped
            ask(sig!(self.query), search).into(sig!(self.hits)),
        ))
    }
}
