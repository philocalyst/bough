//! Pixel gates for the code editor: zgx's `apps_bench` interactions (keystrokes in the middle of a file of
//! zgx's sample, page scrolls, the find bar, find-all keystrokes and new letters) plus caret moves, a newline
//! and backspaces; after every frame presented == full redraw at ring ages 1-4 and on the windowed ring, and at
//! the end the retained editor == a fresh mount of its state.
mod common;
use bough::raw::*;
use bough_app_sketch_code::code;
use common::*;

fn run(ring: usize, lines: usize) {
    let path = std::env::temp_dir().join(format!("bough-code-gate-{}-{ring}-{lines}.rs", std::process::id()));
    std::fs::write(&path, code::source(lines)).unwrap();
    let mut app = code::mount(true, if ring == 0 { 4 } else { ring });
    windowed(&mut app, ring);
    let what = format!("code ring {ring} lines {lines}");
    frame_checked(&mut app, &what);
    app.turn(|c| c.open(&path));
    frame_checked(&mut app, &what);
    let editor = code::editor_handle(&app);
    let at = code::editor_point(&app.owner);
    code::click(&mut app, at);
    let middle = app.owner.doc.len() / 2;
    app.turn(|c| c.set_caret(middle));
    frame_checked(&mut app, &what);
    for k in 0..6 {
        app.key_down(Key::Char(if k % 2 == 0 { 'x' } else { 'y' }), Mods::default());
        frame_checked(&mut app, &format!("{what} keystroke {k}"));
    }
    for key in [Key::Backspace, Key::Left, Key::Left, Key::Enter, Key::Char('"'), Key::Down, Key::Char('z'), Key::Up, Key::Backspace, Key::Home, Key::End] {
        app.key_down(key, Mods::default());
        frame_checked(&mut app, &format!("{what} {key:?}"));
    }
    for k in 0..24 {
        let dy = if (k / 10) % 2 == 0 { 600.0 } else { -600.0 };
        assert!(code::wheel(&mut app, editor, at, dy));
        frame_checked(&mut app, &format!("{what} scroll {k}"));
    }
    assert!(code::wheel(&mut app, editor, at, 37.0));
    frame_checked(&mut app, &format!("{what} scroll a fraction"));
    app.turn(|c| c.open_find());
    frame_checked(&mut app, &format!("{what} find opened"));
    code::click(&mut app, code::FIND_POINT);
    for ch in "Overflo".chars() {
        app.key_down(Key::Char(ch), Mods::default());
        frame_checked(&mut app, &format!("{what} find {ch}"));
    }
    if lines >= 1000 {
        assert!(app.owner.count.ends_with(" matches"), "{}", *app.owner.count);
    }
    for k in 0..6 {
        if k % 2 == 0 {
            app.key_down(Key::Char('w'), Mods::default());
        } else {
            app.key_down(Key::Backspace, Mods::default());
        }
        frame_checked(&mut app, &format!("{what} find-all keystroke {k}"));
    }
    for k in 0..4 {
        app.key_down(Key::Char(char::from(b'a' + k as u8)), Mods::default());
        frame_checked(&mut app, &format!("{what} new letter {k}"));
    }
    for _ in 0..4 {
        app.key_down(Key::Backspace, Mods::default());
        frame_checked(&mut app, &format!("{what} letters taken back"));
    }
    assert!(code::wheel(&mut app, editor, at, 600.0));
    frame_checked(&mut app, &format!("{what} scroll with marks"));
    let mono = bough_app_sketch_code::font_mono();
    let fresh = app.owner.fresh_like(&mono);
    let mut c = code::config(mono);
    c.ring = 4;
    let mut f = App::new(fresh, c, true);
    same_as_fresh(&mut app, &mut f, &what);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn code_every_ring() {
    for ring in RINGS {
        run(ring, 100_000);
    }
}

#[test]
fn code_short_file() {
    // zgx's run as measured with `LINES=24` inherited from a terminal: nothing to scroll
    run(0, 24);
    run(2, 24);
}

#[test]
fn find_counts_the_sample() {
    let mono = bough_app_sketch_code::font_mono();
    let mut c = code::Code::new(&mono, 2.0);
    let path = std::env::temp_dir().join(format!("bough-code-count-{}.rs", std::process::id()));
    std::fs::write(&path, code::source(100_000)).unwrap();
    c.open(&path);
    let hits = c.search.find(&c.doc, b"Overflo").len();
    assert_eq!(hits, 10_000);
    assert_eq!(c.search.find(&c.doc, b"overflow").len(), 10_000, "case folded");
    assert_eq!(c.search.find(&c.doc, b"Overfloa").len(), 0);
    let _ = std::fs::remove_file(&path);
}
