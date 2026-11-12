//! 9. An error boundary: a profile panel that shows a spinner, the profile, or the error with a Retry, while
//! the rest of the screen (a counter here) carries on.
//!
//! An error is an answer: it ends the wait and appears when a value would have, in one frame with the input
//! that produced it. Rust's own types are the boundary: `Option` is "not answered yet", `Result` is "answered
//! with a value or an error", and both are elements, so the panel is a `map` over the cell. Retry asks the same
//! question again; while it is out, the error stays (dimmed by `pending`) instead of flashing to a spinner.
//!
//! Not written: an error state machine, a boundary component with its own reset state, try/catch around
//! the fetch, a "retrying" flag, code that clears the error when the retry succeeds.
use crate::api::{self, User};
use bough::prelude::*;

pub struct Profile {
    pub id: Signal<u64>,
    pub user: Signal<Option<Result<User, api::Error>>>,
    pub clicks: Signal<u32>,
}

impl Profile {
    pub fn new(id: u64) -> Profile {
        Profile { id: Signal::new(id), user: Signal::new(None), clicks: Signal::new(0) }
    }
    pub fn retry(&mut self) {
        self.user.refresh();
    }
    pub fn click(&mut self) {
        *self.clicks += 1;
    }
}

const COL: Flex = Flex::column().gap(4.0);

fn card(u: &User) -> impl Element<Profile> {
    text(format!("{} (#{})", u.name, u.id))
}

fn failed(e: &api::Error) -> impl Element<Profile> + '_ {
    div().layout(COL).child((text(format!("Could not load the profile: {e}")).color(rgb(0xc0392b)), button("Retry").on_click(Profile::retry)))
}

impl View for Profile {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).child((
            // spinner until the first answer; then the answer, a value or an error
            self.user.is_none().then(|| text("Loading\u{2026}")),
            div().opacity(pending(sig!(self.user)).map(|p| if p { 0.6 } else { 1.0 })).child(self.user.as_ref().map(|r| r.as_ref().map(card).map_err(failed))),
            // unaffected by the panel
            button(sig!(self.clicks).map(|n| format!("clicked {n}"))).on_click(Profile::click),
            ask(sig!(self.id), api::try_user).into(sig!(self.user)),
        ))
    }
}
