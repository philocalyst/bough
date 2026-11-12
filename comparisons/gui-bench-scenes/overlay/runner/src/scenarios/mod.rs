//! The scenarios: what each one asks the app to show, at which refresh rates and sizes it runs,
//! which variants a framework has for it, and what it measures. `Cell` names one point of the
//! matrix and its directory in the run.

mod measure;

use serde::Serialize;

use crate::frameworks::Framework;
use crate::layout::Page;

/// One scenario of the matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// N real rows in one scroll container: hover and scroll.
    Rows,
    /// A keyed table and its operations: select, update, swap, remove, insert, replace, append,
    /// clear, and a churn of replacements.
    Table,
    /// A table whose rows and header a timer updates every 16 ms.
    Ticker,
    /// A turning spinner above static rows.
    Spinner,
    /// A focused text input above static rows.
    Typing,
    /// A focused text input that filters the rows.
    Search,
    /// The rows of `Rows` in the framework's virtualized list.
    Virtual,
    /// The rows app under a resize storm and a scale switch.
    Resize,
    /// bough's standing workloads (`apps/shared/scenes.rs`): a spreadsheet of N x 10 cells.
    Sheet,
    /// bough's standing workloads: a design canvas of N layered nodes in 20 frames.
    Canvas,
}

impl Kind {
    /// Every scenario in run order.
    pub const ALL: [Kind; 10] = [
        Kind::Rows,
        Kind::Table,
        Kind::Ticker,
        Kind::Spinner,
        Kind::Typing,
        Kind::Search,
        Kind::Virtual,
        Kind::Resize,
        Kind::Sheet,
        Kind::Canvas,
    ];

    /// Is this one of the heavy scenes (`crate::scenes`)?
    pub fn is_scene(self) -> bool {
        matches!(self, Kind::Sheet | Kind::Canvas)
    }

    /// Returns the points and colours that the layout check reads.
    pub fn checks(self, rows: u32) -> Vec<(i32, i32, u32)> {
        if self.is_scene() { crate::scenes::checks(self) } else { self.page().checks(rows) }
    }

    /// Returns the name in results and on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Rows => "rows",
            Kind::Table => "table",
            Kind::Ticker => "ticker",
            Kind::Spinner => "spinner",
            Kind::Typing => "typing",
            Kind::Search => "search",
            Kind::Virtual => "virtual",
            Kind::Resize => "resize",
            Kind::Sheet => "sheet",
            Kind::Canvas => "canvas",
        }
    }

    /// Returns the scenario with a name.
    pub fn by_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// Returns the scenario the app shows.
    fn app_scenario(self) -> &'static str {
        match self {
            Kind::Resize => "rows",
            other => other.name(),
        }
    }

    /// Returns where the rows start.
    pub fn page(self) -> Page {
        match self {
            Kind::Rows | Kind::Virtual | Kind::Resize => Page::Rows,
            _ => Page::Header,
        }
    }

    /// Returns true when the app draws all the time, so its window never settles.
    pub fn animates(self) -> bool {
        matches!(self, Kind::Ticker | Kind::Spinner)
    }

    /// Returns the default refresh rates.
    pub fn rates(self, quick: bool) -> Vec<f64> {
        match self {
            _ if quick => vec![60.0],
            Kind::Rows | Kind::Ticker | Kind::Spinner | Kind::Virtual => vec![60.0, 144.0],
            Kind::Table | Kind::Typing | Kind::Search | Kind::Resize | Kind::Sheet | Kind::Canvas => {
                vec![60.0]
            }
        }
    }

    /// Returns the default row counts.
    pub fn row_counts(self, quick: bool) -> Vec<u32> {
        match (self, quick) {
            (Kind::Rows, false) => vec![10, 100, 300, 1000, 3000, 10000],
            (Kind::Rows, true) => vec![100, 3000],
            (Kind::Table | Kind::Ticker | Kind::Typing | Kind::Search, false) => vec![1000, 10000],
            (Kind::Table | Kind::Ticker | Kind::Typing | Kind::Search, true) => vec![1000],
            (Kind::Spinner, _) => vec![0, 10000],
            (Kind::Virtual, false) => vec![100_000, 1_000_000],
            (Kind::Virtual, true) => vec![100_000],
            (Kind::Resize, false) => vec![100, 10000],
            (Kind::Resize, true) => vec![1000],
            (Kind::Sheet, false) => vec![1000, 10000],
            (Kind::Sheet, true) => vec![1000],
            (Kind::Canvas, false) => vec![1000, 5000],
            (Kind::Canvas, true) => vec![1000],
        }
    }

    /// Returns the ticker row updates per tick, or one `None` for other scenarios.
    pub fn update_counts(self) -> Vec<Option<u32>> {
        match self {
            Kind::Ticker => vec![Some(1), Some(100)],
            _ => vec![None],
        }
    }

    /// Returns the variants of the scenario for a framework.
    pub fn variants(self, fw: &Framework) -> Vec<&'static str> {
        match self {
            Kind::Table | Kind::Ticker => fw.variants.to_vec(),
            _ => vec!["default"],
        }
    }

    /// Runs the measurements of the scenario.
    pub fn measure(self, run: &mut crate::cell::CellRun) {
        if self.is_scene() {
            crate::scenes::measure(self, run);
        } else {
            measure::run(self, run);
        }
    }
}

/// The repeat counts of the measurements.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Effort {
    /// Hover repeats per iteration.
    pub hover_repeats: u32,
    /// Repeats per iteration of every table operation and of the search probe.
    pub op_repeats: u32,
    /// Measured iterations of every measurement.
    pub iterations: u32,
    /// Replacements in the churn measurement.
    pub churn_cycles: u32,
}

/// One cell of the matrix.
#[derive(Debug, Clone, Serialize)]
pub struct Cell {
    /// The scenario.
    #[serde(rename = "scenario")]
    pub kind: Kind,
    /// The framework name.
    pub framework: String,
    /// The variant, `default` for scenarios without variants.
    pub variant: String,
    /// The refresh rate of the output in Hz.
    pub hz: f64,
    /// The number of rows the app renders.
    pub rows: u32,
    /// The ticker row updates per tick.
    pub updates: Option<u32>,
}

/// The ticker interval in milliseconds.
pub const TICK_MS: u32 = 16;

impl Cell {
    /// Returns the directory of the cell in the run: `<scenario>/<framework>/<variant>/<hz>hz/<size>`.
    /// The size is the row count, with `-u<updates>` for the ticker.
    pub fn dir(&self) -> String {
        let size = match self.updates {
            Some(u) => format!("{}-u{u}", self.rows),
            None => self.rows.to_string(),
        };
        format!(
            "{}/{}/{}/{}hz/{size}",
            self.kind.name(),
            self.framework,
            self.variant,
            self.hz
        )
    }

    /// Returns the arguments of the app.
    pub fn app_args(&self) -> Vec<String> {
        let mut args = vec![
            "--scenario".to_owned(),
            self.kind.app_scenario().to_owned(),
            "--rows".to_owned(),
            self.rows.to_string(),
        ];
        if matches!(self.kind, Kind::Table | Kind::Ticker) {
            args.extend(["--variant".to_owned(), self.variant.clone()]);
        }
        if let Some(updates) = self.updates {
            args.extend([
                "--updates".to_owned(),
                updates.to_string(),
                "--interval-ms".to_owned(),
                TICK_MS.to_string(),
            ]);
        }
        args
    }
}
