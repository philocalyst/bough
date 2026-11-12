#![feature(async_iterator, gen_blocks)]
//! The async lane's programs (`docs/lanes/async/programs/`), compiled as they are written and driven headless
//! with the fake client (`api.rs`) and a fake clock, in re-describing mode (no `.once()`), under the settle
//! audit when `BOUGH_SETTLE_AUDIT=1`.
#[path = "../../../docs/lanes/async/programs/api.rs"]
mod api;
#[path = "../../../docs/lanes/async/programs/p01_search.rs"]
mod p01;
#[path = "../../../docs/lanes/async/programs/p02_pages.rs"]
mod p02;
#[path = "../../../docs/lanes/async/programs/p03_optimistic.rs"]
mod p03;
#[path = "../../../docs/lanes/async/programs/p04_dependent.rs"]
mod p04;
#[path = "../../../docs/lanes/async/programs/p05_stream.rs"]
mod p05;
#[path = "../../../docs/lanes/async/programs/p06_images.rs"]
mod p06;
#[path = "../../../docs/lanes/async/programs/p07_route.rs"]
mod p07;
#[path = "../../../docs/lanes/async/programs/p08_swr.rs"]
mod p08;
#[path = "../../../docs/lanes/async/programs/p09_errors.rs"]
mod p09;
#[path = "../../../docs/lanes/async/programs/p10_race.rs"]
mod p10;
#[path = "../../../docs/lanes/async/programs/p11_retry.rs"]
mod p11;
#[path = "../../../docs/lanes/async/programs/p12_compute.rs"]
mod p12;
#[path = "../../../docs/lanes/async/programs/p13_skeleton.rs"]
mod p13;
#[path = "../../../docs/lanes/async/programs/p14_interrupt.rs"]
mod p14;

use api::server;
use bough::raw::{App, Config, View};
use bough::signal::Signal;
use std::time::Duration;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

fn cfg() -> Config {
    Config::new(480.0, 800.0, 1.0, FONT.to_vec())
}

fn start<V: View>(v: V) -> App<V> {
    server::reset();
    let mut a = App::new(v, cfg(), false);
    a.use_fake_clock();
    a.frame();
    a
}

/// The texts on screen, in paint order.
fn texts<V: View>(a: &App<V>) -> Vec<String> {
    a.reflect_records().into_iter().filter_map(|r| r.text).collect()
}

fn has<V: View>(a: &App<V>, t: &str) -> bool {
    texts(a).iter().any(|x| x == t)
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A turn with a handler, then a frame.
fn act<V: View>(a: &mut App<V>, f: impl FnOnce(&mut V)) {
    a.turn(f);
    a.frame();
}

/// Let the app see what the server did: poll turns until nothing rang.
fn settle<V: View>(a: &mut App<V>) {
    a.poll();
    a.run_until_stalled();
    a.frame();
}

/// The async oracle: a fresh, inert app built from the published state (`snap`, called with held cells reading
/// their shown values), with the same hold and loading bits, shows the same records as the retained app: no
/// torn frame, nothing left over from how the state was reached. Inputs are skipped (they show intent).
fn oracle<V: View>(a: &mut App<V>, ctx: &str, snap: impl FnOnce(&V) -> V) {
    let v = a.published(snap);
    let mut f = App::new(v, cfg().inert(), false);
    f.mirror_status_from(a);
    let key = |r: &bough::reflect::RecordView| -> Option<String> {
        if r.element.is_some_and(|e| e.contains("TextInput") || e.contains("TextField") || e.contains("CaretOp")) {
            return None;
        }
        Some(format!("{} {} {:?} {:?} {:?}", r.depth, r.kind, r.text, r.color, r.layout.map(|l| l.map(|x| (x * 4.0).round() as i32))))
    };
    let skip_input = |v: Vec<bough::reflect::RecordView>| -> Vec<String> {
        // an input's caret and the label after it show intent: skipped
        let mut out = Vec::new();
        let mut after_caret = false;
        for r in v {
            if r.element.is_some_and(|e| e.contains("CaretOp")) {
                after_caret = true;
                continue;
            }
            if after_caret && r.text.is_some() {
                after_caret = false;
                continue;
            }
            if let Some(k) = key(&r) {
                out.push(k);
            }
        }
        out
    };
    let (x, y) = (skip_input(a.reflect_records()), skip_input(f.reflect_records()));
    if x != y {
        let n = x.len().max(y.len());
        let mut d = String::new();
        for i in 0..n {
            let (p, q) = (x.get(i).map(|s| s.as_str()).unwrap_or("-"), y.get(i).map(|s| s.as_str()).unwrap_or("-"));
            d.push_str(&format!("{} {p}\n  fresh: {q}\n", if p == q { " " } else { "!" }));
        }
        panic!("{ctx}: retained differs from a fresh mount of the published state\n{d}");
    }
}

fn calls(name: &str) -> Vec<(String, bool)> {
    server::history(name)
}

// =============================================================================== 1 search

fn search_snap(v: &p01::Search) -> p01::Search {
    p01::Search { query: Signal::new((*v.query).clone()), hits: Signal::new((*v.hits).clone()) }
}

fn hits(q: &str, n: usize) -> Vec<api::Hit> {
    (1..=n).map(|i| api::Hit { title: format!("{q}#{i}") }).collect()
}

/// The heading names the query whose hits are listed under it, in every frame.
fn search_consistent(a: &App<p01::Search>, ctx: &str) {
    let t = texts(a);
    let head = t.iter().find_map(|s| s.strip_prefix("Results for \u{201c}").and_then(|s| s.strip_suffix("\u{201d}"))).expect("a heading").to_string();
    for h in t.iter().filter(|s| s.contains('#')) {
        assert_eq!(h.split('#').next().unwrap(), head, "{ctx}: hit {h} under the heading for {head:?}: {t:?}");
    }
}

#[test]
fn p01_search_as_you_type() {
    let mut a = start(p01::Search::new());
    oracle(&mut a, "mount", search_snap);
    // three keystrokes inside the debounce: one request, for the last
    for (i, q) in ["r", "re", "rec"].iter().enumerate() {
        act(&mut a, |v| *v.query = q.to_string());
        search_consistent(&a, q);
        oracle(&mut a, &format!("typed {q}"), search_snap);
        assert!(has(&a, "Results for \u{201c}\u{201d}"), "the heading waits for the answer");
        assert!(has(&a, "searching\u{2026}"), "pending shows");
        if i < 2 {
            a.advance(ms(50));
        }
    }
    a.advance(ms(200));
    a.frame();
    assert_eq!(calls("search"), vec![("rec".to_string(), false)], "the debounce sent exactly one request");
    // typing after the request went out drops it
    act(&mut a, |v| *v.query = "recs".into());
    assert_eq!(calls("search"), vec![("rec".to_string(), true)], "a newer keystroke cancelled the request in the air");
    a.advance(ms(200));
    a.frame();
    server::answer("search", "recs", hits("recs", 3));
    settle(&mut a);
    search_consistent(&a, "answered");
    oracle(&mut a, "answered", search_snap);
    assert!(has(&a, "Results for \u{201c}recs\u{201d}") && has(&a, "recs#3") && !has(&a, "searching\u{2026}"));
    // back to the query on screen: nets out (no request, nothing held)
    let before = calls("search").len();
    act(&mut a, |v| *v.query = "recsx".into());
    assert!(a.owner.query.pending());
    act(&mut a, |v| *v.query = "recs".into());
    assert!(!a.owner.query.pending(), "back to the shown query releases at once");
    a.advance(ms(500));
    a.frame();
    assert_eq!(calls("search").len(), before, "no request for a query that netted out");
    oracle(&mut a, "netted", search_snap);
    // empty query: ready on the first poll, published in the same turn (no pending frame)
    act(&mut a, |v| v.query.clear());
    assert!(!a.owner.query.pending() && has(&a, "Results for \u{201c}\u{201d}") && !has(&a, "recs#1"));
    oracle(&mut a, "empty", search_snap);
}

#[test]
fn p01_random_schedules_never_tear() {
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }
    for seed in 1..12u64 {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut a = start(p01::Search::new());
        let mut log: Vec<String> = Vec::new();
        for step in 0..80 {
            match r.next() % 5 {
                0 | 1 => {
                    let q = ["a", "ab", "abc", "b", ""][(r.next() % 5) as usize];
                    log.push(format!("type {q:?}"));
                    act(&mut a, |v| *v.query = q.into());
                }
                2 => {
                    let d = 100 + r.next() % 200;
                    log.push(format!("advance {d}"));
                    a.advance(ms(d));
                    a.frame();
                }
                _ => {
                    let p = server::pending();
                    if !p.is_empty() {
                        let c = &p[(r.next() % p.len() as u64) as usize];
                        let arg = c.arg.clone();
                        log.push(format!("answer {arg:?}"));
                        server::answer("search", &arg, hits(&arg, 1 + (r.next() % 3) as usize));
                    } else {
                        log.push("poll".into());
                    }
                    settle(&mut a);
                }
            }
            log.push(format!("   -> {:?} query {:?} kept {} {}", texts(&a), *a.owner.query, a.kept_values(), a.rt.asks.engine.dump().replace('\n', " | ")));
            let ctx = format!("seed {seed} step {step}\n{}", log.join("\n"));
            search_consistent(&a, &ctx);
            oracle(&mut a, &ctx, search_snap);
        }
    }
}
