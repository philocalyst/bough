use bough_guibench::{
    cli::{self, Scenario, Variant},
    driver,
    model::{Coarse, Fine, Rows},
    scenarios::{Search, Spinner, Virtual},
};
fn main() {
    let mut args = std::env::args().skip(1);
    let mut shared = Vec::new();
    let mut op = "mount".to_owned();
    let mut repeats = 0;
    let mut render = false;
    let mut gate = false;
    let mut ring = 4;
    let mut mutant = false;
    let mut plain = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--op" => op = args.next().expect("--op value"),
            "--repeats" => repeats = args.next().expect("--repeats value").parse().unwrap(),
            "--render" => render = true,
            "--gate" => gate = true,
            "--ring" => ring = args.next().expect("--ring value").parse().unwrap(),
            "--mutant" => mutant = true,
            "--plain" => plain = true,
            _ => shared.push(a),
        }
    }
    let s = cli::require(
        cli::parse(
            shared
                .into_iter()
                .map(|s| if s == "resize" { "rows".to_owned() } else { s }),
        )
        .unwrap(),
        &[Variant::Fine, Variant::Coarse],
    )
    .unwrap();
    assert!((1..=4).contains(&ring));
    match s {
        Scenario::Rows { count } => {
            driver::headless(Rows::new(count), &op, repeats, render, gate, ring)
        }
        Scenario::Table {
            count,
            variant,
            ticker,
        } => match variant {
            Variant::Coarse => {
                let mut v = Coarse::new(count, mutant);
                v.plain = plain;
                v.timer = ticker;
                driver::headless(v, &op, repeats, render, gate, ring)
            }
            _ => {
                let mut v = Fine::new(count, mutant);
                v.plain = plain;
                v.timer = ticker;
                driver::headless(v, &op, repeats, render, gate, ring)
            }
        },
        Scenario::Search { count, filter } => {
            driver::headless(Search::new(count, filter), &op, repeats, render, gate, ring)
        }
        Scenario::Spinner { count } => {
            driver::headless(Spinner::new(count), &op, repeats, render, gate, ring)
        }
        Scenario::Virtual { count } => {
            driver::headless(Virtual::new(count), &op, repeats, render, gate, ring)
        }
    }
}
