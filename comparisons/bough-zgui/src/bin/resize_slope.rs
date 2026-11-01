//! zgui's `resize-slope` probe on bough: what one more box costs a configure (a new window width: every
//! row and cell is laid out again through cephal's incremental path and written back), against a
//! whole-document content change (a class on the root recolours every cell: here one signal whose wire
//! fans out to every cell's colour), over documents of 64 / 128 / 256 / 512 rows. Least-squares slopes
//! (µs per box and instructions per box) and their ratio, zgui's verdict quantity.   RENDER=1: GPU too
use bough::prelude::*;
use bough_zgui::*;

const ROWS: [usize; 4] = [64, 128, 256, 512];
const WARMUP: usize = 12;
const REPEATS: usize = 48;

fn slope(points: &[(f64, f64)]) -> f64 {
    let n = points.len() as f64;
    let mx = points.iter().map(|p| p.0).sum::<f64>() / n;
    let my = points.iter().map(|p| p.1).sum::<f64>() / n;
    let (mut cov, mut var) = (0.0, 0.0);
    for (x, y) in points {
        cov += (x - mx) * (y - my);
        var += (x - mx) * (x - mx);
    }
    cov / var
}

fn main() {
    let gpu = std::env::var("RENDER").is_ok_and(|v| v == "1");
    let (mut rt, mut ri, mut ct, mut ci) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for rows in ROWS {
        let mut app = App::new(Slope { rows, warm: Signal::new(false) }, config(SLOPE_W, SLOPE_H), gpu);
        app.frame();
        // boxes: the root, each row and each cell (zgui reads its layout's key count)
        let boxes = (1 + 2 * rows) as f64;
        let width = |turn: usize| SLOPE_W - (turn % 16) as f32 * 8.0;
        let mut s = Samples::new();
        for turn in 0..WARMUP + REPEATS {
            let w = width(turn);
            let (us, i, _) = s.measure(|| {
                app.resize(w, SLOPE_H);
                app.frame()
            });
            if turn >= WARMUP {
                s.push(us, i);
            }
        }
        app.resize(SLOPE_W, SLOPE_H);
        app.frame();
        let mut c = Samples::new();
        for turn in 0..WARMUP + REPEATS {
            let (us, i, _) = c.measure(|| {
                app.turn(|v| {
                    let w = !*v.warm;
                    v.warm.set(w);
                });
                app.frame()
            });
            if turn >= WARMUP {
                c.push(us, i);
            }
        }
        let (r_us, r_i, c_us, c_i) = (spread(&s.us).0, spread(&s.instr).0, spread(&c.us).0, spread(&c.instr).0);
        println!("SIZE rows={rows} boxes={boxes} resize_us={r_us:.3} content_us={c_us:.3} resize_instr={r_i:.0} content_instr={c_i:.0} records={}", app.rt.bough.len());
        rt.push((boxes, r_us));
        ri.push((boxes, r_i));
        ct.push((boxes, c_us));
        ci.push((boxes, c_i));
    }
    let (sr, sc) = (slope(&rt), slope(&ct));
    let (sri, sci) = (slope(&ri), slope(&ci));
    println!("RESIZE slope {sr:.5} us/box, content slope {sc:.5} us/box, ratio {:.4} (zgui recorded 0.8650)", sr / sc);
    println!("RESIZE slope {sri:.1} instr/box, content slope {sci:.1} instr/box, ratio {:.4}", sri / sci);
}
