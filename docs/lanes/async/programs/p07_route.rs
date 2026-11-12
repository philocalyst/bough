//! 7. A route transition that keeps the old screen until the new one is ready, then cross-fades to it.
//!
//! A route is a question whose answer is a page (a loader, as in Remix or React Router): `page =
//! load(route)`. While the next page loads, the old page stays, whole and interactive; the tab strip shows
//! the tab asked for as pending. When the page arrives, the old page fades out while the new one fades in.
//! The fade is two parameters of the kernel (an alpha ramp per screen, from the release time, over 250 ms),
//! advanced by the presenter: no turn runs while it plays, and its damage is the two screens' bounds, known
//! when it starts. Clicking a tab whose page is cached (here: the one on screen) changes nothing; clicking
//! another tab while a page loads asks for that one instead (latest wins) and the hold continues.
//!
//! Not written: a "next page" slot beside the current one, a loading flag, an animation timer, code to
//! remove the old page when the fade ends, a check that the page that arrived is still the one wanted.
use crate::api;
use bough::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Route {
    Inbox,
    Thread(u64),
    Prefs,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Blank,
    Inbox(Vec<String>),
    Thread(u64, Vec<String>),
    Prefs(Vec<(String, bool)>),
}

pub async fn load(r: Route) -> Page {
    match r {
        Route::Inbox => Page::Inbox(api::inbox().await),
        Route::Thread(id) => Page::Thread(id, api::thread(id).await),
        Route::Prefs => Page::Prefs(api::prefs().await),
    }
}

pub struct Mail {
    pub route: Signal<Route>,
    pub page: Signal<Page>,
}

impl Mail {
    pub fn new() -> Mail {
        Mail { route: Signal::new(Route::Inbox), page: Signal::new(Page::Blank) }
    }
    pub fn go(&mut self, r: Route) {
        *self.route = r;
    }
}

const COL: Flex = Flex::column().gap(6.0);
const BAR: Flex = Flex::row().gap(6.0);
const ON: Color = rgb(0x2f6bff);
const OFF: Color = rgb(0x9aa5b1);

fn tab(label: &'static str, r: Route, at: Route, asked: Route) -> impl Element<Mail> {
    // the published route is solid; the one asked for (still loading) is outlined
    let bg = if r == at { ON } else { OFF };
    button(label).bg(bg).border(if r == asked && r != at { 2.0 } else { 0.0 }, ON).on_click(move |v: &mut Mail| v.go(r))
}

impl View for Mail {
    fn view(&self) -> impl Element<Self> + '_ {
        let (at, asked) = (*self.route, *self.route.latest());
        div().layout(COL).child((
            div().layout(BAR).child((tab("Inbox", Route::Inbox, at, asked), tab("Thread 7", Route::Thread(7), at, asked), tab("Settings", Route::Prefs, at, asked))),
            // the screen: a new page is a new screen (its key), and screens cross-fade
            crossfade(Curve::out(0.25), match &*self.page {
                Page::Blank => Arm4::A(text("")),
                Page::Inbox(m) => Arm4::B(div().layout(COL).child(each(m.iter().map(|s| text(s.clone()))))),
                Page::Thread(_, t) => Arm4::C(div().layout(COL).child(each(t.iter().map(|s| text(s.clone()))))),
                Page::Prefs(p) => Arm4::D(div().layout(COL).child(each(p.iter().map(|(k, on)| text(format!("{k}: {}", if *on { "on" } else { "off" }))))))
            }),
            ask(sig!(self.route), load).into(sig!(self.page)),
        ))
    }
}
