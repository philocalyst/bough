//! Extents: 64-bit logical coordinates, validated heights, and the blocked prefix sum against a plain model.
use bough_lay::{Extent, Heights, Uniform};

#[test]
fn uniform_is_exact_past_32_bits_and_refuses_no_height() {
    assert!(Uniform::new(10, 0).is_none() && Uniform::new(10, -3).is_none());
    let u = Uniform::new(100_000_000, 44).unwrap();
    assert_eq!(u.total(), 4_400_000_000);
    assert_eq!(u.top(99_999_999), 4_399_999_956);
    assert_eq!(u.row_at(4_399_999_999), 99_999_999);
    assert_eq!(u.row_at(i64::MAX), 99_999_999);
    assert_eq!(u.row_at(-5), 0);
    assert_eq!(u.anchor(4_399_999_960), (99_999_999, 4));
    // a viewport at the far end, in its own 32-bit frame
    assert_eq!(u.window(4_399_999_000, 1600), (99_999_977, 100_000_000, -12));
    assert_eq!(Uniform::new(0, 44).unwrap().window(0, 800), (0, 0, 0));
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// The model: a vector of optional heights; unknown rows count as the rounded mean of the known ones.
fn model_top(rows: &[Option<i32>], est0: i32, row: usize) -> i64 {
    let known: Vec<i64> = rows.iter().flatten().map(|&h| h as i64).collect();
    let est = if known.is_empty() { est0 as i64 } else { ((known.iter().sum::<i64>() + known.len() as i64 / 2) / known.len() as i64).max(1) };
    rows[..row].iter().map(|h| h.map_or(est, |h| h as i64)).sum()
}

#[test]
fn heights_agree_with_a_plain_model_through_sets_inserts_and_removes() {
    let mut r = Rng(0x51ab);
    for case in 0..40 {
        let n0 = [0usize, 1, 63, 64, 65, 200, 1000][case % 7];
        let mut rows: Vec<Option<i32>> = vec![None; n0];
        let mut e = Heights::new(n0 as u32, 40);
        for step in 0..300 {
            match r.below(10) {
                0 if rows.len() < 1200 => {
                    let at = r.below(rows.len() as u64 + 1) as usize;
                    rows.insert(at, None);
                    e.insert(at as u32);
                }
                1 if !rows.is_empty() => {
                    let at = r.below(rows.len() as u64) as usize;
                    rows.remove(at);
                    e.remove(at as u32);
                }
                _ if !rows.is_empty() => {
                    let at = r.below(rows.len() as u64) as usize;
                    let h = r.below(300) as i32;
                    rows[at] = Some(h);
                    e.set(at as u32, h, 0.0, 1e9);
                    e.fix();
                }
                _ => {}
            }
            assert_eq!(e.len() as usize, rows.len());
            assert_eq!(e.total(), model_top(&rows, 40, rows.len()), "case {case} step {step}: total");
            if !rows.is_empty() {
                let at = r.below(rows.len() as u64) as usize;
                let top = model_top(&rows, 40, at);
                assert_eq!(e.top(at as u32), top, "case {case} step {step}: top({at})");
                // the row at a pixel inside row `at` (when it has any) is `at`
                let h = e.height(at as u32);
                if h > 0 {
                    assert_eq!(e.row_at(top + r.below(h as u64) as i64), at as u32, "case {case} step {step}: row_at");
                }
                assert!(e.row_at(i64::MAX) == rows.len() as u32 - 1 && e.row_at(-1) <= rows.len() as u32 - 1);
            }
        }
    }
}

#[test]
fn a_width_change_reports_only_rows_outside_their_interval() {
    let mut e = Heights::new(500, 40);
    for row in 0..500u32 {
        // row r holds for widths in [r, r + 1000]
        e.set(row, 36, row as f32, row as f32 + 1000.0);
    }
    e.fix();
    let mut bad = Vec::new();
    e.invalid(700.0, |r| bad.push(r));
    assert!(bad.is_empty(), "every row holds at 700");
    e.invalid(1100.5, |r| bad.push(r));
    assert_eq!(bad, (0..=100).collect::<Vec<u32>>(), "rows whose interval ends before 1100.5");
    assert!(e.measured() == (500, true));
}
