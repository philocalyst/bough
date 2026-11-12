//! 4. Dependent resources: a story, its comments (both from the selection) and its author (from the story).
//!
//! Selecting story 7 while story 6 is shown keeps the whole pane on story 6 (its selection highlight, title,
//! comments and author) until story 7, its comments *and its author* are all there, then shows them in one
//! frame: the author's request starts only when the story says who the author is (a waterfall), and the
//! hold passes down the waterfall instead of releasing a half-loaded pane. A story by the same author as the
//! one shown asks for no author at all (the author question's input did not change). The unrelated unread
//! counter is never held.
//!
//! Not written: a combined "pane state" type, a "loading all three" counter, a check that the author
//! belongs to the story shown, any code that runs when the story lands.
use crate::api::{self, Comment, Story, User};
use bough::prelude::*;

pub struct Reader {
    pub sel: Signal<u64>,
    pub story: Signal<Option<Story>>,
    pub comments: Signal<Vec<Comment>>,
    pub author: Signal<Option<User>>,
    pub unread: Signal<u32>,
}

impl Reader {
    pub fn new(sel: u64) -> Reader {
        Reader { sel: Signal::new(sel), story: Signal::new(None), comments: Signal::new(Vec::new()), author: Signal::new(None), unread: Signal::new(0) }
    }
    pub fn open(&mut self, id: u64) {
        *self.sel = id;
    }
}

const COL: Flex = Flex::column().gap(4.0);

fn chip(label: String, on: bool) -> impl Element<Reader> + Interact {
    row(text(label)).px(10.0).py(4.0).rounded(4.0).bg(if on { rgb(0x2f6bff) } else { rgb(0x9aa5b1) })
}

impl View for Reader {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).child((
            // the selection highlight reads the selection: held with the pane
            each((1..=9u64).map(|id| chip(format!("story {id}"), *self.sel == id).on_click(move |v: &mut Reader| v.open(id)))),
            text(sig!(self.unread).map(|n| format!("{n} unread"))),
            self.story.as_ref().map(|s| text(s.title.clone())),
            self.author.as_ref().map(|a| text(format!("by {}", a.name))),
            each(self.comments.iter().map(|c| text(c.text.clone()))),
            // one selection, two questions: they publish together
            ask(sig!(self.sel), api::story).into(sig!(self.story)),
            ask(sig!(self.sel), api::comments).into(sig!(self.comments)),
            // asked from the story's author id: re-asked only when that id changes
            ask(sig!(self.story).map(|s| s.as_ref().map(|s| s.author)), |a| async move { Some(api::user(a?).await) }).into(sig!(self.author)),
        ))
    }
}
