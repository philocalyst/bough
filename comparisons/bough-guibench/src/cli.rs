//! The command line of every app: `--scenario NAME` picks the scenario (`rows` by default) and
//! the other options set its parameters.
//!
//! - `--rows N`: the row count (100 by default).
//! - `--variant fine|coarse|shared|default`: how the table and the ticker hold their state.
//! - `--updates K`, `--interval-ms T`: the ticker updates K rows every T ms (1 and 16 by default).

#![allow(dead_code)]

use std::fmt;

/// The row count without `--rows`.
const DEFAULT_ROWS: usize = 100;
/// The ticker row updates per tick without `--updates`.
const DEFAULT_UPDATES: usize = 1;
/// The ticker interval without `--interval-ms`.
const DEFAULT_INTERVAL_MS: u64 = 16;

/// The usage line of every app.
pub const USAGE: &str = "[--scenario rows|table|ticker|spinner|typing|search|virtual] [--rows N] \
                         [--variant fine|coarse|shared|default] [--updates K] [--interval-ms T]";

/// How a stateful scenario holds and updates its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// The framework's own default way, for frameworks with one way only.
    Default,
    /// Per-row state: an update touches only the rows it changes.
    Fine,
    /// One state for the whole list: an update rebuilds or re-renders the whole list.
    Coarse,
    /// Per-row state, but every row reads one shared value, such as the selection.
    Shared,
}

impl Variant {
    /// Returns the name of the variant on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Fine => "fine",
            Self::Coarse => "coarse",
            Self::Shared => "shared",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        [Self::Default, Self::Fine, Self::Coarse, Self::Shared]
            .into_iter()
            .find(|v| v.name() == name)
    }
}

/// The ticker settings of a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticker {
    /// The rows that every tick updates.
    pub updates: usize,
    /// The time between two ticks in milliseconds.
    pub interval_ms: u64,
}

/// A scenario and its parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// One scroll container that holds `count` real rows.
    Rows {
        /// The number of rows.
        count: usize,
    },
    /// A header and a keyed table of `count` rows that the function keys change. With a ticker,
    /// a timer also updates rows and the header (the `ticker` scenario).
    Table {
        count: usize,
        variant: Variant,
        ticker: Option<Ticker>,
    },
    /// A header with a turning spinner above `count` static rows.
    Spinner { count: usize },
    /// A header with a focused text input above `count` rows. With `filter`, every edit shows
    /// only the rows whose label holds the text (the `search` scenario); without, the rows stay
    /// (the `typing` scenario).
    Search { count: usize, filter: bool },
    /// The rows of `Rows` in the framework's virtualized list: only the visible rows exist.
    Virtual { count: usize },
}

impl Scenario {
    /// Returns the variant of the scenario; scenarios without variants have `Default`.
    pub fn variant(&self) -> Variant {
        match self {
            Self::Table { variant, .. } => *variant,
            _ => Variant::Default,
        }
    }

    /// Returns the window title of the scenario.
    pub fn title(&self) -> &'static str {
        match self {
            Self::Rows { .. } => "gui-bench rows",
            Self::Table { ticker: None, .. } => "gui-bench table",
            Self::Table { .. } => "gui-bench ticker",
            Self::Spinner { .. } => "gui-bench spinner",
            Self::Search { filter: true, .. } => "gui-bench search",
            Self::Search { .. } => "gui-bench typing",
            Self::Virtual { .. } => "gui-bench virtual",
        }
    }
}

/// A command line that names no valid scenario.
#[derive(Debug, PartialEq, Eq)]
pub enum CliError {
    /// An option is the last argument and has no value.
    MissingValue(&'static str),
    /// The value of a numeric option is no whole number.
    BadNumber(&'static str, String),
    /// A scenario name the program does not know.
    UnknownScenario(String),
    /// A variant name the program does not know.
    UnknownVariant(String),
    /// A variant that the app does not implement for the scenario.
    Unsupported(Variant),
    /// An argument the program does not know.
    Unknown(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue(option) => write!(f, "{option} needs a value"),
            Self::BadNumber(option, value) => {
                write!(f, "{option} takes a whole number, got `{value}`")
            }
            Self::UnknownScenario(name) => write!(f, "unknown scenario `{name}`"),
            Self::UnknownVariant(name) => write!(f, "unknown variant `{name}`"),
            Self::Unsupported(variant) => {
                write!(f, "this app has no `{}` variant here", variant.name())
            }
            Self::Unknown(argument) => write!(f, "unknown argument `{argument}`"),
        }
    }
}

/// Reads the scenario from the arguments after the program name.
pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Scenario, CliError> {
    let mut scenario = String::from("rows");
    let mut count = DEFAULT_ROWS;
    let mut variant = Variant::Default;
    let mut updates = DEFAULT_UPDATES;
    let mut interval_ms = DEFAULT_INTERVAL_MS;
    while let Some(argument) = args.next() {
        let (option, inline) = match argument.split_once('=') {
            Some((option, value)) => (option.to_owned(), Some(value.to_owned())),
            None => (argument.clone(), None),
        };
        let mut value = |name: &'static str| {
            inline
                .clone()
                .or_else(|| args.next())
                .ok_or(CliError::MissingValue(name))
        };
        match option.as_str() {
            "--scenario" => scenario = value("--scenario")?,
            "--rows" => count = number("--rows", value("--rows")?)?,
            "--updates" => updates = number("--updates", value("--updates")?)?,
            "--interval-ms" => interval_ms = number("--interval-ms", value("--interval-ms")?)?,
            "--variant" => {
                let name = value("--variant")?;
                variant = Variant::parse(&name).ok_or(CliError::UnknownVariant(name))?;
            }
            _ => return Err(CliError::Unknown(argument)),
        }
    }
    let ticker = Ticker {
        updates,
        interval_ms,
    };
    match scenario.as_str() {
        "rows" => Ok(Scenario::Rows { count }),
        "table" => Ok(Scenario::Table {
            count,
            variant,
            ticker: None,
        }),
        "ticker" => Ok(Scenario::Table {
            count,
            variant,
            ticker: Some(ticker),
        }),
        "spinner" => Ok(Scenario::Spinner { count }),
        "typing" => Ok(Scenario::Search {
            count,
            filter: false,
        }),
        "search" => Ok(Scenario::Search {
            count,
            filter: true,
        }),
        "virtual" => Ok(Scenario::Virtual { count }),
        _ => Err(CliError::UnknownScenario(scenario)),
    }
}

/// Returns the scenario when its variant is one of `supported`. `Default` stands for the first
/// supported variant.
pub fn require(scenario: Scenario, supported: &[Variant]) -> Result<Scenario, CliError> {
    let Scenario::Table {
        count,
        variant,
        ticker,
    } = scenario
    else {
        return Ok(scenario);
    };
    let variant = match variant {
        Variant::Default => supported.first().copied().unwrap_or(Variant::Default),
        v if supported.contains(&v) => v,
        v => return Err(CliError::Unsupported(v)),
    };
    Ok(Scenario::Table {
        count,
        variant,
        ticker,
    })
}

/// Parses the process arguments, checks the variant against `supported`, and exits with status 2
/// and the usage on error. `name` is the program name in messages.
pub fn from_env(name: &str, supported: &[Variant]) -> Scenario {
    match parse(std::env::args().skip(1)).and_then(|s| require(s, supported)) {
        Ok(scenario) => {
            eprintln!("{name}: {scenario:?}");
            scenario
        }
        Err(error) => {
            eprintln!("{name}: {error}");
            eprintln!("usage: {name} {USAGE}");
            std::process::exit(2);
        }
    }
}

fn number<T: std::str::FromStr>(option: &'static str, text: String) -> Result<T, CliError> {
    text.parse().map_err(|_| CliError::BadNumber(option, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Scenario, CliError> {
        parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn no_arguments_give_the_default_rows() {
        assert_eq!(
            run(&[]),
            Ok(Scenario::Rows {
                count: DEFAULT_ROWS
            })
        );
    }

    #[test]
    fn both_forms_set_the_values() {
        assert_eq!(
            run(&["--scenario", "rows", "--rows", "10000"]),
            Ok(Scenario::Rows { count: 10000 })
        );
        assert_eq!(
            run(&["--scenario=rows", "--rows=10"]),
            Ok(Scenario::Rows { count: 10 })
        );
    }

    #[test]
    fn ticker_is_a_table_with_a_ticker() {
        assert_eq!(
            run(&[
                "--scenario",
                "ticker",
                "--rows",
                "1000",
                "--variant",
                "coarse",
                "--updates",
                "100",
                "--interval-ms",
                "8"
            ]),
            Ok(Scenario::Table {
                count: 1000,
                variant: Variant::Coarse,
                ticker: Some(Ticker {
                    updates: 100,
                    interval_ms: 8
                }),
            })
        );
    }

    #[test]
    fn typing_and_search_differ_in_the_filter() {
        assert_eq!(
            run(&["--scenario", "typing"]),
            Ok(Scenario::Search {
                count: DEFAULT_ROWS,
                filter: false
            })
        );
        assert_eq!(
            run(&["--scenario", "search"]),
            Ok(Scenario::Search {
                count: DEFAULT_ROWS,
                filter: true
            })
        );
    }

    #[test]
    fn require_resolves_and_checks_the_variant() {
        let table = run(&["--scenario", "table"]).unwrap();
        assert_eq!(
            require(table, &[Variant::Fine, Variant::Coarse]).map(|s| s.variant()),
            Ok(Variant::Fine)
        );
        let shared = run(&["--scenario", "table", "--variant", "shared"]).unwrap();
        assert_eq!(
            require(shared, &[Variant::Fine, Variant::Coarse]),
            Err(CliError::Unsupported(Variant::Shared))
        );
        let rows = run(&["--variant", "shared"]).unwrap();
        assert_eq!(require(rows, &[Variant::Fine]), Ok(rows));
    }

    #[test]
    fn bad_input_is_an_error() {
        assert_eq!(run(&["--rows"]), Err(CliError::MissingValue("--rows")));
        assert!(matches!(
            run(&["--rows", "x"]),
            Err(CliError::BadNumber("--rows", _))
        ));
        assert!(matches!(
            run(&["--scenario", "menu"]),
            Err(CliError::UnknownScenario(_))
        ));
        assert!(matches!(
            run(&["--variant", "medium"]),
            Err(CliError::UnknownVariant(_))
        ));
        assert!(matches!(run(&["-v"]), Err(CliError::Unknown(_))));
    }
}
