//! Does the pre-armed command stream (frames encoded ahead, each waiting on a shared event) make the surfaces
//! report `IOSurfaceIsInUse`? (The fused presenter's free-surface test relies on it not doing so; its encoders
//! are bindless, so pending frames name no surface.)
use bough_metal::fused::{Fused, Layout, SlotCopy};

fn main() {
    let (w, h) = (640usize, 480usize);
    let mut f = Fused::new(w, h, 2.0, 1 << 20, 4, 1024);
    let l = Layout { w: w as u32, h: h as u32, slot: [4096, 8192, 12288], ..Default::default() };
    f.set_layout(l);
    let rows = vec![1u128; 15];
    let report = |f: &Fused, what: &str| {
        let u: Vec<bool> = (0..4).map(|i| f.is_in_use(i)).collect();
        println!("{what}: in use {u:?}");
    };
    report(&f, "fresh");
    f.draw::<false>(0, &rows, true, 0, &SlotCopy::NONE);
    f.sync();
    report(&f, "after a plain frame (synced)");
    report(&f, "with the pre-armed stream waiting (every frame streams)");
    f.draw::<false>(1, &rows, false, 0, &SlotCopy::NONE);
    std::thread::sleep(std::time::Duration::from_millis(50));
    println!("released frame done: {}", f.released_done());
    report(&f, "after releasing 1 frame (done)");
}
