//! areweactuallyguiyet.mespin.me's two workloads on bough, as its apps select them: `--features scroll`
//! (the default) or `--features dashboard`. Written on `bough::prelude` only: flow layout by cephal, no
//! coordinates, routes as methods, `Application::run`.
#[cfg(all(feature = "scroll", feature = "dashboard"))]
compile_error!("select exactly one workload: scroll or dashboard");
#[cfg(not(any(feature = "scroll", feature = "dashboard")))]
compile_error!("select a workload: --features scroll or --features dashboard");

#[cfg(feature = "dashboard")]
mod dashboard;
#[cfg(feature = "scroll")]
mod scroll;
#[cfg(feature = "scroll")]
mod scroll_config;

fn main() {
    #[cfg(feature = "scroll")]
    scroll::main();
    #[cfg(feature = "dashboard")]
    dashboard::main();
}
