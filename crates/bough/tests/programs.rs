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
#[path = "../../../docs/lanes/async/programs/p01b_search_field.rs"]
mod p01b;
#[path = "../../../docs/lanes/async/programs/p03b_optimistic_overlay.rs"]
mod p03b;
#[path = "../../../docs/lanes/async/programs/p04b_dependent_fields.rs"]
mod p04b;
#[path = "../../../docs/lanes/async/programs/p09b_errors_catch.rs"]
mod p09b;

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
    // released subtrees' nodes go back to the free lists first (reflection maps nodes by handle index)
    while a.reclaiming() {
        a.rt.reclaim(1 << 20);
    }
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

// =============================================================================== 2 pages

fn page(n: u32, more: bool) -> api::Page {
    api::Page { number: n, rows: (0..3).map(|i| format!("p{n} r{i}")).collect(), more }
}

fn pager_snap(v: &p02::Pager) -> p02::Pager {
    p02::Pager { page: Signal::new(*v.page), rows: Signal::new((*v.rows).clone()) }
}

/// The label names the page whose rows are listed.
fn pager_consistent(a: &App<p02::Pager>, ctx: &str) {
    let t = texts(a);
    let n: u32 = t.iter().find_map(|s| s.strip_prefix("Page ")).expect("a label").parse().unwrap();
    for r in t.iter().filter(|s| s.starts_with('p') && s.contains(" r")) {
        assert!(r.starts_with(&format!("p{} ", n - 1)), "{ctx}: row {r} under Page {n}: {t:?}");
    }
}

#[test]
fn p02_pager_shows_whole_pages() {
    let mut a = start(p02::Pager::new());
    assert!(has(&a, "Loading\u{2026}") && has(&a, "Page 1"));
    server::answer("page", "0", page(0, true));
    settle(&mut a);
    pager_consistent(&a, "page 0");
    oracle(&mut a, "page 0", pager_snap);
    // Next, Next, Previous: one request left (page 2 asked, then dropped for page 1 again)
    act(&mut a, p02::Pager::next);
    act(&mut a, p02::Pager::next);
    act(&mut a, p02::Pager::prev);
    pager_consistent(&a, "asked");
    oracle(&mut a, "asked", pager_snap);
    assert!(has(&a, "Page 1") && has(&a, "p0 r0") && a.owner.page.pending());
    assert_eq!(calls("page"), vec![("0".into(), false), ("1".into(), true), ("2".into(), true), ("1".into(), false)]);
    server::answer("page", "1", page(1, true));
    settle(&mut a);
    pager_consistent(&a, "page 1");
    assert!(has(&a, "Page 2") && has(&a, "p1 r2") && !a.owner.page.pending());
    // Next then Previous back to the shown page: nets out
    act(&mut a, p02::Pager::next);
    act(&mut a, p02::Pager::prev);
    assert!(!a.owner.page.pending());
    assert_eq!(calls("page").last(), Some(&("2".into(), true)));
    pager_consistent(&a, "netted");
    oracle(&mut a, "netted", pager_snap);
}

#[test]
fn p02_feed_appends_in_the_order_asked() {
    let mut a = start(p02::Feed::new());
    act(&mut a, p02::Feed::load_more);
    act(&mut a, p02::Feed::load_more);
    assert!(has(&a, "Loading page 1\u{2026}") && has(&a, "Loading page 2\u{2026}"));
    // the second page answers first: it waits for the first
    server::answer("page", "1", page(1, true));
    settle(&mut a);
    assert!(!has(&a, "p1 r0") && has(&a, "Loading page 2\u{2026}"), "{:?}", texts(&a));
    server::answer("page", "0", page(0, true));
    settle(&mut a);
    let t = texts(&a);
    let rows: Vec<&String> = t.iter().filter(|s| s.contains(" r")).collect();
    assert_eq!(rows, ["p0 r0", "p0 r1", "p0 r2", "p1 r0", "p1 r1", "p1 r2"]);
    assert!(!t.iter().any(|s| s.starts_with("Loading")), "{t:?}");
    assert_eq!(*a.owner.loaded, 2);
}

// =============================================================================== 3 optimistic

fn todo(id: u64, title: &str, done: bool) -> api::Todo {
    api::Todo { id, title: title.into(), done }
}

fn todos() -> App<p03::Todos> {
    start(p03::Todos::new(vec![todo(1, "milk", false), todo(2, "eggs", false)]))
}

#[test]
fn p03_toggles_roll_back_without_rollback_code() {
    let mut a = todos();
    act(&mut a, |v| v.flip(1));
    assert!(has(&a, "[x] milk"), "the guess shows at once");
    act(&mut a, |v| v.flip(1));
    assert!(has(&a, "[ ] milk"), "the second guess over the first");
    // the first fails after the second was sent: the second's guess stays
    server::answer("set_done", "1=true", Err::<api::Todo, _>(api::Error::Refused("locked".into())));
    settle(&mut a);
    assert!(has(&a, "[ ] milk") && has(&a, "Could not save: refused: locked"), "{:?}", texts(&a));
    server::answer("set_done", "1=false", Ok::<_, api::Error>(todo(1, "milk", false)));
    settle(&mut a);
    assert!(has(&a, "[ ] milk") && a.owner.toggle.inflight().count() == 0);
    // landings keep the order sent: the second answer waits for the first
    act(&mut a, |v| v.flip(2));
    act(&mut a, |v| v.flip(2));
    server::answer("set_done", "2=false", Ok::<_, api::Error>(todo(2, "eggs", false)));
    settle(&mut a);
    assert!(has(&a, "[ ] eggs") && a.owner.toggle.inflight().count() == 2, "nothing landed yet");
    server::answer("set_done", "2=true", Ok::<_, api::Error>(todo(2, "eggs", true)));
    settle(&mut a);
    assert!(has(&a, "[ ] eggs") && a.owner.items[1].done == false, "the later run landed last");
}

#[test]
fn p03_an_add_becomes_a_row_in_one_frame() {
    let mut a = todos();
    let count = |a: &App<p03::Todos>, t: &str| texts(a).iter().filter(|s| s.contains(t)).count();
    act(&mut a, |v| *v.draft = "bread".into());
    act(&mut a, p03::Todos::submit);
    assert_eq!(count(&a, "bread"), 1, "the in-flight row");
    server::answer("add_todo", "bread", Ok::<_, api::Error>(todo(3, "bread", false)));
    settle(&mut a);
    assert_eq!(count(&a, "bread"), 1, "the real row replaced it in the same frame: {:?}", texts(&a));
    assert!(has(&a, "[ ] bread"));
    act(&mut a, |v| *v.draft = "jam".into());
    act(&mut a, p03::Todos::submit);
    assert_eq!(count(&a, "jam"), 1);
    server::answer("add_todo", "jam", Err::<api::Todo, _>(api::Error::Transient("timeout".into())));
    settle(&mut a);
    assert_eq!(count(&a, "jam"), 0, "the failed add is gone (the rollback is the end of its flight): {:?}", texts(&a));
    assert!(has(&a, "Could not add: network: timeout"));
}

// =============================================================================== 4 dependent

fn story(n: u64) -> api::Story {
    api::Story { id: n, title: format!("Story {n}"), author: if n == 8 { 70 } else { n * 10 } }
}

fn reader_snap(v: &p04::Reader) -> p04::Reader {
    p04::Reader { sel: Signal::new(*v.sel), story: Signal::new((*v.story).clone()), comments: Signal::new((*v.comments).clone()), author: Signal::new((*v.author).clone()), unread: Signal::new(*v.unread) }
}

/// Title, comments, author and the highlighted chip all belong to one story.
fn reader_consistent(a: &App<p04::Reader>, ctx: &str) {
    let recs = a.reflect_records();
    let t: Vec<String> = recs.iter().filter_map(|r| r.text.clone()).collect();
    let Some(n) = t.iter().find_map(|s| s.strip_prefix("Story ")).map(|s| s.parse::<u64>().unwrap()) else { return };
    for c in t.iter().filter(|s| s.contains(": c")) {
        assert!(c.starts_with(&format!("{n}: ")), "{ctx}: comment {c} under Story {n}: {t:?}");
    }
    if let Some(au) = t.iter().find_map(|s| s.strip_prefix("by User ")) {
        assert_eq!(au.parse::<u64>().unwrap(), story(n).author, "{ctx}: author beside Story {n}: {t:?}");
    }
    // the highlighted chip: the box before the text "story n" is blue
    for (i, r) in recs.iter().enumerate() {
        if let Some(s) = r.text.as_deref().and_then(|s| s.strip_prefix("story ")) {
            let k: u64 = s.parse().unwrap();
            let bg = recs[..i].iter().rev().find(|x| x.text.is_none() && x.color.is_some()).and_then(|x| x.color);
            let blue = bg == Some(0xffff6b2f);
            assert_eq!(blue, k == n, "{ctx}: chip {k} highlighted {blue} beside Story {n}");
        }
    }
}

fn comments(n: u64) -> Vec<api::Comment> {
    (0..2).map(|i| api::Comment { text: format!("{n}: c{i}") }).collect()
}

#[test]
fn p04_a_waterfall_publishes_whole() {
    let mut a = start(p04::Reader::new(1));
    server::answer("story", "1", story(1));
    server::answer("comments", "1", comments(1));
    settle(&mut a);
    server::answer("user", "10", api::User { id: 10, name: "User 10".into() });
    settle(&mut a);
    reader_consistent(&a, "loaded");
    oracle(&mut a, "loaded", reader_snap);
    assert!(has(&a, "Story 1") && has(&a, "by User 10") && has(&a, "1: c0"));
    // open story 7: everything waits for story 7, its comments and its author
    act(&mut a, |v| v.open(7));
    act(&mut a, |v| *v.unread += 1);
    assert!(has(&a, "1 unread"), "an unrelated cell is never held");
    for (name, arg) in [("comments", "7"), ("story", "7")] {
        if name == "story" {
            server::answer(name, arg, story(7));
        } else {
            server::answer(name, arg, comments(7));
        }
        settle(&mut a);
        reader_consistent(&a, name);
        oracle(&mut a, name, reader_snap);
        assert!(has(&a, "Story 1"), "{name}: still story 1: {:?}", texts(&a));
    }
    assert!(server::is_pending("user", "70"), "the author is asked once the story says who");
    server::answer("user", "70", api::User { id: 70, name: "User 70".into() });
    settle(&mut a);
    reader_consistent(&a, "story 7");
    oracle(&mut a, "story 7", reader_snap);
    assert!(has(&a, "Story 7") && has(&a, "by User 70") && has(&a, "7: c1"));
    // story 8 has the same author: no author request, published when story and comments are in
    act(&mut a, |v| v.open(8));
    server::answer("story", "8", story(8));
    server::answer("comments", "8", comments(8));
    settle(&mut a);
    reader_consistent(&a, "story 8");
    assert!(has(&a, "Story 8") && has(&a, "by User 70") && !a.owner.sel.pending());
    assert_eq!(calls("user").len(), 2);
}

#[test]
fn action_fields_are_found() {
    assert_eq!(bough::action::action_count::<p03::Todos>(), 2);
    assert_eq!(bough::action::action_count::<p02::Feed>(), 1);
}

// =============================================================================== 5 streams

fn ticker_snap(v: &p05::Ticker) -> p05::Ticker {
    p05::Ticker { symbol: Signal::new((*v.symbol).clone()), price: Signal::new(*v.price) }
}

#[test]
fn p05_ticker_switches_symbol_and_price_together() {
    let mut a = start(p05::Ticker::new("AAPL"));
    assert!(has(&a, "AAPL") && has(&a, "\u{2014}"));
    server::push("ticks", "AAPL", 1.5f64);
    settle(&mut a);
    assert!(has(&a, "1.50"));
    server::push("ticks", "AAPL", 2.25f64);
    settle(&mut a);
    assert!(has(&a, "2.25"));
    oracle(&mut a, "ticking", ticker_snap);
    act(&mut a, |v| v.pick("MSFT"));
    assert!(has(&a, "AAPL") && has(&a, "2.25"), "the old symbol and its price stay together: {:?}", texts(&a));
    assert_eq!(calls("ticks"), vec![("AAPL".into(), true), ("MSFT".into(), false)], "the old subscription was dropped");
    oracle(&mut a, "switching", ticker_snap);
    server::push("ticks", "MSFT", 10.0f64);
    settle(&mut a);
    assert!(has(&a, "MSFT") && has(&a, "10.00") && !has(&a, "AAPL"));
    // two ticks in one go: one per turn, the last stays
    server::push("ticks", "MSFT", 11.0f64);
    server::push("ticks", "MSFT", 12.0f64);
    a.poll();
    assert!(has(&a, "11.00"), "one item per turn: {:?}", texts(&a));
    settle(&mut a);
    assert!(has(&a, "12.00"));
    oracle(&mut a, "ticked", ticker_snap);
}

#[test]
fn p05_chat_streams_into_the_right_lines() {
    let mut a = start(p05::Chat::new());
    act(&mut a, |v| *v.draft = "hi".into());
    act(&mut a, p05::Chat::send);
    assert!(has(&a, "> hi") && has(&a, "Stop"));
    server::push("complete", "hi", api::Token("Hel".into()));
    settle(&mut a);
    server::push("complete", "hi", api::Token("lo".into()));
    settle(&mut a);
    assert!(has(&a, "Hello"));
    act(&mut a, p05::Chat::stop);
    assert!(!has(&a, "Stop") && has(&a, "Hello"));
    assert_eq!(calls("complete"), vec![("hi".into(), true)], "Stop dropped the stream");
    // two messages: the second reply waits for the first and lands in its own line
    act(&mut a, |v| *v.draft = "a".into());
    act(&mut a, p05::Chat::send);
    act(&mut a, |v| *v.draft = "b".into());
    act(&mut a, p05::Chat::send);
    server::push("complete", "b", api::Token("B1".into()));
    settle(&mut a);
    assert!(!has(&a, "B1"), "b's reply waits for a's");
    server::push("complete", "a", api::Token("A1".into()));
    server::close("complete", "a");
    settle(&mut a);
    let t = texts(&a);
    let pos = |s: &str| t.iter().position(|x| x == s).unwrap_or_else(|| panic!("{s} in {t:?}"));
    assert!(pos("> a") < pos("A1") && pos("A1") < pos("> b") && pos("> b") < pos("B1"), "{t:?}");
    server::close("complete", "b");
    settle(&mut a);
    assert!(!has(&a, "Stop"));
}

// =============================================================================== 6 images

#[test]
fn p06_rows_own_their_questions() {
    let photos: bough::list::List<p06::Photo> = bough::list::List::from_iter_in(["u1", "u2", "u3"].map(p06::Photo::new));
    let mut a = start(p06::Gallery { photos });
    let fetched: Vec<String> = calls("fetch").into_iter().map(|c| c.0).collect();
    assert_eq!(fetched, ["u1", "u2", "u3"], "each mounted row asked for its own thumbnail");
    server::answer("fetch", "u2", vec![7u8; 4096]);
    // the decode runs on a worker: wait for it (real time), polling as the host would on its wake-up
    let t0 = std::time::Instant::now();
    while a.owner.photos.get(1).unwrap().pixels.is_none() && t0.elapsed() < Duration::from_secs(5) {
        a.poll();
        std::thread::sleep(ms(1));
    }
    a.frame();
    let img = (*a.owner.photos.get(1).unwrap().pixels).clone().expect("decoded");
    assert!(a.owner.photos.get(0).unwrap().pixels.is_none() && a.owner.photos.get(2).unwrap().pixels.is_none(), "only the row that asked");
    assert!(a.reflect_records().iter().any(|r| r.color == Some(img.rgba)), "the row shows its pixels");
    // a removed row drops its request
    act(&mut a, |v| {
        v.photos.remove(2);
    });
    settle(&mut a);
    assert_eq!(calls("fetch").iter().find(|c| c.0 == "u3").map(|c| c.1), Some(true), "the removed row's fetch was dropped");
}

// =============================================================================== 7 route

fn mail_snap(v: &p07::Mail) -> p07::Mail {
    p07::Mail { route: Signal::new(*v.route), page: Signal::new((*v.page).clone()) }
}

#[test]
fn p07_the_old_screen_stays_until_the_new_one_is_ready() {
    let mut a = start(p07::Mail::new());
    server::answer("inbox", "", vec!["mail 1".to_string(), "mail 2".to_string()]);
    settle(&mut a);
    assert!(has(&a, "mail 2"));
    oracle(&mut a, "inbox", mail_snap);
    act(&mut a, |v| v.go(p07::Route::Thread(7)));
    assert!(has(&a, "mail 2") && a.owner.route.pending(), "the inbox stays while the thread loads");
    oracle(&mut a, "loading thread", mail_snap);
    server::answer("thread", "7", vec!["msg a".to_string()]);
    settle(&mut a);
    assert!(has(&a, "msg a") && !has(&a, "mail 2"), "{:?}", texts(&a));
    oracle(&mut a, "thread", mail_snap);
    // Settings, then Inbox before Settings arrives: Settings is dropped
    act(&mut a, |v| v.go(p07::Route::Prefs));
    act(&mut a, |v| v.go(p07::Route::Inbox));
    assert_eq!(calls("prefs"), vec![(String::new(), true)]);
    assert!(has(&a, "msg a"));
    server::answer("inbox", "", vec!["mail 3".to_string()]);
    settle(&mut a);
    assert!(has(&a, "mail 3") && !has(&a, "msg a"));
    oracle(&mut a, "inbox again", mail_snap);
}

// =============================================================================== 8 swr

fn dash_snap(v: &p08::Dash) -> p08::Dash {
    p08::Dash { stats: Signal::new((*v.stats).clone()), stale: Signal::new((*v.stale).clone()) }
}

#[test]
fn p08_revalidates_quietly() {
    let mut a = start(p08::Dash::new());
    assert!(has(&a, "Loading\u{2026}"));
    server::answer("stats", "", Ok::<_, api::Error>(api::Stats { visitors: 100, revenue: 5 }));
    settle(&mut a);
    assert!(has(&a, "100 visitors") && !a.owner.stats.pending());
    oracle(&mut a, "loaded", dash_snap);
    a.advance(Duration::from_secs(30));
    a.frame();
    assert!(server::is_pending("stats", ""), "the interval asked again");
    assert!(has(&a, "100 visitors") && a.owner.stats.pending(), "the old numbers stay, marked pending");
    oracle(&mut a, "revalidating", dash_snap);
    server::answer("stats", "", Ok::<_, api::Error>(api::Stats { visitors: 200, revenue: 9 }));
    settle(&mut a);
    assert!(has(&a, "200 visitors") && !a.owner.stats.pending());
    act(&mut a, p08::Dash::refresh);
    assert!(server::is_pending("stats", ""));
    server::answer("stats", "", Err::<api::Stats, _>(api::Error::Transient("down".into())));
    settle(&mut a);
    assert!(has(&a, "200 visitors") && has(&a, "Showing earlier numbers (network: down)"));
    oracle(&mut a, "stale", dash_snap);
}

// =============================================================================== 9 errors

fn profile_snap(v: &p09::Profile) -> p09::Profile {
    p09::Profile { id: Signal::new(*v.id), user: Signal::new((*v.user).clone()), clicks: Signal::new(*v.clicks) }
}

#[test]
fn p09_an_error_is_an_answer() {
    let mut a = start(p09::Profile::new(1));
    assert!(has(&a, "Loading\u{2026}"));
    server::answer("try_user", "1", Err::<api::User, _>(api::Error::Refused("private".into())));
    settle(&mut a);
    assert!(has(&a, "Could not load the profile: refused: private") && has(&a, "Retry") && !has(&a, "Loading\u{2026}"), "{:?}", texts(&a));
    oracle(&mut a, "failed", profile_snap);
    act(&mut a, p09::Profile::click);
    assert!(has(&a, "clicked 1"), "the rest of the screen carries on");
    act(&mut a, p09::Profile::retry);
    assert!(a.owner.user.pending() && has(&a, "Retry"), "the error stays (pending) while the retry is out");
    oracle(&mut a, "retrying", profile_snap);
    server::answer("try_user", "1", Ok::<_, api::Error>(api::User { id: 1, name: "Ada".into() }));
    settle(&mut a);
    assert!(has(&a, "Ada (#1)") && !has(&a, "Retry"));
    oracle(&mut a, "loaded", profile_snap);
}

// =============================================================================== 10 race

#[test]
fn p10_first_mirror_wins_and_deadlines_drop_both() {
    let mut a = start(p10::Viewer::new(1));
    assert_eq!(calls("doc_from").len(), 2);
    server::answer("doc_from", "West/1", Ok::<_, api::Error>(api::Doc { id: 1, body: "from the west".into() }));
    settle(&mut a);
    assert!(has(&a, "from the west"));
    assert_eq!(calls("doc_from"), vec![("East/1".into(), true), ("West/1".into(), false)], "the loser was dropped");
    act(&mut a, |v| v.open(2));
    assert!(has(&a, "Document 1") && has(&a, "from the west"), "held while the race runs");
    a.advance(Duration::from_secs(2));
    a.frame();
    assert!(has(&a, "Document 2") && has(&a, "no mirror answered in 2 s"));
    assert!(calls("doc_from")[2..].iter().all(|c| c.1), "both late requests were dropped");
}

// =============================================================================== 11 retry

#[test]
fn p11_backoff_stops_when_the_user_moves_on() {
    let mut a = start(p11::Loader::new(1));
    assert!(has(&a, "loading (attempt 1)"));
    // a stream steps one item per turn: the request goes out at the next one
    settle(&mut a);
    server::answer("doc", "1", Err::<api::Doc, _>(api::Error::Transient("reset".into())));
    settle(&mut a);
    assert!(has(&a, "attempt 1 failed (network: reset); retrying in 250 ms"), "{:?}", texts(&a));
    a.advance(ms(250));
    settle(&mut a);
    assert!(has(&a, "loading (attempt 2)") && server::is_pending("doc", "1"));
    server::answer("doc", "1", Err::<api::Doc, _>(api::Error::Transient("reset".into())));
    settle(&mut a);
    assert!(has(&a, "attempt 2 failed (network: reset); retrying in 500 ms"));
    // the user opens another document in the middle of the backoff: the loop goes, no retry of 1 fires
    act(&mut a, |v| v.open(2));
    a.advance(Duration::from_secs(5));
    settle(&mut a);
    assert_eq!(calls("doc").iter().filter(|c| c.0 == "1").count(), 2, "no third request for document 1");
    server::answer("doc", "2", Ok::<_, api::Error>(api::Doc { id: 2, body: "two".into() }));
    settle(&mut a);
    assert!(has(&a, "two"));
}

// =============================================================================== 12 compute

#[test]
fn p12_long_work_yields_and_stops_when_dropped() {
    let mut a = start(p12::Primes::new(100_000));
    // on the UI thread: one slice per turn
    let turns = a.run_until_stalled();
    assert!(turns >= 5, "a slice per turn ({turns} turns)");
    assert!(has(&a, "9592 primes"), "{:?}", texts(&a));
    let t0 = std::time::Instant::now();
    while !has(&a, "9592 primes (worker)") && t0.elapsed() < Duration::from_secs(10) {
        a.poll();
        a.frame();
        std::thread::sleep(ms(2));
    }
    assert!(has(&a, "9592 primes (worker)"), "{:?}", texts(&a));
    // a new limit drops the running generator at its next slice: the old count never shows
    act(&mut a, |v| v.set(200_000));
    act(&mut a, |v| v.set(50_000));
    a.run_until_stalled();
    a.frame();
    assert!(has(&a, "5133 primes"), "{:?}", texts(&a));
    assert!(!has(&a, "17984 primes"));
}

// =============================================================================== the alternatives (b)

#[test]
fn p01b_field_question_behaves_as_p01() {
    let mut a = start(p01b::Search::new());
    for (i, q) in ["r", "re", "rec"].iter().enumerate() {
        act(&mut a, |v| *v.query = q.to_string());
        assert!(has(&a, "Results for \u{201c}\u{201d}") && has(&a, "searching\u{2026}"), "held: {:?}", texts(&a));
        if i < 2 {
            a.advance(ms(50));
        }
    }
    a.advance(ms(200));
    a.frame();
    assert_eq!(calls("search"), vec![("rec".to_string(), false)]);
    assert!(a.owner.hits.pending());
    server::answer("search", "rec", hits("rec", 2));
    settle(&mut a);
    assert!(has(&a, "Results for \u{201c}rec\u{201d}") && has(&a, "rec#2") && !a.owner.hits.pending());
    // refresh is a method of the field
    act(&mut a, |v| v.hits.refresh());
    assert!(a.owner.hits.pending(), "a refresh is pending at once");
    a.advance(ms(200));
    assert!(server::is_pending("search", "rec"), "and asks after the debounce");
}

#[test]
fn p04b_field_waterfall_publishes_whole() {
    let mut a = start(p04b::Reader::new(1));
    server::answer("story", "1", story(1));
    server::answer("comments", "1", comments(1));
    settle(&mut a);
    server::answer("user", "10", api::User { id: 10, name: "User 10".into() });
    settle(&mut a);
    assert!(has(&a, "Story 1") && has(&a, "by User 10") && has(&a, "1: c0"), "{:?}", texts(&a));
    act(&mut a, |v| v.open(7));
    server::answer("comments", "7", comments(7));
    server::answer("story", "7", story(7));
    settle(&mut a);
    assert!(has(&a, "Story 1") && has(&a, "1: c0") && server::is_pending("user", "70"), "{:?}", texts(&a));
    server::answer("user", "70", api::User { id: 70, name: "User 70".into() });
    settle(&mut a);
    assert!(has(&a, "Story 7") && has(&a, "by User 70") && has(&a, "7: c1") && !has(&a, "1: c0"));
}

#[test]
fn p03b_overlays_cancel_and_roll_back() {
    let mut a = start(p03b::Todos::new(vec![todo(1, "milk", false), todo(2, "eggs", false)]));
    act(&mut a, |v| v.flip(1));
    assert!(has(&a, "[x] milk"), "the overlay shows at once: {:?}", texts(&a));
    act(&mut a, |v| v.flip(1));
    assert!(has(&a, "[ ] milk"), "two toggles cancel on screen");
    assert!(!a.owner.items[0].done, "handlers see the truth");
    // the first fails, the second lands: the server toggled once
    server::answer("toggle_done", "1", Err::<api::Todo, _>(api::Error::Refused("locked".into())));
    settle(&mut a);
    assert!(has(&a, "[x] milk") && has(&a, "Could not save: refused: locked"), "{:?}", texts(&a));
    server::answer("toggle_done", "1", Ok::<_, api::Error>(todo(1, "milk", true)));
    settle(&mut a);
    assert!(has(&a, "[x] milk") && a.owner.items[0].done);
    // an add shows as a row of the list, then is the server's row, in one frame
    act(&mut a, |v| *v.draft = "bread".into());
    act(&mut a, p03b::Todos::submit);
    assert_eq!(texts(&a).iter().filter(|s| s.contains("bread")).count(), 1);
    server::answer("add_todo", "bread", Ok::<_, api::Error>(todo(3, "bread", false)));
    settle(&mut a);
    assert_eq!(texts(&a).iter().filter(|s| s.contains("bread")).count(), 1, "{:?}", texts(&a));
    assert_eq!(a.owner.items.len(), 3);
}

#[test]
fn p09b_errors_reach_the_boundary_and_retry_remounts() {
    let mut a = start(p09b::Profile::new(1));
    assert!(has(&a, "Loading\u{2026}"));
    server::answer("try_user", "1", Err::<api::User, _>(api::Error::Refused("private".into())));
    settle(&mut a);
    assert!(has(&a, "Could not load the profile: refused: private") && has(&a, "Retry") && !has(&a, "Loading\u{2026}"), "{:?}", texts(&a));
    act(&mut a, p09b::Profile::click);
    assert!(has(&a, "clicked 1"));
    // the Retry route: the content mounts again and asks again
    act(&mut a, |v| (bough::catch::Retry::<p09b::Profile>::for_test(0))(v));
    assert!(has(&a, "Loading\u{2026}") && server::is_pending("try_user", "1"), "{:?}", texts(&a));
    server::answer("try_user", "1", Ok::<_, api::Error>(api::User { id: 1, name: "Ada".into() }));
    settle(&mut a);
    assert!(has(&a, "Ada (#1)") && !has(&a, "Retry"), "{:?}", texts(&a));
}
