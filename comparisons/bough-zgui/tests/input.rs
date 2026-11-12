//! Pixel-exactness of every interaction: after every input turn and frame, the presented surface equals a
//! from-scratch full redraw of the retained state (same kernels), at every repair age 1..=4 (round-robin
//! rings of 1..4 surfaces) and with the windowed content-addressed ring; and at the end the retained state
//! equals a fresh mount at the same signal values. Covers hover (paint writes, content addressing),
//! clicks, focus rings, typing and caret moves, wheel glides over a virtual list and a scroll view (page
//! offsets, moving clips), the gallery's animations, label churn, and resize (cephal relayout).
use bough::raw::*;
use bough_zgui::gallery::{turn as gturn, Gallery, TICK_60};
use bough_zgui::*;

const RINGS: [usize; 5] = [1, 2, 3, 4, 0];

fn app_ring<V: View>(v: V, mut c: Config, ring: usize) -> App<V> {
    c.ring = if ring == 0 { 4 } else { ring };
    let mut a = App::new(v, c, true);
    a.windowed = ring == 0;
    a
}

fn crop(px: &[u8], full_w: usize, w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        out.extend_from_slice(&px[y * full_w * 4..y * full_w * 4 + w * 4]);
    }
    out
}

fn first_diff(a: &[u8], b: &[u8], w: usize) -> Option<(usize, usize, u8, u8)> {
    a.iter().zip(b).position(|(x, y)| x != y).map(|i| ((i / 4) % w, (i / 4) / w, a[i], b[i]))
}

/// The window region of the presented surface vs a full redraw (both cropped to the current window).
fn assert_exact<V: View>(app: &mut App<V>, full_w: usize, what: &str) {
    // the surfaces follow the window (bough-metal `Fused::resize`): their width is the stride
    let _ = full_w;
    let full_w = app.surface_width();
    let (w, h) = (app.rt.win_w as usize, app.rt.win_h as usize);
    let px = crop(&app.presented_pixels(), full_w, w, h);
    let r = crop(&app.reference_pixels(), full_w, w, h);
    assert!(px == r, "{what}: presented != full redraw at {:?}", first_diff(&px, &r, w));
}

/// Run `steps`; after each, frame and compare. Returns the app.
fn run<V: View>(name: &str, mut app: App<V>, steps: usize, mut step: impl FnMut(&mut App<V>, usize)) -> App<V> {
    let w = app.rt.win_w as usize;
    let name = &format!("{name} (windowed {}, ring {})", app.windowed, app.surfaces());
    app.frame();
    for t in 0..steps {
        step(&mut app, t);
        app.frame();
        assert!(!app.bins_overflowed(), "{name}: bin overflow at step {t}");
        assert_exact(&mut app, w, &format!("{name} step {t}"));
    }
    app
}

fn same_as_fresh<V: View>(name: &str, app: &mut App<V>, mut fresh: App<V>) {
    // the surfaces' width is the stride (they follow the window)
    let w = app.surface_width();
    let (cw, ch) = (app.rt.win_w as usize, app.rt.win_h as usize);
    let px = crop(&app.presented_pixels(), w, cw, ch);
    fresh.frame();
    let fw = fresh.surface_width();
    let fp = crop(&fresh.presented_pixels(), fw, cw, ch);
    assert!(px == fp, "{name}: retained != fresh mount at {:?}", first_diff(&px, &fp, cw));
}

#[test]
fn hover_crossings_are_pixel_exact_and_cells_answer_moves() {
    for ring in RINGS {
        let app = app_ring(Table { rows: 120 }, config(WIDTH, HEIGHT), ring);
        // row centres down and up, moves inside one row's cell, then off every row (below the table)
        let path: Vec<(f32, f32)> = (0..40)
            .map(|i| table_row_centre(i % 40))
            .chain([(800.0, 12.0), (900.0, 14.0), (1500.0, 6.0), (20.0, 12.0), (20.0, 37.0), (800.0, 61.0)])
            .chain((0..12).rev().map(table_row_centre))
            .collect();
        let n = path.len();
        let mut app = run("hover", app, n + 1, |a, t| if t < n { a.pointer_move(path[t].0, path[t].1) } else { a.pointer_leave() });
        let st = app.rt.input.stats;
        assert!(st.cell_hits >= 1, "a move inside a row's cell is answered by the cell: {st:?}");
        assert!(st.crossings >= 50, "{st:?}");
        same_as_fresh("hover", &mut app, App::new(Table { rows: 120 }, config(WIDTH, HEIGHT), true));
    }
}

#[test]
fn hover_toggles_are_served_by_held_surfaces() {
    // content addressing: a crossing back to an earlier hover state finds a surface that holds it
    let mut app = app_ring(Table { rows: 60 }, config(WIDTH, HEIGHT), 0);
    app.frame();
    let w = app.rt.win_w as usize;
    for t in 0..30 {
        let (x, y) = table_row_centre(t % 2);
        app.pointer_move(x, y);
        app.frame();
        assert_exact(&mut app, w, &format!("toggle {t}"));
    }
    assert!(app.surf_stats.held + app.surf_stats.on_screen >= 20, "{:?}", app.surf_stats);
}

#[test]
fn wheel_glides_over_a_virtual_list_are_pixel_exact() {
    for ring in RINGS {
        let app = app_ring(LongList { scroll: Signal::new(0.0) }, config(WIDTH, HEIGHT), ring);
        let mut app = run("list glide", app, 70, |a, t| {
            if t == 0 {
                a.pointer_move(800.0, 500.0);
            }
            match t {
                1 | 20 | 23 => a.wheel(800.0, 500.0, 0.0, 6.0),
                45 => a.wheel(800.0, 500.0, 0.0, -9.0),
                _ => {
                    a.tick(0.008_333);
                }
            }
        });
        while app.animating() {
            app.tick(0.008_333);
            app.frame();
        }
        let s = *app.owner.scroll;
        assert!(s > 0.0);
        same_as_fresh("list glide", &mut app, App::new(LongList { scroll: Signal::new(s) }, config(WIDTH, HEIGHT), true));
    }
}

#[test]
fn gallery_interactions_are_pixel_exact() {
    for ring in RINGS {
        let app = app_ring(Gallery::new(), config(WIDTH, HEIGHT), ring);
        let keys: Vec<Key> = "Tab"
            .split(',')
            .flat_map(|_| std::iter::repeat_n(Key::Tab, 22))
            .chain("xyz".chars().map(Key::Char))
            .chain([Key::Left, Key::Left, Key::Char('Q'), Key::Home, Key::Char('W'), Key::End, Key::Backspace, Key::Backspace, Key::Delete])
            .collect();
        let nk = keys.len();
        let mut app = run("gallery", app, nk + 40, |a, t| {
            if t < nk {
                a.key_down(keys[t], Mods::default());
                return;
            }
            match t - nk {
                // swatch hover, press, release; another swatch
                0 => a.pointer_move(41.0, 41.0),
                1 => a.pointer_down(41.0, 41.0, Button::Primary),
                2 => a.pointer_up(41.0, 41.0, Button::Primary),
                3 => a.pointer_move(173.0, 41.0),
                4 => a.pointer_down(173.0, 41.0, Button::Primary),
                5 => a.pointer_up(173.0, 41.0, Button::Primary),
                // the glide over the root (moving clips and the focus ring with the content), animations ticking
                6 => a.pointer_move(800.0, 500.0),
                7 | 12 => a.wheel(800.0, 500.0, 0.0, 3.0),
                25 => a.wheel(800.0, 500.0, 0.0, -2.0),
                _ => {
                    gturn(a, TICK_60);
                }
            }
        });
        // settle: glides end, pointer and focus leave; then the state is the signals' alone
        while app.animating() {
            gturn(&mut app, TICK_60);
        }
        app.pointer_leave();
        app.set_focus(None, false);
        app.frame();
        let g = &app.owner;
        let fresh = Gallery { scroll: Signal::new(*g.scroll), picked: Signal::new(*g.picked), value: Signal::new((*g.value).clone()), clock: Signal::new(*g.clock) };
        assert_eq!(*g.value, "WAda LovelacexQ");
        same_as_fresh("gallery", &mut app, App::new(fresh, config(WIDTH, HEIGHT), true));
    }
}

#[test]
fn label_churn_is_pixel_exact() {
    for ring in RINGS {
        let app = app_ring(Churn { zoom: Signal::new(0) }, config(CHURN_W, CHURN_H), ring);
        let mut app = run("churn", app, 40, |a, t| {
            a.turn(|c| {
                c.zoom.set(t + 1);
            })
        });
        same_as_fresh("churn", &mut app, App::new(Churn { zoom: Signal::new(40) }, config(CHURN_W, CHURN_H), true));
    }
}

#[test]
fn resize_relayout_is_pixel_exact() {
    for ring in RINGS {
        let app = app_ring(Slope { rows: 48, warm: Signal::new(false) }, config(SLOPE_W, SLOPE_H), ring);
        let widths = [992.0, 960.0, 880.0, 999.0, 640.0, 1000.0, 712.0];
        let mut app = run("resize", app, widths.len() * 2, |a, t| {
            if t % 2 == 0 {
                a.resize(widths[t / 2], SLOPE_H - (t as f32) * 8.0);
            } else {
                a.turn(|v| {
                    let w = !*v.warm;
                    v.warm.set(w);
                });
            }
        });
        // a fresh mount at the final size and colour
        let (w, h) = (app.rt.win_w as f32, app.rt.win_h as f32);
        let warm = *app.owner.warm;
        same_as_fresh("resize", &mut app, App::new(Slope { rows: 48, warm: Signal::new(warm) }, config(w, h), true));
    }
}


#[test]
fn shell_tooltips_and_dock_drag_are_pixel_exact() {
    use bough_zgui::shell::Shell;
    for ring in RINGS {
        let app = app_ring(Shell::new(), config(WIDTH, HEIGHT), ring);
        // a sweep of the head (fading tooltips handed over), a row's hover, then the dock dragged up and down
        let mut app = run("shell", app, 12 * 3 + 4 + 22 + 2, |a, t| {
            if t < 36 {
                let k = t / 3;
                match t % 3 {
                    0 => a.pointer_move(25.0 + 38.0 * k as f32, 21.0),
                    _ => {
                        a.tick(1.0 / 60.0);
                    }
                }
                return;
            }
            match t - 36 {
                0 => a.pointer_move(800.0, 200.0),
                1 => a.pointer_move(800.0, 692.5),
                2 => a.pointer_down(800.0, 692.5, Button::Primary),
                3 => a.pointer_move(800.0, 600.0),
                k if k < 26 => a.pointer_move(800.0, 600.0 + (k as f32 - 3.0) * 7.0),
                26 => a.pointer_up(800.0, 754.0, Button::Primary),
                _ => a.pointer_leave(),
            }
        });
        let (split, top) = (*app.owner.split, *app.owner.dock_top);
        assert!(split > 692.0 && top == split + 1.0, "{split} {top}");
        let mut fresh = Shell::new();
        fresh.split = Signal::new(split);
        fresh.dock_top = Signal::new(top);
        same_as_fresh("shell", &mut app, App::new(fresh, config(WIDTH, HEIGHT), true));
    }
}

/// A nested document: parents with children with grandchildren, overlapping and overflowing, some of
/// them interactive (an `on_hover` route that logs enter/leave), some pointer-transparent.
struct Nest {
    log: Vec<(u32, bool)>,
}

fn rect(seed: u32) -> [f32; 4] {
    let mut r = bough_core::rng::Rng::new(seed as u64);
    [r.range(0, 700) as f32, r.range(0, 500) as f32, r.range(20, 260) as f32, r.range(20, 200) as f32]
}

fn leaf(id: u32) -> impl Element<Nest> {
    let [x, y, w, h] = rect(id);
    quad(x * 0.3, y * 0.3, w * 0.5, h * 0.5).bg(Color::rgba(200, (id * 37 % 255) as u8, 90, 255)).on_hover(move |n: &mut Nest, e| n.log.push((id, e.on)))
}

impl View for Nest {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, 800.0, 600.0).bg(rgb(0x101010)),
            each((0..12u32).map(|p| {
                let [x, y, w, h] = rect(1000 + p);
                quad(x, y, w, h)
                    .bg(Color::rgba(40, 60, (p * 20) as u8, 255))
                    .children(each((0..4u32).map(move |c| {
                        let id = 100 + p * 10 + c;
                        let [cx, cy, cw, ch] = rect(id);
                        quad(cx * 0.4 - 40.0, cy * 0.4 - 40.0, cw * 0.6, ch * 0.6)
                            .bg(Color::rgba(90, (c * 50) as u8, 140, 255))
                            .children((leaf(id * 10 + 1), no_hit(quad(0.0, 0.0, 30.0, 30.0).bg(rgb(0xffffff)))))
                            .on_hover(move |n: &mut Nest, e| n.log.push((id, e.on)))
                    })))
                    .on_hover(move |n: &mut Nest, e| n.log.push((p, e.on)))
            })),
        )
    }
}

#[test]
fn hover_enter_leave_sets_equal_the_oracle() {
    use bough_core::{KF_INTERACT, KF_NOHIT};
    let mut app = App::new(Nest { log: Vec::new() }, config(800.0, 600.0), false);
    app.frame();
    // the act ids: map each interactive record's handle to the id its route logs, by entering it alone
    let recs = app.rt.bough.records();
    let mut rng = bough_core::rng::Rng::new(7);
    let mut prev: Vec<usize> = Vec::new();
    let mut moves = 0;
    for step in 0..3000 {
        let (x, y) = if step % 50 == 49 { (-5.0, -5.0) } else { (rng.range(0, 799) as f32, rng.range(0, 599) as f32) };
        let before = app.owner.log.len();
        if x < 0.0 {
            app.pointer_leave();
        } else {
            app.pointer_move(x, y);
        }
        app.frame();
        moves += 1;
        // oracle: the topmost record under the pixel (not NOHIT), then its interactive ancestors-or-self
        let mut cur: Vec<usize> = Vec::new();
        if x >= 0.0 {
            let (px, py) = (x as i32, y as i32);
            let vis = |r: &bough_core::page::Record| if r.clip == 0 { r.ink } else { r.ink.intersect(app.rt.bough.clip_rect(r.clip)).canon() };
            if let Some(top) = recs.iter().rposition(|r| r.kind & KF_NOHIT == 0 && vis(r).contains_point(px, py)) {
                let mut i = Some(top);
                while let Some(j) = i {
                    if recs[j].kind & KF_INTERACT != 0 {
                        cur.push(j);
                    }
                    let d = recs[j].depth;
                    i = (0..j).rev().find(|&k| recs[k].depth < d);
                }
            }
        }
        // the app's hover set (positions) equals the oracle's
        let mut got: Vec<usize> = app.rt.input.hovered().iter().map(|h| app.rt.bough.pos_of(*h).unwrap()).collect();
        got.sort();
        let mut want = cur.clone();
        want.sort();
        assert_eq!(got, want, "step {step} at ({x},{y})");
        // the routes ran for exactly the symmetric difference
        let entered = cur.iter().filter(|p| !prev.contains(p)).count();
        let left = prev.iter().filter(|p| !cur.contains(p)).count();
        let log = &app.owner.log[before..];
        assert_eq!(log.iter().filter(|e| e.1).count(), entered, "step {step}: enters");
        assert_eq!(log.iter().filter(|e| !e.1).count(), left, "step {step}: leaves");
        prev = cur;
    }
    assert!(moves == 3000 && app.rt.input.stats.crossings > 500, "{:?}", app.rt.input.stats);
}

#[test]
fn laid_out_gallery_interactions_and_resize_are_pixel_exact() {
    use bough_zgui::gallery::{turn_l, GalleryL};
    for ring in RINGS {
        let app = app_ring(GalleryL::new(), config(WIDTH, HEIGHT), ring);
        let keys: Vec<Key> = std::iter::repeat_n(Key::Tab, 22).chain("xyz".chars().map(Key::Char)).chain([Key::Left, Key::Char('Q'), Key::End, Key::Backspace]).collect();
        let nk = keys.len();
        let mut app = run("gallery-l", app, nk + 34, |a, t| {
            if t < nk {
                a.key_down(keys[t], Mods::default());
                return;
            }
            match t - nk {
                0 => a.pointer_move(41.0, 41.0),
                1 => a.pointer_down(41.0, 41.0, Button::Primary),
                2 => a.pointer_up(41.0, 41.0, Button::Primary),
                3 => a.pointer_move(800.0, 500.0),
                4 | 9 => a.wheel(800.0, 500.0, 0.0, 3.0),
                // relayout at narrower and shorter windows (the surfaces keep their size: shrink only)
                20 => a.resize(1480.0, 1000.0),
                24 => a.resize(1352.0, 920.0),
                28 => a.resize(1544.0, 960.0),
                _ => {
                    turn_l(a, TICK_60);
                }
            }
        });
        while app.animating() {
            turn_l(&mut app, TICK_60);
        }
        app.pointer_leave();
        app.set_focus(None, false);
        app.frame();
        let g = &app.owner;
        let fresh = GalleryL { scroll: Signal::new(*g.scroll), picked: Signal::new(*g.picked), value: Signal::new((*g.value).clone()), clock: Signal::new(*g.clock), slide: Signal::new(*g.slide) };
        assert_eq!(*g.value, "Ada LovelacexyQ");
        let (w, h) = (app.rt.win_w as f32, app.rt.win_h as f32);
        same_as_fresh("gallery-l", &mut app, App::new(fresh, config(w, h), true));
    }
}

#[test]
fn laid_out_shell_drag_and_resize_are_pixel_exact() {
    use bough_zgui::shell::ShellL;
    for ring in RINGS {
        let app = app_ring(ShellL::new(), config(WIDTH, HEIGHT), ring);
        let mut app = run("shell-l", app, 40, |a, t| match t {
            0 => a.pointer_move(25.0, 21.0),
            1 | 2 | 3 => {
                a.tick(1.0 / 60.0);
            }
            4 => a.pointer_move(800.0, 692.5),
            5 => a.pointer_down(800.0, 692.5, Button::Primary),
            k if k < 26 => a.pointer_move(800.0, 692.5 - (k as f32 - 5.0) * 9.0),
            26 => a.pointer_up(800.0, 512.5, Button::Primary),
            30 => a.resize(1440.0, 920.0),
            34 => a.resize(1280.0, 840.0),
            _ => a.pointer_leave(),
        });
        let split = *app.owner.split;
        assert!(split < 65.0, "{split}");
        let mut fresh = ShellL::new();
        fresh.split = Signal::new(split);
        let (w, h) = (app.rt.win_w as f32, app.rt.win_h as f32);
        same_as_fresh("shell-l", &mut app, App::new(fresh, config(w, h), true));
    }
}

#[test]
fn thread_resize_rewraps_pixel_exact() {
    use bough_zgui::thread::{thread_config, Thread};
    for ring in RINGS {
        let app = app_ring(Thread { scroll: Signal::new(0.0) }, thread_config(WIDTH, HEIGHT), ring);
        let widths = [1400.0, 700.0, 1180.0, 520.0, 1560.0];
        let mut app = run("thread", app, widths.len() + 6, |a, t| {
            if t < widths.len() {
                a.resize(widths[t], HEIGHT);
            } else if t == widths.len() {
                a.pointer_move(300.0, 500.0);
                a.wheel(300.0, 500.0, 0.0, 9.0);
            } else {
                a.tick(1.0 / 60.0);
            }
        });
        while app.animating() {
            app.tick(1.0 / 60.0);
            app.frame();
        }
        let s = *app.owner.scroll;
        let (w, h) = (app.rt.win_w as f32, app.rt.win_h as f32);
        same_as_fresh("thread", &mut app, App::new(Thread { scroll: Signal::new(s) }, thread_config(w, h), true));
    }
}

/// A focused field's blinking caret: pixel exact at every flip, and after the first two flips every frame is
/// a surface that already holds the state (no GPU work), as the blink scene.
#[test]
fn caret_blink_is_served_by_held_surfaces() {
    use bough_zgui::gallery::Gallery;
    let mut app = app_ring(Gallery::new(), config(WIDTH, HEIGHT), 0);
    app.frame();
    for _ in 0..22 {
        app.key_down(Key::Tab, Mods::default());
        app.frame();
    }
    assert!(app.rt.input.focused_field());
    let w = app.rt.win_w as usize;
    let gpu0 = app.surf_stats.gpu;
    for t in 0..40 {
        assert!(app.tick(0.53), "flip {t}");
        assert!(app.frame());
        assert_exact(&mut app, w, &format!("blink flip {t}"));
    }
    assert!(app.surf_stats.gpu - gpu0 <= 2, "GPU frames after the first two flips: {:?}", app.surf_stats);
}

/// A scroll view nested in another, with a tooltip in the outer content: the inner view's space and clips and
/// the tooltip's rect follow the outer scroll; pixel exact, and equal to a fresh mount at the same offsets.
struct Nested {
    outer: Signal<f32>,
    inner: Signal<f32>,
}

fn nested_row(i: usize) -> impl Element<Nested> {
    quad(10.0, i as f32 * 40.0, 300.0, 34.0).bg(Color::rgba(40, (i * 30 % 255) as u8, 120, 255)).children(text(format!("row {i}")).at(8.0, 8.0).size(14.0).line_height(18.0).color(rgb(0xffffff)))
}

impl View for Nested {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, 800.0, 600.0).bg(rgb(0x101010)),
            scroll_y(
                sig!(self.outer),
                [20.0, 20.0, 700.0, 500.0],
                1200.0,
                (
                    each((0..40).map(nested_row)),
                    tooltip(quad(400.0, 300.0, 60.0, 30.0).bg(rgb(0x3d7ef7)), "inside the outer view", [380.0, 340.0, 180.0, 28.0]),
                    clip(380.0, 600.0, 320.0, 300.0, scroll_y(sig!(self.inner), [380.0, 600.0, 300.0, 200.0], 400.0, each((0..15).map(|i| quad(0.0, i as f32 * 40.0, 280.0, 34.0).bg(Color::rgba(200, (i * 17 % 255) as u8, 60, 255))))).bg(rgb(0x202830))),
                ),
            )
            .bg(rgb(0x181818))
            .line(40.0),
        )
    }
}

#[test]
fn nested_views_and_tooltips_follow_an_outer_scroll() {
    for ring in RINGS {
        let app = app_ring(Nested { outer: Signal::new(0.0), inner: Signal::new(0.0) }, config(800.0, 600.0), ring);
        let mut app = run("nested", app, 60, |a, t| {
            match t {
            0 => a.pointer_move(500.0, 300.0),
            1 => a.wheel(500.0, 300.0, 0.0, 6.0), // outer: 240, the inner view comes into the port
            20 => a.pointer_move(500.0, 450.0),
            21 => a.wheel(500.0, 450.0, 0.0, 3.0), // the inner view
            40 => {
                a.pointer_move(440.0, 95.0); // the tooltip trigger, moved up 240 with the outer content
                assert_eq!(a.rt.input.hovered().len(), 2, "the trigger (and the outer port) are hovered");
            }
            41 => a.wheel(440.0, 95.0, 0.0, -1.0), // the shown tooltip moves with the outer view
            _ => {
                a.tick(1.0 / 60.0);
            }
        }});
        while app.animating() {
            app.tick(1.0 / 60.0);
            app.frame();
        }
        app.pointer_leave();
        app.frame();
        let (o, i) = (*app.owner.outer, *app.owner.inner);
        assert!(o > 0.0 && i > 0.0, "both views scrolled: {o} {i}");
        same_as_fresh("nested", &mut app, App::new(Nested { outer: Signal::new(o), inner: Signal::new(i) }, config(800.0, 600.0), true));
    }
}
