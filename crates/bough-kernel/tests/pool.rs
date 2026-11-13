//! The worker pool's lifetime proof (review finding K01): `run` erases the lifetime of a borrowed closure,
//! so it must not leave its frame, by return or by unwinding, while a worker can still call it; and two
//! callers must not share the job slot.
use bough_kernel::cpu::Pool;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};

#[test]
fn run_waits_for_its_workers_when_the_caller_panics() {
    let pool = Pool::new(1);
    let (started, release, completed) = (AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false));
    let work = |id: usize| {
        if id == 0 {
            while !started.load(SeqCst) {
                std::thread::yield_now();
            }
            // let the worker go a moment after the caller starts to unwind
            std::thread::scope(|s| {
                s.spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    release.store(true, SeqCst);
                });
                panic!("intentional caller failure");
            });
        } else {
            started.store(true, SeqCst);
            while !release.load(SeqCst) {
                std::thread::yield_now();
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
            completed.store(true, SeqCst);
        }
    };
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pool.run(1, &work)));
    assert!(r.is_err());
    assert!(completed.load(SeqCst), "run returned (unwound) while a worker was still inside the closure");
}

#[test]
fn a_worker_panic_is_reported_after_every_worker_has_finished() {
    let pool = Pool::new(2);
    let done = AtomicUsize::new(0);
    let work = |id: usize| {
        if id == 1 {
            panic!("intentional worker failure");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        done.fetch_add(1, SeqCst);
    };
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pool.run(2, &work)));
    assert!(r.is_err(), "a worker's panic was swallowed");
    assert_eq!(done.load(SeqCst), 2, "run ended before the other calls did");
    // the pool is usable afterwards
    let n = AtomicUsize::new(0);
    pool.run(2, &|_| {
        n.fetch_add(1, SeqCst);
    });
    assert_eq!(n.load(SeqCst), 3);
}

#[test]
fn overlapping_runs_do_not_share_the_job() {
    let pool = Pool::new(2);
    let inside = AtomicUsize::new(0);
    let worst = AtomicUsize::new(0);
    let total = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..50 {
                    // each run's closure counts its own calls: with a shared job slot a worker would run
                    // another caller's closure, or one already dropped
                    let mine = AtomicUsize::new(0);
                    let now = inside.fetch_add(1, SeqCst) + 1;
                    worst.fetch_max(now, SeqCst);
                    pool.run(2, &|_| {
                        mine.fetch_add(1, SeqCst);
                    });
                    inside.fetch_sub(1, SeqCst);
                    assert_eq!(mine.load(SeqCst), 3);
                    total.fetch_add(1, SeqCst);
                }
            });
        }
    });
    assert_eq!(total.load(SeqCst), 200);
}
