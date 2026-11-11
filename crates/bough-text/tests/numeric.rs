//! Invention B's oracle: `NumFmt` must render exactly what `format!` renders.

use bough_text::numeric::{as_str, NumFmt};
use zgx_workload::heavy;

fn full() -> bool {
    !cfg!(debug_assertions) || std::env::var("BOUGH_FULL").is_ok()
}

/// Every f32 whose exponent is in [-10, 20) (and their negatives, sampled), for {:.1}; every
/// 5th for {:.0} and {:.2}.
#[test]
fn exhaustive_binades_against_format() {
    let lo = ((127 - 10) as u32) << 23;
    let hi = ((127 + 20) as u32) << 23;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8) as u32;
    let stride = if full() { 1 } else { 997 };
    let checked = std::sync::atomic::AtomicU64::new(0);
    std::thread::scope(|sc| {
        for t in 0..threads {
            let checked = &checked;
            sc.spawn(move || {
                let fmts = [NumFmt::new("", Some(0), ""), NumFmt::new("", Some(1), " ms"), NumFmt::new("", Some(2), "%")];
                let mut buf = [0u8; 16];
                let mut s = String::new();
                let mut n = 0u64;
                let mut bits = lo + t * stride;
                while bits < hi {
                    let v = f32::from_bits(bits);
                    for (k, f) in fmts.iter().enumerate() {
                        if k != 1 && bits % 5 != 0 {
                            continue;
                        }
                        for v in [v, -v] {
                            if v < 0.0 && bits % 7 != 0 {
                                continue;
                            }
                            s.clear();
                            use std::fmt::Write;
                            match k {
                                0 => write!(s, "{v:.0}").unwrap(),
                                1 => write!(s, "{v:.1} ms").unwrap(),
                                _ => write!(s, "{v:.2}%").unwrap(),
                            }
                            match f.render_f32(v, &mut buf) {
                                Some(len) => assert_eq!(as_str(&buf, len), s, "bits {bits:#x}"),
                                None => assert!(s.len() > 16, "refused {s:?}"),
                            }
                            n += 1;
                        }
                    }
                    bits += threads * stride;
                }
                checked.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
            });
        }
    });
    let n = checked.into_inner();
    eprintln!("numeric: {n} renders equal to format!");
    assert!(n > 1000);
}

#[test]
fn heavy_values_and_integers() {
    let mut buf = [0u8; 16];
    for live in 0..5000u64 {
        for t in 0..heavy::TILES {
            let s = heavy::sample(900 + t, live);
            let (f, v_f, v_u) = match t {
                0 => (NumFmt::new("", Some(0), ""), Some(12_000. + 40_000. * s), None),
                1 => (NumFmt::new("", Some(2), "%"), Some(3. * s), None),
                2 => (NumFmt::new("", Some(1), " ms"), Some(4. + 30. * s), None),
                3 => (NumFmt::new("", Some(1), " ms"), Some(40. + 200. * s), None),
                4 => (NumFmt::new("", None, ""), None, Some(800 + (s * 400.) as u32)),
                _ => (NumFmt::new("", None, ""), None, Some((s * 5_000.) as u32)),
            };
            let len = match (v_f, v_u) {
                (Some(v), _) => f.render_f32(v, &mut buf),
                (_, Some(u)) => f.render_u32(u, &mut buf),
                _ => unreachable!(),
            }
            .unwrap();
            assert_eq!(as_str(&buf, len), heavy::tile_value(t, live));
        }
        let ms = NumFmt::new("", Some(1), " ms");
        for c in 0..heavy::CARDS {
            let len = ms.render_f32(4. + 96. * heavy::sample(c, live), &mut buf).unwrap();
            assert_eq!(as_str(&buf, len), heavy::card_value(c, live));
        }
        let mbs = NumFmt::new("", Some(2), " MB/s");
        for r in 0..30 {
            let len = ms.render_f32(2. + 120. * heavy::sample(r + 2000, live), &mut buf).unwrap();
            assert_eq!(as_str(&buf, len), heavy::row_latency(r, live));
            let len = mbs.render_f32(0.2 + 48. * heavy::sample(r + 3000, live), &mut buf).unwrap();
            assert_eq!(as_str(&buf, len), heavy::row_throughput(r, live));
        }
    }
    for u in (0..u32::MAX).step_by(65_521).chain([0, 9, 10, 99, 100, u32::MAX]) {
        let len = NumFmt::new("#", None, "").render_u32(u, &mut buf).unwrap();
        assert_eq!(as_str(&buf, len), format!("#{u}"));
    }
}
