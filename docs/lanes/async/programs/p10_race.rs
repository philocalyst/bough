//! 10. A race: a document fetched from two mirrors at once, the first answer wins and the slower request is
//! dropped; and a deadline: if neither answers within 2 s the panel says so (and the late answer, if any, is
//! never shown, because its future no longer exists).
//!
//! A race is not a framework feature: it is a future combinator over two futures, and losing is being
//! dropped. The framework's part is that the whole race is one question, cancelled as one when the document
//! id changes.
//!
//! Not written: two request ids, a "first one wins" flag, aborting the loser by hand, a timer and its
//! clearing, a guard against the late answer.
use crate::api::{self, Doc, Mirror};
use bough::prelude::*;
use std::time::Duration;

pub struct Viewer {
    pub id: Signal<u64>,
    pub doc: Signal<Option<Result<Doc, String>>>,
}

impl Viewer {
    pub fn new(id: u64) -> Viewer {
        Viewer { id: Signal::new(id), doc: Signal::new(None) }
    }
    pub fn open(&mut self, id: u64) {
        *self.id = id;
    }
}

/// The faster mirror's answer, within two seconds.
pub async fn fastest(id: u64) -> Result<Doc, String> {
    let either = race(api::doc_from(Mirror::East, id), api::doc_from(Mirror::West, id));
    match timeout(Duration::from_secs(2), either).await {
        Some(Ok(d)) => Ok(d),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("no mirror answered in 2 s".to_string()),
    }
}

const COL: Flex = Flex::column().gap(4.0);

impl View for Viewer {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).child((
            text(sig!(self.id).map(|id| format!("Document {id}"))),
            self.doc.as_ref().map(|r| r.as_ref().map(|d| text(d.body.clone())).map_err(|e| text(e.clone()).color(rgb(0xc0392b)))),
            ask(sig!(self.id), fastest).into(sig!(self.doc)),
        ))
    }
}
