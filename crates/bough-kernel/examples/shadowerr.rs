//! E7: the integer shadow against the exact convolution of a rounded box with the spline kernel (floats,
//! chord integrated over y in 1/32 px steps). Prints the largest coverage error in 1/255 units.
use bough_kernel::{k, stage};

fn cdf(u: f64) -> f64 {
    let p = |v: f64| v.max(0.0).powi(3);
    ((p(u + 1.5) - 3.0 * p(u + 0.5) + 3.0 * p(u - 0.5) - p(u - 1.5)) / 6.0).clamp(0.0, 1.0)
}
fn pdf(u: f64) -> f64 {
    let p = |v: f64| v.max(0.0).powi(2);
    (p(u + 1.5) - 3.0 * p(u + 0.5) + 3.0 * p(u - 0.5) - p(u - 1.5)) / 2.0
}
fn reference(x: f64, y: f64, b: [f64; 4], r: f64, w: f64) -> f64 {
    let (lo, hi) = ((y - 1.5 * w).max(b[1]), (y + 1.5 * w).min(b[3]));
    if hi <= lo {
        return 0.0;
    }
    let n = (((hi - lo) * 32.0).ceil() as usize).max(8);
    let h = (hi - lo) / n as f64;
    let mut s = 0.0;
    for i in 0..n {
        let v = lo + (i as f64 + 0.5) * h;
        let d = (b[1] + r - v).max(v - (b[3] - r)).max(0.0);
        let inset = r - (r * r - d * d).max(0.0).sqrt();
        s += pdf((y - v) / w) / w * (cdf((x - (b[0] + inset)) / w) - cdf((x - (b[2] - inset)) / w)) * h;
    }
    s
}
fn main() {
    println!("radius\tsigma\tbox\tmax_err_255\tmean_err_255");
    for (bw, bh) in [(80.0f32, 60.0f32), (24.0, 18.0), (200.0, 9.0)] {
        for r in [0.0f32, 4.0, 8.0, 16.0, 30.0] {
            for sigma in [0.5f32, 1.0, 2.0, 4.0, 8.0, 16.0] {
                let (x0, y0) = (70.375f32, 70.625f32);
                let Some(c) = stage::shadow(x0, y0, bw, bh, r, sigma, 0xff00_0000, &stage::NO_CLIP) else { continue };
                let b = [c.b.x as f64 / 256.0, c.b.y as f64 / 256.0, c.b.z as f64 / 256.0, c.b.w as f64 / 256.0];
                let (rr, w) = (c.rad.x as f64 / 256.0, c.rad.y as f64 / 256.0);
                let (mut worst, mut sum, mut n) = (0.0f64, 0.0, 0);
                for y in c.win.y..c.win.w {
                    for x in c.win.x..c.win.z {
                        let got = k::shadow(c, x * 256 + 128, y * 256 + 128) as f64 / 32768.0;
                        let want = reference(x as f64 + 0.5, y as f64 + 0.5, b, rr, w);
                        let e = (got - want).abs() * 255.0;
                        worst = worst.max(e);
                        sum += e;
                        n += 1;
                    }
                }
                println!("{rr}\t{sigma}\t{bw}x{bh}\t{worst:.2}\t{:.3}", sum / n as f64);
            }
        }
    }
}
