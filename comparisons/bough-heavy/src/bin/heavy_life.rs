//! The heavy dashboard's startup, memory and shutdown for the quick bench (`bench/quick/run.sh`): headless, scale
//! 2, the protocol of `bench/quick/life.rs`; the turns between the first frame and the close request are the
//! workload's `ZGX_MODE` (the launcher sets `both`).
#[path = "../../../../bench/quick/life.rs"]
mod life;

use bough::raw::*;
use bough_heavy::*;
use zgx_workload::heavy::*;

fn main() {
    let mut work = Heavy::from_env();
    let mode = work.mode;
    let dash = Dash::new(&work);
    life::run(
        || App::new(dash, config(2.0), true),
        |app, _| {
            work.tick();
            app.turn(|d| publish(d, &work, mode));
        },
    );
}
