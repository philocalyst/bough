//! scene_diff <zgx.rgba> <bough.bgra> W H [diff.rgba]: compare two frames (zgx RGBA vs bough BGRA),
//! report the differing-pixel fractions at several thresholds, and the best whole-pixel shift.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let a: Vec<u8> = std::fs::read(&args[1]).unwrap();
    let b: Vec<u8> = std::fs::read(&args[2]).unwrap();
    let (w, h): (usize, usize) = (args[3].parse().unwrap(), args[4].parse().unwrap());
    assert!(a.len() == w * h * 4 && b.len() == w * h * 4, "sizes {} {} vs {}", a.len(), b.len(), w * h * 4);
    let m = |i: usize, j: usize| {
        let (pa, pb) = (&a[i * 4..i * 4 + 4], &b[j * 4..j * 4 + 4]);
        (pa[0] as i32 - pb[2] as i32).abs().max((pa[1] as i32 - pb[1] as i32).abs()).max((pa[2] as i32 - pb[0] as i32).abs())
    };
    let d: Vec<u8> = (0..w * h).map(|i| m(i, i) as u8).collect();
    let frac = |t: u8| 100.0 * d.iter().filter(|&&v| v > t).count() as f64 / (w * h) as f64;
    println!("{w}x{h}: >0 {:.2}%  >16 {:.2}%  >64 {:.3}%  max {}", frac(0), frac(16), frac(64), d.iter().max().unwrap());
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            let (mut n, mut tot) = (0usize, 0usize);
            for y in 2..h - 2 {
                for x in 2..w - 2 {
                    let (xs, ys) = ((x as i32 + dx) as usize, (y as i32 + dy) as usize);
                    n += (m(y * w + x, ys * w + xs) > 16) as usize;
                    tot += 1;
                }
            }
            println!("shift dx={dx} dy={dy}: >16 {:.3}%", 100.0 * n as f64 / tot as f64);
        }
    }
    if let Some(p) = args.get(5) {
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
