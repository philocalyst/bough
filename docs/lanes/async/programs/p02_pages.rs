//! 2. A paginated list: a pager (one page at a time) and a feed (pages appended, "load more").
//!
//! Pager: the label and the rows switch together; Next, Next, Previous with the answers landing in any order
//! shows each published page whole (page 3's rows never appear under "Page 2"); going back to the page on
//! screen cancels the request in flight and costs nothing (it nets out).
//! Feed: appending is a command, not a question, so it is an `Action`: every page asked for is appended, in
//! the order asked, whatever order the answers come back in.
//!
//! Not written: page tokens, a "which page is this answer for" check, a loading flag, an "already loading"
//! guard, reordering of late answers.
use crate::api::{self, Page};
use bough::prelude::*;

pub struct Pager {
    pub page: Signal<u32>,
    pub rows: Signal<Option<Page>>,
}

impl Pager {
    pub fn new() -> Pager {
        Pager { page: Signal::new(0), rows: Signal::new(None) }
    }
    pub fn next(&mut self) {
        if self.rows.as_ref().is_some_and(|p| p.more) {
            *self.page += 1;
        }
    }
    pub fn prev(&mut self) {
        if *self.page > 0 {
            *self.page -= 1;
        }
    }
}

const COL: Flex = Flex::column().gap(4.0);
const BAR: Flex = Flex::row().gap(8.0);

impl View for Pager {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).child((
            text(sig!(self.page).map(|p| format!("Page {}", p + 1))),
            self.rows.as_ref().map(|p| each(p.rows.iter().map(|r| text(r.clone())))),
            self.rows.is_none().then(|| text("Loading\u{2026}")),
            div()
                .layout(BAR)
                .bg(rgb(0xf4f5f7))
                .opacity(pending(sig!(self.page)).map(|p| if p { 0.5 } else { 1.0 }))
                .child((button("Previous").on_click(Pager::prev), button("Next").on_click(Pager::next))),
            ask(sig!(self.page), api::page).into(sig!(self.rows)),
        ))
    }
}

pub struct Feed {
    pub rows: Signal<Vec<String>>,
    pub loaded: Signal<u32>,
    pub more: Action<u32, Page>,
}

impl Feed {
    pub fn new() -> Feed {
        Feed { rows: Signal::new(Vec::new()), loaded: Signal::new(0), more: Action::new(api::page).then(Feed::appended) }
    }
    /// The "load more" row came into view (or was clicked).
    pub fn load_more(&mut self) {
        let next = *self.loaded + self.more.inflight().count() as u32;
        self.more.run(next);
    }
    fn appended(&mut self, p: Page) {
        self.rows.extend(p.rows);
        *self.loaded += 1;
    }
}

impl View for Feed {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).child((
            each(self.rows.iter().map(|r| text(r.clone()))),
            // pages asked for and not yet appended show as placeholders, in order
            each(self.more.inflight().map(|n| text(format!("Loading page {}\u{2026}", n + 1)))),
            button("Load more").on_click(Feed::load_more),
        ))
    }
}
