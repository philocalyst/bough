//! One matrix cell: a fresh session at the cell's refresh rate, the app with the cell's scenario,
//! variant and size, and the measurements of that scenario. Every cell writes `cell.json`, the app
//! log, a screenshot and one JSON report per measurement into its own directory. A failed
//! measurement records its error and the cell goes on.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use fakeland::scenario::Position;
use fakeland::{
    App, CaptureOptions, CaptureTarget, Fakeland, LaunchSpec, OutputConfig, SamplingConfig,
    Session, WindowRef,
};
use fakeland_bench::AnalysisInput;
use fakeland_bench::Analyzer;
use fakeland_bench::analyzers::Startup;
use fakeland_types::{Hz, Nanos};
use serde::Serialize;

use crate::frameworks::Framework;
use crate::layout::OUTPUT;
use crate::scenarios::{Cell, Effort};

/// The sample interval of resource sampling.
const SAMPLE_INTERVAL: Nanos = Nanos::from_millis(50);
/// The frames an animating app shows before the warmup time starts.
const FRAMES_AT_START: u32 = 2;
/// The time an animating app runs after its first frames, before its measurements start.
const ANIMATION_WARMUP: std::time::Duration = std::time::Duration::from_secs(1);
/// The largest difference per color channel that the layout check accepts.
const LAYOUT_TOLERANCE: i32 = 6;

/// The outcome of one cell, as `cell.json` and the run index hold it.
#[derive(Debug, Clone, Serialize)]
pub struct CellOutcome {
    /// The cell.
    pub cell: Cell,
    /// The cell directory relative to the run directory.
    pub dir: String,
    /// The app arguments.
    pub args: Vec<String>,
    /// The measurements that wrote a report.
    pub done: Vec<String>,
    /// The measurements that failed, with their errors.
    pub errors: Vec<(String, String)>,
    /// The measurements the cell skips, with the reason.
    pub skipped: Vec<(String, String)>,
}

/// A running cell: its session, its app and where its files go.
pub struct CellRun<'a> {
    pub fw: &'a Framework,
    pub cell: Cell,
    pub effort: Effort,
    pub session: Session,
    pub app: App,
    pub out: PathBuf,
    pub outcome: CellOutcome,
}

impl CellRun<'_> {
    /// Runs one measurement and records whether it wrote its report.
    pub fn step(&mut self, name: &str, f: impl FnOnce(&CellRun) -> anyhow::Result<()>) {
        eprintln!("    {name}");
        match f(self) {
            Ok(()) => self.outcome.done.push(name.to_owned()),
            Err(e) => {
                eprintln!("    {name} failed: {e:#}");
                self.outcome
                    .errors
                    .push((name.to_owned(), format!("{e:#}")));
            }
        }
    }

    /// Records a measurement that the cell skips.
    pub fn skip(&mut self, name: &str, reason: &str) {
        self.outcome
            .skipped
            .push((name.to_owned(), reason.to_owned()));
    }

    /// Writes a value as pretty JSON into the cell directory.
    pub fn save(&self, file: &str, value: &impl Serialize) -> anyhow::Result<()> {
        save(&self.out.join(file), value)
    }
}

/// Runs one cell and writes its files below `run_dir`.
pub fn run(
    fw: &Framework,
    binary: &Path,
    cell: Cell,
    effort: Effort,
    run_dir: &Path,
) -> CellOutcome {
    let dir = cell.dir();
    let args = cell.app_args();
    let mut outcome = CellOutcome {
        cell: cell.clone(),
        dir: dir.clone(),
        args: args.clone(),
        done: Vec::new(),
        errors: Vec::new(),
        skipped: Vec::new(),
    };
    let out = run_dir.join(&dir);
    if let Err(e) = std::fs::create_dir_all(&out) {
        outcome.errors.push(("setup".into(), e.to_string()));
        return outcome;
    }
    let (session, app) = match start(fw, binary, &cell, &args, &out) {
        Ok(started) => started,
        Err(e) => {
            outcome.errors.push(("startup".into(), format!("{e:#}")));
            let _ = save(&out.join("cell.json"), &outcome);
            return outcome;
        }
    };
    outcome.done.push("startup".into());
    let mut run = CellRun {
        fw,
        cell: cell.clone(),
        effort,
        session,
        app,
        out,
        outcome,
    };
    match check_layout(&run) {
        Ok(()) => {
            run.outcome.done.push("layout".into());
            cell.kind.measure(&mut run);
        }
        Err(e) => {
            eprintln!("    layout check failed: {e:#}");
            run.outcome.errors.push(("layout".into(), format!("{e:#}")));
        }
    }
    let _ = std::fs::write(run.out.join("app.log"), run.app.stderr());
    if let Err(e) = run.session.blocking_shutdown() {
        run.outcome.errors.push(("shutdown".into(), e.to_string()));
    }
    let _ = run.save("cell.json", &run.outcome);
    run.outcome
}

/// Starts a session, launches the app, waits until it settles and writes the startup report.
fn start(
    fw: &Framework,
    binary: &Path,
    cell: &Cell,
    args: &[String],
    out: &Path,
) -> anyhow::Result<(Session, App)> {
    let session = Fakeland::builder()
        .output(OutputConfig::new("DP-1").mode(OUTPUT.0, OUTPUT.1, Hz(cell.hz)))
        .app_marks(fw.marks)
        .sampling(SamplingConfig::every(SAMPLE_INTERVAL))
        .start()
        .context("start the session")?;
    let app = session
        .blocking_launch(LaunchSpec::new(binary.to_string_lossy()).args(args.iter().cloned()))
        .context("launch the app")?;
    let window = app
        .blocking_wait_window(Nanos::from_secs(120))
        .with_context(|| format!("no window: {}", app.stderr()))?;
    if cell.kind.animates() {
        // The ticker and the spinner never stop drawing, so the window never settles. Wait for
        // the first frames and then a fixed time; an app that draws less often than it should
        // still gets measured, and its pacing report shows it.
        // An app that stops drawing after its first frame is a result, not a failure: its
        // pacing report shows it.
        if let Err(e) = session.blocking_wait_frames(
            WindowRef::id(window.id),
            FRAMES_AT_START,
            Nanos::from_secs(10),
        ) {
            eprintln!("    the animation shows no frames after the first: {e}");
        }
        std::thread::sleep(ANIMATION_WARMUP);
    } else {
        session
            .blocking_wait_settled(
                WindowRef::id(window.id),
                Nanos::from_millis(300),
                Nanos::from_secs(60),
            )
            .with_context(|| format!("the window never settles: {}", app.stderr()))?;
    }
    let trace = session.blocking_trace_snapshot()?;
    let startup = Startup {
        command: Some(fw.binary()),
    }
    .analyze(&AnalysisInput::new(trace));
    save(&out.join("startup.json"), &startup)?;
    Ok((session, app))
}

/// Takes a screenshot into `screenshot.png` and checks the colors at the points the layout of the
/// scenario fixes. A mismatch means the app draws another layout, and its probes would watch the
/// wrong pixels.
fn check_layout(run: &CellRun) -> anyhow::Result<()> {
    // The pointer starts at the top left corner, over the first row. It moves to the bottom of
    // the window, so the hover color stays away from the rows the check reads.
    run.session.blocking_pointer_move(Position::output(
        f64::from(OUTPUT.0 / 2),
        f64::from(OUTPUT.1 - 8),
    ))?;
    if run.cell.kind.animates() {
        std::thread::sleep(std::time::Duration::from_millis(300));
    } else {
        run.session.blocking_wait_settled(
            WindowRef::Focused,
            Nanos::from_millis(300),
            Nanos::from_secs(10),
        )?;
    }
    let shot = run.session.blocking_screenshot(
        CaptureTarget::default(),
        CaptureOptions {
            include_cursor: false,
            max_dim: None,
        },
    )?;
    std::fs::write(run.out.join("screenshot.png"), shot.image.encode_png()?)?;
    let mut wrong = Vec::new();
    for (x, y, want) in run.cell.kind.checks(run.cell.rows) {
        let got = shot.image.pixel(x as u32, y as u32).unwrap_or([0; 4]);
        let want = [(want >> 16) as u8, (want >> 8) as u8, want as u8];
        let off = (0..3).any(|i| (i32::from(got[i]) - i32::from(want[i])).abs() > LAYOUT_TOLERANCE);
        if off {
            wrong.push(format!(
                "({x}, {y}) is #{:02x}{:02x}{:02x}, not #{:02x}{:02x}{:02x}",
                got[0], got[1], got[2], want[0], want[1], want[2]
            ));
        }
    }
    if !wrong.is_empty() {
        bail!("the layout differs from the spec: {}", wrong.join("; "));
    }
    Ok(())
}

/// Writes a value as pretty JSON.
pub fn save(path: &PathBuf, value: &impl Serialize) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(value)?;
    std::fs::write(path, text).with_context(|| format!("write {}", path.display()))
}
