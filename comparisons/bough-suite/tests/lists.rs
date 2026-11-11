//! Pixel gates of the list scenes (presented == full redraw at ring ages 1..4 and the windowed ring;
//! retained == fresh mount at the end): update_bench's `rows` (keyed rows, labels changing width) and
//! `function-prop` (the keyed `select` wire), list_state_bench (row edits in a clipped list, a numeric
//! count), script_layout_bench (whole-list permutations of rows of three heights), and random splices,
//! moves and edits of a list with a header and a footer around it; the same with a viewport (a virtual list
//! region scrolled, jumped and spliced); script_layout's `animate = true` (every frame of the motion, and
//! the settled state == a fresh mount).
#![cfg(target_os = "macos")]
mod common;
use bough::layout::prelude::*;
use bough::raw::*;
use bough_suite::*;
use common::*;

fn cfg(w: f32, h: f32) -> Config {
    Config::new(w, h, 1.0, font()).once()
}

#[test]
fn update_rows_are_pixel_exact() {
    let published = upd_published(60);
    for ring in RINGS {
        check(
            "update rows",
            app_ring(URows::new(), cfg(600.0, 900.0), ring),
            60,
            3,
            |a, t| a.turn(|v| urows_step(v, &published[t])),
            || {
                let mut v = URows::new();
                urows_step(&mut v, &published[60]);
                App::new(v, with_ring(cfg(600.0, 900.0), ring), true)
            },
        );
    }
}

#[test]
fn function_prop_select_is_pixel_exact() {
    for ring in RINGS {
        check(
            "function-prop",
            app_ring(USelect { selected: Signal::new(0) }, cfg(200.0, 900.0), ring),
            70,
            3,
            |a, t| a.turn(|v| *v.selected = (t * 7) % ROWS + 1),
            || App::new(USelect { selected: Signal::new((70 * 7) % ROWS + 1) }, with_ring(cfg(200.0, 900.0), ring), true),
        );
    }
}

#[test]
fn list_state_is_pixel_exact() {
    for ring in RINGS {
        check(
            "list-state",
            app_ring(ListState::new(400), cfg(800.0, 600.0), ring),
            90,
            3,
            |a, _| a.turn(|v| v.tick()),
            || {
                let mut v = ListState::new(400);
                for _ in 0..90 {
                    v.tick();
                }
                v.tick = 0;
                App::new(v, with_ring(cfg(800.0, 600.0), ring), true)
            },
        );
    }
}

#[test]
fn script_layout_reorders_are_pixel_exact() {
    for ring in RINGS {
        let mut r = 1u64;
        let mut perms: Vec<u64> = Vec::new();
        for _ in 0..30 {
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            perms.push(r);
        }
        let perms2 = perms.clone();
        check(
            "script-layout",
            app_ring(Reorder::new(300), cfg(400.0, 900.0), ring),
            30,
            1,
            move |a, t| {
                let k = perms[t - 1];
                a.turn(|v| match k % 3 {
                    0 => v.rows.reverse(),
                    1 => v.rows.sort_by_key(|b| (b.i as u64).wrapping_mul(k) % 97),
                    _ => {
                        let n = v.rows.len();
                        v.rows.move_to((k % n as u64) as usize, ((k >> 20) % n as u64) as usize)
                    }
                })
            },
            move || {
                let mut v = Reorder::new(300);
                for &k in &perms2 {
                    match k % 3 {
                        0 => v.rows.reverse(),
                        1 => v.rows.sort_by_key(|b| (b.i as u64).wrapping_mul(k) % 97),
                        _ => {
                            let n = v.rows.len();
                            v.rows.move_to((k % n as u64) as usize, ((k >> 20) % n as u64) as usize)
                        }
                    }
                }
                let order: Vec<u32> = v.rows.iter().map(|b| b.i).collect();
                App::new(Reorder { rows: List::from_iter_in(order.into_iter().map(|i| Bar { i })), animate: false }, with_ring(cfg(400.0, 900.0), ring), true)
            },
        );
    }
}

/// A list between a header and a footer: splices move the footer, edits change row heights.
struct Doc {
    rows: List<URow>,
    title: Signal<String>,
}
impl View for Doc {
    fn view(&self) -> impl Element<Self> + '_ {
        column((label(sig!(self.title)).font(18.0, 24.0), list(sig!(self.rows)).gap(3.0).px(4.0).py(2.0).bg(rgb(0x202830)), block(300.0, 12.0).bg(rgb(0xff8800)))).w(500.0).gap(5.0)
    }
}
fn doc_rows(labels: &[String]) -> List<URow> {
    List::from_iter_in(labels.iter().map(|l| URow { label: Signal::new(l.clone()) }))
}

#[test]
fn list_splices_moves_and_edits_are_pixel_exact() {
    for ring in RINGS {
        let mut model: Vec<String> = (0..12).map(|i| format!("row {i}")).collect();
        let mut r = 7u64;
        let mut script: Vec<(u64, u64)> = Vec::new();
        for _ in 0..60 {
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            script.push((r % 7, r >> 8));
        }
        // the oracle: the same ops on a Vec of labels
        let mut seq = 0;
        let apply = |v: &mut Vec<String>, (k, x): (u64, u64), seq: &mut u32| {
            let n = v.len();
            match k {
                0 | 1 => {
                    *seq += 1;
                    v.insert((x as usize) % (n + 1), format!("new {} {}", seq, "x".repeat((x % 9) as usize)))
                }
                2 if n > 0 => {
                    v.remove((x as usize) % n);
                }
                3 if n > 1 => {
                    let a = v.remove((x as usize) % n);
                    v.insert(((x >> 9) as usize) % n, a);
                }
                4 if n > 0 => v[(x as usize) % n] = format!("edited {}\nsecond line", x % 50),
                5 => v.reverse(),
                _ if n > 0 => v[(x as usize) % n] = format!("e{}", x % 1000),
                _ => {}
            }
        };
        let script2 = script.clone();
        let mut final_model = model.clone();
        let mut s2 = 0;
        for &op in &script2 {
            apply(&mut final_model, op, &mut s2);
        }
        let fm = final_model.clone();
        check(
            "list splices",
            app_ring(Doc { rows: doc_rows(&model), title: Signal::new("list".into()) }, cfg(520.0, 700.0), ring),
            script.len(),
            2,
            |a, t| {
                let op = script[t - 1];
                let before = model.clone();
                apply(&mut model, op, &mut seq);
                a.turn(|v| {
                    let n = before.len();
                    let (k, x) = op;
                    match k {
                        0 | 1 => {
                            v.rows.insert((x as usize) % (n + 1), URow { label: Signal::new(model[(x as usize) % (n + 1)].clone()) });
                        }
                        2 if n > 0 => {
                            v.rows.remove((x as usize) % n);
                        }
                        3 if n > 1 => v.rows.move_to((x as usize) % n, ((x >> 9) as usize) % n),
                        5 => v.rows.reverse(),
                        _ if n > 0 => {
                            let i = (x as usize) % n;
                            v.rows.edit(i).label.set(model[i].clone());
                        }
                        _ => {}
                    };
                });
            },
            || App::new(Doc { rows: doc_rows(&fm), title: Signal::new("list".into()) }, with_ring(cfg(520.0, 700.0), ring), true),
        );
    }
}

#[test]
fn teardown_and_mount_are_pixel_exact() {
    for ring in RINGS {
        check(
            "teardown",
            app_ring(Teardown { n: 60, show: true }, cfg(1200.0, 900.0), ring),
            4,
            1,
            |a, t| a.turn(|v| v.show = t % 2 == 0),
            || App::new(Teardown { n: 60, show: true }, with_ring(cfg(1200.0, 900.0), ring), true),
        );
    }
}

/// A row of wrapping text (1-4 lines): variable heights under a 20-point estimate.
struct PRow {
    text: Signal<String>,
    c: u32,
}
impl View for PRow {
    fn view(&self) -> impl Element<Self> + '_ {
        column(para(sig!(self.text)).font(13.0, 18.0).w(180.0)).px(4.0).py(3.0).bg(rgb(self.c))
    }
}
/// A virtual list (a 400-point viewport) between a header and a footer.
struct VDoc {
    rows: List<PRow>,
    scroll: Signal<f32>,
}
impl View for VDoc {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            label("virtual").font(18.0, 24.0),
            list(sig!(self.rows)).w(220.0).h(400.0).scroll(sig!(self.scroll)).estimate(20.0).gap(3.0).px(4.0).py(2.0).bg(rgb(0x202830)),
            block(300.0, 12.0).bg(rgb(0xff8800)),
        ))
        .w(500.0)
        .gap(5.0)
    }
}
fn ptext(x: u64) -> String {
    let w = ["lorem", "ipsum", "dolor", "sit", "amet", "consectetur", "adipiscing", "elit"];
    (0..1 + x % 17).map(|i| w[((x >> (i % 13)) as usize + i as usize) % w.len()]).collect::<Vec<_>>().join(" ")
}

#[test]
fn virtual_list_scrolls_jumps_and_splices_are_pixel_exact() {
    for ring in RINGS {
        let mut a = app_ring(VDoc { rows: List::from_iter_in((0..300u64).map(|i| PRow { text: Signal::new(ptext(i * 7919)), c: 0x203040 + (i as u32 % 9) * 0x0a0a0a })), scroll: Signal::new(0.0) }, cfg(520.0, 700.0), ring);
        a.frame();
        let w = a.rt.win_w as usize;
        let mut r = 11u64;
        let mut seq = 1000u64;
        let mut checked = 0;
        for t in 1..=120usize {
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            let (k, x) = (r % 9, r >> 9);
            let n = a.owner.rows.len();
            let jump = a.rt.list_row_offset_pt(0, (x as usize) % n.max(1));
            a.turn(|v| match k {
                0 => {
                    seq += 1;
                    v.rows.insert((x as usize) % (n + 1), PRow { text: Signal::new(ptext(seq * 31)), c: 0x405060 });
                }
                1 if n > 0 => drop(v.rows.remove((x as usize) % n)),
                2 if n > 1 => v.rows.move_to((x as usize) % n, ((x >> 11) as usize) % n),
                3 if n > 0 => {
                    let i = (x as usize) % n;
                    v.rows.edit(i).text.set(ptext(x));
                }
                4 => _ = v.scroll.set(jump),
                5 => v.rows.reverse(),
                _ => {
                    let d = (x % 160) as f32 - 70.0;
                    let at = (*v.scroll + d).max(0.0);
                    v.scroll.set(at);
                }
            });
            let at = a.rt.list_scroll_pt(0);
            a.owner.scroll.set_silent(at);
            let drew = a.frame();
            assert!(!a.bins_overflowed());
            if drew {
                let (px, reference) = (a.presented_pixels(), a.reference_pixels());
                assert!(px == reference, "virtual ring {ring} step {t}: presented != full redraw at {:?}", first_diff(&px, &reference, w));
                checked += 1;
            }
        }
        assert!(checked > 20);
        // retained == a fresh mount of the same rows at the same offset with the same heights
        let px = a.presented_pixels();
        let rows = List::from_iter_in(a.owner.rows.iter().map(|p| PRow { text: Signal::new((*p.text).clone()), c: p.c }));
        let mut f = App::new(VDoc { rows, scroll: Signal::new(0.0) }, with_ring(cfg(520.0, 700.0), ring), true);
        f.frame();
        let h = a.rt.lists.regs()[0].idx.heights().to_vec();
        f.rt.list_set_heights(0, &h);
        let s = a.rt.list_scroll_pt(0);
        f.turn(|v| _ = v.scroll.set(s));
        f.frame();
        assert_eq!(f.rt.list_window(0), a.rt.list_window(0));
        let fp = f.presented_pixels();
        assert!(px == fp, "virtual ring {ring}: retained != fresh mount at {:?}", first_diff(&px, &fp, w));
    }
}

#[test]
fn script_layout_animation_is_pixel_exact() {
    for ring in RINGS {
        let mut a = app_ring(Reorder::animated(300), cfg(400.0, 900.0), ring);
        a.frame();
        let w = a.rt.win_w as usize;
        let mut r = 5u64;
        let mut checked = 0;
        let mut frames = 0;
        for change in 0..8 {
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            let k = r;
            a.turn(|v| match k % 3 {
                0 => v.rows.reverse(),
                1 => v.rows.sort_by_key(|b| (b.i as u64).wrapping_mul(k) % 97),
                _ => {
                    let n = v.rows.len();
                    v.rows.move_to((k % n as u64) as usize, ((k >> 20) % n as u64) as usize)
                }
            });
            a.frame();
            // part of the motion (the next change interrupts it), or all of it
            let budget = if change % 2 == 0 { 20 + (k % 30) as usize } else { usize::MAX };
            let mut f = 0;
            while a.animating() && f < budget {
                a.tick(1.0 / 120.0);
                let drew = a.frame();
                assert!(!a.bins_overflowed());
                if drew && f % 3 == 0 {
                    let (px, reference) = (a.presented_pixels(), a.reference_pixels());
                    assert!(px == reference, "animate ring {ring} change {change} frame {f}: presented != full redraw at {:?}", first_diff(&px, &reference, w));
                    checked += 1;
                }
                f += 1;
                frames += 1;
            }
        }
        assert!(checked > 30 && frames > 100);
        while a.animating() {
            a.tick(1.0 / 120.0);
            a.frame();
        }
        assert!(!a.frame(), "idle once settled");
        let px = a.presented_pixels();
        let order: Vec<u32> = a.owner.rows.iter().map(|b| b.i).collect();
        let mut f = App::new(Reorder { rows: List::from_iter_in(order.into_iter().map(|i| Bar { i })), animate: true }, with_ring(cfg(400.0, 900.0), ring), true);
        f.frame();
        let fp = f.presented_pixels();
        assert!(px == fp, "animate ring {ring}: settled != fresh mount at {:?}", first_diff(&px, &fp, w));
    }
}
