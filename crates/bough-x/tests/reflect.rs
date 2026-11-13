//! Reflection in the core, held to the stores and to the view: the walk names every live record and nothing
//! else; an entity's values are what the pixels were shaded from; rows nobody materialised are described from
//! the view alone; source locations are the view's own lines. The last test is the thin inspector: pick, read,
//! write a theme word, see its dependents damaged.
#[path = "../scenes/sheet.rs"]
mod sheet;
use bough_x::prelude::*;
use bough_x::reflect::{Ent, Reflect, Val};
use bough_x::soft::Soft;
use bough_x::expr::StageId;
use sheet::*;

const SRC: &str = include_str!("../scenes/sheet.rs");

/// The 1-based line of the `n`-th occurrence of `pat` in the scene's source.
fn line_of(pat: &str, n: usize) -> u32 {
    SRC.lines().enumerate().filter(|(_, l)| l.contains(pat)).nth(n).map(|(i, _)| i as u32 + 1).expect("pattern in the scene")
}

fn cents(v: f32) -> i32 {
    (v as f64 * 100.0).round() as i32
}

fn mounted(rows: usize) -> (Sheet, World<Soft>) {
    let sheet = Sheet::new(rows);
    let mut world = World::<Soft>::new(W, H, NWORDS);
    world.mount(&sheet);
    (sheet, world)
}

#[test]
fn the_walk_is_the_stores() {
    let (sheet, world) = mounted(10_000);
    let r = Reflect::learn(&world.root, &sheet);
    assert!(r.current(&world.root));
    let items = r.items(&world.root, &world.shown);
    let (records, _, _) = world.root.stats();
    assert_eq!(items.len(), records, "one item per live record");
    for it in &items {
        assert!(!it.info.ops.is_empty(), "{} describes its operands", it.kind());
        assert!(it.location().file().ends_with("sheet.rs"));
        assert_eq!(it.words().len() * 4, it.info.ops.iter().map(|o| (o.word + o.words) as usize * 4).max().unwrap());
    }
    // the kinds of the sheet, and nothing unexposed: 34 rows of the body at most
    let rows: std::collections::BTreeSet<u32> = items.iter().filter(|i| i.ent.path.n == 2).map(|i| i.ent.row).collect();
    assert!(rows.len() <= 36 && *rows.iter().next().unwrap() == 0);
}

#[test]
fn pick_a_cell_and_read_it() {
    let (sheet, world) = mounted(10_000);
    let r = Reflect::learn(&world.root, &sheet);
    let (row, col) = (3u32, 2usize);
    let (x, y) = (GUTTER + col as i32 * CELL_W + 50, BODY.y0 + row as i32 * CELL_H + 10);
    let stack = r.stack(&world.root, &world.shown, x, y);
    let kinds: Vec<_> = stack.iter().map(|i| i.kind()).collect();
    assert_eq!(&kinds[..2], ["num", "fill"], "the number over its cell's fill; then whatever is below ({kinds:?})");
    let top = &stack[0];
    // entity: the body list (root > scope > list), row 3, the number of cell 2 (gutter fill and number first)
    assert_eq!((top.ent.path.n, top.ent.row, top.ent.pos), (2, row, 3 + 2 * col as u32));
    assert_eq!(top.get("value").unwrap().val, Val::Num(cents(value(row as usize, col)), 2));
    assert_eq!(top.text().unwrap(), Val::Num(cents(value(row as usize, col)), 2).text());
    let color = top.get("color").unwrap();
    assert_eq!(color.stage(), StageId::Turn, "a Rust function of the theme word");
    assert!(color.ty().contains("ink") && color.ty().contains("Theme"), "{}", color.ty());
    assert_eq!(top.get("value").unwrap().stage(), StageId::Mount);
    assert_eq!(top.location().line(), line_of("s.list(", 0));
    // the fill under it: its colour reads theme and hover, its record word is the column and the row's parity
    let fill = &stack[1];
    assert_eq!(fill.get("color").unwrap().words, &[col as u32 | ((row & 1) << 8)]);
    assert!(fill.info.reads & (1 << Theme::SLOT | 1 << Hover::SLOT) == (1 << Theme::SLOT | 1 << Hover::SLOT));
    // layout: the cell's rect in the body's space, and on screen
    assert_eq!(top.rect, Rect::new(GUTTER + col as i32 * CELL_W, row as i32 * CELL_H, CELL_W, CELL_H));
    assert_eq!(top.screen, top.rect.shift((0, BODY.y0)));
}

#[test]
fn locations_are_the_views_lines() {
    let (sheet, world) = mounted(100);
    let r = Reflect::learn(&world.root, &sheet);
    let items = r.items(&world.root, &world.shown);
    let label = |t: &str| items.iter().find(|i| i.kind() == "label" && i.text().as_deref() == Some(t)).unwrap();
    assert_eq!(label("C").location().line(), line_of("s.row(", 0), "a header label: the first row of the view");
    assert_eq!(label("Σ").location().line(), line_of("s.row(", 2), "the footer");
    let outline = items.iter().find(|i| i.kind() == "outline").unwrap();
    assert_eq!(outline.location().line(), line_of("s.row(", 1), "the selection outline, inside the body's scope");
    let (_, _, loc) = r.lists()[0];
    assert_eq!(loc.line(), line_of("s.list(", 0));
}

#[test]
fn rows_nobody_materialised_are_described_on_demand() {
    let (sheet, world) = mounted(10_000);
    let r = Reflect::learn(&world.root, &sheet);
    let (path, list, _) = r.lists()[0].clone();
    assert_eq!(list.n, 10_000);
    let before = world.root.stats();
    let snaps = Reflect::rows(&sheet, &world.shown, &path, 5_000..5_002);
    assert_eq!(world.root.stats(), before, "nothing was materialised");
    assert_eq!(snaps.len(), 2 * (2 + 2 * COLS));
    for s in &snaps {
        assert_eq!(s.ent.path, path);
        let c = s.ent.pos as usize;
        if s.kind == "num" && c >= 2 {
            let col = (c - 3) / 2;
            assert_eq!(s.get("value"), Some(Val::Num(cents(value(s.ent.row as usize, col)), 2)));
        }
        assert_eq!(s.rect, list.shape[c].shift((list.x, list.y + s.ent.row as i32 * list.row_h)));
    }
    // a row that is materialised describes the same way from the stores and from the view
    let stored: Vec<_> = r.items(&world.root, &world.shown).into_iter().filter(|i| i.ent.path == path && i.ent.row == 7).map(|i| i.snap()).collect();
    let mut fresh = Reflect::rows(&sheet, &world.shown, &path, 7..8);
    let mut stored = stored;
    stored.sort_by_key(|s| s.ent.pos);
    fresh.sort_by_key(|s| s.ent.pos);
    assert_eq!(stored, fresh);
}

#[test]
fn an_entity_survives_scrolling() {
    let (mut sheet, mut world) = mounted(10_000);
    let r = Reflect::learn(&world.root, &sheet);
    let e = Ent { path: r.lists()[0].0, row: 3, pos: 5 };
    let v0 = r.find(&world.root, &world.shown, e).unwrap().get("value").unwrap().val;
    let s0 = r.find(&world.root, &world.shown, e).unwrap().screen;
    sheet.scroll_by(CELL_H);
    world.settle(&sheet);
    let it = r.find(&world.root, &world.shown, e).unwrap();
    assert_eq!(it.get("value").unwrap().val, v0);
    assert_eq!(it.screen, s0.shift((0, -CELL_H)));
    // past the ring's capacity, a row's slot is not its index: row 75 lives at slot 11
    sheet.scroll_by(69 * CELL_H);
    world.settle(&sheet);
    let e75 = Ent { row: 75, ..e };
    let it = r.find(&world.root, &world.shown, e75).expect("row 75 is exposed");
    assert_eq!(it.get("value").unwrap().val, Val::Num(cents(value(75, 1)), 2));
    assert!(r.find(&world.root, &world.shown, e).is_none(), "scrolled out: not materialised");
    let s = Reflect::rows(&sheet, &world.shown, &e.path, 3..4);
    assert_eq!(s.iter().find(|s| s.ent == e).unwrap().get("value"), Some(v0));
}

/// The inspector, as thin as it can be: pick an element, read its location, operands and layout, write a theme
/// word, and see every dependent and nothing else damaged.
#[test]
fn thin_inspector() {
    let (mut sheet, mut world) = mounted(10_000);
    world.tiles.clear();
    let r = Reflect::learn(&world.root, &sheet);
    // pick: the header of column D, its label on top
    let (x, y) = (GUTTER + 3 * CELL_W + 20, TOOLBAR + 10);
    let stack = r.stack(&world.root, &world.shown, x, y);
    let head = stack.iter().find(|i| i.kind() == "fill").unwrap();
    assert!(head.info.hears, "the header fill has a pointer handler");
    assert_eq!(head.location().line(), line_of("s.row(", 0));
    // theme: who reads the word, now
    let old = world.shown;
    let deps: Vec<_> = r.items(&world.root, &world.shown).into_iter().filter(|i| i.info.reads & (1 << Theme::SLOT) != 0).map(|i| (i.ent, i.screen.and(i.clip), i.get("color").map(|o| o.val))).collect();
    assert!(deps.len() > 500);
    let quiet: Vec<_> = r.items(&world.root, &world.shown).into_iter().filter(|i| i.info.reads & (1 << Theme::SLOT) == 0).map(|i| (i.ent, i.ops().map(|o| o.val).collect::<Vec<_>>())).collect();
    // write the word, as an inspector would: a plain store into the block
    sheet.u.set::<Theme>(1);
    world.settle(&sheet);
    let new = world.shown;
    let items = r.items(&world.root, &world.shown);
    let mut changed = 0;
    for (e, rect, c0) in &deps {
        let it = items.iter().find(|i| i.ent == *e).unwrap();
        if it.get("color").map(|o| o.val) != *c0 {
            changed += 1;
            assert!(it.changed(&old, &new), "the core's derivative agrees with the values read");
            // every tile under it is damaged
            let mut hit = false;
            world.tiles.each(|t| hit |= t.hits(*rect));
            assert!(hit || rect.empty(), "{e:?} changed colour but its tiles were not damaged");
        }
    }
    assert!(changed > 500, "{changed} records changed colour");
    for (e, vals) in &quiet {
        let it = items.iter().find(|i| i.ent == *e).unwrap();
        assert_eq!(&it.ops().map(|o| o.val).collect::<Vec<_>>(), vals, "a record that does not read the theme kept its values");
    }
}

#[test]
fn a_moving_record_says_where_its_paint_puts_it() {
    let (mut sheet, mut world) = mounted(100);
    sheet.select(3 * COLS as u32 + 2, world.now());
    world.settle(&sheet);
    world.tick(1_000); // the glide is over
    let r = Reflect::learn(&world.root, &sheet);
    let u = world.shown;
    let o = r.items(&world.root, &u).into_iter().find(|i| i.kind() == "outline").unwrap();
    let s = o.snap();
    assert_eq!(s.off, (2 * CELL_W, 3 * CELL_H), "the offset of cell (3, C) from cell (0, A)");
    assert_eq!(o.screen, s.rect.shift((0, BODY.y0)).shift(s.off));
}
