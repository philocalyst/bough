//! zgx's `drag_probe` SAME_WINDOW measurement on bough, windowed: synthetic AppKit mouse events posted to
//! this process alone (the user's pointer never moves) drag a chip along its window, three times; each drag
//! is 120 `LeftMouseDragged` events 8 ms apart from (60, 24) to (330, 150), measured from the 40th; the
//! main thread's retired instructions over the moves (`thread_selfcounts`, as zgx reads them), less the
//! cost of making and posting the events, less the same span held still. bough's drag preview is an
//! `offset` group following the pointer (one space write and one `translate_run` per move).
//!
//!   drag_probe            (windowed: take the windowed lock first: $BOUGH_WINDOWED_LOCK, default $TMPDIR/bough-windowed.lock)
use bough::backend::host::run_interactive;
use bough::raw::*;
use bough_zgui::font_bytes;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSWindow};
use objc2_foundation::{NSPoint, NSRunLoop, NSTimer};
use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;

unsafe extern "C" {
    fn thread_selfcounts(kind: i32, buffer: *mut u64, size: usize) -> i32;
}
/// The main thread's retired instructions so far (xnu's per-thread counters, kind 1: instructions, cycles).
fn instructions() -> u64 {
    let mut c = [0u64; 2];
    // SAFETY: kind 1 fills two u64s; `c` has 16 bytes.
    unsafe { thread_selfcounts(1, c.as_mut_ptr(), 16) };
    c[0]
}

const CHIP: Color = Color::rgba(0x3d, 0x7e, 0xf7, 0x33);
const PREVIEW: Color = rgb(0x3d7ef7);

struct Probe {
    drag: Signal<(f32, f32)>,
    dragging: Signal<bool>,
    words: Signal<String>,
}

impl View for Probe {
    fn view(&self) -> impl Element<Self> + '_ {
        let label = |s: &'static str| text(s).at(24.0, 6.0).size(14.0).line_height(16.0).color(rgb(0xffffff));
        (
            quad(0.0, 0.0, 420.0, 200.0).bg(rgb(0x1e1e1e)),
            quad(10.0, 10.0, 120.0, 28.0)
                .bg(CHIP)
                .rounded(8.0)
                .children(label("Some text"))
                .on_press(|p: &mut Probe, _| {
                    p.dragging.set(true);
                })
                .on_drag(|p: &mut Probe, e| {
                    p.drag.set((e.dx, e.dy));
                    p.dragging.set(e.on);
                }),
            quad(10.0, 48.0, 120.0, 28.0).bg(Color::rgba(128, 128, 128, 64)).rounded(8.0).children(label("tab")),
            quad(10.0, 86.0, 40.0, 8.0).bg(PREVIEW),
            text_field(sig!(self.words)).rect(10.0, 104.0, 240.0, 30.0),
            // the drag preview: shown while dragging, following the pointer
            no_hit(clip_by(sig!(self.dragging), |d| if *d { [0.0, 0.0, 420.0, 200.0] } else { [0.0, 0.0, 0.0, 0.0] }, offset(sig!(self.drag), quad(10.0, 10.0, 120.0, 28.0).bg(PREVIEW).rounded(8.0).children(label("Some text"))))),
        )
    }
}

fn post(window: &NSWindow, kind: NSEventType, x: f64, y: f64) {
    let mtm = MainThreadMarker::new().unwrap();
    let height = window.contentView().unwrap().frame().size.height;
    let e = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        kind,
        NSPoint::new(x, height - y),
        NSEventModifierFlags::empty(),
        0.0,
        window.windowNumber(),
        None,
        0,
        1,
        if kind == NSEventType::LeftMouseUp { 0.0 } else { 1.0 },
    )
    .unwrap();
    NSApplication::sharedApplication(mtm).postEvent_atStart(&e, false);
}

/// The script: a list of (delay ms, action) run by chained one-shot timers.
enum Act {
    Move(f64, f64),
    Down(f64, f64),
    Drag(f64, f64, bool),
    Up(f64, f64),
    Start,
    Stop,
    Still,
    StillEnd(usize),
}

pub fn main() {
    let cfg = |s: f32| Config::new(420.0, 200.0, s, font_bytes()).once();
    let probe = Probe { drag: Signal::new((0.0, 0.0)), dragging: Signal::new(false), words: Signal::new("drag these words".into()) };
    run_interactive(
        "Drag",
        cfg,
        probe,
        11.5,
        move |host, app| {
            if std::env::var_os("BOUGH_STREAM").is_some() {
                // throughput mode: frames pre-encoded, released by a shared event (+1 frame to screen)
                app.borrow_mut().stream(4);
            }
            let window = host.window.clone();
            let mut script: Vec<(u64, Act)> = vec![(300, Act::Move(60.0, 24.0))];
            for _ in 0..3 {
                let (steps, from) = (120usize, 40usize);
                script.push((200, Act::Down(60.0, 24.0)));
                for step in 1..=steps {
                    let t = step as f64 / steps as f64;
                    let (x, y) = (60.0 + 270.0 * t, 24.0 + 126.0 * t);
                    if step == from {
                        script.push((100, Act::Start));
                    }
                    script.push((if step == from { 0 } else { 8 }, Act::Drag(x, y, step >= from)));
                }
                script.push((8, Act::Stop));
                script.push((100, Act::Still));
                for _ in from..=steps {
                    script.push((8, Act::Move(-1.0, -1.0)));
                }
                script.push((0, Act::StillEnd(steps + 1 - from)));
                script.push((100, Act::Up(330.0, 150.0)));
            }
            let script = Rc::new(script);
            let k = Rc::new(Cell::new(0usize));
            // measurement state: start, posting cost, spent
            let acc = Rc::new(RefCell::new((0u64, 0u64, 0u64, 0u64)));
            fn schedule(script: Rc<Vec<(u64, Act)>>, k: Rc<Cell<usize>>, acc: Rc<RefCell<(u64, u64, u64, u64)>>, window: objc2::rc::Retained<NSWindow>) {
                let i = k.get();
                if i >= script.len() {
                    println!("DONE");
                    std::process::exit(0);
                }
                let delay = script[i].0 as f64 / 1000.0;
                let blk = block2::RcBlock::new(move |_t: NonNull<NSTimer>| {
                    let i = k.get();
                    k.set(i + 1);
                    match script[i].1 {
                        Act::Move(x, y) if x >= 0.0 => post(&window, NSEventType::MouseMoved, x, y),
                        Act::Move(..) => {}
                        Act::Down(x, y) => post(&window, NSEventType::LeftMouseDown, x, y),
                        Act::Up(x, y) => post(&window, NSEventType::LeftMouseUp, x, y),
                        Act::Drag(x, y, measured) => {
                            let before = instructions();
                            post(&window, NSEventType::LeftMouseDragged, x, y);
                            if measured {
                                acc.borrow_mut().1 += instructions() - before;
                            }
                        }
                        Act::Start => {
                            let mut a = acc.borrow_mut();
                            a.0 = instructions();
                            a.1 = 0;
                        }
                        Act::Stop => {
                            let mut a = acc.borrow_mut();
                            a.2 = instructions() - a.0 - a.1;
                        }
                        Act::Still => acc.borrow_mut().3 = instructions(),
                        Act::StillEnd(moves) => {
                            let a = *acc.borrow();
                            let still = instructions() - a.3;
                            let spent = a.2.saturating_sub(still);
                            println!("SAME_WINDOW moves={moves} instructions_per_move={} (moves {} still {})", spent / moves as u64, a.2 / moves as u64, still / moves as u64);
                        }
                    }
                    schedule(script.clone(), k.clone(), acc.clone(), window.clone());
                });
                let t = unsafe { NSTimer::timerWithTimeInterval_repeats_block(delay, false, &blk) };
                unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, objc2_foundation::NSRunLoopCommonModes) };
            }
            schedule(script, k, acc, window);
        },
        |a| eprintln!("frames {} surfaces {:?}", a.frames_presented(), a.surf_stats),
    )
}
