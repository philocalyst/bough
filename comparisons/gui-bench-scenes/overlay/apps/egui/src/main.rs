//! egui under gui-bench: one window per run that shows the scenario the command line names.

#![forbid(unsafe_code)]

#[path = "../../shared/cli.rs"]
mod cli;
mod font;
mod page;
mod rows;
mod scenes;
mod scroll;
mod search;
#[path = "../../shared/scenes.rs"]
mod heavy;
#[path = "../../shared/spec.rs"]
mod spec;
mod spinner;
mod table;
mod virtual_rows;
mod wheel;
mod window;

use crate::cli::{Scenario, Variant};

const NAME: &str = "egui-bench";

fn main() -> eframe::Result {
    // the heavy scenes (sheet, canvas) come before gui-bench's own command line
    if let Some(scene) = heavy::from_env() {
        return scenes::run(scene);
    }
    // egui is immediate mode, so the table has one variant: the immediate one.
    let scenario = cli::from_env(NAME, &[Variant::Default]);
    let title = scenario.title();
    match scenario {
        Scenario::Rows { count } => rows::run(title, count),
        Scenario::Table { count, ticker, .. } => table::run(title, count, ticker),
        Scenario::Spinner { count } => spinner::run(title, count),
        Scenario::Search { count, filter } => search::run(title, count, filter),
        Scenario::Virtual { count } => virtual_rows::run(title, count),
    }
}
