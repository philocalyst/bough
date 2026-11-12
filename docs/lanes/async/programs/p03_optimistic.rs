//! 3. An optimistic mutation with rollback: a todo list whose adds and toggles show at once and are undone
//! if the server refuses them.
//!
//! The optimistic state is not a copy of the list. `items` only ever holds what the server said; what is
//! shown is `items` with the inputs of the actions still in flight laid over it, computed where it is
//! shown. A failed action leaves the in-flight set, so its guess disappears: that *is* the rollback. Two
//! quick toggles of one row with the first failing after the second was sent leave the second's guess on
//! screen, and when it lands, the server's answer. The in-flight row of an add and the real row the server
//! returns change places in one frame (the landing and the end of the flight are one turn).
//!
//! Not written: a shadow list, temporary ids and their reconciliation, rollback code, "is a newer toggle
//! pending for this row" checks, flicker handling.
use crate::api::{self, Todo};
use bough::prelude::*;

pub struct Todos {
    pub items: Signal<Vec<Todo>>,
    pub draft: Signal<String>,
    pub add: Action<String, Result<Todo, api::Error>>,
    pub toggle: Action<(u64, bool), Result<Todo, api::Error>>,
    pub error: Signal<Option<String>>,
}

impl Todos {
    pub fn new(items: Vec<Todo>) -> Todos {
        Todos {
            items: Signal::new(items),
            draft: Signal::new(String::new()),
            add: Action::new(api::add_todo).then(Todos::added),
            toggle: Action::new(|(id, done)| api::set_done(id, done)).then(Todos::toggled),
            error: Signal::new(None),
        }
    }

    pub fn submit(&mut self) {
        let title = std::mem::take(&mut *self.draft);
        if !title.trim().is_empty() {
            self.add.run(title);
        }
    }
    fn added(&mut self, r: Result<Todo, api::Error>) {
        match r {
            Ok(t) => self.items.push(t),
            Err(e) => *self.error = Some(format!("Could not add: {e}")),
        }
    }

    /// Done as shown: the newest toggle of this row still in flight, else what the server said.
    pub fn done(&self, t: &Todo) -> bool {
        self.toggle.inflight().filter(|(id, _)| *id == t.id).last().map_or(t.done, |&(_, d)| d)
    }
    pub fn flip(&mut self, id: u64) {
        if let Some(t) = self.items.iter().find(|t| t.id == id) {
            let want = !self.done(t);
            self.toggle.run((id, want));
        }
    }
    fn toggled(&mut self, r: Result<Todo, api::Error>) {
        match r {
            Ok(t) => {
                if let Some(x) = self.items.iter_mut().find(|x| x.id == t.id) {
                    *x = t;
                }
            }
            Err(e) => *self.error = Some(format!("Could not save: {e}")),
        }
    }
}

const COL: Flex = Flex::column().gap(4.0);
const GREY: Color = rgb(0x8a8a8a);

impl View for Todos {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).child((
            each(self.items.iter().map(|t| {
                let id = t.id;
                button(if self.done(t) { format!("[x] {}", t.title) } else { format!("[ ] {}", t.title) }).on_click(move |v: &mut Todos| v.flip(id))
            })),
            // adds in flight, shown where they will land, until they land (as real rows) or fail (gone)
            each(self.add.inflight().map(|title| text(title.clone()).color(GREY))),
            text_input(sig!(self.draft)),
            button("Add").on_click(Todos::submit),
            self.error.as_ref().map(|e| text(e.clone()).color(rgb(0xc0392b))),
        ))
    }
}
