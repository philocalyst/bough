use bough_guibench::{
    cli::{self, Scenario, Variant},
    driver,
    model::{Coarse, Fine, Rows},
    scenarios::{Search, Spinner, Virtual},
};
fn main() {
    let args = std::env::args().skip(1).map(|x| {
        if x == "resize" {
            "rows".to_owned()
        } else if x == "--scenario=resize" {
            "--scenario=rows".to_owned()
        } else {
            x
        }
    });
    let s = cli::require(cli::parse(args).unwrap(), &[Variant::Fine, Variant::Coarse]).unwrap();
    match s {
        Scenario::Rows { count } => driver::window(Rows::new(count), s.title()),
        Scenario::Table {
            count,
            variant,
            ticker,
        } => {
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
