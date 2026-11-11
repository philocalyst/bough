#[cfg(all(feature = "scroll", feature = "dashboard"))]
compile_error!("features `scroll` and `dashboard` cannot be enabled together");
#[cfg(not(any(feature = "scroll", feature = "dashboard")))]
compile_error!("enable exactly one of `scroll` or `dashboard`");

#[cfg(feature = "dashboard")]
mod dashboard;
#[cfg(feature = "scroll")]
mod scroll;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "scroll")]
    return scroll::main();
    #[cfg(feature = "dashboard")]
    return dashboard::main();
}
