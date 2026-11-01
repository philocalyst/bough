//! frame_diff <zgx.rgba> <bough.bgra> [diff.rgba]: compare two 2560x1600 frames (zgx RGBA vs bough
//! BGRA), report per-region differing-pixel fractions, optionally write a diff image (RGBA).
fn main() {
    let a: Vec<u8> = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let b: Vec<u8> = std::fs::read(std::env::args().nth(2).unwrap()).unwrap();
    let (w, h) = (2560usize, 1600usize);
    let mut d = vec![0u8; w * h];
    for i in 0..w * h {
        let (pa, pb) = (&a[i * 4..i * 4 + 4], &b[i * 4..i * 4 + 4]);
        let m = (pa[0] as i32 - pb[2] as i32).abs().max((pa[1] as i32 - pb[1] as i32).abs()).max((pa[2] as i32 - pb[0] as i32).abs());
        d[i] = m as u8;
    }
    let frac = |x0: usize, y0: usize, x1: usize, y1: usize, t: u8| {
        let mut n = 0usize;
        for y in y0..y1 {
            for x in x0..x1 {
                n += (d[y * w + x] > t) as usize;
            }
        }
        100.0 * n as f64 / ((x1 - x0) * (y1 - y0)) as f64
    };
    println!("whole: >0 {:.2}%  >16 {:.2}%  >64 {:.3}%  max {}", frac(0, 0, w, h, 0), frac(0, 0, w, h, 16), frac(0, 0, w, h, 64), d.iter().max().unwrap());
    for (name, r) in [("title", (0, 0, 2560, 90)), ("tiles", (30, 96, 2540, 420)), ("cards", (30, 230, 1580, 1368)), ("table", (1584, 240, 2540, 1344)), ("log", (30, 1370, 2540, 1580))] {
        println!("{name:6} >16: {:.2}%  >64: {:.3}%", frac(r.0, r.1, r.2, r.3, 16), frac(r.0, r.1, r.2, r.3, 64));
    }
    // does a whole-pixel shift of bough's frame explain the text differences? (text rows only)
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            let (mut n, mut tot) = (0usize, 0usize);
            for y in 100..1560usize {
                for x in 40..2500usize {
                    let (xs, ys) = ((x as i32 + dx) as usize, (y as i32 + dy) as usize);
                    let (pa, pb) = (&a[(y * w + x) * 4..][..4], &b[(ys * w + xs) * 4..][..4]);
                    let m = (pa[0] as i32 - pb[2] as i32).abs().max((pa[1] as i32 - pb[1] as i32).abs()).max((pa[2] as i32 - pb[0] as i32).abs());
                    n += (m > 16) as usize;
                    tot += 1;
                }
            }
            println!("shift dx={dx} dy={dy}: >16 {:.3}%", 100.0 * n as f64 / tot as f64);
        }
    }
    if let Some(p) = std::env::args().nth(3) {
        let mut o = vec![255u8; w * h * 4];
        for i in 0..w * h {
            let g = (a[i * 4] as u32 / 4) as u8;
            let hot = d[i] > 16;
            o[i * 4] = if hot { 255 } else { g };
            o[i * 4 + 1] = if hot { 0 } else { g };
            o[i * 4 + 2] = if hot { 0 } else { g };
        }
        std::fs::write(p, o).unwrap();
    }
}
