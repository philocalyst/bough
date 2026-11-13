//! `workload` with `bough_mem::count::Counting` as the global allocator, so each phase row carries allocations
//! per step. A counting allocator adds a few atomic adds per allocation: read instructions and time from
//! `workload`, allocations from this one.
#[global_allocator]
static A: bough_mem::count::Counting = bough_mem::count::Counting;

fn main() {
    let names: Vec<String> = std::env::args().skip(1).collect();
    let cfg = bough_workloads::harness::RunCfg::from_env();
    if !bough_workloads::run_all(&names, &cfg) {
        std::process::exit(2);
    }
}
