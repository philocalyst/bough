//! zgx `variable_list_bench`, native: the same JSON line per row count, on `bough::mlist::HeightIndex`
//! (a Fenwick tree over integer device px; scale 1, so px = points; zgx's f32 points with compensated f64
//! sums become exact integer sums). Core index microbenchmark, not GUI frame, RSS or framework ranking.
//!
//! * variable scan: `row_at` of the viewport's two edges + the rows' tops from one `prefix`;
//! * fixed scan: the closed form for equal heights (zgx's `VirtualList`), with the checksum equal to the
//!   variable scan's before any height changes (asserted);
//! * update / equal update: `set`; heterogeneous scan: the scan after the updates;
//! * append_resize: `HeightIndex` has no resize/append (null). A rebuild per append would be O(n) each.
use bough::mlist::HeightIndex;
use std::{hint::black_box, time::Instant};

const OPERATIONS: usize = 10_000;
const ESTIMATE: i32 = 28;
const VIEWPORT: i64 = 400;
const OVERSCAN: usize = 2;

/// Identical viewport positions for the equal-height fixed/variable scan.
fn offset(index: usize, total: i64) -> i64 {
    ((index as u64 * 104_729 % 10_000) as f64 / 10_000. * (total - VIEWPORT).max(0) as f64) as i64
}

/// zgx's `visible_range`, on the index: the rows the viewport shows, widened by the overscan.
fn visible_range(h: &HeightIndex, pos: i64) -> std::ops::Range<usize> {
    let pos = pos.clamp(0, (h.total() - VIEWPORT).max(0));
    let first = h.row_at(pos);
    let end = (h.row_at(pos + VIEWPORT - 1) + 1).min(h.len());
    first.saturating_sub(OVERSCAN)..(end + OVERSCAN).min(h.len())
}

/// Equal heights, closed form (zgx's `VirtualList`).
struct Fixed {
    count: usize,
    row_height: i32,
}
impl Fixed {
    fn content_height(&self) -> i64 {
        self.count as i64 * self.row_height as i64
    }
    fn visible_range(&self, pos: i64) -> std::ops::Range<usize> {
        let pos = pos.clamp(0, (self.content_height() - VIEWPORT).max(0));
        let rh = self.row_height as i64;
        let first = (pos / rh) as usize;
        let end = ((pos + VIEWPORT + rh - 1) / rh) as usize;
        first.saturating_sub(OVERSCAN)..(end + OVERSCAN).min(self.count)
    }
    fn row_offset(&self, i: usize) -> i64 {
        i as i64 * self.row_height as i64
    }
}

fn scan(h: &HeightIndex, position: impl Fn(usize) -> i64) -> (f64, usize) {
    let (mut checksum, mut rows) = (0f64, 0usize);
    for operation in 0..OPERATIONS {
        let range = visible_range(h, black_box(position(operation)));
        let mut top = h.prefix(range.start);
        for index in range {
            let height = h.height(index);
            checksum += black_box(index as f64 + top as f64 + height as f64);
            top += height as i64;
            rows += 1;
        }
    }
    (checksum, rows)
}

fn main() {
    for count in [100_000, 1_000_000] {
        let started = Instant::now();
        let mut heights = HeightIndex::new(black_box(count), black_box(ESTIMATE));
        let construction_ns = started.elapsed().as_nanos();
        let started = Instant::now();
        let fixed = black_box(Fixed { count: black_box(count), row_height: black_box(ESTIMATE) });
        let fixed_construction_ns = started.elapsed().as_nanos();
        assert_eq!(heights.total(), fixed.content_height());

        let started = Instant::now();
        let (variable_checksum, variable_rows) = scan(&heights, |op| offset(op, fixed.content_height()));
        let variable_scan_ns = started.elapsed().as_nanos();
        let started = Instant::now();
        let mut fixed_checksum = 0f64;
        let mut fixed_rows = 0usize;
        for operation in 0..OPERATIONS {
            let position = black_box(offset(operation, fixed.content_height()));
            for index in fixed.visible_range(position) {
                fixed_checksum += black_box(index as f64 + fixed.row_offset(index) as f64 + fixed.row_height as f64);
                fixed_rows += 1;
            }
        }
        let fixed_scan_ns = started.elapsed().as_nanos();
        assert_eq!(variable_rows, fixed_rows);
        assert_eq!(variable_checksum, fixed_checksum);
        assert!(variable_rows <= OPERATIONS * 20);

        let started = Instant::now();
        let mut expected_delta = 0i64;
        for operation in 0..OPERATIONS {
            let index = operation * 7919 % count;
            let height = 16 + (operation % 97) as i32;
            expected_delta += (height - heights.height(index)) as i64;
            black_box(heights.set(black_box(index), black_box(height)));
        }
        let update_ns = started.elapsed().as_nanos();
        assert_eq!(heights.total(), count as i64 * ESTIMATE as i64 + expected_delta);
        let started = Instant::now();
        for operation in 0..OPERATIONS {
            let index = operation * 7919 % count;
            let height = heights.height(index);
            assert_eq!(black_box(heights.set(black_box(index), black_box(height))), 0);
        }
        let equal_update_ns = started.elapsed().as_nanos();

        let started = Instant::now();
        let (heterogeneous_checksum, heterogeneous_rows) = scan(&heights, |op| offset(op, heights.total()));
        let heterogeneous_scan_ns = started.elapsed().as_nanos();
        assert!(heterogeneous_checksum.is_finite());
        // Logical payload: i32 per row plus i32 per Fenwick slot.
        let logical_index_bytes = count * 4 + (count + 1) * 4;
        println!(
            "{{\"rows\":{count},\"operations\":{OPERATIONS},\"construction_ns\":{construction_ns},\"fixed_construction_ns\":{fixed_construction_ns},\"variable_scan_ns\":{variable_scan_ns},\"fixed_scan_ns\":{fixed_scan_ns},\"visited_equal_rows\":{variable_rows},\"update_ns\":{update_ns},\"equal_update_ns\":{equal_update_ns},\"heterogeneous_scan_ns\":{heterogeneous_scan_ns},\"visited_heterogeneous_rows\":{heterogeneous_rows},\"append_resize_ns\":null,\"logical_index_payload_bytes_before_appends\":{logical_index_bytes},\"fixed_struct_bytes\":{},\"checksum\":{}}}",
            size_of::<Fixed>(),
            black_box(heterogeneous_checksum)
        );
    }
}
