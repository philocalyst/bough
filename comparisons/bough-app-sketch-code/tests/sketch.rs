//! Pixel gates for Sketch: zgx's `apps_bench` interactions (the drawing opened, a shape pressed and dragged
//! with snapping, released, Escape, pans, zooms) plus a resize by a handle, a marquee, drawing a shape,
//! nudges, undo and redo, a Space-drag pan; after every frame presented == full redraw at ring ages 1-4 and on
//! the windowed ring, and at the end the retained app == a fresh mount of the state it reached.
mod common;
use bough::raw::*;
use bough_app_sketch_code::sketch::{self, Sketch};
use common::*;

fn step(app: &mut App<Sketch>, what: &str) {
    sketch::paint(app);
    frame_checked(app, what);
}

fn run(ring: usize, shapes: usize) {
    let dir = std::env::temp_dir().join(format!("bough-sketch-gate-{}-{ring}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("busy.json");
    std::fs::write(&path, sketch::busy_json(shapes)).unwrap();
    let mut app = sketch::mount(true, if ring == 0 { 4 } else { ring }, shapes + 64);
    windowed(&mut app, ring);
    let what = format!("sketch ring {ring}");
    step(&mut app, &what);
    app.turn(|s| s.open(&path));
    step(&mut app, &format!("{what} open"));
    let board = sketch::board_handle(&app);
    // zgx's drag: pressed on the first shape, moved (2k, k)
    let (x, y) = sketch::at(16.0, 16.0);
    app.pointer_down(x, y, Button::Primary);
    step(&mut app, &format!("{what} press"));
    for k in 0..24 {
        let k = k as f32;
        app.pointer_move(x + k * 2.0, y + k);
        step(&mut app, &format!("{what} drag {k}"));
    }
    app.pointer_up(x, y, Button::Primary);
    step(&mut app, &format!("{what} release"));
    assert!(app.owner.dirty, "the drag moved the shape");
    app.key_down(Key::Escape, Mods::default());
    step(&mut app, &format!("{what} escape"));
    // pans and zooms about at(400, 300)
    let (px, py) = sketch::at(400.0, 300.0);
    for k in 0..22 {
        let s = if (k / 10) % 2 == 0 { 12.0 } else { -12.0 };
        assert!(sketch::wheel(&mut app, board, px, py, s, s / 2.0));
        step(&mut app, &format!("{what} pan {k}"));
    }
    for k in 0..8 {
        assert!(sketch::pinch(&mut app, board, px, py, if k % 2 == 0 { 0.05 } else { -0.05 }));
        step(&mut app, &format!("{what} zoom {k}"));
    }
    // a fractional pan (the drawing's sub-pixel offset changes: every instance re-derived)
    assert!(sketch::wheel(&mut app, board, px, py, 0.3, 0.7));
    step(&mut app, &format!("{what} pan a fraction"));
    // select an ellipse (the second shape), resize it by its south-east handle
    let v = app.owner.view;
    let e = app.owner.shapes[1];
    let (ex, ey) = (sketch::BX + v.x + (e.x + e.w / 2.0) * v.zoom, sketch::BY + v.y + (e.y + e.h / 2.0) * v.zoom);
    app.pointer_down(ex, ey, Button::Primary);
    step(&mut app, &format!("{what} select ellipse"));
    app.pointer_up(ex, ey, Button::Primary);
    step(&mut app, &format!("{what} select ellipse up"));
    let (hx, hy) = (sketch::BX + v.x + (e.x + e.w) * v.zoom, sketch::BY + v.y + (e.y + e.h) * v.zoom);
    app.pointer_down(hx, hy, Button::Primary);
    for k in 1..6 {
        app.pointer_move(hx + 7.0 * k as f32, hy + 3.0 * k as f32);
        step(&mut app, &format!("{what} resize {k}"));
    }
    app.pointer_up(hx + 35.0, hy + 15.0, Button::Primary);
    step(&mut app, &format!("{what} resize up"));
    for key in [Key::Right, Key::Down, Key::Down] {
        app.key_down(key, Mods::default());
        step(&mut app, &format!("{what} nudge"));
    }
    // another shape dragged far (pressed on it: a move, its old place left empty)
    app.pointer_down(sketch::BX + 700.0, sketch::BY + 700.0, Button::Primary);
    app.pointer_move(sketch::BX + 650.0, sketch::BY + 600.0);
    step(&mut app, &format!("{what} second drag"));
    app.pointer_up(sketch::BX + 650.0, sketch::BY + 600.0, Button::Primary);
    step(&mut app, &format!("{what} second drag up"));
    // a marquee from the empty board left of the drawing over the first shapes, then shift-added to
    app.pointer_down(sketch::BX + 12.0, sketch::BY + 700.0, Button::Primary);
    app.pointer_move(sketch::BX + 40.0, sketch::BY + 650.0);
    step(&mut app, &format!("{what} marquee empty"));
    app.pointer_move(sketch::BX + 200.0, sketch::BY + 30.0);
    step(&mut app, &format!("{what} marquee over shapes"));
    app.pointer_up(sketch::BX + 200.0, sketch::BY + 30.0, Button::Primary);
    step(&mut app, &format!("{what} marquee up"));
    assert!(app.owner.selected.len() > 2, "the marquee selected shapes");
    // draw a rectangle with the tool
    app.key_down(Key::Char('r'), Mods::default());
    app.pointer_down(sketch::BX + 900.0, sketch::BY + 650.0, Button::Primary);
    for k in 1..4 {
        app.pointer_move(sketch::BX + 900.0 + 20.0 * k as f32, sketch::BY + 650.0 + 15.0 * k as f32);
        step(&mut app, &format!("{what} draw {k}"));
    }
    app.pointer_up(sketch::BX + 960.0, sketch::BY + 695.0, Button::Primary);
    step(&mut app, &format!("{what} drawn"));
    // undo, redo, select all, Escape
    let cmd = Mods { cmd: true, ..Mods::default() };
    app.key_down(Key::Char('z'), cmd);
    step(&mut app, &format!("{what} undo"));
    app.key_down(Key::Char('z'), Mods { shift: true, ..cmd });
    step(&mut app, &format!("{what} redo"));
    app.key_down(Key::Char('a'), cmd);
    step(&mut app, &format!("{what} select all"));
    app.key_down(Key::Escape, Mods::default());
    step(&mut app, &format!("{what} escape again"));
    // the retained app against a fresh mount of the same state
    let fresh = app.owner.fresh_like();
    let mut f = sketch::mount_view(fresh, true, 4);
    same_as_fresh(&mut app, &mut f, &what);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sketch_every_ring() {
    for ring in RINGS {
        run(ring, 500);
    }
}

#[test]
fn snapping_follows_zgx() {
    // the drag's positions: zgx's `shift` over the others' lines, then the grid
    let mut app = sketch::mount(false, 0, 564);
    let dir = std::env::temp_dir().join(format!("bough-sketch-snap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("busy.json");
    std::fs::write(&path, sketch::busy_json(500)).unwrap();
    app.turn(|s| s.open(&path));
    let (x, y) = sketch::at(16.0, 16.0);
    app.pointer_down(x, y, Button::Primary);
    assert_eq!(app.owner.selected, vec![1]);
    let mut seen = Vec::new();
    for k in 0..22 {
        app.pointer_move(x + 2.0 * k as f32, y + k as f32);
        let s = app.owner.shapes[0];
        seen.push((s.x, s.y));
        assert_eq!(*app.owner.f_x, sketch::shown(s.x));
    }
    // every position lies on another shape's line or the 8-point grid
    for (sx, sy) in seen {
        assert!(sx % 8.0 == 0.0 && sy % 8.0 == 0.0, "({sx}, {sy})");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
