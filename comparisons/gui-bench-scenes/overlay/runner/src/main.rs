//! Runs the GUI benchmark matrix under fakeland. For every scenario, framework, variant, refresh
//! rate and size, the runner starts a headless session, launches the framework's app and runs the
//! scenario's measurements. Results go to `results/<run>/<scenario>/<framework>/<variant>/<hz>hz/<size>/`
//! as JSON, with `run.json` as the index; `results/latest` links to the newest run.

mod cell;
mod frameworks;
mod layout;
mod limits;
mod phases;
mod scenarios;
mod scenes;
mod steps;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use clap::Parser;
use serde_json::json;

use crate::frameworks::Framework;
use crate::scenarios::{Cell, Effort, Kind};

/// The format of `run.json` and the run directory.
const FORMAT: u32 = 2;

/// Runs the GUI benchmark matrix under fakeland.
#[derive(Debug, Parser)]
struct Args {
    /// The scenarios, comma separated: rows, table, ticker, spinner, typing, search, virtual,
    /// resize.
    #[arg(
        long,
        value_delimiter = ',',
        default_value = "rows,table,ticker,spinner,typing,search,virtual,resize"
    )]
    scenarios: Vec<String>,
    /// The frameworks, comma separated.
    #[arg(
        long,
        value_delimiter = ',',
        default_value = "zgui,gpui-kit,gpui-kit-fast,freya,freya-056a5b4,vizia,floem,iced,egui,dioxus,dioxus-skia,bough"
    )]
    frameworks: Vec<String>,
    /// Only these variants of the table and ticker scenarios, comma separated. All by default.
    #[arg(long, value_delimiter = ',')]
    variants: Vec<String>,
    /// The row counts, comma separated, for every selected scenario. Each scenario has its own
    /// default.
    #[arg(long, value_delimiter = ',')]
    rows: Vec<u32>,
    /// The output refresh rates in Hz, comma separated. Each scenario has its own default.
    #[arg(long, value_delimiter = ',')]
    hz: Vec<f64>,
    /// Hover repeats per iteration.
    #[arg(long, default_value_t = 12)]
    repeats: u32,
    /// Repeats per iteration of the table operations and the search probe.
    #[arg(long, default_value_t = 8)]
    op_repeats: u32,
    /// Measured iterations of every measurement.
    #[arg(long, default_value_t = 3)]
    iterations: u32,
    /// Replacements in the table churn.
    #[arg(long, default_value_t = 40)]
    churn: u32,
    /// A small matrix for a quick check: the smallest sizes at 60 Hz with fewer repeats.
    #[arg(long)]
    quick: bool,
    /// Continue a run in `--out`: cells whose `cell.json` has no errors keep their results, the
    /// other cells run again.
    #[arg(long, requires = "out")]
    resume: bool,
    /// The run directory. The default is `results/run-<unix time>`.
    #[arg(long)]
    out: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let mut args = Args::parse();
    limits::raise_open_files();
    if args.quick {
        args.repeats = 6;
        args.op_repeats = 4;
        args.iterations = 2;
        args.churn = 10;
    }
    let root = frameworks::root();
    let mut kinds = Vec::new();
    for name in &args.scenarios {
        kinds.push(Kind::by_name(name).with_context(|| format!("unknown scenario {name}"))?);
    }
    let mut selected = Vec::new();
    for name in &args.frameworks {
        let fw = Framework::by_name(name).with_context(|| format!("unknown framework {name}"))?;
        let binary = fw.binary_path(&root);
        if !binary.exists() {
            bail!("{} is missing; run `just build`", binary.display());
        }
        selected.push((fw, binary));
    }
    let effort = Effort {
        hover_repeats: args.repeats,
        op_repeats: args.op_repeats,
        iterations: args.iterations,
        churn_cycles: args.churn,
    };
    let cells = matrix(&args, &kinds, &selected);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let run_dir = args
        .out
        .clone()
        .unwrap_or_else(|| root.join("results").join(format!("run-{stamp}")));
    std::fs::create_dir_all(&run_dir)?;
    link_latest(&root, &run_dir);

    let total = cells.len();
    let mut outcomes = Vec::new();
    for (index, (fw, binary, cell)) in cells.into_iter().enumerate() {
        let dir = run_dir.join(cell.dir());
        if args.resume {
            if let Some(done) = finished(&dir) {
                eprintln!("[{}/{total}] {} (kept)", index + 1, cell.dir());
                outcomes.push(done);
                continue;
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
        eprintln!("[{}/{total}] {}", index + 1, cell.dir());
        let outcome = cell::run(fw, binary, cell, effort, &run_dir);
        outcomes.push(serde_json::to_value(outcome)?);
        write_index(&run_dir, &args, effort, stamp, &outcomes)?;
    }
    write_index(&run_dir, &args, effort, stamp, &outcomes)?;
    let failed: usize = outcomes
        .iter()
        .filter_map(|c| c["errors"].as_array().map(Vec::len))
        .sum();
    eprintln!(
        "results: {} ({failed} failed measurements)",
        run_dir.display()
    );
    Ok(())
}

/// Returns every cell of the selected scenarios and frameworks, in run order: scenario, then
/// framework, variant, refresh rate and size.
fn matrix<'a>(
    args: &Args,
    kinds: &[Kind],
    selected: &'a [(&'static Framework, PathBuf)],
) -> Vec<(&'static Framework, &'a Path, Cell)> {
    let mut cells = Vec::new();
    for &kind in kinds {
        let rates = if args.hz.is_empty() {
            kind.rates(args.quick)
        } else {
            args.hz.clone()
        };
        let rows = if args.rows.is_empty() {
            kind.row_counts(args.quick)
        } else {
            args.rows.clone()
        };
        for (fw, binary) in selected {
            let variants = kind.variants(fw).into_iter().filter(|v| {
                args.variants.is_empty() || *v == "default" || args.variants.iter().any(|a| a == v)
            });
            for variant in variants {
                for &hz in &rates {
                    for &count in &rows {
                        for updates in kind.update_counts() {
                            let cell = Cell {
                                kind,
                                framework: fw.name.to_owned(),
                                variant: variant.to_owned(),
                                hz,
                                rows: count,
                                updates,
                            };
                            cells.push((*fw, binary.as_path(), cell));
                        }
                    }
                }
            }
        }
    }
    cells
}

/// Returns the outcome of a cell that a previous run finished without errors, or `None`.
fn finished(dir: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(dir.join("cell.json")).ok()?;
    let outcome: serde_json::Value = serde_json::from_str(&text).ok()?;
    let clean = outcome["errors"].as_array().is_some_and(Vec::is_empty);
    clean.then_some(outcome)
}

fn write_index(
    run_dir: &Path,
    args: &Args,
    effort: Effort,
    stamp: u64,
    cells: &[serde_json::Value],
) -> anyhow::Result<()> {
    let root = frameworks::root();
    let index = json!({
        "format": FORMAT,
        "started_unix": stamp,
        "scenarios": args.scenarios,
        "frameworks": args.frameworks,
        "effort": effort,
        "output": { "width": layout::OUTPUT.0, "height": layout::OUTPUT.1 },
        "row_height": layout::ROW_HEIGHT,
        "header_height": layout::HEADER_HEIGHT,
        "scroll": { "velocity": steps::SCROLL_VELOCITY, "seconds": steps::SCROLL_SECONDS },
        "tick_ms": scenarios::TICK_MS,
        "revisions": {
            "gui-bench": revision(&root),
            "fakeland": revision(&root.join("../fakeland")),
            "zgui": revision(&root.join("../zgui-fakeland-marks")),
        },
        "cells": cells,
    });
    cell::save(&run_dir.join("run.json"), &index)
}

/// Returns the short commit of a checkout, with `+dirty` when it has changes.
fn revision(path: &Path) -> Option<String> {
    let git = |args: &[&str]| {
        // `--no-optional-locks`: a status refresh takes no `index.lock`, so a commit that runs
        // while the matrix runs never meets a held lock.
        Command::new("git")
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let commit = git(&["rev-parse", "--short", "HEAD"])?;
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    Some(if dirty {
        format!("{commit}+dirty")
    } else {
        commit
    })
}

/// Points `results/latest` at the run directory.
fn link_latest(root: &Path, run_dir: &Path) {
    let latest = root.join("results/latest");
    let _ = std::fs::remove_file(&latest);
    let target = run_dir
        .canonicalize()
        .unwrap_or_else(|_| run_dir.to_path_buf());
    if let Err(e) = std::os::unix::fs::symlink(&target, &latest) {
        eprintln!("cannot link {}: {e}", latest.display());
    }
}
