//! Defect check (round 7, async): `reader.rs` in re-describing mode (no `.once()`).
//! `Reader<T>`: no tearing. Two readers fed by one source form a cohort; whatever order their jobs
//! complete in (superseded tickets, out of order, interleaved with unrelated writes), every frame shows
//! one committed selection in all three consumers (both readers and the held source's sink), the
//! `latest(..)` sink shows current intent, and an unrelated signal is never held.
use bough::raw::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct S {
    sel: Signal<u32>,
    story: Reader<String>,
    comments: Reader<String>,
    unread: Signal<u32>,
}

fn story_req(id: &u32) -> Request<String> {
    let id = *id;
    Request::new(move || format!("story {id}"))
}
fn comments_req(id: &u32) -> Request<String> {
    let id = *id;
    Request::new(move || format!("comments {id}"))
}

impl View for S {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            text(sig!(self.story).map(|s: Option<&String>| s.cloned().unwrap_or_else(|| "loading".into()))).at(0.0, 0.0),
            text(sig!(self.comments).map(|s: Option<&String>| s.cloned().unwrap_or_else(|| "loading".into()))).at(0.0, 20.0),
            text(sig!(self.sel).map(|i| format!("sel {i}"))).at(0.0, 40.0),
            text(latest(sig!(self.sel).map(|i| format!("now {i}")))).at(0.0, 60.0),
            text(sig!(self.story).pending().map(|p| if p { "pending".to_string() } else { "idle".to_string() })).at(0.0, 80.0),
            text(sig!(self.unread).map(|u| format!("unread {u}"))).at(0.0, 100.0),
            // producers declared after their consumers: the hold must not depend on declaration order
            produce(sig!(self.sel), sig!(self.story), story_req),
            produce(sig!(self.sel), sig!(self.comments), comments_req),
        )
    }
}

#[derive(Debug, Default)]
struct Shown {
    story: Option<u32>,
    comments: Option<u32>,
    sel: u32,
    now: u32,
    pending: bool,
    unread: u32,
}

fn num(s: &str, p: &str) -> Option<u32> {
    s.strip_prefix(p).and_then(|x| x.parse().ok())
}
fn shown(a: &App<S>) -> Shown {
    let mut o = Shown::default();
    for t in &a.rt.texts {
        let owned = String::from_utf8_lossy(t.bytes()).into_owned();
        let s = owned.as_str();
        if let Some(n) = num(s, "story ") {
            o.story = Some(n);
        } else if let Some(n) = num(s, "comments ") {
            o.comments = Some(n);
        } else if let Some(n) = num(s, "sel ") {
            o.sel = n;
        } else if let Some(n) = num(s, "now ") {
            o.now = n;
        } else if let Some(n) = num(s, "unread ") {
            o.unread = n;
        } else if s == "pending" {
            o.pending = true;
        }
    }
    o
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// The cohort invariant: both readers and the held sink show the same committed selection (before first
/// readiness both readers show "loading" and the sink the initial value).
fn check(a: &App<S>, ctx: &str) -> Shown {
    let s = shown(a);
    match (s.story, s.comments) {
        (Some(x), Some(y)) => {
            assert_eq!(x, y, "{ctx}: torn readers {s:?}");
            assert_eq!(x, s.sel, "{ctx}: held source sink out of its cohort {s:?}");
        }
        (None, None) => assert_eq!(s.sel, 0, "{ctx}: sink moved before first readiness {s:?}"),
        _ => panic!("{ctx}: one reader ready without the other {s:?}"),
    }
    assert_eq!(s.now, *a.owner.sel, "{ctx}: the latest(..) sink lags current intent");
    assert_eq!(s.unread, *a.owner.unread, "{ctx}: an unrelated signal was held");
    assert_eq!(s.pending, a.owner.story.is_pending(), "{ctx}: pending flag");
    s
}

#[test]
fn a_cohort_commits_in_one_epoch() {
    let mut a = App::new(S { sel: Signal::new(0), story: Reader::new(), comments: Reader::new(), unread: Signal::new(0) }, Config::new(200.0, 140.0, 1.0, FONT.to_vec()), false);
    a.frame();
    check(&a, "mount");
    assert_eq!(a.rt.jobs_queued(), 2);
    a.run_jobs(1);
    a.poll();
    a.frame();
    let s = check(&a, "one of two done");
    assert_eq!(s.story, None);
    a.run_jobs(1);
    a.poll();
    a.frame();
    let s = check(&a, "both done");
    assert_eq!((s.story, s.pending), (Some(0), false));
    // a new selection: held until both land
    a.turn(|v| v.sel.set(7).then_some(()).unwrap());
    a.frame();
    let s = check(&a, "requested 7");
    assert_eq!((s.story, s.sel, s.now, s.pending), (Some(0), 0, 7, true));
    a.turn(|v| *v.unread += 1);
    a.frame();
    check(&a, "unrelated write while pending");
    a.run_jobs(1);
    a.poll();
    a.frame();
    check(&a, "7: one done");
    a.run_jobs(1);
    a.poll();
    a.frame();
    let s = check(&a, "7: both done");
    assert_eq!((s.story, s.sel, s.pending), (Some(7), 7, false));
}

#[test]
fn no_tearing_under_random_schedules() {
    for seed in 1..30u64 {
        let mut r = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
        let mut a = App::new(S { sel: Signal::new(0), story: Reader::new(), comments: Reader::new(), unread: Signal::new(0) }, Config::new(200.0, 140.0, 1.0, FONT.to_vec()), false);
        a.frame();
        for step in 0..120 {
            let ctx = format!("seed {seed} step {step}");
            match r.next() % 6 {
                0 | 1 => {
                    let v = (r.next() % 9) as u32;
                    a.turn(|s| {
                        s.sel.set(v);
                    });
                }
                2 => a.turn(|s| *s.unread += 1),
                3 => {
                    let n = a.rt.jobs_queued();
                    if n > 0 {
                        let i = (r.next() % n as u64) as usize;
                        a.rt.run_job_at(i);
                    }
                    a.poll();
                }
                4 => {
                    a.run_jobs(1);
                    a.poll();
                }
                _ => a.poll(),
            }
            a.frame();
            check(&a, &ctx);
        }
        // drain: everything lands on the current selection
        while a.run_jobs(16) > 0 {}
        a.poll();
        a.frame();
        let s = check(&a, &format!("seed {seed} drained"));
        assert_eq!(s.story, Some(*a.owner.sel), "seed {seed}: drained to the current selection");
        assert!(!s.pending);
    }
}
