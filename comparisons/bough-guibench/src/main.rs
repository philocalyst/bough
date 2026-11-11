use bough_guibench::{
    cli::{self, Scenario, Variant},
    driver,
    model::{Coarse, Fine, Rows},
};
fn main() {
    let s = cli::from_env("bough-bench", &[Variant::Fine, Variant::Coarse]);
    match s {
        Scenario::Rows { count } => driver::window(Rows::new(count), s.title()),
        Scenario::Table {
            count,
            variant,
            ticker: None,
        } => match variant {
            Variant::Coarse => driver::window(Coarse::new(count, false), s.title()),
            _ => driver::window(Fine::new(count, false), s.title()),
        },
        _ => {
            eprintln!("scenario not implemented in rows/table stage: {s:?}");
            std::process::exit(2);
        }
    }
}
