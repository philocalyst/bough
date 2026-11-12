//! Record-space paint damage (round 7, `dirty.rs`): the projection of the paint plane marks exactly the tiles
//! that marking each record's visible ink at the write would have marked, wherever structural edits and
//! translations put the records in between, and skips records released since their mark.
use bough_core::damage::DamageRing;
use bough_core::rng::Rng;
use bough_core::{Bough, Handle, NRect, RecordInit};

const W: i32 = 1280;
const H: i32 = 800;

fn build(rng: &mut Rng, n: usize) -> (Bough, Vec<Handle>) {
    let mut b = Bough::new(W, H);
    let clips = [0u16, b.new_clip(NRect::new(100, 100, 700, 500)), b.new_clip(NRect::new(0, 0, 40, 40))];
    let mut hs = Vec::new();
    let mut recs = Vec::new();
    for i in 0..n {
        let h = b.new_handle();
        hs.push(h);
        let (x, y) = ((rng.below(1500) as i32) - 100, (i as i32 * 7) % 1200 - 100);
        let ink = if i % 37 == 5 { NRect::EMPTY } else { NRect::new(x, y, x + 1 + rng.below(300) as i32, y + 1 + rng.below(60) as i32) };
        recs.push(RecordInit { depth: 1, ink, kind: 1, payload: i as u32, handle: h.idx, clip: clips[i % 3] });
    }
    b.insert_run(0, &recs);
    (b, hs)
}

/// What a write-time mark does (the façade's old `paint_write`).
fn mark_now(b: &Bough, ring: &mut DamageRing, h: Handle) {
    if let Some(ink) = b.visible_ink_handle(h) {
        let ink = ink.intersect(b.window()).canon();
        if !ink.is_empty() {
            ring.add_inside(ink);
        }
    }
}

#[test]
fn projection_equals_marks_at_the_final_place() {
    let mut rng = Rng::new(0xD1F7);
    for round in 0..40 {
        let (mut b, hs) = build(&mut rng, 50 + round * 13);
        let mut dirty = Vec::new();
        for _ in 0..1 + rng.below(60) {
            let h = hs[rng.below(hs.len() as u64) as usize];
            b.mark_paint(h.idx);
            dirty.push(h);
        }
        // between the writes and the frame: a translation, a removal (some marked records die), an insert
        // that moves records within and across pages
        let n = hs.len();
        b.translate_run(n / 4, n / 2, 13, -29);
        b.remove_run(n / 3, 5);
        let extra: Vec<RecordInit> = (0..9).map(|k| RecordInit::new(1, NRect::new(k, k, k + 5, k + 5), 1, 0)).collect();
        b.insert_run(2, &extra);
        assert!(b.paint_pending());
        let (mut got, mut want) = (DamageRing::new(W, H), DamageRing::new(W, H));
        for &h in &dirty {
            mark_now(&b, &mut want, h);
        }
        let any = b.project_paint_onto(got.cur_mut());
        assert!(!b.paint_pending());
        assert_eq!(got.plane(0).rows, want.plane(0).rows, "round {round}");
        assert_eq!(any, !want.plane(0).is_clear());
        // the plane is empty afterwards: a second projection marks nothing
        let mut again = DamageRing::new(W, H);
        assert!(!b.project_paint_onto(again.cur_mut()));
        assert!(again.plane(0).is_clear());
    }
}

/// The negative control: a projection that ignored the page frame (the mutant "ink without `off`") differs
/// from the reference as soon as a marked record has been translated.
#[test]
fn a_translated_record_is_projected_where_it_is() {
    let mut b = Bough::new(W, H);
    let h = b.new_handle();
    b.insert_run(0, &[RecordInit { depth: 1, ink: NRect::new(10, 10, 20, 20), kind: 1, payload: 0, handle: h.idx, clip: 0 }]);
    b.mark_paint(h.idx);
    b.translate_run(0, 1, 640, 320);
    let mut got = DamageRing::new(W, H);
    b.project_paint_onto(got.cur_mut());
    assert!(got.plane(0).tile(20, 10), "the tile of the record's new place");
    assert!(!got.plane(0).tile(0, 0), "not the tile of its old place");
    // the Bough's own ring takes the same marks
    b.mark_paint(h.idx);
    b.next_epoch();
    assert!(b.plane(0).is_clear());
    assert!(b.project_paint());
    assert!(b.plane(0).tile(20, 10));
}

/// A page's seal moves when one of its records is written (a column, or a projected paint write) and not
/// when another page's is.
#[test]
fn a_page_seal_moves_with_its_page_only() {
    let mut rng = Rng::new(3);
    let (mut b, hs) = build(&mut rng, 300);
    let seals = |b: &Bough| {
        let mut v = Vec::new();
        b.page_seals(&mut v);
        v
    };
    let s0 = seals(&b);
    assert!(s0.len() >= 4, "several pages");
    assert_eq!(s0.iter().map(|p| p.3).sum::<u32>(), 300);
    // a paint write to a record of the last page: nothing moves until it is projected, then that page only
    let h = *hs.last().unwrap();
    b.mark_paint(h.idx);
    assert_eq!(seals(&b), s0);
    b.project_paint();
    let s1 = seals(&b);
    let changed: Vec<usize> = (0..s0.len()).filter(|&i| s0[i].1 != s1[i].1).collect();
    assert_eq!(changed, vec![s0.len() - 1]);
    assert_eq!(b.page_seal_at(299), (s1[s0.len() - 1].0, s1[s0.len() - 1].1));
    // an ink write through a handle of the first page
    b.set_ink_handle(hs[1], NRect::new(1, 1, 9, 9));
    let s2 = seals(&b);
    let changed: Vec<usize> = (0..s1.len()).filter(|&i| s1[i].1 != s2[i].1).collect();
    assert_eq!(changed, vec![0]);
    // text content: the façade's touch
    b.seal_touch(hs[70]);
    let s3 = seals(&b);
    assert_eq!((0..s2.len()).filter(|&i| s2[i].1 != s3[i].1).count(), 1);
}

/// The projection's two shortcuts are exact: a page out of the window marks nothing (as its records one by
/// one would), and once every tile of the window is marked the rest of the plane is cleared without a look.
#[test]
fn projection_of_everything_and_of_offscreen_pages() {
    let mut b = Bough::new(W, H);
    let mut hs = Vec::new();
    let mut recs = Vec::new();
    // 4,000 rows of 32 px: most pages are far below the window
    for i in 0..4000 {
        let h = b.new_handle();
        hs.push(h);
        recs.push(RecordInit { depth: 1, ink: NRect::new(0, (i * 32).min(30000), W, (i * 32 + 32).min(30032)), kind: 1, payload: i as u32, handle: h.idx, clip: 0 });
    }
    b.insert_run(0, &recs);
    // off-screen only
    for h in &hs[200..] {
        b.mark_paint(h.idx);
    }
    let mut got = DamageRing::new(W, H);
    assert!(!b.project_paint_onto(got.cur_mut()));
    assert!(got.plane(0).is_clear() && !b.paint_pending());
    // everything: the window ends up wholly marked, and equal to the one-by-one marks
    let (mut got, mut want) = (DamageRing::new(W, H), DamageRing::new(W, H));
    for h in &hs {
        b.mark_paint(h.idx);
        mark_now(&b, &mut want, *h);
    }
    assert!(b.project_paint_onto(got.cur_mut()));
    assert_eq!(got.plane(0).rows, want.plane(0).rows);
    assert!(got.plane(0).covers(NRect::new(0, 0, W, H), 25));
    assert!(!b.paint_pending());
}
