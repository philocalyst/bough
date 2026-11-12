//! R1 on adversarial scenes: for every archetype shape (each primitive, compositions with and without an
//! opaque top, clips, operands that read the block), the fast CPU kernel, the archetype specification and the
//! round-7 oracle (`cpu::naive` over the scene staged as generic commands) give the same words, on fresh
//! frames and on frames retained across block changes.
use bough_kernel::cpu::{self, Pool};
use bough_kernel::scenes::{self, Rng};
use bough_paint::expr::*;
use bough_paint::paint::*;
use bough_paint::scene::Scene;

type Pan = Off<Lane, I<Uw<0>>, I<Uw<1>>>;
/// Hover (word 2 against a record word) over a theme mix (word 3) of two opaque record colours.
type Themed = SelV<Eq<Uw<2>, Rec>, Pm<K<0xff6b3bff>>, MixV<Pmo<Rec>, Pmo<Rec>, Uw<3>>>;
type Any = Pm<Rec>;

type ASolid = Solid<Pan, Themed>;
type ARRect = RRect<Pan, I<Rec>, Any>;
type ABorder = Clip<Border<Pan, I<Rec>, I<Rec>, Any, Any>, Off<RecRect, I<Uw<0>>, I<Uw<1>>>>;
type AGlyph = Glyph<Pan, Rec, Any>;
type AGrad = Grad<Pan, I<Rec>, I<Rec>, I<Rec>, Pmo<Rec>, Any>;
/// A card: shadow under an opaque themed body (static cut).
type ACard = Hit<Over<Shadow<Off<Grow<Pan, I<Rec>>, Ki<0>, I<Rec>>, I<Rec>, I<Rec>, Any, Tab>, RRect<Pan, I<Rec>, Themed>>, Rec>;
/// Two shadows under a translucent bordered box whose fill fades with word 4 (no cut possible).
type AGlass = Over<Over<Shadow<Pan, Ki<2048>, Ki<3072>, Any>, Shadow<Off<Pan, Ki<512>, Ki<1024>>, Ki<2048>, I<Rec>, Any, Tab>>, Border<Pan, Ki<2048>, Ki<384>, Any, Fade<Any, Uw<4>>>>;
/// A box breathing between two rects on its own phase of the clock (word 63), rounding and recolouring.
type AAnim = RRect<MixR<Pan, Off<RecRect, I<Uw<0>>, I<Uw<1>>>, Smooth<Tri<Uw<63>, Rec, Rec>>>, MixI<I<Rec>, I<Rec>, Tri<Uw<63>, Rec, Rec>>, MixV<Any, Any, Tri<Uw<63>, Rec, Rec>>>;

fn q(v: f32) -> i32 {
    (v * 256.0).round() as i32
}
fn r(v: u32) -> Rec {
    Rec(v)
}
fn ri(v: i32) -> I<Rec> {
    I { a: Rec(v as u32) }
}
fn pm(c: u32) -> Any {
    Pm { c: Rec(c) }
}
fn pan() -> Pan {
    Off { b: Lane, dx: I { a: Uw }, dy: I { a: Uw } }
}
fn themed(id: u32, a: u32, b: u32) -> Themed {
    SelV { c: Eq { a: Uw, b: Rec(id) }, a: Pm { c: K }, b: MixV { a: Pmo { c: Rec(a) }, b: Pmo { c: Rec(b) }, t: Uw } }
}
fn tri(phase: u32, period: u32) -> Tri<Uw<63>, Rec, Rec> {
    Tri { now: Uw, phase: Rec(phase), period: Rec(period) }
}

fn build(seed: u64, fw: u32, fh: u32, n: usize) -> Scene {
    let mut g = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut s = Scene::new(fw, fh, 1 << 18);
    s.atlas = scenes::atlas();
    s.aw = scenes::ATLAS_W;
    let c_solid = s.column::<ASolid>(n);
    let c_rrect = s.column::<ARRect>(n);
    let c_border = s.column::<ABorder>(n);
    let c_glyph = s.column::<AGlyph>(n);
    let c_grad = s.column::<AGrad>(n);
    let c_card = s.column::<ACard>(n);
    let c_glass = s.column::<AGlass>(n);
    let c_anim = s.column::<AAnim>(n);
    let (w, h) = (fw as f32, fh as f32);
    for _ in 0..n {
        let (x, y) = (g.pos(-20.0, w), g.pos(-20.0, h));
        let (bw, bh) = (g.pos(1.0, 140.0), g.pos(1.0, 110.0));
        let rect = [q(x), q(y), q(x + bw), q(y + bh)];
        let rad = match g.u(5) {
            0 => 0.0,
            1 => g.f(0.0, 3.0),
            2 => 9999.0,
            3 => g.f(60.0, 140.0),
            _ => g.f(2.0, 24.0),
        };
        let (o1, o2) = (g.u(2) == 0, g.u(3) == 0);
        let (c1, c2) = (g.color(o1), g.color(o2));
        match g.u(10) {
            0 => {
                let rect = [q(x.round()), q(y.round()), q(x.round() + bw.round()), q(y.round() + bh.round())];
                s.push(c_solid, rect, Solid { b: pan(), c: themed(g.u(6), c1, c2) });
            }
            1 => {
                s.push(c_rrect, rect, RRect { b: pan(), r: ri(q(rad)), c: pm(c2) });
            }
            2 => {
                let k = [q(g.pos(-10.0, w)), q(g.pos(-10.0, h)), q(g.pos(0.0, w + 20.0)), q(g.pos(0.0, h + 20.0))];
                let k = [k[0].min(k[2]), k[1].min(k[3]), k[0].max(k[2]), k[1].max(k[3])];
                s.push(c_border, rect, Clip { p: Border { b: pan(), r: ri(q(rad)), w: ri(q(g.f(0.25, 5.0))), c: pm(c1), fill: pm(c2) }, k: Off { b: rec_rect(k), dx: I { a: Uw }, dy: I { a: Uw } } });
            }
            3 => {
                let (gx, gy) = (x.round(), y.round());
                let rect = [q(gx), q(gy), q(gx) + scenes::CELL_W as i32 * 256, q(gy) + scenes::CELL_H as i32 * 256];
                s.push(c_glyph, rect, Glyph { b: pan(), texel: r(scenes::cell(g.u(scenes::ATLAS_CELLS))), c: pm(c1) });
            }
            4 => {
                let a = g.f(0.0, 6.28);
                let (dx, dy) = if g.u(3) == 0 { (0.0, 1.0) } else { (a.sin(), -a.cos()) };
                let len = ((bw * dx).abs() + (bh * dy).abs()).max(1.0);
                let sc = (1u32 << 26) as f32 / len;
                s.push(c_grad, rect, Grad { b: pan(), r: ri(q(rad)), dx: ri((dx * sc).round() as i32), dy: ri((dy * sc).round() as i32), c: Pmo { c: r(c1) }, c2: pm(c2) });
            }
            5 | 6 => {
                let blur = q(2.0 * g.f(0.3, 14.0));
                s.push(
                    c_card,
                    rect,
                    Hit {
                        p: Over(
                            Shadow { b: Off { b: Grow { b: pan(), d: ri(q(g.f(-2.0, 3.0))) }, dx: Ki, dy: ri(q(g.f(0.0, 12.0))) }, r: ri(q(rad)), blur: ri(blur), c: pm(g.color(false)), tab: Tab::NONE },
                            RRect { b: pan(), r: ri(q(rad)), c: themed(g.u(6), c1, c2) },
                        ),
                        id: r(g.u(6)),
                    },
                );
            }
            7 => {
                s.push(c_glass, rect, Over(Over(Shadow { b: pan(), r: Ki, blur: Ki, c: pm(g.color(false)), tab: K }, Shadow { b: Off { b: pan(), dx: Ki, dy: Ki }, r: Ki, blur: ri(q(g.f(1.0, 20.0))), c: pm(g.color(false)), tab: Tab::NONE }), Border { b: pan(), r: Ki, w: Ki, c: pm(c1), fill: Fade { a: pm(g.color(false)), k: Uw } }));
            }
            _ => {
                let (x2, y2) = (g.pos(-20.0, w), g.pos(-20.0, h));
                let to = [q(x2), q(y2), q(x2 + g.pos(1.0, 140.0)), q(y2 + g.pos(1.0, 110.0))];
                let (ph, per) = (g.u(4000), 200 + g.u(3000));
                s.push(c_anim, rect, RRect { b: MixR { a: pan(), b: Off { b: rec_rect(to), dx: I { a: Uw }, dy: I { a: Uw } }, t: Smooth { t: tri(ph, per) } }, r: MixI { a: ri(q(rad.min(40.0))), b: ri(q(g.f(0.0, 30.0))), t: tri(ph, per) }, c: MixV { a: pm(c1), b: pm(c2), t: tri(ph, per) } });
            }
        }
    }
    s
}

fn block(g: &mut Rng) -> U {
    let mut u = U::default();
    u.w[0] = (g.u(64 * 256) as i32 - 32 * 256) as u32;
    u.w[1] = (g.u(64 * 256) as i32 - 32 * 256) as u32;
    u.w[2] = g.u(7);
    u.w[3] = [0, 32768, 12345, 20000][g.u(4) as usize];
    u.w[4] = g.u(40000);
    u.w[63] = g.u(100_000);
    u
}
/// One word of the block changed (what an interaction is).
fn step(g: &mut Rng, u: &mut U) {
    let b = block(g);
    let i = [0, 1, 2, 2, 2, 3, 4, 63, 63, 63][g.u(10) as usize];
    u.w[i] = b.w[i];
}

fn seeds() -> u64 {
    std::env::var("BOUGH_EXACT_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(12)
}
fn diff(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

#[test]
fn archetype_kernels_equal_the_oracle() {
    let (fw, fh) = (320u32, 250u32);
    let stride = fw as usize + 3;
    let pool = Pool::new(2);
    let mut bad = 0usize;
    let mut h = 0u64;
    for seed in 0..seeds() {
        let mut s = build(seed, fw, fh, 110);
        let mut g = Rng(seed * 77 + 5);
        let mut u = block(&mut g);
        let mut kept = vec![0u32; stride * fh as usize];
        s.frame(&u, &mut kept, stride, &pool, 0);
        for k in 0..6 {
            // the oracle: the scene as generic commands, every pixel folding every command
            let ks = s.stage(&u);
            let mut oracle = vec![0u32; stride * fh as usize];
            cpu::naive(&ks, fw, fh, &cpu::Atlas::new(&s.atlas, s.aw), 0, &mut oracle, stride);
            h ^= bough_kernel::hash(&oracle).rotate_left((seed * 6 + k) as u32);
            // the archetypes' specification (what the GPU function computes)
            let mut spec = vec![0u32; stride * fh as usize];
            s.spec(&u, &mut spec, stride);
            let d_spec = diff(&spec, &oracle);
            // the fast kernel, retained across the block changes so far
            let d_kept = diff(&kept, &oracle);
            // the fast kernel, fresh, with and without the cut and flat rows, one core and three
            let mut d_fresh = 0;
            for (cut, flat, extra) in [(true, true, 0), (false, true, 2), (true, false, 2), (false, false, 0)] {
                let mut f = build(seed, fw, fh, 110);
                (f.cut, f.flat) = (cut, flat);
                let mut px = vec![0u32; stride * fh as usize];
                f.frame(&u, &mut px, stride, &pool, extra);
                d_fresh += diff(&px, &oracle);
            }
            if d_spec + d_kept + d_fresh != 0 {
                eprintln!("seed {seed} step {k}: spec {d_spec}, retained {d_kept}, fresh {d_fresh} differing words");
                bad += 1;
            }
            step(&mut g, &mut u);
            s.frame(&u, &mut kept, stride, &pool, (k % 2) as usize * 2);
        }
    }
    eprintln!("exact: {} scenes x 6 block states, oracle hash {h:016x}, {bad} failing states", seeds());
    assert_eq!(bad, 0);
}

/// R3: the generated source. The WGSL of every archetype validates under naga, and every leaf a code tree
/// names exists under that name in naga's MSL of the leaves (the run-time path appends calls by name).
#[test]
fn generated_source_is_valid() {
    let s = build(1, 320, 250, 16);
    let descs: Vec<&bough_paint::scene::Desc> = s.cols.iter().map(|c| c.desc).collect();
    let wgsl = bough_paint::emit::wgsl(&descs);
    let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&wgsl)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
    let mut names = vec!["x_over", "x_in", "x_union", "x_and", "x_rect", "c_core", "c_win", "x_texel", "x_store", "x_neu", "x_nei", "x_nev", "x_ner"];
    for d in &descs {
        bough_paint::emit::leaves(d.code, &mut names);
    }
    names.sort_unstable();
    names.dedup();
    for n in &names {
        assert!(bough_paint::emit::LEAVES_MSL.contains(&format!(" {n}(\n")), "leaf {n} is not in the MSL under that name");
    }
    eprintln!("generated: {} archetypes, {} bytes of WGSL (leaves {}), {} leaves named", descs.len(), wgsl.len(), bough_paint::emit::LEAVES_WGSL.len(), names.len());
}

#[cfg(target_os = "macos")]
#[test]
fn gpu_functions_equal_the_oracle() {
    use bough_paint::metal::Gpu;
    let (fw, fh) = (320u32, 250u32);
    let stride = fw as usize + 3;
    let words = stride * fh as usize;
    let pool = Pool::new(2);
    let (mut bad, mut picks, mut overflow) = (0usize, 0usize, 0u32);
    let (mut hits, mut shaded, mut full) = (0usize, 0u32, 0u32);
    for seed in 0..seeds() {
        let mut s = build(seed, fw, fh, 110);
        let Some(mut gpu) = Gpu::new(&s, words, 256) else {
            eprintln!("no Metal device: skipped");
            return;
        };
        let mut res = Gpu::new(&s, words, 256).unwrap();
        let mut g = Rng(seed * 77 + 5);
        let mut u = block(&mut g);
        let mut kept = vec![0u32; words];
        let mut shown = u;
        for k in 0..6 {
            // the pick's reference: the CPU scene still holds the frame the surface shows
            let (px, py) = (g.u(fw) as i32, g.u(fh) as i32);
            let want_pick = (k > 0).then(|| s.pick(&shown, px, py));
            s.frame(&u, &mut kept, stride, &pool, 0);
            shown = u;
            let ks = s.stage(&u);
            let mut oracle = vec![0u32; words];
            cpu::naive(&ks, fw, fh, &cpu::Atlas::new(&s.atlas, s.aw), 0, &mut oracle, stride);
            // A: the generated shade functions over the CPU's bins, every tile
            let all: Vec<u32> = (0..(s.tx * s.ty) as u32).collect();
            gpu.out_mut(words).fill(0);
            gpu.shade_with(&s, &u, &all, stride);
            let d_a = diff(gpu.out(words), &oracle);
            // B: the resident frame: the CPU sent the block and a pointer, nothing else; retained across steps
            let (_, pick) = res.frame(&u, Some((px, py)), stride);
            overflow += res.last().1;
            hits += (pick != NO_HIT) as usize;
            if k > 0 {
                shaded += res.last().0;
                full += (s.tx * s.ty) as u32;
            }
            let d_b = diff(res.out(words), &oracle);
            let d_p = want_pick.is_some_and(|w| w != pick) as usize;
            picks += want_pick.is_some() as usize;
            if d_a + d_b + d_p != 0 {
                eprintln!("seed {seed} step {k}: shade over CPU bins {d_a}, resident {d_b} differing words, pick mismatch {d_p}");
                bad += 1;
            }
            step(&mut g, &mut u);
        }
    }
    eprintln!("gpu exact: {} scenes x 6 block states, {picks} picks ({hits} on a record with an id), retained frames shaded {shaded} of {full} tiles, {overflow} slab overflows, {bad} failing states", seeds());
    assert!(hits > 0 && shaded < full);
    assert_eq!(bad, 0);
    assert_eq!(overflow, 0);
}

/// Negative controls for the GPU gate: a rounding constant changed in the leaves, a layer dropped from the
/// generated functions, the damage of a changed operand dropped from the window pass. Each must be seen.
#[cfg(target_os = "macos")]
#[test]
fn gpu_mutants_are_caught() {
    use bough_paint::metal::{Gpu, source};
    let (fw, fh) = (320u32, 250u32);
    let stride = fw as usize + 3;
    let words = stride * fh as usize;
    let s = build(3, fw, fh, 110);
    let descs: Vec<&bough_paint::scene::Desc> = s.cols.iter().map(|c| c.desc).collect();
    let lays: Vec<(usize, usize)> = s.cols.iter().map(|c| (c.base, c.desc.words)).collect();
    let good = source(&descs, &lays, s.tab_region().0);
    let mutants = [
        ("rounding", good.replacen("constant uint HALF = 16384u;", "constant uint HALF = 16383u;", 1)),
        ("dropped layer", good.replacen("    a0 = x_over(a0, p_rrect(", "    // a0 = x_over(a0, p_rrect(", 1)),
        ("pixel cut one too wide", good.replacen("c.z >= x + 2 && c.w >= y + 2", "c.z >= x + 1 && c.w >= y + 2", 1)),
        ("dropped damage", good.replace(") != 0u) || a", ") != 0u) || false && a")),
    ];
    for (name, src) in mutants {
        assert!(src != good, "{name}: the mutation did not apply");
        let Some(mut m) = Gpu::with_source(&s, words, 256, &src) else { return };
        let mut ok = Gpu::new(&s, words, 256).unwrap();
        let mut g = Rng(99);
        let mut u = block(&mut g);
        let mut d = 0;
        for _ in 0..8 {
            m.frame(&u, None, stride);
            ok.frame(&u, None, stride);
            d += diff(m.out(words), ok.out(words));
            step(&mut g, &mut u);
        }
        eprintln!("gpu mutant {name}: {d} differing words over 8 retained frames");
        assert!(d > 0, "{name} was not caught");
    }
}

/// R12: the corner tables. The GPU's fill and the CPU's write the same entries; with every entry one too
/// large (the CPU mutant feature, or the same edit to the GPU kernel's source) the frames differ from the
/// oracle's, so the tables are read and the gate sees them.
#[cfg(target_os = "macos")]
#[test]
fn corner_tables_are_the_closed_form() {
    use bough_paint::metal::{Gpu, source};
    let (fw, fh) = (320u32, 250u32);
    let stride = fw as usize;
    let words = stride * fh as usize;
    let mut a = build(5, fw, fh, 110);
    let mut b = build(5, fw, fh, 110);
    let jobs = a.tab_jobs().len();
    assert!(jobs > 0, "the scene has no shadow in the table regime");
    let Some(mut g) = Gpu::new(&a, words, 256) else { return };
    let (_, entries) = g.fill_tabs(&mut a);
    b.fill_tabs();
    let used = a.tab_region().1;
    let d = a.tabs()[..used].iter().zip(&b.tabs()[..used]).filter(|(x, y)| x != y).count();
    let max = a.tabs()[..used].iter().copied().filter(|&v| v != u32::MAX).max().unwrap();
    eprintln!("corner tables: {jobs} keys, {entries} entries, {used} words; gpu fill against cpu fill: {d} differing words{}", if cfg!(feature = "mutant_tab") { " (mutant: expected)" } else { "" });
    eprintln!("corner tables: largest word {max}");
    if !cfg!(feature = "mutant_tab") {
        assert_eq!(d, 0);
    }
    // the tables are in use: a pixel-aligned block makes aligned shadows read them
    let pool = Pool::new(2);
    let mut u = U::default();
    u.w[3] = 20000;
    let mut px = vec![0u32; words];
    let fs = a.frame(&u, &mut px, stride, &pool, 0);
    eprintln!("corner tables: cpu frame: {} corner pixels, {} from a table", fs.raster.px_corner, fs.raster.px_table);
    assert!(fs.raster.px_table > 0, "no corner pixel was answered from a table");
    // the GPU kernel's fill, mutated: the resident frame must differ from the good one
    let descs: Vec<&bough_paint::scene::Desc> = a.cols.iter().map(|c| c.desc).collect();
    let lays: Vec<(usize, usize)> = a.cols.iter().map(|c| (c.base, c.desc.words)).collect();
    let good = source(&descs, &lays, a.tab_region().0);
    let bad = good.replacen("= x_cfill(", "= 1u + x_cfill(", 1);
    assert!(bad != good);
    let mut c = build(5, fw, fh, 110);
    let mut m = Gpu::with_source(&c, words, 256, &bad).unwrap();
    m.fill_tabs(&mut c);
    m.frame(&u, None, stride);
    g.frame(&u, None, stride);
    let dm = diff(m.out(words), g.out(words));
    eprintln!("corner tables: gpu fill mutant: {dm} differing words");
    assert!(dm > 0, "a wrong table entry was not seen");
}
