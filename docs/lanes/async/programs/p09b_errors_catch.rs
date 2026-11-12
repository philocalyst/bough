//! 9b. Errors raised to the nearest boundary (an alternative to p09, for comparison).
//!
//! In p09 the cell holds `Option<Result<User, Error>>` and the view maps it: explicit, and the boundary is the
//! `map`. Here the cell holds only what the panel shows when things work (`Option<User>`), the question's `Err`
//! goes up to the nearest `catch` in the view (`ok_into`), and the `catch` shows its fallback, built from the
//! error, in place of its content. Its `retry` asks every question inside it again; the fallback stays (with
//! `pending` true on the cells) until an answer lands. Inside the async fns, `?` is ordinary Rust: `async fn
//! profile(id) -> Result<User, Error> { let raw = fetch(id).await?; Ok(parse(raw)?) }`, and whatever error it
//! returns reaches the boundary.
//!
//! What it costs: a second way to handle errors (a `then` handler can still match); errors are typed per
//! boundary (`catch` is generic over the error type, so a question whose error type differs from its boundary's
//! does not compile); a value cell can no longer hold "stale value plus error" by itself (p08's form needs `then`).
use crate::api::{self, User};
use bough::prelude::*;

pub struct Profile {
    pub id: Signal<u64>,
    pub user: Signal<Option<User>>,
    pub clicks: Signal<u32>,
}

impl Profile {
    pub fn new(id: u64) -> Profile {
        Profile { id: Signal::new(id), user: Signal::new(None), clicks: Signal::new(0) }
    }
    pub fn click(&mut self) {
        *self.clicks += 1;
    }
}

const COL: Flex = Flex::column().gap(4.0);

impl View for Profile {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).child((
            catch(
                (
                    self.user.is_none().then(|| text("Loading\u{2026}")),
                    self.user.as_ref().map(|u| text(format!("{} (#{})", u.name, u.id))),
                    ask(sig!(self.id), api::try_user).ok_into(sig!(self.user)),
                ),
                |e: &api::Error, retry| div().layout(COL).child((text(format!("Could not load the profile: {e}")).color(rgb(0xc0392b)), button("Retry").on_click(retry))),
            ),
            button(sig!(self.clicks).map(|n| format!("clicked {n}"))).on_click(Profile::click),
        ))
    }
}
