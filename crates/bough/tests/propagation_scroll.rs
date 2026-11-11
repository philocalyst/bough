//! Ordinary operands under a scroll port must agree with a fresh description.
use bough::app::{App, Config};
use bough::prelude::*;

struct Document {
    rows: Vec<(f32, u32)>,
    y: Signal<f32>,
    inner_y: Signal<f32>,
    leading: f32,
    width: f32,
}
impl Document {
    fn new() -> Self {
        Self {
            rows: (0..10)
                .map(|i| (70.0 + i as f32, 0x102030 + i * 0x101010))
                .collect(),
            y: Signal::new(0.0),
            inner_y: Signal::new(0.0),
            leading: 8.0,
            width: 160.0,
        }
    }
    fn snapshot(&self) -> Self {
        Self {
            rows: self.rows.clone(),
            y: Signal::new(*self.y),
            inner_y: Signal::new(*self.inner_y),
            leading: self.leading,
            width: self.width,
        }
    }
}
impl View for Document {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            block(10.0, self.leading),
            column((
                column((
                    block(90.0, 24.0).bg(rgb(self.rows.first().map_or(0, |r| r.1))),
                    block(90.0, 24.0).bg(rgb(0x00aa88)),
                ))
                .w(100.0)
                .h(32.0)
                .scroll(sig!(self.inner_y)),
                each(self.rows.iter().map(|&(w, c)| block(w, 24.0).bg(rgb(c)))),
            ))
            .w(self.width)
            .h(120.0)
            .scroll(sig!(self.y)),
            block(30.0, 12.0).bg(rgb(0xffaa00)),
        ))
        .w(220.0)
    }
}
fn config(ring: usize) -> Config {
    let mut c = Config::new(240.0, 180.0, 1.0, zgx_workload::FONT_BYTES.to_vec());
    c.ring = ring;
    c
}
fn compare(a: &[u8], b: &[u8], context: &str) {
    assert_eq!(a.len(), b.len(), "{context}: dimensions");
    assert_eq!(
        a.iter().zip(b).filter(|(a, b)| a != b).count(),
        0,
        "{context}: unequal bytes"
    );
}

#[test]
fn scroll_children_equal_fresh_after_each_operation_at_every_ring_age() {
    for ring in 1..=4 {
        let mut a = App::new(Document::new(), config(ring), true);
        a.frame();
        for op in 0..12 {
            a.turn(|d| match op {
                0 => d.rows[0].1 = 0xff0000,
                1 => d.rows[1].0 = 38.0,
                2 => {
                    d.y.set(29.0);
                    d.inner_y.set(7.0);
                }
                3 => d.rows.insert(0, (120.0, 0x00ff00)),
                4 => {
                    d.rows.remove(1);
                }
                5 => d.rows.swap(0, 3),
                6 => d.leading = 19.0,
                7 => d.width = 130.0,
                8 => d.rows.truncate(2),
                9 => d.y.set(0.0),
                10 => d.rows.clear(),
                11 => d.rows.push((60.0, 0x8833ff)),
                _ => unreachable!(),
            });
            a.frame();
            let retained = a.reference_pixels();
            compare(
                &a.presented_pixels(),
                &retained,
                &format!("ring {ring} op {op}: presented/full"),
            );
            let mut fresh = App::new(a.owner.snapshot(), config(ring), true);
            fresh.frame();
            compare(
                &retained,
                &fresh.reference_pixels(),
                &format!("ring {ring} op {op}: retained/fresh"),
            );
        }
    }
}
