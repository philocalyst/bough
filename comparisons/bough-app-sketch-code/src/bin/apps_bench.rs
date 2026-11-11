//! zgx's `apps_bench` (`crates/zgx-script/examples/apps_bench.rs` + `support/apps.rs`) for its Sketch and Code
//! apps on bough: the same windows, files and interactions, each followed by its frame, `REPS` (20) measured
//! after two warm-ups, printed as zgx prints them (`name: 1.23 ms, 0.45 M instructions`).
//!
//! `APP=sketch|code` (both if unset), `REPS`, `SHAPES` (500), `LINES` (100,000), `RENDER=0` (no GPU: the frame
//! ends at the tile list; zgx's RENDER=0 draws nothing either), `ONLY=` / `SKIP=` as zgx's, `FULLSCAN=1`
//! (find-all reads the whole text for every new query, as zgx's does: the refinement of kept searches off).
use bough::raw::*;
use bough_app_sketch_code::harness::{finish, measure};
use bough_app_sketch_code::{code, env_usize, sketch};

fn frames<V: View>(app: &mut App<V>, n: usize) {
    for _ in 0..n {
        app.frame();
    }
}

fn run_code(render: bool, reps: usize) {
    let lines = env_usize("LINES", 100_000);
    // a name of one length whatever the process's id (zgx's harness)
    let path = std::env::temp_dir().join(format!("zgx-apps-bench-{:010}.rs", std::process::id()));
    std::fs::write(&path, code::source(lines)).unwrap();
    let mut app = code::mount(render, 0);
    app.owner.search.refine = std::env::var("FULLSCAN").map_or(true, |v| v != "1");
    frames(&mut app, 3);
    app.turn(|c| c.open(&path));
    frames(&mut app, 3);
    let editor = code::editor_handle(&app);
    let at = code::editor_point(&app.owner);
    code::click(&mut app, at);
    let middle = app.owner.doc.len() / 2;
    app.turn(|c| c.set_caret(middle));
    frames(&mut app, 2);
    measure(
        &mut app,
        "code: keystroke",
        reps,
        |app, k| {
            app.key_down(Key::Char(if k % 2 == 0 { 'x' } else { 'y' }), Mods::default());
            app.frame();
        },
        finish,
    );
    measure(
        &mut app,
        "code: scroll a page",
        reps,
        |app, k| {
            let dy = if (k / 10) % 2 == 0 { 600.0 } else { -600.0 };
            assert!(code::wheel(app, editor, at, dy), "the editor is under the wheel");
            app.frame();
        },
        finish,
    );
    if std::env::var("SPLIT").is_ok() {
        // where a page scroll goes: the handler (rows projected, texts assembled), the settle (the texts'
        // rings: lines matched and shaped, damage marked), the frame
        let (mut h, mut s, mut f) = (0u64, 0u64, 0u64);
        for k in 0..20 {
            let dy = if (k / 10) % 2 == 0 { 600.0 } else { -600.0 };
            let i0 = bough_app_sketch_code::instructions();
            app.write(|c| c.on_wheel(0.0, dy));
            let i1 = bough_app_sketch_code::instructions();
            app.flush();
            let i2 = bough_app_sketch_code::instructions();
            app.frame();
            let i3 = bough_app_sketch_code::instructions();
            (h, s, f) = (h + i1 - i0, s + i2 - i1, f + i3 - i2);
        }
        eprintln!("scroll split: handler {:.3} M, settle {:.3} M, frame {:.3} M", h as f64 / 2e7, s as f64 / 2e7, f as f64 / 2e7);
    }
    app.turn(|c| c.open_find());
    code::click(&mut app, code::FIND_POINT);
    frames(&mut app, 2);
    code::type_keys(&mut app, "Overflo");
    frames(&mut app, 2);
    measure(
        &mut app,
        "code: find-all keystroke",
        reps,
        |app, k| {
            if k % 2 == 0 {
                app.key_down(Key::Char('w'), Mods::default());
            } else {
                app.key_down(Key::Backspace, Mods::default());
            }
            app.frame();
        },
        finish,
    );
    measure(
        &mut app,
        "code: find-all, a new letter",
        reps,
        |app, k| {
            app.key_down(Key::Char(char::from(b'a' + (k % 26) as u8)), Mods::default());
            app.frame();
        },
        finish,
    );
    if std::env::var("VERBOSE").is_ok() {
        eprintln!("count {:?} query {:?} scanned {} B", *app.owner.count, *app.owner.query, app.owner.search.scanned);
    }
    let _ = std::fs::remove_file(&path);
}

fn main() {
    let render = std::env::var("RENDER").map_or(true, |v| v != "0");
    let reps = env_usize("REPS", 20);
    let only = std::env::var("APP").ok();
    if only.as_deref().is_none_or(|a| a == "sketch") {
        sketch::bench(render, reps);
    }
    if only.as_deref().is_none_or(|a| a == "code") {
        run_code(render, reps);
    }
}
