//! bough in gui-bench: gui-bench's command line and scenarios (shared spec), bough's window (Wayland on Linux);
//! the heavy scenes (`apps/shared/scenes.rs`) first.
fn main() {
    if let Some(scene) = bough_guibench_scenes::spec::from_env() {
        bough_guibench_scenes::run(scene)
    }
    bough_guibench::run_cli()
}
