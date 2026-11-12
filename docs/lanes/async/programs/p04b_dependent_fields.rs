//! 4b. Dependent resources with field questions (an alternative to p04, for comparison).
//!
//! The same screen as p04: the story and its comments from the selection, the author from the story, published
//! whole. The relations are declared beside the state they fill, like a spreadsheet's formulas, and `view` only
//! shows. The waterfall (author from story) is a mapped source on a field that is itself an answer.
use crate::api::{self, Comment, Story, User};
use bough::prelude::*;

pub struct Reader {
    pub sel: Signal<u64>,
    pub story: Answer<Option<Story>>,
    pub comments: Answer<Vec<Comment>>,
    pub author: Answer<Option<User>>,
    pub unread: Signal<u32>,
}

impl Reader {
    pub fn new(sel: u64) -> Reader {
        Reader {
            sel: Signal::new(sel),
            story: Answer::to(field!(Reader.sel), api::story),
            comments: Answer::to(field!(Reader.sel), api::comments),
            author: Answer::to(field!(Reader.story).map(|s| s.as_ref().map(|s| s.author)), |a| async move { Some(api::user(a?).await) }),
            unread: Signal::new(0),
        }
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
            each((1..=9u64).map(|id| chip(format!("story {id}"), *self.sel == id).on_click(move |v: &mut Reader| v.open(id)))),
            text(sig!(self.unread).map(|n| format!("{n} unread"))),
            self.story.as_ref().map(|s| text(s.title.clone())),
            self.author.as_ref().map(|a| text(format!("by {}", a.name))),
            each(self.comments.iter().map(|c| text(c.text.clone()))),
        ))
    }
}
