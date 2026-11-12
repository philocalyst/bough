//! The mapped scene image (threads lane): a scene saved by one world is another world's scene by mapping the
//! file. Checked on the sheet:
//!
//! * the mapped scene paints the pixels the saved one did, before the view has run at all;
//! * adopting it with the same state writes nothing and damages nothing, and the script goes on as if mounted;
//! * adopting it with another state (edits, theme, scroll, rows inserted) repairs it: retained == fresh mount;
//! * a view of another shape is refused;
//! * an image with flipped bits or cut short is refused or repaired, and never takes a pass out of bounds.
#[path = "../scenes/sheet.rs"]
mod sheet;
use bough_x::arena::Refused;
use bough_x::prelude::*;
use bough_x::soft::Soft;
use sheet::*;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bough-x-image-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}
fn pixels(w: &World<Soft>) -> Vec<u32> {
    let mut s = Soft::new(W, H);
    w.paint(&mut s);
    s.px
}
fn fresh(sheet: &Sheet, now: u32) -> Vec<u32> {
    let mut w = World::<Soft>::new(W, H, NWORDS);
    w.tick(now);
    w.mount(sheet);
    pixels(&w)
}
/// A sheet a few interactions in, and its world.
fn lived_in(rows: usize) -> (Sheet, World<Soft>) {
    let mut sheet = Sheet::new(rows);
    let mut world = World::<Soft>::new(W, H, NWORDS);
    world.mount(&sheet);
    sheet.scroll_by(7 * CELL_H + 13);
    sheet.edit(9, 3, 123.45);
    world.pointer(&mut sheet, GUTTER + 2 * CELL_W + 50, TOOLBAR + 10);
    world.settle(&sheet);
    world.tiles.clear();
    (sheet, world)
}

#[test]
fn a_mapped_scene_is_the_saved_scene() {
    let (mut sheet, mut world) = lived_in(10_000);
    let path = tmp("same.bx");
    let bytes = world.save(&path).unwrap();
    let want = pixels(&world);
    let (recs, _, _) = world.stats();
    drop(world);

    let mut w2 = World::<Soft>::open(&path).unwrap();
    // before the view has run: the published half and the block it was shaded with
    let mut s = Soft::new(W, H);
    w2.paint_front(&w2.shown.clone(), &mut s);
    assert!(s.px == want, "the mapped scene does not paint what was saved");

    w2.adopt(&sheet, NWORDS).unwrap();
    assert!(w2.tiles.count() == 0, "adopting the same state damaged {} tiles", w2.tiles.count());
    assert!(w2.ar.head().sum.iter().all(|&s| s == 0), "adopting the same state wrote a page");
    assert!(w2.stats().0 == recs);
    println!("image: {bytes} bytes for {recs} records");

    // and it lives on as a mounted world does
    let mut retained = Soft::new(W, H);
    retained.px.copy_from_slice(&want);
    for k in 0..40 {
        match k % 5 {
            0 => sheet.scroll_by(3 * CELL_H),
            1 => sheet.toggle_theme(),
            2 => sheet.edit(20 + k, k % COLS, k as f32),
            3 => sheet.insert_row(12, [k as f32; COLS], false),
            _ => {
                w2.pointer(&mut sheet, GUTTER + (k as i32 % 10) * CELL_W + 50, TOOLBAR + 10);
            }
        }
        w2.settle(&sheet);
        retained.repair(&w2.tiles, |s| w2.paint(s));
        w2.tiles.clear();
        assert!(retained.px == pixels(&w2), "step {k}: retained != full after adoption");
        assert!(retained.px == fresh(&sheet, w2.now()), "step {k}: adopted world != fresh mount");
    }
}

#[test]
fn a_stale_image_is_repaired() {
    let (mut sheet, mut world) = lived_in(10_000);
    let path = tmp("stale.bx");
    world.save(&path).unwrap();
    let old = pixels(&world);
    drop(world);
    // the application's state moved on between the runs
    sheet.toggle_theme();
    sheet.scroll_by(40 * CELL_H);
    sheet.edit(50, 2, 999.0);
    sheet.insert_row(49, [1.0; COLS], false);
    let mut w2 = World::<Soft>::open(&path).unwrap();
    w2.adopt(&sheet, NWORDS).unwrap();
    let mut retained = Soft::new(W, H);
    retained.px.copy_from_slice(&old);
    assert!(w2.tiles.count() != 0);
    retained.repair(&w2.tiles, |s| w2.paint(s));
    assert!(retained.px == fresh(&sheet, w2.now()), "a stale image was not repaired by its damage");
}

#[test]
fn another_shape_is_refused() {
    let (_, mut world) = lived_in(10_000);
    let path = tmp("shape.bx");
    world.save(&path).unwrap();
    drop(world);
    // a sheet too short to fill the window has other columns sizes in its list: not this image's shape
    let short = Sheet::new(0);
    let mut w2 = World::<Soft>::open(&path).unwrap();
    assert!(w2.adopt(&short, NWORDS).is_err(), "a view with no rows adopted the image of one with rows");
}

#[test]
fn corruption_is_refused_or_repaired() {
    let (sheet, mut world) = lived_in(10_000);
    let path = tmp("good.bx");
    world.save(&path).unwrap();
    drop(world);
    let good = std::fs::read(&path).unwrap();
    let want = fresh(&sheet, 0);
    let mut seed = 0x1234_5678_9abc_def1u64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let (mut refused, mut repaired, mut clean) = (0, 0, 0);
    let bad = tmp("bad.bx");
    let trials: usize = std::env::var("TRIALS").ok().and_then(|t| t.parse().ok()).unwrap_or(300);
    for t in 0..trials {
        let mut b = good.clone();
        match t % 4 {
            // truncation
            0 => b.truncate(rnd() as usize % b.len()),
            // one bit anywhere
            1 => {
                let i = rnd() as usize % b.len();
                b[i] ^= 1 << (rnd() % 8);
            }
            // a burst in the structural part: the head, the plan, the ranges
            2 => {
                let i = rnd() as usize % (400 << 10).min(b.len());
                for k in 0..16.min(b.len() - i) {
                    b[i + k] = rnd() as u8;
                }
            }
            // a burst in the columns: record words and rects
            _ => {
                let i = (360 << 10) + rnd() as usize % (b.len() - (360 << 10) - 64);
                for k in 0..64 {
                    b[i + k] = rnd() as u8;
                }
            }
        }
        std::fs::write(&bad, &b).unwrap();
        match World::<Soft>::open(&bad) {
            Err(_) => refused += 1,
            Ok(mut w) => {
                // whatever it holds, painting it stays in bounds
                let mut s = Soft::new(W, H);
                w.paint_front(&w.shown.clone(), &mut s);
                match w.adopt(&sheet, NWORDS) {
                    Err(_) => refused += 1,
                    Ok(()) => {
                        if w.tiles.count() == 0 { clean += 1 } else { repaired += 1 }
                        assert!(pixels(&w) == want, "trial {t}: an adopted image does not show the view");
                    }
                }
            }
        }
    }
    println!("corruption: {trials} trials, {refused} refused, {repaired} repaired, {clean} untouched where it matters");
    assert!(refused > 0 && repaired + clean > 0);
    // a foreign binary's image: the head says so
    let mut b = good.clone();
    b[16] ^= 0xff;
    std::fs::write(&bad, &b).unwrap();
    assert!(matches!(World::<Soft>::open(&bad), Err(Refused::Binary)));
}
