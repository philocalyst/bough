//! `bough-bench`: the binary gui-bench's runner launches.
#[cfg(target_os = "linux")]
fn main() {
    use bough_guibench_x::cli::{self, Scenario, Variant};
    use bough_guibench_x::scene::{Model, Scene};
    use bough_guibench_x::{rows, search, spec, spinner, table};
    use bough_host::run::{Window, run};

    // The model is made by the loop, after the connection's first write: the compositor lists its globals
    // while the application builds its state.
    fn go<V: Model>(v: impl FnOnce() -> V, title: &str) -> ! {
        let seconds = std::env::var("GUIBENCH_SECONDS").ok().and_then(|s| s.parse().ok());
        let win = Window { title, app_id: "gui-bench.bough", w: spec::WINDOW.0 as i32, h: spec::WINDOW.1 as i32, seconds };
        match run(win, || Scene::new(v())) {
            Ok(_) => std::process::exit(0),
            Err(e) => {
                eprintln!("bough-bench: {e}");
                std::process::exit(1)
            }
        }
    }
    let args = std::env::args().skip(1).map(|x| match x.as_str() {
        "resize" => "rows".to_owned(),
        "--scenario=resize" => "--scenario=rows".to_owned(),
        _ => x,
    });
    let s = match cli::parse(args).and_then(|s| cli::require(s, &[Variant::Fine, Variant::Coarse])) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bough-bench: {e}");
            eprintln!("usage: bough-bench {}", cli::USAGE);
            std::process::exit(2);
        }
    };
    match s {
        Scenario::Rows { count } | Scenario::Virtual { count } => go(|| rows::Rows::new(count), s.title()),
        Scenario::Table { count, variant, ticker } => {
            if variant == Variant::Coarse {
                go(|| table::Coarse::new(count, ticker), s.title())
            } else {
                go(|| table::Table::new(count, ticker), s.title())
            }
        }
        Scenario::Search { count, filter } => go(|| search::Search::new(count, filter), s.title()),
        Scenario::Spinner { count } => go(|| spinner::Spinner::new(count), s.title()),
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("bough-bench: the window host of this build is Linux/Wayland only; the scenarios and their gates run headless (cargo test -p bough-guibench-x)");
    std::process::exit(2);
}
