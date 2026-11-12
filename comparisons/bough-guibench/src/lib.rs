//! Held-out gui-bench port. Application modules use the prelude; harness escapes live in driver.
pub mod cli;
pub mod driver;
pub mod model;
pub mod scenarios;
pub mod spec;

use crate::{
    cli::{Scenario, Variant},
    model::{Coarse, Fine, Rows},
    scenarios::{Search, Spinner, Virtual},
};

/// The gui-bench app's entry (`bough-bench`, the binary gui-bench's runner launches): gui-bench's command line,
/// then the scenario's window through bough's portable host (AppKit on macOS, Wayland on Linux).
pub fn run_cli() -> ! {
    // `resize` runs the rows scenario (the runner passes `rows`; older runners passed `resize`)
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
    eprintln!("bough-bench: {s:?}");
    match s {
        Scenario::Rows { count } => driver::window(Rows::new(count), s.title()),
        Scenario::Table { count, variant, ticker } => {
            if variant == Variant::Coarse {
                let mut v = Coarse::new(count, false);
                v.timer = ticker;
                driver::window(v, s.title())
            } else {
                let mut v = Fine::new(count, false);
                v.timer = ticker;
                driver::window(v, s.title())
            }
        }
        Scenario::Search { count, filter } => driver::window(Search::new(count, filter), s.title()),
        Scenario::Spinner { count } => driver::window(Spinner::new(count), s.title()),
        Scenario::Virtual { count } => driver::window(Virtual::new(count), s.title()),
    }
}
