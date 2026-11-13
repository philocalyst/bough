//! The hot paths of the new core on the sheet, as cells: what `examples/hot_x.rs` counts and `examples/alloc_x.rs`
//! watches for allocations and page faults (threads lane, round 8).
#![allow(dead_code)]
#[path = "sheet.rs"]
pub mod sheet;
use bough_x::prelude::*;
use bough_x::soft::Soft;
use sheet::*;
use std::hint::black_box;

impl Bed {
    /// The sheet of `rows` rows, mounted, with its image saved for the start-up cells.
    pub fn new(rows: usize) -> Bed {
        let sheet = Sheet::new(rows);
        let mut world = World::<Soft>::new(W, H, NWORDS);
        world.mount(&sheet);
        world.tiles.clear();
        let image = std::env::temp_dir().join(format!("hot_x-{}.bx", std::process::id()));
        world.save(&image).expect("writing the image");
        Bed { world, sheet, soft: Soft::new(W, H), rows, image }
    }
}

/// What a cell runs on.
pub struct Bed {
    pub world: World<Soft>,
    pub sheet: Sheet,
    pub soft: Soft,
    pub rows: usize,
    /// An image of the sheet as first mounted, for the start-up cells.
    pub image: std::path::PathBuf,
}

pub type Cell = (&'static str, &'static str, fn(&mut Bed, usize));

/// Every cell: name, which hot path of ROUND8 section 8 it is, and one repetition. A repetition leaves the bed
/// in a state from which the next one does the same amount of work.
pub const CELLS: &[Cell] = &[
    ("floor", "nothing: the bracket and the loop", |_, _| {}),
    ("idle", "a turn in which nothing changed", |b, _| b.world.settle(&b.sheet)),
    ("hover", "state write to damaged tiles: pointer onto / off a header (hit, handler, settle)", |b, k| {
        let y = if k % 2 == 0 { TOOLBAR + HEADER / 2 } else { TOOLBAR / 2 };
        b.world.pointer(&mut b.sheet, GUTTER + 3 * CELL_W + CELL_W / 2, y);
        b.world.settle(&b.sheet);
    }),
    ("hover_word", "state write to damaged tiles: the hover word alone (no hit test)", |b, k| {
        b.sheet.u.set::<Hover>(if k % 2 == 0 { 3 } else { NO_HOVER });
        b.world.settle(&b.sheet);
    }),
    ("theme", "state write to damaged tiles: every exposed record changes", |b, _| {
        b.sheet.toggle_theme();
        b.world.settle(&b.sheet);
    }),
    ("edit_exposed", "relation edit: one exposed cell and its total", |b, k| {
        let top = (b.sheet.u.get::<Scroll>() / CELL_H) as usize;
        b.sheet.edit((top + (k * 7) % 30).min(b.rows - 1), (k * 3) % COLS, (k * 37 % 9973) as f32 / 100.0);
        b.world.settle(&b.sheet);
    }),
    ("edit_unexposed", "relation edit: a cell nobody sees and its total", |b, k| {
        b.sheet.edit(b.rows - 1 - (k % 8).min(b.rows - 1), (k * 3) % COLS, (k * 37 % 9973) as f32 / 100.0);
        b.world.settle(&b.sheet);
    }),
    ("insert_remove", "relation edit: a row inserted in the window, then removed", |b, k| {
        let top = (b.sheet.u.get::<Scroll>() / CELL_H) as usize;
        let at = (top + 5 + (k / 2) % 20).min(b.sheet.rows.len() - 1);
        if k % 2 == 0 { b.sheet.insert_row(at, [k as f32; COLS], false) } else { b.sheet.remove_row(at, false) }
        b.world.settle(&b.sheet);
    }),
    ("scroll_6px", "exposure: the space moves, no row enters", |b, k| {
        b.sheet.scroll_by(if (k / 4) % 2 == 0 { 6 } else { -6 });
        b.world.settle(&b.sheet);
    }),
    ("scroll_3_rows", "exposure: three rows enter", |b, k| {
        b.sheet.scroll_by(if (k / 4) % 2 == 0 { 3 * CELL_H } else { -3 * CELL_H });
        b.world.settle(&b.sheet);
    }),
    ("scroll_page", "exposure materialising a page: 34 rows enter", |b, k| {
        b.sheet.scroll_by(if (k / 4) % 2 == 0 { 1500 } else { -1500 });
        b.world.settle(&b.sheet);
    }),
    ("scroll_far", "exposure: a jump of 100 windows, every row is new", |b, k| {
        let max = (b.rows as i32 * CELL_H - (BODY.y1 - BODY.y0)).max(0);
        let y = ((k as i64 * 150_000) % (max as i64 + 1)) as i32;
        let d = y - b.sheet.u.get::<Scroll>();
        b.sheet.scroll_by(d);
        b.world.settle(&b.sheet);
    }),
    ("redescribe_idle", "re-describing mode: the exposed view described again and compared", |b, _| b.world.redescribe(&b.sheet)),
    ("world_new", "a world made and dropped: the arena's reservation and its release, nothing mounted", |_, _| {
        let w = World::<Soft>::new(W, H, NWORDS);
        black_box(&w);
    }),
    ("mount", "first mount into a fresh world (allocation included; the previous world is dropped)", |b, _| {
        let mut w = World::<Soft>::new(W, H, NWORDS);
        w.mount(&b.sheet);
        black_box(&w);
        b.world = w;
    }),
    ("image_open", "start-up: map the image of the mounted scene; it can be painted (the view has not run)", |b, _| {
        let w = World::<Soft>::open(&b.image).expect("the image this run saved");
        black_box(w.shown.w[0]);
        drop(w);
    }),
    ("image_adopt", "start-up: map the image, then describe the view against it (exact; the world can take turns)", |b, _| {
        let mut w = World::<Soft>::open(&b.image).expect("the image this run saved");
        w.adopt(&b.sheet, NWORDS).expect("its own view");
        black_box(&w);
        b.world = w;
    }),
    ("frame_full", "a full frame through the reference raster (oracle; 2560 x 1600)", |b, _| {
        b.soft.records = 0;
        b.world.paint(&mut b.soft);
        black_box(b.soft.px[0]);
    }),
];

