//! The laid-out gallery and shell against zgui's own layout: content height, and for sample points the
//! topmost record's ink (bough) next to the dumped box (zgui).
use bough::prelude::*;
use bough_zgui::gallery::GalleryL;
use bough_zgui::shell::ShellL;
use bough_zgui::*;

fn probe<V: View>(app: &App<V>, name: &str, pts: &[(f32, f32, &str)]) {
    for &(x, y, what) in pts {
        let top = app.rt.bough.hit_top(x as i32, y as i32);
        let ink = top.map(|p| app.rt.bough.visible_at(p));
        println!("{name} ({x},{y}) {what}: {:?}", ink);
    }
}

fn main() {
    let t0 = std::time::Instant::now();
    let mut g = App::new(GalleryL::new(), config(WIDTH, HEIGHT), false);
    g.frame();
    println!("gallery: {} records, mounted+laid out+first frame in {:?}", g.rt.bough.len(), t0.elapsed());
    let roots: Vec<u32> = (0..g.rt.lay.info.len() as u32).filter(|&r| g.rt.lay.info[r as usize].live && g.rt.lay.info[r as usize].parent == bough::rt::NONE).collect();
    for r in roots {
        println!("  group {r}: rect {:?}", g.rt.lay.eng.rect(r));
    }
    probe(&g, "gallery", &[(41.0, 41.0, "swatch 24,24 34x34"), (129.0, 41.0, "swatch 112,24"), (1300.0, 820.0, "field 1165,803 379x34"), (60.0, 210.0, "Button heading 41,205")]);
    let t0 = std::time::Instant::now();
    let mut s = App::new(ShellL::new(), config(WIDTH, HEIGHT), false);
    s.frame();
    println!("shell: {} records in {:?}", s.rt.bough.len(), t0.elapsed());
    probe(&s, "shell", &[(25.0, 21.0, "tip-0 8,4 34x34"), (800.0, 692.5, "handle 0,688 1600x9"), (100.0, 56.0, "row 0 0,42 1600x28"), (20.0, 700.0, "log line 0 at 693")]);
}
