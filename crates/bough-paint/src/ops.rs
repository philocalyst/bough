//! The leaves. Every function an expression or a paint is made of is defined here, once, as concrete
//! integer Rust in the wgsl-rs subset: rustc compiles it into the per-archetype CPU kernels, wgsl-rs and naga
//! turn it into the WGSL and MSL the generated GPU functions call. A combinator (`expr.rs`) or a primitive
//! (`paint.rs`) is nothing but the name of one of these and its operands, so the two executors cannot
//! disagree about what a combinator means.
//!
//! The arithmetic of a pixel is not defined here: it is the round-7 kernel's (`bough_kernel::km`: `sd`, `cov`,
//! `scale`, `blend`, `shadow`, ...), imported. What is here is (a) value operations for expressions, (b) the
//! integer form of the kernel's staging (`mk_*`: operands to a `Cmd`; `stage.rs` does this in floats on the
//! CPU, which is why the generic kernel needs a CPU pass per changed record), and (c) one guarded shade per
//! primitive.
//!
//! Names called from generated code carry no trailing digit (naga's MSL writer renames those).
#[wgsl_rs::wgsl]
pub mod ops {
    use bough_kernel::km::*;
    use wgsl_rs::std::*;

    // ---- values ----

    pub fn x_i(a: u32) -> i32 {
        a as i32
    }
    pub fn x_eq(a: u32, b: u32) -> u32 {
        select(0u32, 1u32, a == b)
    }
    pub fn x_sel(c: u32, a: u32, b: u32) -> u32 {
        select(b, a, c != 0u32)
    }
    pub fn x_selv(c: u32, a: Vec4u, b: Vec4u) -> Vec4u {
        if c != 0u32 {
            return a;
        }
        b
    }
    /// Packed RGBA8 to premultiplied Q15.
    pub fn x_pm(c: u32) -> Vec4u {
        premul(c)
    }
    /// The same with alpha forced to 255: opaque whatever the word holds.
    pub fn x_pmo(c: u32) -> Vec4u {
        premul(c | 0xFF000000u32)
    }
    pub fn x_mixv(a: Vec4u, b: Vec4u, t: u32) -> Vec4u {
        mix4u(a, b, min(t, ONE))
    }
    pub fn x_fade(a: Vec4u, k: u32) -> Vec4u {
        scale(a, min(k, ONE))
    }
    /// A looping there-and-back progress (Q15): 0 at `now + phase == 0 mod 2 period`, ONE one period later.
    pub fn x_tri(now: u32, phase: u32, period: u32) -> u32 {
        let p = clamp(period, 1u32, 65535u32);
        let m = (now + phase) % (2u32 * p);
        let e = select(2u32 * p - m, m, m < p);
        (e * ONE) / p
    }
    /// Smoothstep of a Q15 progress.
    pub fn x_smooth(t0: u32) -> u32 {
        let t = min(t0, ONE);
        mul15(mul15(t, t), 3u32 * ONE - 2u32 * t)
    }
    /// One-shot progress (Q15) of a transition started at `t0` lasting `dur` ticks.
    pub fn x_prog(now: u32, t0: u32, dur: u32) -> u32 {
        progress(now, t0, max(dur, 1u32), 0u32)
    }
    pub fn x_rect(a: u32, b: u32, c: u32, d: u32) -> Vec4i {
        vec4i(a as i32, b as i32, c as i32, d as i32)
    }
    /// A rect moved by (dx, dy), Q8.
    pub fn x_off(b: Vec4i, dx: i32, dy: i32) -> Vec4i {
        vec4i(b.x() + dx, b.y() + dy, b.z() + dx, b.w() + dy)
    }
    /// A rect grown by `d` on every side, Q8 (a shadow's spread).
    pub fn x_grow(b: Vec4i, d: i32) -> Vec4i {
        vec4i(b.x() - d, b.y() - d, b.z() + d, b.w() + d)
    }
    pub fn x_mixr(a: Vec4i, b: Vec4i, t: u32) -> Vec4i {
        mix4i(a, b, min(t, ONE))
    }
    pub fn x_mixi(a: i32, b: i32, t: u32) -> i32 {
        mix_i(a, b, min(t, ONE))
    }
    /// The first `h` (Q8) of a rect's height: a header band.
    pub fn x_top(b: Vec4i, h: i32) -> Vec4i {
        vec4i(b.x(), b.y(), b.z(), min(b.y() + h, b.w()))
    }
    pub fn x_and(a: Vec4i, b: Vec4i) -> Vec4i {
        vec4i(max(a.x(), b.x()), max(a.y(), b.y()), min(a.z(), b.z()), min(a.w(), b.w()))
    }
    /// The bounding window of two pixel windows (an empty one, x0 >= x1, is the identity).
    pub fn x_union(a: Vec4i, b: Vec4i) -> Vec4i {
        if b.x() >= b.z() {
            return a;
        }
        if a.x() >= a.z() {
            return b;
        }
        vec4i(min(a.x(), b.x()), min(a.y(), b.y()), max(a.z(), b.z()), max(a.w(), b.w()))
    }
    pub fn x_over(acc: Vec4u, sh: Vec4u) -> Vec4u {
        blend(acc, sh)
    }
    pub fn x_in(w: Vec4i, x: i32, y: i32) -> bool {
        inside(w, x, y)
    }
    pub fn x_nev(a: Vec4u, b: Vec4u) -> bool {
        a.x() != b.x() || a.y() != b.y() || a.z() != b.z() || a.w() != b.w()
    }
    pub fn x_ner(a: Vec4i, b: Vec4i) -> bool {
        a.x() != b.x() || a.y() != b.y() || a.z() != b.z() || a.w() != b.w()
    }
    pub fn x_neu(a: u32, b: u32) -> bool {
        a != b
    }
    pub fn x_nei(a: i32, b: i32) -> bool {
        a != b
    }
    pub fn x_store(acc: Vec4u) -> u32 {
        store8(acc)
    }

    // ---- staging in integers (the twin of bough-kernel's stage.rs, which takes floats) ----

    /// The first pixel whose centre is at or after `q` (Q8).
    pub fn pxc(q: i32) -> i32 {
        (q + 127i32) >> 8u32
    }
    /// Pixels whose centres lie in [a, b) cut by `clip` (all Q8); the zero window when there are none.
    pub fn window(ax: i32, ay: i32, bx: i32, by: i32, clip: Vec4i) -> Vec4i {
        let x0 = pxc(min(max(ax, clip.x()), clip.z()));
        let y0 = pxc(min(max(ay, clip.y()), clip.w()));
        let x1 = pxc(min(max(bx, clip.x()), clip.z()));
        let y1 = pxc(min(max(by, clip.y()), clip.w()));
        if x0 < x1 && y0 < y1 {
            return vec4i(max(x0, -32768i32), max(y0, -32768i32), min(x1, 32767i32), min(y1, 32767i32));
        }
        vec4i(0i32, 0i32, 0i32, 0i32)
    }
    /// Pixels whose centres are at least `kc` inside the box (coverage provably ONE), within `win`.
    pub fn core_of(b: Vec4i, kc: i32, win: Vec4i) -> Vec4i {
        let x0 = max(pxc(b.x() + kc), win.x());
        let y0 = max(pxc(b.y() + kc), win.y());
        let x1 = min(((b.z() - kc - 128i32) >> 8u32) + 1i32, win.z());
        let y1 = min(((b.w() - kc - 128i32) >> 8u32) + 1i32, win.w());
        if x0 < x1 && y0 < y1 {
            return vec4i(x0, y0, x1, y1);
        }
        vec4i(0i32, 0i32, 0i32, 0i32)
    }
    /// The root shift for a corner radius: its legs (radius + the AA margin) must fit 14 bits.
    pub fn shift_of(r: i32) -> u32 {
        let mut s = 0u32;
        while ((r + 512i32) >> s) > 16383i32 {
            s += 1u32;
        }
        s
    }
    /// A radius clamped to half the box's shorter side.
    pub fn radius_in(b: Vec4i, r: i32) -> i32 {
        max(min(r, min(b.z() - b.x(), b.w() - b.y()) / 2i32), 0i32)
    }
    pub fn mk(kind: u32, sh: u32, win: Vec4i, core: Vec4i, b: Vec4i, rad: Vec4i, aux: u32, pm: Vec4u, pm2: Vec4u) -> Cmd {
        Cmd {
            win: win,
            core: core,
            b: b,
            par: vec4u(kind, sh, aux, 0u32),
            rad: rad,
            pm: pm,
            pm2: pm2,
            xi: vec4i(0i32, 0i32, 0i32, 0i32),
            xk: vec4i(0i32, 0i32, 0i32, 0i32),
            xs: 0i32,
        }
    }
    /// A solid box: every pixel whose centre it holds takes `pm`.
    pub fn mk_solid(b: Vec4i, pm: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x(), b.y(), b.z(), b.w(), clip);
        mk(K_SOLID, 0u32, win, win, b, vec4i(0i32, 0i32, 0i32, 0i32), 0u32, pm, splat(0u32))
    }
    pub fn mk_rrect(b: Vec4i, r0: i32, pm: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x() - 256i32, b.y() - 256i32, b.z() + 256i32, b.w() + 256i32, clip);
        let r = radius_in(b, r0);
        mk(K_RRECT, shift_of(r), win, core_of(b, max(r, 128i32) + 136i32, win), b, vec4i(r, 0i32, 0i32, 0i32), 0u32, pm, splat(0u32))
    }
    /// A rounded box filled with `fill` carrying a border of `bw` (Q8) in `pm` inside its edge.
    pub fn mk_border(b: Vec4i, r0: i32, bw0: i32, pm: Vec4u, fill: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x() - 256i32, b.y() - 256i32, b.z() + 256i32, b.w() + 256i32, clip);
        let r = radius_in(b, r0);
        let bw = max(bw0, 0i32);
        mk(K_BORDER, shift_of(r), win, core_of(b, max(r, 128i32 + bw) + 136i32, win), b, vec4i(r, bw, 0i32, 0i32), 0u32, pm, fill)
    }
    /// The shadow of a rounded box blurred by a spline of width `blur` (Q8, two sigma).
    pub fn mk_shadow(b: Vec4i, r0: i32, blur: i32, pm: Vec4u, clip: Vec4i) -> Cmd {
        let bw = min(max(blur, 16i32), 32768i32);
        let ext = bw + bw / 2i32 + 256i32;
        let win = window(b.x() - ext, b.y() - ext, b.z() + ext, b.w() + ext, clip);
        let r = radius_in(b, r0);
        let inv = (16777216i32 + bw - 1i32) / bw;
        let mut curve = 0i32;
        if r > 0i32 {
            curve = min((bw * 32768i32) / (8i32 * r), 32768i32);
        }
        mk(K_SHADOW, shift_of(r), win, core_of(b, bw + bw / 2i32 + r + 2i32, win), b, vec4i(r, bw, inv, curve), 0u32, pm, splat(0u32))
    }
    /// A two-stop linear gradient in a rounded box; (dx, dy) is the direction scaled so the parameter
    /// reaches ONE across the box (computed where the box's size is known).
    pub fn mk_grad(b: Vec4i, r0: i32, dx: i32, dy: i32, pm: Vec4u, pm2: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x() - 256i32, b.y() - 256i32, b.z() + 256i32, b.w() + 256i32, clip);
        let r = radius_in(b, r0);
        mk(K_GRAD, shift_of(r), win, core_of(b, max(r, 128i32) + 136i32, win), b, vec4i(r, 0i32, dx, dy), 0u32, pm, pm2)
    }
    /// A glyph: the box is its texels' box (whole pixels), `texel` its first texel in the atlas.
    pub fn mk_glyph(b: Vec4i, texel: u32, pm: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x(), b.y(), b.z(), b.w(), clip);
        mk(K_GLYPH, 0u32, win, vec4i(0i32, 0i32, 0i32, 0i32), b, vec4i(0i32, 0i32, 0i32, 0i32), texel, pm, splat(0u32))
    }

    pub fn c_win(c: Cmd) -> Vec4i {
        c.win
    }
    pub fn c_core(c: Cmd) -> Vec4i {
        c.core
    }

    // ---- one guarded shade per primitive: the kernel's definition restricted to a kind the type fixed ----

    pub fn p_solid(c: Cmd, x: i32, y: i32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        c.pm
    }
    pub fn p_rrect(c: Cmd, x: i32, y: i32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        if inside(c.core, x, y) {
            return c.pm;
        }
        sh_rrect(c, x * 256i32 + 128i32, y * 256i32 + 128i32)
    }
    pub fn p_border(c: Cmd, x: i32, y: i32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        if inside(c.core, x, y) {
            return c.pm2;
        }
        sh_border(c, x * 256i32 + 128i32, y * 256i32 + 128i32)
    }
    pub fn p_shadow(c: Cmd, x: i32, y: i32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        if inside(c.core, x, y) {
            return c.pm;
        }
        sh_shadow(c, x * 256i32 + 128i32, y * 256i32 + 128i32)
    }
    pub fn p_grad(c: Cmd, x: i32, y: i32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        if inside(c.core, x, y) {
            return grad_color(c, x * 256i32 + 128i32, y * 256i32 + 128i32);
        }
        sh_grad(c, x * 256i32 + 128i32, y * 256i32 + 128i32)
    }
    /// The atlas index of the texel a glyph reads at (x, y); NO_TEXEL outside it.
    pub fn x_texel(c: Cmd, x: i32, y: i32, aw: u32) -> u32 {
        if !inside(c.win, x, y) {
            return NO_TEXEL;
        }
        texel(c, x, y, aw)
    }
    /// A glyph from its coverage byte (0 where there is no texel).
    pub fn p_glyph(c: Cmd, x: i32, y: i32, cov8: u32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        scale(c.pm, q15(cov8))
    }
}
