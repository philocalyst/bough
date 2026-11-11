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
    b.next_epoch();
    b
}

fn oracle(rs: &[RecordInit]) -> Oracle {
    let mut o = Oracle::new(SIDE, SIDE);
    o.insert_run(0, rs);
    o.next_epoch();
    o
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
        "R6 counter overhead={ov}; samples={SAMPLES}; prefix={}",
        bough_core::prefix::NAME
    );
    negative_control();
    println!("n,shape,operation,k,pages,batch,sample,instructions,instructions_per_op,output");
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
        eprintln!("finished core N={n}");
    }
}
