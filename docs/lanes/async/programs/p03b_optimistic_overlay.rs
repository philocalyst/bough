//! 3b. Optimistic mutations as overlays (an alternative to p03, for comparison).
//!
//! In p03 the view rebuilds the optimistic state per row from `inflight()`. Here each action says once how a run
//! in flight looks on the data it will change (`shows`): while runs are in flight, every description reads
//! `items` with their overlays applied in the order sent; a run that lands or fails stops being applied in the
//! same turn its landing runs. So the view reads `items` and nothing else, and there is still no rollback code.
//!
//! The toggle is a command (`toggle(id)`), not a value computed from what is shown: two quick clicks are two
//! overlays that cancel on screen and two commands that cancel on the server, and the handler needs no shown
//! state. (With an absolute `set_done(id, value)` the handler would have to read what is shown, and handlers read
//! the truth: see the costs below.)
//!
//! What it costs: handlers, landings and wires see `items` as the server said (the truth); only descriptions
//! see it with overlays. An app that re-describes (every app) shows the overlays everywhere at every frame; a
//! handler that must act on what is shown asks for it (`self.toggle.applied(&self.items)`, a copy). A pending add
//! is an ordinary `Todo` in the list (here id 0), so styling it apart needs a marker in the data. Each
//! description with runs in flight copies `items` once.
use crate::api::{self, Todo};
use bough::prelude::*;

pub struct Todos {
    pub items: Signal<Vec<Todo>>,
    pub draft: Signal<String>,
    pub add: Action<String, Result<Todo, api::Error>>,
    pub toggle: Action<u64, Result<Todo, api::Error>>,
    pub error: Signal<Option<String>>,
}

impl Todos {
    pub fn new(items: Vec<Todo>) -> Todos {
        Todos {
            items: Signal::new(items),
            draft: Signal::new(String::new()),
            add: Action::new(api::add_todo).then(Todos::added).shows(field!(Todos.items), |items, title: &String| items.push(Todo { id: 0, title: title.clone(), done: false })),
            toggle: Action::new(api::toggle_done).then(Todos::toggled).shows(field!(Todos.items), |items, id: &u64| {
                if let Some(t) = items.iter_mut().find(|t| t.id == *id) {
                    t.done = !t.done;
                }
            }),
            error: Signal::new(None),
        }
    }
    pub fn submit(&mut self) {
        let title = std::mem::take(&mut *self.draft);
        if !title.trim().is_empty() {
            self.add.run(title);
        }
    }
    pub fn flip(&mut self, id: u64) {
        self.toggle.run(id);
    }
    fn added(&mut self, r: Result<Todo, api::Error>) {
        match r {
            Ok(t) => self.items.push(t),
            Err(e) => *self.error = Some(format!("Could not add: {e}")),
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

impl View for Todos {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).child((
            each(self.items.iter().map(|t| {
                let id = t.id;
                let label = if t.done { format!("[x] {}", t.title) } else { format!("[ ] {}", t.title) };
                // a pending add is a row of the list like the others (id 0 until the server names it)
                row(text(label).color(if id == 0 { rgb(0x8a8a8a) } else { rgb(0x000000) })).on_click(move |v: &mut Todos| v.flip(id))
            })),
            text_input(sig!(self.draft)),
            button("Add").on_click(Todos::submit),
            self.error.as_ref().map(|e| text(e.clone()).color(rgb(0xc0392b))),
        ))
    }
}
