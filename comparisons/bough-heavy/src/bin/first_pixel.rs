//! Cold first pixel, headless (round 4): wall time from `main` to the heavy dashboard's first frame complete on the
//! GPU, split into App::new (device + region + mount + layout + shaping) and the first frame. Prints the epoch time
//! of the first pixel too, so a launcher can add exec + dyld (bench/floor/first_pixel.py).
use bough::raw::*;
use bough_heavy::*;
use zgx_workload::heavy::*;

fn main() {
    let t0 = std::time::Instant::now();
    let work = Heavy::from_env();
    let mut app = App::new(Dash::new(&work), config(2.0), true);
    let t1 = t0.elapsed();
    app.frame();
    app.wait_presented();
    let t2 = t0.elapsed();
    let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros();
    println!("first_pixel app_new_ms={:.2} first_frame_ms={:.2} total_ms={:.2} epoch_us={epoch}", t1.as_secs_f64() * 1e3, (t2 - t1).as_secs_f64() * 1e3, t2.as_secs_f64() * 1e3);
}
