//! Parts and overrides (`part.rs`): a `DatePicker` written as another crate would write it (on the prelude,
//! generic over its consumer's owner type, exposing no styling prop and no callback), whose day cell a
//! consumer restyles and whose select route a consumer intercepts and extends. Gates: the override is in the
//! retained records (paint, layout box, text colour); it survives re-description without damage; the
//! guards run in order and can stop the route; the retained frame equals a full redraw and a fresh mount;
//! reflection lists the parts with their provenance and edits them. Compile-fail cases: `part.rs` doctests.
use bough::input::Button;
use bough::prelude::*;
use bough::raw::{App, Config};

/// The "third-party" component: only `bough::prelude`, no knowledge of who uses it.
mod datepicker {
    use bough::prelude::*;

    pub const DAY: Part<BoxStyle> = Part::new("date-picker.day");
    pub const DAY_LABEL: Part<TextStyle> = Part::new("date-picker.day-label");
    pub const CELL: Color = rgb(0xeeeeee);
    pub const SELECTED: Color = rgb(0x2f6bff);
    pub const INK: Color = rgb(0x202020);

    fn day<V: 'static>(d: u32, selected: Wired<'_, u32, V>) -> impl Element<V> + '_ {
        let at = selected.lens();
        row((text(format!("{d}")).color(INK).part(DAY_LABEL),))
            .center()
            .w(28.0)
            .h(28.0)
            .bg(selected.map(move |s| if *s == d { SELECTED } else { CELL }))
            .rounded(4.0)
            .on_click(move |v: &mut V| {
                at.of(v).set(d);
            })
            .part_at(DAY, d)
    }

    /// A month of 35 days; a click on a day writes it to `selected`.
    pub fn date_picker<V: 'static>(selected: Wired<'_, u32, V>) -> impl Element<V> + '_ {
        column(each((0..5u32).map(move |w| row(each((0..7u32).map(move |c| day(w * 7 + c + 1, selected)))).gap(2.0)))).gap(2.0).p(4.0).bg(rgb(0xffffff))
    }
}
use datepicker::{date_picker, CELL, DAY, DAY_LABEL, SELECTED};

const S: f32 = 2.0;
const SUNDAY: Color = rgb(0xffd0d0);
const NAVY: Color = rgb(0x000080);

/// The component as its author wrote it.
struct Plain {
    date: Signal<u32>,
}
impl View for Plain {
    fn view(&self) -> impl Element<Self> + '_ {
        div().p(10.0).bg(rgb(0x303030)).child(date_picker(sig!(self.date)))
    }
}

/// A consumer: Sundays are tinted and closed (the select route is stopped and counted), every day is a
/// 32 pt pill, the labels are navy, and every selection that did happen is logged.
struct Booking {
    date: Signal<u32>,
    refused: Signal<u32>,
    log: Vec<u32>,
}
impl View for Booking {
    fn view(&self) -> impl Element<Self> + '_ {
        div().p(10.0).bg(rgb(0x303030)).child(
            date_picker(sig!(self.date))
                .restyle(DAY, |s| (if s.index() % 7 == 0 { s.bg(SUNDAY) } else { s }).rounded(14.0).w(32.0))
                .restyle(DAY_LABEL, |t| t.color(NAVY))
                .intercept(DAY, |me: &mut Booking, e: &PartEv| {
                    if e.index % 7 == 0 {
                        *me.refused += 1;
                        Flow::Stop
                    } else {
                        Flow::Continue
                    }
                })
                .after(DAY, |me: &mut Booking, e: &PartEv| me.log.push(e.index)),
        )
    }
}

fn font() -> Vec<u8> {
    std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap()
}
fn app<V: View>(v: V) -> App<V> {
    let mut app = App::new(v, Config::new(320.0, 240.0, S, font()), true);
    app.frame();
    app
}

/// (fill colour, corner radius px, width px) of the box drawn by record `handle`.
fn box_of<V: View>(app: &App<V>, handle: u32) -> (u32, f32, f32) {
    let n = app.rt.nodes.iter().find(|n| n.live && n.handle.idx == handle && n.text == bough::rt::NONE).expect("a quad node");
    let i = &app.rt.inst[app.rt.runs[n.run as usize].first as usize];
    (i.color, i.radius, i.size[0])
}
fn text_color<V: View>(app: &App<V>, handle: u32) -> u32 {
    app.rt.nodes.iter().find(|n| n.live && n.handle.idx == handle && n.text != bough::rt::NONE).expect("a text node").color
}
fn day_part<V: View>(app: &App<V>, d: u32) -> bough::part::PartView {
    app.reflect_parts().into_iter().find(|p| p.name == "date-picker.day" && p.index == d).expect("the day's part")
}
fn click_day<V: View>(app: &mut App<V>, d: u32) {
    let p = day_part(app, d);
    let [x, y, w, h] = app.record_view(p.pos).layout.expect("a layout box");
    let (cx, cy) = ((x + w / 2.0) / S, (y + h / 2.0) / S);
    app.pointer_down(cx, cy, Button::Primary);
    app.pointer_up(cx, cy, Button::Primary);
}
fn exact<V: View>(app: &mut App<V>, what: &str) {
    let (a, b) = (app.presented_pixels(), app.reference_pixels());
    let n = a.iter().zip(&b).filter(|(x, y)| x != y).count();
    assert_eq!(n, 0, "{what}: presented differs from a full redraw in {n} bytes");
}

#[test]
fn the_component_alone_is_as_its_author_wrote_it() {
    let mut app = app(Plain { date: Signal::new(0) });
    let parts = app.reflect_parts();
    assert_eq!(parts.iter().filter(|p| p.name == "date-picker.day").count(), 35);
    assert_eq!(parts.iter().filter(|p| p.name == "date-picker.day-label").count(), 35);
    assert!(parts[0].source.contains("tests/parts.rs:"), "provenance of the part site: {}", parts[0].source);
    for d in [1, 7, 20] {
        assert_eq!(box_of(&app, day_part(&app, d).handle), (CELL.0, 4.0 * S, 28.0 * S), "day {d}");
    }
    click_day(&mut app, 7);
    assert!(app.frame());
    let mut date = 0;
    app.turn(|v| date = *v.date);
    assert_eq!(date, 7);
    assert_eq!(box_of(&app, day_part(&app, 7).handle).0, SELECTED.0);
    exact(&mut app, "after selecting");
}

#[test]
fn a_consumer_restyles_the_day_cell_and_its_label() {
    let app = app(Booking { date: Signal::new(0), refused: Signal::new(0), log: Vec::new() });
    for d in 1..=35 {
        let (color, radius, w) = box_of(&app, day_part(&app, d).handle);
        assert_eq!(color, if d % 7 == 0 { SUNDAY.0 } else { CELL.0 }, "day {d}: Sundays are tinted, the rest keep the author's operand");
        assert_eq!((radius, w), (14.0 * S, 32.0 * S), "day {d}: a 32 pt pill");
        // layout answered the override too: the box is 32 pt wide where the author said 28
        let lw = app.record_view(day_part(&app, d).pos).layout.unwrap()[2];
        assert_eq!(lw, 32.0 * S);
    }
    for p in app.reflect_parts().iter().filter(|p| p.name == "date-picker.day-label") {
        assert_eq!(text_color(&app, p.handle), NAVY.0);
    }
}

#[test]
fn a_consumer_intercepts_and_extends_the_select_route() {
    let mut app = app(Booking { date: Signal::new(0), refused: Signal::new(0), log: Vec::new() });
    let state = |app: &mut App<Booking>| {
        let mut s = (0, 0, Vec::new());
        app.write(|v| s = (*v.date, *v.refused, v.log.clone()));
        s
    };
    click_day(&mut app, 9);
    assert_eq!(state(&mut app), (9, 0, vec![9]), "a weekday: the guard continues, the component selects, `after` logs");
    assert!(app.frame());
    // the author's wire still drives the cell (only Sundays lost theirs to a constant)
    assert_eq!(box_of(&app, day_part(&app, 9).handle).0, SELECTED.0);
    click_day(&mut app, 14);
    assert_eq!(state(&mut app), (9, 1, vec![9]), "a Sunday: the guard stops the component's route and `after` does not run");
    click_day(&mut app, 15);
    assert_eq!(state(&mut app), (15, 1, vec![9, 15]));
    app.frame();
    exact(&mut app, "after three clicks (each handler re-described the view)");
    // the override is still the retained state after the re-descriptions
    assert_eq!(box_of(&app, day_part(&app, 14).handle), (SUNDAY.0, 14.0 * S, 32.0 * S));
    assert_eq!(box_of(&app, day_part(&app, 15).handle), (SELECTED.0, 14.0 * S, 32.0 * S));
    // and equals a fresh mount of the same state
    let mut fresh = self::app(Booking { date: Signal::new(15), refused: Signal::new(1), log: vec![9, 15] });
    assert!(app.reference_pixels() == fresh.reference_pixels(), "retained == fresh mount");
}

/// An override rewrites the description, so describing again changes nothing: a handler that touches no
/// visible state leaves no damage (were the override a patch applied after the author's style, every
/// re-description would write the author's style and the patch again).
#[test]
fn a_redescription_under_an_override_is_quiet() {
    let mut app = app(Booking { date: Signal::new(0), refused: Signal::new(0), log: Vec::new() });
    assert!(!app.frame(), "idle");
    for _ in 0..3 {
        app.turn(|v| v.log.push(99)); // a plain field: the view re-describes
        assert!(!app.frame(), "the re-description found the retained records as described");
    }
    click_day(&mut app, 7); // refused: only `refused` changed, which nothing shows
    assert!(!app.frame());
}

/// A component that wraps the picker and restyles its days, wrapped again by an application: the outermost
/// wrapper has the last word on what both set, and what only the inner one set stays.
struct Nested {
    date: Signal<u32>,
}
fn range_picker<V: 'static>(d: Wired<'_, u32, V>) -> impl Element<V> + '_ {
    date_picker(d).restyle(DAY, |s| s.rounded(2.0).border(1.0, rgb(0x00ff00)))
}
impl View for Nested {
    fn view(&self) -> impl Element<Self> + '_ {
        div().p(10.0).child(range_picker(sig!(self.date)).restyle(DAY, |s| s.rounded(9.0)))
    }
}

#[test]
fn nested_wrappers_compose_and_the_outermost_wins() {
    let app = app(Nested { date: Signal::new(0) });
    let n = app.rt.nodes.iter().find(|n| n.live && n.handle.idx == day_part(&app, 3).handle).unwrap();
    let i = &app.rt.inst[app.rt.runs[n.run as usize].first as usize];
    assert_eq!(i.radius, 9.0 * S, "the application's radius");
    assert_eq!((i.bw, i.color), (1.0 * S, rgb(0x00ff00).0), "the wrapping component's border");
}

/// The inspector's view of a window with parts and a foreign element: both are listed, with provenance, in
/// paint order, and both edit paths are ordinary writes (a paint write on a record; a signal write through
/// value reflection).
struct Both {
    date: Signal<u32>,
}
impl View for Both {
    fn view(&self) -> impl Element<Self> + '_ {
        div().p(10.0).gap(10.0).bg(rgb(0x303030)).child(date_picker(sig!(self.date))).child(foreign(42).w(200.0).h(40.0))
    }
}

#[test]
fn the_inspector_lists_and_edits_parts_and_foreign_elements() {
    let mut app = app(Both { date: Signal::new(0) });
    let parts = app.reflect_parts();
    let foreign = app.reflect_foreign();
    assert_eq!((parts.len(), foreign.len()), (70, 1));
    assert_eq!(foreign[0].id, 42);
    assert!(foreign[0].source.contains("tests/parts.rs:") && parts[0].source.contains("tests/parts.rs:"));
    // the records carry their element types (no opt-in) and the foreign record its kind
    let recs = app.reflect_records();
    let fr = recs.iter().find(|r| r.handle == foreign[0].handle).unwrap();
    assert_eq!(fr.kind & 0x1F, bough::rt::REC_FOREIGN);
    assert!(fr.element.unwrap().contains("Foreign"));
    assert!(recs.iter().find(|r| r.handle == parts[0].handle).unwrap().element.unwrap().contains("LBox"));
    // edit a day's paint through its part handle
    let d3 = day_part(&app, 3);
    assert!(app.reflect_set_color(d3.handle, rgb(0xff0000)));
    assert!(app.frame());
    exact(&mut app, "after the inspector's paint edit");
    let [x, y, _, _] = app.record_view(d3.pos).layout.unwrap();
    let px = app.presented_pixels();
    let i = (((y as i32 + 6) * (320.0 * S) as i32 + x as i32 + 6) * 4) as usize;
    assert_eq!(&px[i..i + 4], &[0, 0, 255, 255], "the edited cell is red (BGRA)");
    // edit the owner's state by path: an ordinary write, the component's wire answers
    assert!(app.reflect_set("date", "12"));
    app.frame();
    assert_eq!(box_of(&app, day_part(&app, 12).handle).0, SELECTED.0);
    exact(&mut app, "after the inspector's state edit");
}
