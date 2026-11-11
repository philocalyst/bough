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
    let mut mounted = false;
    let mut blocks = 0;
    let mut scale = 1.0;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--op" => op = args.next().expect("--op value"),
            "--repeats" => repeats = args.next().expect("--repeats value").parse().unwrap(),
            "--render" => render = true,
            "--gate" => gate = true,
            "--ring" => ring = args.next().expect("--ring value").parse().unwrap(),
            "--mutant" => mutant = true,
            "--plain" => plain = true,
            "--mounted" => mounted = true,
            "--blocks" => blocks = args.next().expect("--blocks value").parse().unwrap(),
            "--scale" => scale = args.next().expect("--scale value").parse().unwrap(),
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
    let o = driver::Opts {
        repeats,
        render,
        gate,
        ring,
        blocks,
        scale,
    };
    match s {
        Scenario::Rows { count } => {
            driver::headless(Rows::new(count), &op, o)
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
                driver::headless(v, &op, o)
            }
            _ => {
                let mut v = Fine::new(count, mutant);
                v.plain = plain;
                v.mounted = mounted;
                v.timer = ticker;
                driver::headless(v, &op, o)
            }
        },
        Scenario::Search { count, filter } => {
            let mut v = Search::new(count, filter);
            v.plain = plain;
            driver::headless(v, &op, o)
        }
        Scenario::Spinner { count } => {
            driver::headless(Spinner::new(count), &op, o)
        }
        Scenario::Virtual { count } => {
            driver::headless(Virtual::new(count), &op, o)
        }
    }
}
