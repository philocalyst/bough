//! The world's flat column table against the walk of the store tree it replaces: for random block changes the
//! two leave the same damage and the same sweeps.
#[path = "../scenes/sheet.rs"]
mod sheet;
use bough_x::prelude::*;
use bough_x::soft::Soft;
use bough_x::store::Tiles;
use sheet::*;

#[test]
fn table_walk_equals_tree_walk() {
    let mut sheet = Sheet::new(2_000);
    let mut world = World::<Soft>::new(W, H, NWORDS);
    world.mount(&sheet);
    world.tiles.clear();
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut rnd = |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    for step in 0..300u32 {
        world.tick(step * 20);
        match rnd(5) {
            0 => sheet.toggle_theme(),
            1 => sheet.hover_header(rnd(COLS as u64) as u32, rnd(2) == 0),
            2 => sheet.select(rnd(300) as u32, world.now()),
            3 => sheet.scroll_by(rnd(900) as i32 - 300),
            _ => {
                let c = rnd(COLS as u64) as usize;
                sheet.edit(rnd(40) as usize, c, rnd(100_000) as f32 / 100.0)
            }
        }
        // the reference: the tree walk, from the same state, into its own damage
        let mut new = sheet.u;
        new.w[56..].copy_from_slice(&world.shown.w[56..]);
        let ch = world.shown.diff(&new, NWORDS);
        let mut want = Tiles::new(W, H);
        want.sweeps = world.tiles.sweeps.clone();
        world.root.delta(&world.shown, &new, ch, &mut want);
        // rows the list pass will damage are not part of this comparison: take the block's damage alone
        let had_lists = ch & (1 << Scroll::SLOT) != 0;
        world.settle(&sheet);
        if !had_lists && rnd(1) == 0 {
            // an edit also runs the list pass; its marks are a superset
            for (a, b) in want.bits.iter().zip(&world.tiles.bits) {
                assert!(a & !b == 0, "step {step}: the table missed damage the tree walk found");
            }
        }
        let key = |t: &Tiles| {
            let mut v: Vec<_> = t.sweeps.iter().map(|s| (s.col, s.until, s.rect)).collect();
            v.sort_by_key(|s| s.0);
            v
        };
        if !had_lists {
            assert_eq!(key(&want), key(&world.tiles), "step {step}: sweeps differ");
        }
        world.tiles.clear();
    }
}
