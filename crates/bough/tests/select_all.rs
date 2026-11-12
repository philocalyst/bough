//! A text field's select-all (ctrl or cmd + A): the next edit replaces or clears the whole text, a movement key
//! ends the selection, and the window equals a fresh mount of the same text (the highlight leaves no trace).
use bough::prelude::*;
use bough::raw::{App, Config};

struct Form {
    q: Signal<String>,
}

impl View for Form {
    fn view(&self) -> impl Element<Self> + '_ {
        column((text_input(sig!(self.q)).size(240.0, 28.0),)).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))
    }
}

fn app(q: &str) -> App<Form> {
    let mut a = App::new(Form { q: Signal::new(q.to_string()) }, Config::new(320.0, 80.0, 1.0, bough::ui::default_font()), true);
    a.frame();
    let field = (0..a.record_count()).find(|&p| a.record_view(p).kind & 0x40 != 0).expect("a focusable field");
    let h = a.rt.bough.handle_at(field);
    a.set_focus(h, false);
    a.key_down(Key::End, Mods::default());
    a.frame();
    a
}

#[test]
fn select_all_then_edit() {
    let ctrl = Mods { ctrl: true, ..Mods::default() };
    let mut a = app("");
    for c in "the quick fox".chars() {
        a.key_down(Key::Char(c), Mods::default());
        a.frame();
    }
    assert_eq!(*a.owner.q, "the quick fox");
    a.key_down(Key::Char('a'), ctrl);
    a.frame();
    a.key_down(Key::Backspace, Mods::default());
    a.frame();
    assert_eq!(*a.owner.q, "", "ctrl+A, backspace clears the field");
    for c in "dog".chars() {
        a.key_down(Key::Char(c), Mods::default());
    }
    a.key_down(Key::Char('a'), Mods { cmd: true, ..Mods::default() });
    a.key_down(Key::Char('z'), Mods::default());
    a.frame();
    assert_eq!(*a.owner.q, "z", "cmd+A, a character replaces the text");
    a.key_down(Key::Char('a'), ctrl);
    a.key_down(Key::Right, Mods::default());
    a.key_down(Key::Char('y'), Mods::default());
    a.frame();
    assert_eq!(*a.owner.q, "zy", "a movement ends the selection");
    // the same text in a fresh window: the same pixels (caret at the end, no highlight left)
    let mut f = app("zy");
    let (r, fresh) = (a.presented_pixels(), f.presented_pixels());
    let bad = r.iter().zip(&fresh).filter(|(x, y)| x != y).count();
    assert!(bad == 0, "{bad} bytes differ from a fresh field holding the same text");
}
