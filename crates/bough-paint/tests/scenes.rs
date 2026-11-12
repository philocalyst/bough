//! R1 on the phase-1 scenes: the canvas and the animation scene, drawn by the per-archetype CPU kernel and
//! by the GPU-resident frame, retained across an interaction script, against the round-7 oracle
//! (`cpu::reference`) of the scene's *plain* model: a second description that uses none of this crate's
//! expression types, leaves or integer staging (`scenes.rs`).
use bough_kernel::cpu::{self, Pool};
use bough_paint::expr::{U, U_PICK, U_TIME};
use bough_paint::scenes::{Model, anim, canvas};
use bough_paint::scene::Scene;

fn diff(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}
fn oracle(s: &Scene, m: &Model, u: &U, stride: usize, pool: &Pool) -> Vec<u32> {
    let mut out = vec![0u32; stride * s.fh as usize];
    cpu::reference(&m.oracle(u), s.fw, s.fh, &cpu::Atlas::new(&s.atlas, s.aw), 0, &mut out, stride, pool);
    out
}

/// One interaction: an optional pointer position, then block words to write.
type Step = (Option<(i32, i32)>, &'static [(usize, u32)]);

fn run(name: &str, mut s: Scene, m: Model, script: &[Step]) {
    let stride = s.fw as usize;
    let words = stride * s.fh as usize;
    let pool = Pool::new(3);
    #[cfg(target_os = "macos")]
    let mut gpu = bough_paint::metal::Gpu::new(&s, words, 192);
    let mut u = U::default();
    let mut kept = vec![0u32; words];
    let mut bad = 0;
    for (k, (ptr, writes)) in script.iter().enumerate() {
        for &(w, v) in writes.iter() {
            u.w[w] = v;
        }
        // the CPU path picks in the frame the surface shows, then draws
        if let (Some((x, y)), true) = (ptr, k > 0) {
            let shown = s.prev;
            u.w[U_PICK] = s.pick(&shown, *x, *y);
        }
        let fs = s.frame(&u, &mut kept, stride, &pool, 3);
        let want = oracle(&s, &m, &u, stride, &pool);
        let d_cpu = diff(&kept, &want);
        #[allow(unused_mut)]
        let (mut d_gpu, mut d_pick, mut gpu_tiles) = (0, 0, 0);
        #[cfg(target_os = "macos")]
        if let Some(g) = gpu.as_mut() {
            let (_, pick) = g.frame(&u, if k > 0 { *ptr } else { None }, stride);
            d_gpu = diff(g.out(words), &want);
            d_pick = (pick != u.w[U_PICK]) as usize;
            gpu_tiles = g.last().0;
            assert_eq!(g.last().1, 0, "{name} step {k}: a tile's slab overflowed");
        }
        eprintln!("{name} step {k}: mask {:#x}, cpu visited {} columns / {} records, rebinned {}, {} tiles (gpu {gpu_tiles}); differing words cpu {d_cpu}, gpu {d_gpu}, pick mismatch {d_pick}", fs.mask, fs.cols_visited, fs.recs_visited, fs.rebinned, fs.tiles);
        bad += (d_cpu + d_gpu + d_pick != 0) as usize;
    }
    assert_eq!(bad, 0, "{name}");
    // negative control: the comparison is live (every 50th node of the plain model nudged by a quarter
    // pixel; one node alone may be hidden or off screen)
    let mut m2 = m;
    for n in m2.nodes.iter_mut().step_by(50) {
        n.rect[0] -= 64;
        n.rect[2] += 64;
    }
    let d = diff(&kept, &oracle(&s, &m2, &u, stride, &pool));
    eprintln!("{name}: nudged model nodes differ in {d} words");
    assert!(d > 0, "{name}: the oracle did not see a changed node");
}

#[test]
fn canvas_is_exact() {
    let frames = std::env::var("BOUGH_CANVAS_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let (s, m) = canvas::build(frames);
    eprintln!("canvas: {frames} artboards, {} records in {} columns, {} model nodes", s.records(), s.cols.len(), m.nodes.len());
    let on = canvas::row_point(3);
    let script: &[Step] = &[
        (None, &[]),
        (Some(on), &[]),
        (None, &[(2, 32768)]),
        (None, &[(2, 12000)]),
        (Some(canvas::NOWHERE), &[]),
        (None, &[(0, (-72 * 256i32) as u32), (1, (-36 * 256 - 128i32) as u32)]),
        (Some((700, 300)), &[]),
        (None, &[(3, canvas::DRAG_ID + 1), (4, (10 * 256 + 64) as u32), (5, (7 * 256) as u32)]),
        (None, &[(U_TIME, 137)]),
        (Some(canvas::row_point(1)), &[(U_TIME, 420)]),
        (None, &[(0, 0), (1, 40 * 256), (2, 0)]),
    ];
    run("canvas", s, m, script);
}

#[test]
fn anim_is_exact() {
    let (s, m) = anim::build(600);
    eprintln!("anim: {} records in {} columns", s.records(), s.cols.len());
    let script: &[Step] = &[(None, &[]), (None, &[(U_TIME, 16)]), (None, &[(U_TIME, 33)]), (None, &[(U_TIME, 800)]), (None, &[(U_TIME, 12345)]), (None, &[(2, 32768)]), (None, &[(U_TIME, 12361), (2, 20000)])];
    run("anim", s, m, script);
}
