//! Review findings K02 and K08: an empty glyph atlas is transparent on every executor, and a transition
//! whose derived fields the interpolation does not carry is admitted as its target state.
use bough_kernel::cpu::{self, Atlas, Binner, Damage, Opt, Pool, Prog};
use bough_kernel::stage::{self, Scene};

fn frame(s: &Scene, fw: u32, fh: u32, atlas: &Atlas, now: u32) -> (Vec<u32>, Vec<u32>) {
    let pool = Pool::new(1);
    let stride = fw as usize;
    let mut naive = vec![0u32; stride * fh as usize];
    cpu::naive(s, fw, fh, atlas, now, &mut naive, stride);
    let mut fast = vec![0u32; stride * fh as usize];
    let mut prog = Prog::default();
    Binner::default().bin(s, fw, fh, fw, atlas, now, &Damage::All, true, &mut prog);
    cpu::raster(s, &prog, atlas, &mut fast, stride, &pool, 1, Opt::default());
    (naive, fast)
}

#[test]
fn an_empty_atlas_is_transparent_on_the_fast_path_too() {
    let clip = stage::NO_CLIP;
    let mut s = Scene::default();
    s.push(stage::solid(0.0, 0.0, 64.0, 64.0, 0xff20_4060, &clip).unwrap());
    s.push(stage::glyph(8.0, 8.0, 12, 16, 0, 0xffff_ffff, &clip).unwrap());
    let empty: [u8; 0] = [];
    let (naive, fast) = frame(&s, 64, 64, &Atlas::new(&empty, 1), 0);
    assert_eq!(naive, fast);
}

#[test]
fn a_transition_the_interpolation_cannot_carry_shows_its_target() {
    let clip = stage::NO_CLIP;
    // a shadow whose blur changes: the inverse blur width is a derived field `k::at` keeps from the start
    let a = stage::shadow(20.0, 20.0, 60.0, 40.0, 6.0, 4.0, 0x8000_0000, &clip).unwrap();
    let b = stage::shadow(20.0, 20.0, 60.0, 40.0, 6.0, 12.0, 0x8000_0000, &clip).unwrap();
    let atlas_px: [u8; 0] = [];
    let atlas = Atlas::new(&atlas_px, 1);
    let mut anim = Scene::default();
    anim.push_anim(a, b, 100, 50, 0);
    let mut direct = Scene::default();
    direct.push(b);
    for now in [100, 125, 150, 151, 100_000] {
        let (n1, f1) = frame(&anim, 128, 96, &atlas, now);
        let (n2, _) = frame(&direct, 128, 96, &atlas, now);
        assert_eq!(n1, f1, "executors disagree at tick {now}");
        assert_eq!(n1, n2, "the transition is not at its target at tick {now}");
    }
    // one the interpolation does carry (the box moves, nothing derived changes) still animates and arrives
    let b2 = stage::shadow(40.0, 30.0, 60.0, 40.0, 6.0, 4.0, 0x8000_0000, &clip).unwrap();
    let mut anim = Scene::default();
    anim.push_anim(a, b2, 100, 50, 0);
    let mut direct = Scene::default();
    direct.push(b2);
    let (mid, _) = frame(&anim, 128, 96, &atlas, 125);
    let (end, _) = frame(&anim, 128, 96, &atlas, 150);
    let (want, _) = frame(&direct, 128, 96, &atlas, 150);
    assert_ne!(mid, end);
    assert_eq!(end, want);
    // the clock wrapping does not send a finished transition back to its start
    let mut late = Scene::default();
    late.push_anim(a, b2, 0xFFFF_FFF0, 8, 0);
    let (wrapped, _) = frame(&late, 128, 96, &atlas, 5);
    assert_eq!(wrapped, want);
}
