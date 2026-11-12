//! 14. An interrupted transition that reverses: three tabs whose pages load once and are kept. Switching to a
//! tab that has not loaded holds the current page until its data arrives, then slides and fades to it; going
//! back to a loaded tab is immediate (its question answers on its first poll, so nothing is held).
//!
//! Interrupt: from Inbox, open Sent; Sent's data arrives and the slide to Sent starts; 120 ms into the 300 ms
//! slide the user clicks Inbox again. Inbox is the screen that is leaving and is still mounted, so it is
//! revived rather than mounted again, and the slide reverses *from where it is*: both screens' kernel
//! parameters are retargeted at the click time with the value the curve had then as their new start (the CPU
//! evaluates the same curve function the kernel does), so the motion is continuous. Sent finishes leaving
//! and is released at the end of the reversed slide.
//!
//! Not written: an animation state per screen, a "currently leaving" list, reading back the current offset
//! to reverse from it, re-creating the inbox, a guard against the late answer of a tab no longer wanted.
use crate::api;
use bough::prelude::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Inbox,
    Sent,
    Spam,
}

impl Tab {
    fn key(self) -> u64 {
        self as u64
    }
}

pub struct Tabs {
    pub tab: Signal<Tab>,
    pub pages: Signal<HashMap<Tab, Vec<String>>>,
}

impl Tabs {
    pub fn new() -> Tabs {
        Tabs { tab: Signal::new(Tab::Inbox), pages: Signal::new(HashMap::new()) }
    }
    pub fn go(&mut self, t: Tab) {
        *self.tab = t;
    }
    fn loaded(&mut self, (t, rows): (Tab, Option<Vec<String>>)) {
        if let Some(rows) = rows {
            self.pages.insert(t, rows);
        }
    }
}

/// A tab already in the map answers at once (no request, nothing held).
pub async fn load(t: Tab, have: bool) -> (Tab, Option<Vec<String>>) {
    if have {
        return (t, None);
    }
    let id = match t {
        Tab::Inbox => 1,
        Tab::Sent => 2,
        Tab::Spam => 3,
    };
    (t, Some(api::thread(id).await))
}

const COL: Flex = Flex::column().gap(6.0);
const BAR: Flex = Flex::row().gap(6.0);

impl View for Tabs {
    fn view(&self) -> impl Element<Self> + '_ {
        let t = *self.tab;
        div().layout(COL).child((
            div().layout(BAR).child((
                button("Inbox").on_click(|v: &mut Tabs| v.go(Tab::Inbox)),
                button("Sent").on_click(|v: &mut Tabs| v.go(Tab::Sent)),
                button("Spam").on_click(|v: &mut Tabs| v.go(Tab::Spam)),
            )),
            slide(Curve::smooth(0.3), keyed(t.key(), div().layout(COL).child(each(self.pages.get(&t).into_iter().flatten().map(|s| text(s.clone())))))),
            ask((sig!(self.tab), sig!(self.pages)).map(|(t, p)| (*t, p.contains_key(t))), |(t, have)| load(t, have)).then(Tabs::loaded),
        ))
    }
}
