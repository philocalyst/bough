//! `workload [scene...|all]`: run the standing workloads' phases and print one TSV row per phase
//! (settings from the environment: see `bough_workloads::harness`). `bench/scenes/run.sh` adds the identity record.
//!
//!     cargo run --profile dist -p bough-workloads --bin workload -- sheet
//!     SCENE_SCALE=0.1 PHASES=cold,steady REPS=16 target/dist/workload all
fn main() {
    #[cfg(feature = "hotpath")]
    let _hp = bough::hotpath::HotpathGuardBuilder::new("workload").build();
    let names: Vec<String> = std::env::args().skip(1).collect();
    let cfg = bough_workloads::harness::RunCfg::from_env();
    if !bough_workloads::run_all(&names, &cfg) {
        std::process::exit(2);
    }
}
