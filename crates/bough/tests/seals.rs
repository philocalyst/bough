//! Seals (`seal.rs`): a dynamic item whose description is bit-identical to the retained one is stepped over.
//! The oracle is a fresh mount: after every change the retained tree's full redraw must equal the full
//! redraw of a new `App` mounted from the same state, and the presented (repaired) frame must equal both.
//! The sequence changes, one at a time, everything a row's description can carry (a colour, a text, a layout
//! spec, the number of rows, the order of rows) and the context a row inherits (a clip and an origin above
//! it). `BOUGH_SEAL_MUTANT=1` makes the check ignore text bytes: the gate must fail then
//! (`a_seal_that_ignores_text_is_caught`).
use bough::prelude::*;
use bough::raw::{App, Config};

#[derive(Clone)]
struct Table {
    rows: Vec<(u32, String)>,
    selected: u32,
    row_h: f32,
    wide: bool,
    indent: f32,
    /// Window size in points (a change is a resize of the retained app, and the fresh mount's size).
    win: (f32, f32),
    /// Rows as wide as the root (their notes rewrap with the window) instead of a fixed width.
    fill: bool,
}

impl View for Table {
    fn view(&self) -> impl Element<Self> + '_ {
        let line = move |i: usize| {
            let (id, label) = &self.rows[i];
            let bg = if *id == self.selected { rgb(0x2f6bff) } else if i % 2 == 0 { rgb(0xf5f5f5) } else { rgb(0xe6e6e6) };
            // a nested container per item: a row of two labels above a note that wraps at the item's width, so
            // an item that moves carries a multi-line text and a box two levels down
            let head = row((text(*id).w(48.0), text(label.as_str()).color(rgb(0x101010)))).h(self.row_h);
            let note = para("a note long enough to wrap onto a second line at this width").font(10.0, 12.0).color(rgb(0x404040));
            let item = div().child(head).child(note).bg(bg);
            if self.fill { item.w_pct(1.0) } else { item.w(if self.wide { 300.0 } else { 240.0 }) }
        };
        div().bg(rgb(0xffffff)).pad(self.indent, 4.0, 0.0, 0.0).child(each((0..self.rows.len()).map(line)))
    }
}

fn config_at(win: (f32, f32)) -> Config {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    Config::new(win.0, win.1, 2.0, font)
}
fn config() -> Config {
    config_at(table().win)
}

fn table() -> Table {
    Table { rows: (0..12).map(|i| (i, format!("Row {i}"))).collect(), selected: u32::MAX, row_h: 20.0, wide: false, indent: 0.0, win: (360.0, 300.0), fill: false }
}

/// Apply every step; after each, compare retained, presented and fresh. Returns the first failing step.
fn run() -> Result<(), String> {
    let steps: Vec<(&str, Box<dyn Fn(&mut Table)>)> = vec![
        ("select 3", Box::new(|t| t.selected = 3)),
        ("select 7", Box::new(|t| t.selected = 7)),
        ("nothing", Box::new(|_| {})),
        ("relabel 5", Box::new(|t| t.rows[5].1 = "Renamed".into())),
        ("relabel 5 same length", Box::new(|t| t.rows[5].1 = "Renamee".into())),
        ("renumber 2", Box::new(|t| t.rows[2].0 = 1234)),
        ("taller rows", Box::new(|t| t.row_h = 26.0)),
        ("wider rows", Box::new(|t| t.wide = true)),
        ("indent (the context above every row)", Box::new(|t| t.indent = 18.0)),
        ("remove first", Box::new(|t| drop(t.rows.remove(0)))),
        ("remove middle", Box::new(|t| drop(t.rows.remove(4)))),
        ("insert front", Box::new(|t| t.rows.insert(0, (99, "Inserted".into())))),
        ("insert middle", Box::new(|t| t.rows.insert(6, (98, "Also inserted".into())))),
        ("swap 1 and 8", Box::new(|t| t.rows.swap(1, 8))),
        ("append", Box::new(|t| t.rows.push((77, "Appended".into())))),
        ("truncate", Box::new(|t| t.rows.truncate(5))),
        ("clear", Box::new(|t| t.rows.clear())),
        ("refill", Box::new(|t| t.rows = (0..9).map(|i| (i + 40, format!("Again {i}"))).collect())),
        ("unindent", Box::new(|t| t.indent = 0.0)),
        // width changes: the window alone (fixed-width rows keep their layout), rows that follow the window
        // (their notes rewrap), and structure edits while they do
        ("narrower window", Box::new(|t| t.win = (300.0, 300.0))),
        ("rows fill the root", Box::new(|t| t.fill = true)),
        ("wider window", Box::new(|t| t.win = (420.0, 300.0))),
        ("remove first of filled rows", Box::new(|t| drop(t.rows.remove(0)))),
        ("narrow window, filled rows", Box::new(|t| t.win = (200.0, 300.0))),
        ("insert into filled rows", Box::new(|t| t.rows.insert(1, (55, "Late".into())))),
        ("fixed-width rows again", Box::new(|t| t.fill = false)),
        ("first window size", Box::new(|t| t.win = (360.0, 300.0))),
        ("deselect", Box::new(|t| t.selected = u32::MAX)),
    ];
    for ring in 1..=4usize {
        let mut cfg = config();
        cfg.ring = ring;
        let mut app = App::new(table(), cfg, true);
        app.frame();
        let mut state = table();
        for (name, step) in &steps {
            let win = state.win;
            step(&mut state);
            app.turn(|t| step(t));
            if state.win != win {
                app.resize(state.win.0, state.win.1);
            }
            app.frame();
            let (retained, presented) = (app.reference_pixels(), app.presented_pixels());
            let mut fresh = App::new(state.clone(), config_at(state.win), true);
            fresh.frame();
            let fresh = fresh.reference_pixels();
            if retained != fresh {
                return Err(format!("ring {ring}, after `{name}`: retained != fresh mount"));
            }
            if presented != retained {
                return Err(format!("ring {ring}, after `{name}`: presented != full redraw"));
            }
        }
    }
    Ok(())
}

#[test]
fn sealed_rows_equal_a_fresh_mount_after_every_change() {
    if std::env::var_os("BOUGH_SEAL_MUTANT").is_some() || std::env::var_os("BOUGH_MOVE_MUTANT").is_some() || std::env::var_os("BOUGH_MERGE_MUTANT").is_some() {
        return; // the mutants' own tests below
    }
    run().unwrap();
}

/// The negative control of the moved-only write-back (`Rt::lines_move`): with the mutant on, a text that only
/// moved leaves its last line where it was, and the gate says so at the first step that moves a row.
/// Run alone: `BOUGH_MOVE_MUTANT=1 cargo test --release -p bough --test seals -- a_moved_text_that_forgets`.
#[test]
fn a_moved_text_that_forgets_its_last_line_is_caught() {
    if std::env::var_os("BOUGH_MOVE_MUTANT").is_none() {
        return;
    }
    let e = run().expect_err("the mutant passed the gate");
    assert!(e.contains("taller rows") || e.contains("indent"), "caught at the wrong step: {e}");
}

/// The negative control: with the mutant on, a text change inside a sealed row is missed and the gate says so.
/// Run alone: `BOUGH_SEAL_MUTANT=1 cargo test --release -p bough --test seals -- a_seal_that_ignores_text`.
#[test]
fn a_seal_that_ignores_text_is_caught() {
    if std::env::var_os("BOUGH_SEAL_MUTANT").is_none() {
        return;
    }
    let e = run().expect_err("the mutant passed the gate");
    assert!(e.contains("relabel 5"), "caught at the wrong step: {e}");
}

/// The negative control of the merged ink batch (`Bough::set_inks_handles_merged`): with the mutant on, the
/// places moved records left are not damaged, so the presented frame keeps their old pixels.
/// Run alone: `BOUGH_MERGE_MUTANT=1 cargo test --release -p bough --test seals -- a_merged_batch_that_forgets`.
#[test]
fn a_merged_batch_that_forgets_old_inks_is_caught() {
    if std::env::var_os("BOUGH_MERGE_MUTANT").is_none() {
        return;
    }
    let e = run().expect_err("the mutant passed the gate");
    assert!(e.contains("presented != full redraw"), "caught for the wrong reason: {e}");
}
