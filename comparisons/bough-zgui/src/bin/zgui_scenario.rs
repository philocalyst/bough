//! zgui's `zgui-bench scenario <name>` on bough: the same documents, event sequences and measured spans
//! (crates/zgui-bench/src/scenario/*.rs), printed as zgui prints them plus an `INSTR` line per timed
//! measurement (retired instructions over exactly the timed span).
//!
//!   zgui_scenario idle|hover-storm|scroll|cold-start|kitchen-sink|app-shell      RENDER=1: GPU frames too
//!
//! A zgui span is "deliver one event, then `settle(64)`": the event's handlers, restyle, layout and every
//! frame the loop runs until quiet, through its `NullRenderer` (display list, no device). The bough span is
//! the input turn plus `frame()`, which ends at the frame slot's tile list (the GPU's input), with
//! `RENDER=1` the Metal repair as well.
use bough::raw::*;
use bough_zgui::*;

const TICK_120: f32 = 0.008_333;
const TICK_60: f32 = 0.016_667;

fn gpu() -> bool {
    std::env::var("RENDER").is_ok_and(|v| v == "1")
}

/// zgui's `quiet()`: the first frame, then eight 60 Hz turns, so what follows is measured from rest.
fn quiet<V: View>(app: &mut App<V>) {
    app.frame();
    for _ in 0..8 {
        app.tick(TICK_60);
        app.frame();
    }
}

fn idle() {
    let mut app = App::new(Table { rows: STILL_ROWS }, config(WIDTH, HEIGHT), gpu());
    quiet(&mut app);
    let mut s = Samples::new();
    let mut frames = 0u64;
    for _ in 0..600 {
        let drew = s.time(|| {
            app.tick(TICK_60);
            app.frame()
        });
        frames += drew as u64;
    }
    println!("SCENARIO idle {} records over {STILL_ROWS} rows, 600 turns with nothing touched", app.rt.bough.len());
    count("idle", "idle.frames", "frames", frames as f64);
    report("idle", "idle.turn", "us", &s, 1.0);
}

fn hover_storm() {
    let mut app = App::new(Table { rows: TABLE_ROWS }, config(WIDTH, HEIGHT), gpu());
    quiet(&mut app);
    let rows = visible_table_rows();
    let mut s = Samples::new();
    let mut tiles = 0u64;
    let mut frames = 0u64;
    let st0 = app.rt.input.stats;
    for step in 0..240 {
        let (x, y) = table_row_centre(step % rows);
        let drew = s.time(|| {
            app.pointer_move(x, y);
            app.frame()
        });
        frames += drew as u64;
        tiles += app.repaired_tiles as u64 * drew as u64;
        // zgui: advance(8333 us) + pump (not measured)
        app.tick(TICK_120);
        app.frame();
    }
    let st = app.rt.input.stats;
    assert!(st.crossings - st0.crossings >= 240, "every step crossed a row");
    println!(
        "SCENARIO hover-storm {TABLE_ROWS} rows built, {rows} on screen, 240 crossings at 120 Hz; {} records; hits {} cell hits {} crossings {}",
        app.rt.bough.len(),
        st.hits - st0.hits,
        st.cell_hits - st0.cell_hits,
        st.crossings - st0.crossings
    );
    report("hover-storm", "hover.crossing", "us", &s, 1.0);
    count("hover-storm", "hover.tiles_repaired_per_frame", "tiles", tiles as f64 / frames.max(1) as f64);
    count("hover-storm", "hover.hit_rebuilds", "rebuilds", st.hit_rebuilds as f64);
}

fn scroll() {
    let mut app = App::new(LongList { scroll: Signal::new(0.0) }, config(WIDTH, HEIGHT), gpu());
    quiet(&mut app);
    let (mx, my) = (WIDTH / 2.0, HEIGHT / 2.0);
    app.pointer_move(mx, my);
    app.frame();
    let mut translation = Samples::new();
    let mut recycle = Samples::new();
    let (mut rebuilt_rows, mut stray_layout) = (0u64, 0u64);
    let st0 = app.rt.input.stats;
    for tick in 0..600 {
        if tick % 40 == 0 {
            app.wheel(mx, my, 0.0, 6.0);
            if std::env::var_os("TRACE").is_some() {
                eprintln!("{tick}\tnotch {:?} chain {}", app.rt.input.stats, app.rt.input.hovered().len());
            }
            app.frame();
            app.tick(TICK_120);
            app.frame();
            continue;
        }
        let rows0 = app.rt.ctr.rows_mounted;
        let lay0 = app.rt.lay.stats.computes;
        let (us, instr, drew) = translation.measure(|| {
            app.tick(TICK_120);
            app.frame()
        });
        if !drew {
            continue;
        }
        let built = app.rt.ctr.rows_mounted - rows0;
        if std::env::var_os("TRACE").is_some() {
            eprintln!("{tick}\t{:.4}\t{built}", *app.owner.scroll);
        }
        if built == 0 {
            translation.push(us, instr);
            stray_layout += app.rt.lay.stats.computes - lay0;
        } else {
            recycle.push(us, instr);
            rebuilt_rows += built;
        }
    }
    assert!(!translation.is_empty() && !recycle.is_empty(), "both kinds of frame measured");
    println!(
        "SCENARIO scroll {LIST_ROWS} rows, {} translation frames and {} recycle frames at 120 Hz; offset {:.1}",
        translation.len(),
        recycle.len(),
        *app.owner.scroll
    );
    report("scroll", "scroll.translation", "us", &translation, 1.0);
    report("scroll", "scroll.recycle", "us", &recycle, 1.0);
    count("scroll", "scroll.translation.relayouts", "computes", stray_layout as f64);
    count("scroll", "scroll.translation.hit_rebuilds", "rebuilds", (app.rt.input.stats.hit_rebuilds - st0.hit_rebuilds) as f64);
    println!("NOTE\tscroll\tone row shift mounts {:.2} rows", rebuilt_rows as f64 / recycle.len().max(1) as f64);
}

fn main() {
    // process entry, for the first-pixel scenarios
    let (entry, proc0, main0) = (std::time::Instant::now(), bough::sys::perf::snap().instrs, bough::sys::perf::thread_instructions());
    let raster = std::env::var("RASTER_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    let name = std::env::args().nth(1).unwrap_or_else(|| "hover-storm".into());
    match name.as_str() {
        "first-pixel" => gallery::first_pixel::<gallery::GalleryL>(entry, proc0, main0, true, raster),
        "first-pixel-serial" => gallery::first_pixel::<gallery::GalleryL>(entry, proc0, main0, false, 0),
        "first-pixel-dump" => gallery::first_pixel::<gallery::Gallery>(entry, proc0, main0, true, raster),
        "idle" => idle(),
        "hover-storm" => hover_storm(),
        "scroll" => scroll(),
        "cold-start" => gallery::cold_start::<gallery::GalleryL>(gpu()),
        "kitchen-sink" => gallery::kitchen::<gallery::GalleryL>(gpu()),
        "app-shell" => shell::run::<shell::ShellL>(gpu()),
        "thread-resize" => thread::run(gpu()),
        "cold-start-dump" => gallery::cold_start::<gallery::Gallery>(gpu()),
        "kitchen-sink-dump" => gallery::kitchen::<gallery::Gallery>(gpu()),
        "app-shell-dump" => shell::run::<shell::Shell>(gpu()),
        other => panic!("unknown scenario {other}"),
    }
}
