//! R6 diagnostic. No production hooks. Setup, oracle and destruction stay outside counters.
use bough_core::bough::RecordInit;
use bough_core::oracle::{NR, Oracle};
use bough_core::{Bough, NRect, Plane};
use bough_damage_lab::measure;
use std::hint::black_box;

const SIDE: i32 = 3200;
const SAMPLES: usize = 5;

fn records(n: usize, interleaved: bool, chains: bool) -> Vec<RecordInit> {
    (0..n)
        .map(|i| {
            let tile = if interleaved {
                i.saturating_sub(1) * 9973 % 10000
            } else {
                i.saturating_sub(1) * 10000 / (n - 1)
            };
            let (x, y) = ((tile % 100) as i32 * 32, (tile / 100) as i32 * 32);
            RecordInit::new(
                if i == 0 {
                    0
                } else if chains {
                    (1 + (i - 1) % 32) as u16
                } else {
                    1
                },
                if i == 0 {
                    NRect::EMPTY
                } else {
                    NRect::new(x + 2, y + 2, x + 14, y + 14)
                },
                1,
                i as u32,
            )
        })
        .collect()
}

fn build(rs: &[RecordInit]) -> Bough {
    let mut b = Bough::new(SIDE, SIDE);
    b.insert_run(0, rs);
    if warm() {
        // A bulk build leaves the directory's columns at exact capacity, so the first split of a fresh
        // tree reallocates all five (O(pages) of copy and of kernel page faults, once per doubling).
        // That is an amortised allocation cost, not a per-edit cost: `R6_WARM=1` takes it here with a
        // split and its inverse at the tail (the page structure and the records come back unchanged).
        let (n, starts) = (b.len(), b.page_starts());
        let pad = [RecordInit::new(1, NRect::EMPTY, 1, 0); 64];
        b.insert_run(n, &pad);
        b.remove_run(n, 64);
        assert_eq!(
            b.page_starts(),
            starts,
            "warm-up changed the page structure"
        );
    }
    b.next_epoch();
    b
}

fn warm() -> bool {
    std::env::var("R6_WARM").is_ok_and(|s| s == "1")
}

fn oracle(rs: &[RecordInit]) -> Oracle {
    let mut o = Oracle::new(SIDE, SIDE);
    o.insert_run(0, rs);
    o.next_epoch();
    o
}

/// Read every page header once, outside the interval. `ri_instructions` includes kernel work, and
/// on a loaded machine the kernel ages pages within milliseconds: an edit that then walks 15,000
/// page headers takes a soft fault on each 16 KiB page it re-touches (a front insert at 1e6 read
/// 563 k / 658 k / 828 k instructions with 1 / 2 / 4 such trees alive). Those are not core costs.
fn settle(b: &Bough) {
    black_box(b.page_versions());
}

fn want(name: &str) -> bool {
    std::env::var("R6_FILTER")
        .ok()
        .is_none_or(|s| name.contains(&s))
}

#[inline(never)]
fn timed(mut f: impl FnMut(), count: usize, overhead: u64) -> u64 {
    let a = measure::counters().0;
    for _ in 0..count {
        f();
    }
    let z = measure::counters().0;
    (z - a).saturating_sub(overhead)
}

fn emit(
    n: usize,
    shape: &str,
    op: &str,
    k: usize,
    pages: usize,
    batch: usize,
    samples: &[u64],
    output: usize,
) {
    for (sample, &instructions) in samples.iter().enumerate() {
        println!(
            "{n},{shape},{op},{k},{pages},{batch},{sample},{instructions},{:.3},{output}",
            instructions as f64 / batch as f64
        );
    }
}

#[derive(Clone, Copy)]
enum Edit {
    Insert(usize, usize),
    Remove(usize, usize),
    Move(usize, usize, usize),
    Translate(usize, usize),
}

fn apply(b: &mut Bough, edit: Edit, new: &[RecordInit]) {
    match edit {
        Edit::Insert(at, k) => b.insert_run(at, &new[..k]),
        Edit::Remove(at, k) => b.remove_run(at, k),
        Edit::Move(at, k, to) => b.move_run(at, k, to),
        Edit::Translate(at, k) => b.translate_run(at, k, 1, 1),
    }
}

fn edit_cell(rs: &[RecordInit], name: &str, edit: Edit, k: usize, ov: u64) {
    if !want(name) {
        return;
    }
    let n = rs.len();
    let new: Vec<_> = (0..64)
        .map(|i| RecordInit::new(1, NRect::new(2, 2, 14, 14), 1, (n + i) as u32))
        .collect();
    let mut values = Vec::new();
    let mut pages = 0;
    for _ in 0..SAMPLES {
        let mut b = build(rs);
        pages = b.num_pages();
        let starts = b.page_starts();
        let mut o = oracle(rs);
        settle(&b);
        values.push(timed(
            || apply(black_box(&mut b), edit, black_box(&new)),
            1,
            ov,
        ));
        match edit {
            Edit::Insert(at, k) => o.insert_run(at, &new[..k]),
            Edit::Remove(at, k) => o.remove_run(at, k, &starts),
            Edit::Move(at, k, to) => o.move_run(at, k, to, 0, &starts),
            Edit::Translate(at, k) => o.translate_run(at, k, 1, 1),
        }
        assert_eq!(b.records(), o.recs, "{name} records");
        let bp = b.repair(1);
        let op = o.repair(1);
        for y in 0..100 {
            assert_eq!(bp.rows[y], op.row(y), "{name} damage row {y}");
        }
        b.check_invariants();
    }
    emit(n, "ordered-wide", name, k, pages, 1, &values, 0);
}

fn builds(rs: &[RecordInit], ov: u64) {
    let n = rs.len();
    for (name, chunk) in [("bulk-build", n), ("build-append8", 8)] {
        if !want(name) {
            continue;
        }
        let mut values = Vec::new();
        let mut pages = 0;
        for _ in 0..SAMPLES {
            let mut b = None;
            values.push(timed(
                || {
                    let mut dst = Bough::new(SIDE, SIDE);
                    for r in black_box(rs).chunks(chunk) {
                        dst.insert_run(dst.len(), r);
                    }
                    b = Some(dst);
                },
                1,
                ov,
            ));
            let b = b.unwrap();
            pages = b.num_pages();
            assert_eq!(b.records(), oracle(rs).recs);
            b.check_invariants();
        }
        emit(n, "ordered-wide", name, n, pages, 1, &values, n);
    }
}

fn freezes(rs: &[RecordInit], ov: u64) {
    for name in ["freeze-cold", "freeze-idle", "freeze-one", "freeze-all"] {
        if !want(name) {
            continue;
        }
        let mut values = Vec::new();
        let mut pages = 0;
        let mut copied = 0;
        for _ in 0..SAMPLES {
            let mut b = build(rs);
            pages = b.num_pages();
            let mut dst = Bough::new(SIDE, SIDE);
            if name != "freeze-cold" {
                b.freeze_into(&mut dst);
            }
            if name == "freeze-one" {
                b.set_ink(1, NRect::new(3, 3, 15, 15));
            }
            if name == "freeze-all" {
                b.translate_run(1, b.len() - 1, 1, 1);
            }
            settle(&b);
            settle(&dst);
            values.push(timed(
                || {
                    copied = black_box(&b).freeze_into(black_box(&mut dst));
                },
                1,
                ov,
            ));
            assert_eq!(dst.records(), b.records());
        }
        emit(rs.len(), "ordered-wide", name, 0, pages, 1, &values, copied);
    }
}

fn queries(rs: &[RecordInit], chains: bool, ov: u64) {
    let b = build(rs);
    let o = oracle(rs);
    let n = rs.len();
    let mut out = Vec::with_capacity(64);
    let pos = n - 1;
    assert_eq!(b.parent(pos), o.parent(pos));
    assert_eq!(b.subtree_end(0), o.subtree_end(0));
    assert_eq!(b.subtree_end(pos), o.subtree_end(pos));
    b.ancestors(pos, &mut out);
    let mut expected = Vec::new();
    let mut p = o.parent(pos);
    while let Some(i) = p {
        expected.push(i);
        p = o.parent(i);
    }
    assert_eq!(out, expected);
    for name in [
        "parent-last",
        "ancestors-last",
        "subtree-root",
        "subtree-leaf",
    ] {
        if !want(name) {
            continue;
        }
        let mut values = Vec::new();
        let count = 1024;
        for _ in 0..SAMPLES {
            let sample = match name {
                "parent-last" => timed(
                    || {
                        black_box(b.parent(black_box(pos)));
                    },
                    count,
                    ov,
                ),
                "ancestors-last" => timed(
                    || {
                        b.ancestors(black_box(pos), black_box(&mut out));
                    },
                    count,
                    ov,
                ),
                "subtree-root" => timed(
                    || {
                        black_box(b.subtree_end(black_box(0)));
                    },
                    count,
                    ov,
                ),
                _ => timed(
                    || {
                        black_box(b.subtree_end(black_box(pos)));
                    },
                    count,
                    ov,
                ),
            };
            values.push(sample);
        }
        emit(
            n,
            if chains {
                "ordered-chains32"
            } else {
                "ordered-wide"
            },
            name,
            0,
            b.num_pages(),
            count,
            &values,
            expected.len(),
        );
    }
}

fn spatial(rs: &[RecordInit], interleaved: bool, ov: u64) {
    let mut b = build(rs);
    let mut o = oracle(rs);
    let n = rs.len();
    let shape = if interleaved {
        "interleaved-wide"
    } else {
        "ordered-wide"
    };
    let mut out = Vec::with_capacity(n + 8);
    let batch = (1_000_000 / n).clamp(4, 1024);
    for percent in [1, 10, 100] {
        for dispersed in [false, true] {
            let name = format!(
                "cull-{percent}-{}",
                if dispersed { "dispersed" } else { "band" }
            );
            if !want(&name) {
                continue;
            }
            b.next_epoch();
            o.next_epoch();
            for t in 0..percent * 100 {
                let tile = if dispersed { t * 9973 % 10000 } else { t };
                let (x, y) = ((tile % 100) * 32, (tile / 100) * 32);
                let r = NRect::new(x, y, x + 32, y + 32);
                b.damage_rect(r);
                o.add_damage(NR::from_nrect(r));
            }
            let mut plane = Plane::EMPTY;
            let bbox = b.repair_into(1, &mut plane);
            b.cull_tiles(&plane, bbox, &mut out);
            assert_eq!(out, o.cull_tiles(&o.repair(1)), "{shape} {name}");
            let len = out.len();
            let values: Vec<_> = (0..SAMPLES)
                .map(|_| {
                    timed(
                        || {
                            b.cull_tiles(black_box(&plane), black_box(bbox), black_box(&mut out));
                            black_box(out.len());
                        },
                        batch,
                        ov,
                    )
                })
                .collect();
            emit(
                n,
                shape,
                &name,
                percent as usize,
                b.num_pages(),
                batch,
                &values,
                len,
            );
        }
    }
    for (name, x, y) in [("hit-present", 3, 3), ("hit-gap", 16, 16)] {
        if !want(name) {
            continue;
        }
        assert_eq!(b.hit(x, y), o.hit(x, y));
        let values: Vec<_> = (0..SAMPLES)
            .map(|_| {
                timed(
                    || {
                        black_box(b.hit(black_box(x), black_box(y)));
                    },
                    1024,
                    ov,
                )
            })
            .collect();
        emit(
            n,
            shape,
            name,
            0,
            b.num_pages(),
            1024,
            &values,
            usize::from(b.hit(x, y).is_some()),
        );
    }
}

/// Handle- and position-addressed writes and lookups: the paths a directory change can tax (they
/// read a page's directory index and update its page and chunk summaries), and what a structural
/// edit leaves for them to pay (`insert8-front+pos*`: the edit, then `pos_of` through a handle on 64
/// pages / on every page).
fn handles(rs: &[RecordInit], ov: u64) {
    let n = rs.len();
    let fresh = |every_page: bool| {
        let mut b = build(rs);
        let at: Vec<usize> = if every_page {
            b.page_starts().into_iter().map(|s| s.max(1)).collect()
        } else {
            (0..64).map(|i| 1 + i * (n - 2) / 63).collect()
        };
        let hs: Vec<_> = at.iter().map(|&p| b.attach_handle(p)).collect();
        (b, at, hs)
    };
    let (mut b, at, hs) = fresh(false);
    let pages = b.num_pages();
    let mid = 32;
    let (h, pos) = (hs[mid], at[mid]);
    let ink0 = b.ink_at(pos);
    let [x0, y0, x1, y1] = ink0.to_xyxy();
    let inks = [NRect::new(x0 + 1, y0 + 1, x1 - 1, y1 - 1), ink0];
    let count = 1024;
    let mut i = 0usize;
    let batch: Vec<_> = hs
        .iter()
        .map(|&h| (h, NRect::new(40, 40, 52, 52)))
        .collect();
    let batch0: Vec<_> = hs
        .iter()
        .zip(&at)
        .map(|(&h, &p)| (h, b.ink_at(p)))
        .collect();
    for name in [
        "set-ink-pos",
        "set-ink-handle",
        "pos-of-handle",
        "touch-handle",
        "set-inks-handles64",
    ] {
        if !want(name) {
            continue;
        }
        let mut values = Vec::new();
        for _ in 0..SAMPLES {
            b.next_epoch();
            values.push(match name {
                "set-ink-pos" => timed(
                    || {
                        b.set_ink(black_box(pos), inks[i & 1]);
                        i += 1;
                    },
                    count,
                    ov,
                ),
                "set-ink-handle" => timed(
                    || {
                        black_box(b.set_ink_handle(black_box(h), inks[i & 1]));
                        i += 1;
                    },
                    count,
                    ov,
                ),
                "pos-of-handle" => timed(
                    || {
                        black_box(b.pos_of(black_box(h)));
                    },
                    count,
                    ov,
                ),
                "touch-handle" => timed(
                    || {
                        black_box(b.touch_handle(black_box(h)));
                    },
                    count,
                    ov,
                ),
                _ => timed(
                    || {
                        b.set_inks_handles(
                            black_box(if i & 1 == 0 { &batch } else { &batch0 }),
                            true,
                        );
                        i += 1;
                    },
                    count,
                    ov,
                ),
            });
        }
        assert_eq!(b.pos_of(h), Some(pos));
        assert_eq!(b.ink_at(pos), ink0, "{name} left the ink changed");
        b.check_invariants();
        emit(n, "ordered-wide", name, 0, pages, count, &values, 0);
    }
    // Record state planes: what a flag flip costs (the signal path): one record through its handle,
    // the consumer's turn after it (flips + commit), a range write of half the tree, and the summary
    // query over the whole tree with nothing set and with one record set.
    for name in [
        "state-set-handle",
        "state-flip-turn",
        "state-run-half",
        "state-any-none",
        "state-any-one",
    ] {
        if !want(name) {
            continue;
        }
        let mut values = Vec::new();
        let mut hits = 0usize;
        for _ in 0..SAMPLES {
            b.set_state_run(0, n, 5, false);
            b.commit_state(5);
            if name == "state-any-one" {
                b.set_state(n - 1, 5, true);
            }
            values.push(match name {
                "state-set-handle" => timed(
                    || {
                        black_box(b.set_state_handle(black_box(h), 5, i & 1 == 0));
                        i += 1;
                    },
                    count,
                    ov,
                ),
                "state-flip-turn" => timed(
                    || {
                        b.set_state_handle(black_box(h), 5, i & 1 == 0);
                        i += 1;
                        b.flips(5, |pos, now| hits += pos + now as usize);
                        b.commit_state(5);
                    },
                    count,
                    ov,
                ),
                "state-run-half" => timed(
                    || {
                        b.set_state_run(black_box(n / 4), n / 2, 5, i & 1 == 0);
                        i += 1;
                    },
                    count,
                    ov,
                ),
                _ => timed(
                    || {
                        hits += black_box(b.any_state(black_box(0), n, 5)) as usize;
                    },
                    count,
                    ov,
                ),
            });
        }
        black_box(hits);
        b.set_state_run(0, n, 5, false);
        b.commit_state(5);
        b.check_invariants();
        emit(n, "ordered-wide", name, 0, pages, count, &values, 0);
    }
    let new: Vec<_> = (0..8)
        .map(|i| RecordInit::new(1, NRect::new(2, 2, 14, 14), 1, (n + i) as u32))
        .collect();
    for (name, every) in [
        ("insert8-front+pos64", false),
        ("insert8-front+posall", true),
    ] {
        if !want(name) {
            continue;
        }
        let mut values = Vec::new();
        let mut nh = 0;
        for _ in 0..SAMPLES {
            let (mut b, at, hs) = fresh(every);
            nh = hs.len();
            let mut got = vec![None; nh];
            settle(&b);
            values.push(timed(
                || {
                    b.insert_run(black_box(1), black_box(&new));
                    for (g, &h) in got.iter_mut().zip(&hs) {
                        *g = b.pos_of(h);
                    }
                },
                1,
                ov,
            ));
            for (g, &p) in got.iter().zip(&at) {
                assert_eq!(*g, Some(p + if p >= 1 { 8 } else { 0 }), "{name}");
            }
            b.check_invariants();
        }
        emit(n, "ordered-wide", name, 8, pages, 1, &values, nh);
    }
}

/// Page-splitting inserts at one place, the directory's adversarial case: 256 inserts of 64 records
/// at the front, each timed. `insert64-front-worst` is the dearest of them (a rebalance of the widest
/// window it needed, or the directory growing), `insert64-front-amortised` their mean.
fn overflow(rs: &[RecordInit], ov: u64) {
    if !want("insert64-front-worst") && !want("insert64-front-amortised") {
        return;
    }
    let n = rs.len();
    let new: Vec<_> = (0..64)
        .map(|i| RecordInit::new(1, NRect::new(2, 2, 14, 14), 1, (n + i) as u32))
        .collect();
    let (mut worst, mut mean) = (Vec::new(), Vec::new());
    let mut pages = 0;
    let count = 256;
    for _ in 0..SAMPLES {
        let mut b = build(rs);
        pages = b.num_pages();
        settle(&b);
        let (mut total, mut max) = (0u64, 0u64);
        for _ in 0..count {
            let one = timed(|| b.insert_run(black_box(1), black_box(&new)), 1, ov);
            total += one;
            max = max.max(one);
        }
        b.check_invariants();
        worst.push(max);
        mean.push(total / count as u64);
    }
    emit(
        n,
        "ordered-wide",
        "insert64-front-worst",
        64,
        pages,
        1,
        &worst,
        count,
    );
    emit(
        n,
        "ordered-wide",
        "insert64-front-amortised",
        64,
        pages,
        1,
        &mean,
        count,
    );
}

/// Content beyond the 16-bit pixel domain (the store lane's attack on the exact-coordinate work): rows
/// of 32 px, five records each, mounted blank and then given their inks through handles, as a layout
/// write-back does. `writeback-row` is the whole write-back per row; `far-set-ink` moves the last row's
/// first record by a pixel and back; `far-scroll` scrolls the whole column by 3 px. A far row must cost
/// what a near one does: rows from 1,024 on are outside the domain.
pub fn far(ov: u64) {
    use bough_core::WRect;
    for rows in [1000usize, 3000, 10_000, 100_000] {
        if std::env::var("R6_MAX_N")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .is_some_and(|max| rows > max)
        {
            continue;
        }
        let rect = |row: usize, k: usize| {
            WRect::new(
                8 + 40 * k as i32,
                32 * row as i32,
                44 + 40 * k as i32,
                32 * row as i32 + 30,
            )
        };
        let n = 1 + rows * 5;
        let (mut wb, mut si, mut sc) = (Vec::new(), Vec::new(), Vec::new());
        let mut pages = 0;
        for _ in 0..SAMPLES {
            let mut b = Bough::new(SIDE, SIDE);
            b.insert_run(0, &[RecordInit::new(0, NRect::EMPTY, 1, 0)]);
            let mut recs = Vec::with_capacity(n);
            let mut hs = Vec::with_capacity(n);
            for i in 1..n {
                let h = b.new_handle();
                hs.push(h);
                recs.push(RecordInit {
                    handle: h.idx,
                    ..RecordInit::new(1, NRect::EMPTY, 1, i as u32)
                });
            }
            b.insert_run(1, &recs);
            b.next_epoch();
            settle(&b);
            wb.push(timed(
                || {
                    for (i, &h) in hs.iter().enumerate() {
                        b.set_ink_handle_edges_wide(h, rect(i / 5, i % 5), 15, 2);
                    }
                },
                1,
                ov,
            ));
            pages = b.num_pages();
            b.check_invariants();
            for (i, _) in hs.iter().enumerate().step_by(997) {
                assert_eq!(b.ink_wide_at(1 + i), rect(i / 5, i % 5));
            }
            let (h, r0) = (hs[n - 6], rect(rows - 1, 0));
            let r1 = WRect::new(r0.0[0] + 1, r0.0[1], r0.0[2] + 1, r0.0[3]);
            let mut i = 0usize;
            si.push(timed(
                || {
                    black_box(
                        b.set_ink_handle_wide(black_box(h), if i & 1 == 0 { r1 } else { r0 }),
                    );
                    i += 1;
                },
                1024,
                ov,
            ));
            sc.push(timed(
                || {
                    b.scroll_run(
                        1,
                        n - 1,
                        0,
                        if i & 1 == 0 { -3 } else { 3 },
                        NRect::new(0, 0, SIDE, SIDE),
                    );
                    i += 1;
                },
                64,
                ov,
            ));
            b.check_invariants();
        }
        let per_row: Vec<u64> = wb.iter().map(|v| v / rows as u64).collect();
        emit(rows, "rows32", "writeback-row", 5, pages, 1, &per_row, 0);
        emit(rows, "rows32", "far-set-ink", 1, pages, 1024, &si, 0);
        emit(rows, "rows32", "far-scroll", n - 1, pages, 64, &sc, 0);
    }
}

fn negative_control() {
    let rs = records(1000, false, false);
    let b = build(&rs);
    let mut o = oracle(&rs);
    o.move_run(1, 8, 992, 0, &b.page_starts());
    // Mutant: omit Bough's move. The very comparison used after real edits must reject it.
    assert_ne!(b.records(), o.recs, "omitted-move mutant escaped");
    eprintln!("negative control: omitted move rejected by sequence oracle");
}

pub fn run(spatial_only: bool) {
    assert!(!cfg!(debug_assertions), "use release");
    measure::prefer_p_cores();
    let ov = measure::overhead();
    assert!(
        measure::counters().0 > 0,
        "instruction counters unavailable"
    );
    eprintln!(
        "R6 counter overhead={ov}; samples={SAMPLES}; prefix={}; warm={}",
        bough_core::prefix::NAME,
        warm()
    );
    negative_control();
    println!("n,shape,operation,k,pages,batch,sample,instructions,instructions_per_op,output");
    if !spatial_only && std::env::var("R6_FAR").is_ok() {
        far(ov);
        return;
    }
    for n in [1000, 10000, 100000, 1000000] {
        if std::env::var("R6_MAX_N")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .is_some_and(|max| n > max)
        {
            continue;
        }
        let rs = records(n, false, false);
        if spatial_only {
            spatial(&rs, false, ov);
            spatial(&records(n, true, false), true, ov);
            continue;
        }
        for k in [8, 64] {
            for (place, at) in [("front", 1), ("middle", n / 2), ("end", n - k)] {
                edit_cell(
                    &rs,
                    &format!("insert{k}-{place}"),
                    Edit::Insert(if place == "end" { n } else { at }, k),
                    k,
                    ov,
                );
                edit_cell(
                    &rs,
                    &format!("remove{k}-{place}"),
                    Edit::Remove(at, k),
                    k,
                    ov,
                );
                edit_cell(
                    &rs,
                    &format!("move{k}-{place}"),
                    Edit::Move(at, k, if place == "front" { n - k } else { 1 }),
                    k,
                    ov,
                );
            }
        }
        edit_cell(&rs, "append1", Edit::Insert(n, 1), 1, ov);
        edit_cell(&rs, "translate64", Edit::Translate(n / 2, 64), 64, ov);
        edit_cell(
            &rs,
            "translate-half",
            Edit::Translate(n / 4, n / 2),
            n / 2,
            ov,
        );
        builds(&rs, ov);
        freezes(&rs, ov);
        queries(&rs, false, ov);
        queries(&records(n, false, true), true, ov);
        handles(&rs, ov);
        overflow(&rs, ov);
        eprintln!("finished core N={n}");
    }
}
