//! Render the laid-out gallery (or shell) at a scroll offset and write the presented frame as a PNG:
//! `snapshot <gallery|shell> <scroll_y> <out.png> [x y w h]` (a crop, device px).
use bough::raw::*;
use bough_zgui::gallery::{Doc, GalleryL};
use bough_zgui::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let scroll: f32 = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let out = a.get(3).cloned().unwrap_or_else(|| "snapshot.png".into());
    let crop: Vec<usize> = a[4.min(a.len())..].iter().filter_map(|v| v.parse().ok()).collect();
    let mut app = App::new(GalleryL::fresh(), config(WIDTH, HEIGHT), true);
    app.frame();
    if scroll != 0.0 {
        app.turn(|g: &mut GalleryL| { g.scroll.set(scroll); });
        app.frame();
    }
    let px = app.presented_pixels();
    let fw = app.surface_width();
    let (x0, y0, w, h) = if crop.len() == 4 { (crop[0], crop[1], crop[2], crop[3]) } else { (0, 0, WIDTH as usize, HEIGHT as usize) };
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let i = (y * fw + x) * 4;
            rgba.extend_from_slice(&[px[i + 2], px[i + 1], px[i], 255]);
        }
    }
    let f = std::fs::File::create(&out).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&rgba).unwrap();
    eprintln!("wrote {out} ({w}x{h}); images rasterised {}", app.rt.images.rasterised);
}
