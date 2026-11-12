//! Reference pictures of the scenes gui-bench ports (macOS, Metal): each scene at full size in its first frame
//! (light), with the hover target hovered, after `t` (the other theme) and after `e` (the spec's edit), as PNG
//! files, so a port can be compared with the reference by eye. The scene's keys go through the focused root, as in
//! the gui-bench window host.
//!
//!     cargo run --profile dist -p bough-workloads --example snap -- <out dir> [ide dash chat sheet canvas]
use bough::prelude::*;
use bough::raw::{App, Config, Mods};
use bough_workloads::guibench_spec as gs;
use bough_workloads::Scene;
use std::path::Path;

fn save(app: &mut App<impl View>, path: &Path) {
    app.frame();
    let (w, h) = (app.rt.win_w as u32, app.rt.win_h as u32);
    let mut px = app.presented_pixels();
    for p in px.chunks_exact_mut(4) {
        p.swap(0, 2); // BGRA to RGBA
    }
    let f = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().expect("png header").write_image_data(&px).expect("png data");
    // raw RGBA beside it, for reading pixel values without a PNG decoder
    std::fs::write(path.with_extension("rgba"), &px).expect("raw");
    eprintln!("wrote {}", path.display());
}

fn focus_root<V: View>(a: &mut App<V>) {
    if let Some(pos) = (0..a.record_count()).find(|&p| a.record_view(p).kind & 0x40 != 0) {
        let h = a.rt.bough.handle_at(pos);
        a.set_focus(h, false);
    }
}

fn snap<S: Scene + View>(out: &Path, model: S, hover: (f32, f32), away: (f32, f32)) {
    let (w, h) = S::SIZE;
    let mut c = Config::new(w, h, 1.0, bough_workloads::font());
    c.caps = model.caps();
    let mut app = App::new(model, c, true);
    app.frame();
    focus_root(&mut app);
    let name = S::NAME;
    app.pointer_move(away.0, away.1);
    save(&mut app, &out.join(format!("{name}-0-base.png")));
    app.pointer_move(hover.0, hover.1);
    save(&mut app, &out.join(format!("{name}-1-hover.png")));
    app.pointer_move(away.0, away.1);
    app.key_down(Key::Char('t'), Mods::default());
    save(&mut app, &out.join(format!("{name}-2-theme.png")));
    app.key_down(Key::Char('t'), Mods::default());
    app.key_down(Key::Char('e'), Mods::default());
    save(&mut app, &out.join(format!("{name}-3-edit.png")));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = Path::new(args.first().map(String::as_str).unwrap_or("snaps")).to_path_buf();
    std::fs::create_dir_all(&out).expect("out dir");
    let want: Vec<&str> = if args.len() > 1 { args[1..].iter().map(String::as_str).collect() } else { vec!["ide", "dash", "chat"] };
    let f = |p: (f32, f32)| p;
    for s in want {
        match s {
            "ide" => snap(&out, bough_workloads::ide::Ide::build(1.0), f(gs::ide::HOVER), f(gs::ide::AWAY)),
            "dash" => snap(&out, bough_workloads::dash::Dash::build(1.0), f(gs::dash::HOVER), f(gs::dash::AWAY)),
            "chat" => snap(&out, bough_workloads::chat::Chat::build(1.0), f(gs::chat::HOVER), f(gs::chat::AWAY)),
            other => eprintln!("{other}: not a scene here"),
        }
    }
}
