use bough::prelude::*;
use bough_zgui::thread::{thread_config, Thread};
use bough_zgui::*;
fn main() {
    let mut a = App::new(Thread { scroll: Signal::new(0.0) }, thread_config(WIDTH, HEIGHT), false);
    a.frame();
    let roots: Vec<u32> = (0..a.rt.lay.info.len() as u32).filter(|&r| a.rt.lay.info[r as usize].live && a.rt.lay.info[r as usize].parent == bough::rt::NONE).collect();
    println!("roots {:?}", roots.iter().map(|&r| (r, a.rt.lay.info[r as usize].avail_w, a.rt.lay.eng.rect(r))).collect::<Vec<_>>());
    let r = roots[0];
    let c0 = a.rt.lay_child(r, 0);
    println!("msg0 {:?} child1 {:?}", a.rt.lay.eng.rect(c0), a.rt.lay_child_rect(c0, 1));
    let s0 = a.rt.lay.stats;
    a.resize(1800.0, 1000.0);
    a.frame();
    println!("after: root {:?} avail {} msg0 {:?} para {:?}; stats {:?} -> {:?}", a.rt.lay.eng.rect(r), a.rt.lay.info[r as usize].avail_w, a.rt.lay.eng.rect(c0), a.rt.lay_child_rect(c0, 1), s0, a.rt.lay.stats);
}
