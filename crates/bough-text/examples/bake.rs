//! Bake a face at a size into a `bough_face` image (the build step of `stage::bake`).
//!
//!     cargo run --release -p bough-text --example bake -- FONT.ttf SIZE_PX OUT.face
//!
//! An application's build script calls `bough_text::stage::bake::bake` the same way and embeds the result with
//! `include_bytes!`; this tool is for images that are committed (the default face).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 4 {
        eprintln!("usage: bake FONT.ttf SIZE_PX OUT.face");
        std::process::exit(2);
    }
    let font = std::fs::read(&a[1]).expect("font file");
    let size: f32 = a[2].parse().expect("size in pixels");
    let img = bough_text::stage::bake::bake(&font, size).expect("a face the table shaper models");
    let f = bough_face::Face::new(Box::leak(img.clone().into_boxed_slice()));
    std::fs::write(&a[3], &img).expect("write");
    println!("{}: {} bytes, {} px, upem {}, digit cell {:?}", a[3], img.len(), f.size(), f.upem(), f.cell());
}
