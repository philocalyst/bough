//! Where `Fused::new` spends its time: device, queues, library, the two pipelines, buffers, surfaces.
use objc2_metal::*;
use std::time::Instant;

fn main() {
    let t = Instant::now();
    let device = MTLCreateSystemDefaultDevice().expect("device");
    let a = t.elapsed();
    let _q = device.newCommandQueue().unwrap();
    let b = t.elapsed();
    let f = bough_metal::fused::Fused::new(1920, 1440, 2.0, 4 << 20, 4, 1024);
    let c = t.elapsed();
    std::hint::black_box(&f);
    eprintln!("device {a:?}, queue {:?}, Fused::new (device again + pipelines + buffers + surfaces) {:?}", b - a, c - b);
}
