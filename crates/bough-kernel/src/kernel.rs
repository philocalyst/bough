/// The one kernel. Everything a pixel's value depends on is defined here, once, in integers:
///
/// ```text
/// pixel(x, y) = store8(fold(blend, 0, [shade(cmd, x, y, texel) for cmd in program]))
/// ```
///
/// * colour: premultiplied RGBA in Q15 (`ONE` = 32768 is exactly 1.0, so `mul15(v, ONE) == v` and
///   `mul15(v, 0) == 0`);
/// * geometry: Q8 (1/256 px); pixel centres are `256 x + 128`;
/// * a float is used once, as the first guess of `isqrt`, and an integer check corrects it: no result depends
///   on how any float operation rounds, contracts or approximates.
///
/// This source is compiled twice: by rustc for the CPU executors (`cpu.rs` calls these functions; the loops
/// vectorise), and through wgsl-rs -> WGSL -> naga -> MSL for the GPU (`fs_ktile`). The identities the
/// executors rely on (lib.rs "Identities") are properties of these functions and are tested as such.
/// User shader nodes. This module stands where an application's own would: plain Rust in the wgsl-rs
/// subset, imported by the kernel, so a user node runs on every executor from this one source.
#[wgsl_rs::wgsl]
pub mod fx {
    use wgsl_rs::std::*;

    /// `id` selects the node's shader. (x, y) is the point in the box's frame and (w, h) the box, all Q8;
    /// `now` is the presenter's clock; a and b are the node's two premultiplied Q15 colours.
    pub fn user_shade(id: u32, x: i32, y: i32, w: i32, h: i32, now: u32, a: Vec4u, b: Vec4u) -> Vec4u {
        if id == 0u32 {
            return stripes(x, y, now, a, b);
        }
        ripple(x, y, w, h, now, a, b)
    }
    /// Diagonal stripes, 16 px period, sliding one pixel per 16 ticks, with a 1 px soft edge.
    pub fn stripes(x: i32, y: i32, now: u32, a: Vec4u, b: Vec4u) -> Vec4u {
        let ph = ((x + y + ((now & 0xFFFFu32) as i32) * 16i32) & 4095i32) as u32;
        // a triangle wave of the phase, sharpened to a 1 px ramp around its midpoint
        let tri = min(ph, 4096u32 - ph);
        let t = (clamp((tri as i32) - 896i32, 0i32, 256i32) as u32) << 7u32;
        lerp15(a, b, t)
    }
    /// Concentric rings about the box centre moving outward with time.
    pub fn ripple(x: i32, y: i32, w: i32, h: i32, now: u32, a: Vec4u, b: Vec4u) -> Vec4u {
        let dx = (abs(x - (w >> 1u32)) >> 4u32) as u32;
        let dy = (abs(y - (h >> 1u32)) >> 4u32) as u32;
        let d2 = min(dx, 16383u32) * min(dx, 16383u32) + min(dy, 16383u32) * min(dy, 16383u32);
        // squared distance in (1/16 px)^2: rings equally spaced in d^2 need no root
        let ph = ((d2 >> 6u32) + 0x10000u32 - (now & 0x1FFFu32) * 8u32) & 1023u32;
        let tri = min(ph, 1024u32 - ph);
        lerp15(a, b, tri << 6u32)
    }
    pub fn lerp15(a: Vec4u, b: Vec4u, t: u32) -> Vec4u {
        let k = 32768u32 - t;
        vec4u(
            (a.x() * k + b.x() * t + 16384u32) >> 15u32,
            (a.y() * k + b.y() * t + 16384u32) >> 15u32,
            (a.z() * k + b.z() * t + 16384u32) >> 15u32,
            (a.w() * k + b.w() * t + 16384u32) >> 15u32,
        )
    }
}

#[wgsl_rs::wgsl]
pub mod kernel {
    use super::fx::*;
    use wgsl_rs::std::*;

    storage!(group(0), binding(0), CMDS: RuntimeArray<u32>);
    storage!(group(0), binding(1), PROG: RuntimeArray<u32>);
    storage!(group(0), binding(2), ATLAS: RuntimeArray<u32>);
    storage!(group(0), binding(3), read_write, OUT: RuntimeArray<u32>);

    /// 1.0 in Q15.
    pub const ONE: u32 = 32768u32;
    pub const HALF: u32 = 16384u32;
    /// Words of a packed command (`load`).
    pub const CMD_WORDS: u32 = 20u32;
    /// Words of the program header; the tile table (4 words per tile: x, y, list offset, list length) follows.
    pub const PROG_HDR: u32 = 8u32;
    pub const K_SOLID: u32 = 0u32;
    pub const K_RRECT: u32 = 1u32;
    pub const K_BORDER: u32 = 2u32;
    pub const K_GLYPH: u32 = 3u32;
    pub const K_SHADOW: u32 = 4u32;
    pub const K_GRAD: u32 = 5u32;
    /// A user shader node (`fx::user_shade`): coverage and colour are the user's, the window, the blend and
    /// the store are the kernel's.
    pub const K_USER: u32 = 6u32;
    /// Word 0 flag: the command is a transition. The next slot holds its end state; word 3 of this slot is
    /// the start time and word 3 of the next is (duration << 2 | ease), times in presenter ticks.
    pub const F_ANIM: u32 = 0x10000u32;
    pub const NO_TEXEL: u32 = 0xFFFFFFFFu32;

    /// A staged command. `par` = (kind, isqrt shift, aux0, aux1); `rad` = (radius, border width, aux2, aux3):
    /// * glyph: aux0 = index of its first texel in the atlas, aux2/aux3 = its size in px;
    /// * shadow: `rad.y` = blur width w (Q8, two sigma), aux2 = 2^24 / w;
    /// * gradient: aux2/aux3 = direction scaled so that `t` reaches ONE across the box.
    #[derive(Clone, Copy, Wgsl)]
    pub struct Cmd {
        /// Pixel window (x0, y0, x1, y1), exclusive: `shade` is 0 outside it.
        pub win: Vec4i,
        /// Pixels whose `shade` is provably `core_value` (may be empty).
        pub core: Vec4i,
        /// Geometry edges (x0, y0, x1, y1) in Q8.
        pub b: Vec4i,
        pub par: Vec4u,
        pub rad: Vec4i,
        /// Premultiplied Q15 colours.
        pub pm: Vec4u,
        pub pm2: Vec4u,
    }

    pub fn splat(v: u32) -> Vec4u {
        vec4u(v, v, v, v)
    }
    /// 8-bit unorm to Q15 (255 -> ONE exactly; monotone; `ch8(q15(t)) == t`).
    pub fn q15(t: u32) -> u32 {
        (t << 7u32) + (t >> 1u32) + (t >> 7u32)
    }
    /// Q15 product, round half up. `mul15(a, ONE) == a`, `mul15(a, 0) == 0`, monotone in both arguments.
    pub fn mul15(a: u32, b: u32) -> u32 {
        (a * b + HALF) >> 15u32
    }
    pub fn scale(v: Vec4u, k: u32) -> Vec4u {
        vec4u(mul15(v.x(), k), mul15(v.y(), k), mul15(v.z(), k), mul15(v.w(), k))
    }
    /// Packed RGBA8 (r | g << 8 | b << 16 | a << 24) to premultiplied Q15.
    pub fn premul(c: u32) -> Vec4u {
        let a = q15(c >> 24u32);
        vec4u(mul15(q15(c & 255u32), a), mul15(q15((c >> 8u32) & 255u32), a), mul15(q15((c >> 16u32) & 255u32), a), a)
    }
    /// Source over. With `sh.rgb <= sh.a <= ONE` and `acc <= ONE` the result is `<= ONE` (no clamp exists).
    /// `blend(acc, 0) == acc`; `blend(acc, sh) == sh` when `sh.a == ONE`.
    pub fn blend(acc: Vec4u, sh: Vec4u) -> Vec4u {
        let k = ONE - sh.w();
        vec4u(sh.x() + mul15(acc.x(), k), sh.y() + mul15(acc.y(), k), sh.z() + mul15(acc.z(), k), sh.w() + mul15(acc.w(), k))
    }
    /// Q15 to 8-bit unorm, round half up.
    pub fn ch8(v: u32) -> u32 {
        (v * 255u32 + HALF) >> 15u32
    }
    /// The stored pixel: BGRA8 as one little-endian word.
    pub fn store8(acc: Vec4u) -> u32 {
        ch8(acc.z()) | (ch8(acc.y()) << 8u32) | (ch8(acc.x()) << 16u32) | (ch8(acc.w()) << 24u32)
    }

    /// floor(sqrt(s)) for s < 2^30. The float root is a guess (any implementation within a few units of the
    /// true root will do: a hardware reciprocal estimate, fast math); the integer comparisons decide.
    pub fn isqrt(s: u32) -> u32 {
        let mut r = sqrt(s as f32) as u32;
        if r * r > s {
            r -= 1u32;
        }
        if r * r > s {
            r -= 1u32;
        }
        if (r + 1u32) * (r + 1u32) <= s {
            r += 1u32;
        }
        if (r + 1u32) * (r + 1u32) <= s {
            r += 1u32;
        }
        r
    }

    /// Signed distance (Q8) from the point (cx, cy) to the rounded box `b` of corner radius `r` (all Q8), in
    /// edge form. `k` is the command's root shift: the corner's legs are measured in units of 2^k / 256 px
    /// (k = 0 up to a 62 px radius).
    pub fn sd(cx: i32, cy: i32, b: Vec4i, r: i32, k: u32) -> i32 {
        let qx = max(cx - b.z(), b.x() - cx) + r;
        let qy = max(cy - b.w(), b.y() - cy) + r;
        let mx = min((max(qx, 0i32) as u32) >> k, 16383u32);
        let my = min((max(qy, 0i32) as u32) >> k, 16383u32);
        let len = (isqrt(mx * mx + my * my) << k) as i32;
        len + min(max(qx, qy), 0i32) - r
    }
    /// Coverage (Q15) of a pixel whose centre is at signed distance `d` (Q8) from an edge: a 1 px ramp.
    pub fn cov(d: i32) -> u32 {
        (clamp(128i32 - d, 0i32, 256i32) as u32) << 7u32
    }

    pub fn inside(w: Vec4i, x: i32, y: i32) -> bool {
        x >= w.x() && x < w.z() && y >= w.y() && y < w.w()
    }

    pub fn sh_rrect(c: Cmd, cx: i32, cy: i32) -> Vec4u {
        scale(c.pm, cov(sd(cx, cy, c.b, c.rad.x(), c.par.y())))
    }
    /// A rounded box of colour `pm2` with a border of width `rad.y` and colour `pm` inside its edge.
    pub fn sh_border(c: Cmd, cx: i32, cy: i32) -> Vec4u {
        let d = sd(cx, cy, c.b, c.rad.x(), c.par.y());
        let outer = cov(d);
        let inner = min(cov(d + c.rad.y()), outer);
        scale(c.pm, outer - inner) + scale(c.pm2, inner)
    }
    /// A two-stop linear gradient (`pm` to `pm2`, both premultiplied) in a rounded box.
    pub fn sh_grad(c: Cmd, cx: i32, cy: i32) -> Vec4u {
        let px = (cx - ((c.b.x() + c.b.z()) >> 1u32)) >> 4u32;
        let py = (cy - ((c.b.y() + c.b.w()) >> 1u32)) >> 4u32;
        let t = clamp(((px * c.rad.z() + py * c.rad.w()) >> 15u32) + 16384i32, 0i32, 32768i32) as u32;
        let col = scale(c.pm, ONE - t) + scale(c.pm2, t);
        scale(col, cov(sd(cx, cy, c.b, c.rad.x(), c.par.y())))
    }

    /// The blur kernel is the quadratic B-spline (three box blurs of width w; variance w^2 / 4, so w = two
    /// sigma; support +-1.5 w): its CDF and density are sums of truncated powers, exact in integers.
    /// `u` is the offset in units of w (Q8). Returns Q15; `cdf3(-384) == 0`, `cdf3(384) == ONE`.
    pub fn cdf3(u0: i32) -> u32 {
        let u = clamp(u0, -384i32, 384i32);
        let a = u + 384i32;
        let b = max(u + 128i32, 0i32);
        let c = max(u - 128i32, 0i32);
        ((a * a * a - 3i32 * b * b * b + 3i32 * c * c * c) as u32) / 3072u32
    }
    /// The spline's density at `u` (Q8), Q15 (0.75 at 0).
    pub fn pdf3(u0: i32) -> u32 {
        let u = clamp(u0, -384i32, 384i32);
        let a = u + 384i32;
        let b = max(u + 128i32, 0i32);
        let c = max(u - 128i32, 0i32);
        ((a * a - 3i32 * b * b + 3i32 * c * c) as u32) >> 2u32
    }
    /// An offset `t` (Q8 px) in units of the blur width (Q8), saturated to the kernel's support.
    pub fn in_w(t: i32, w: i32, inv: i32) -> i32 {
        let lim = w + (w >> 1u32) + 1i32;
        (clamp(t, -lim, lim) * inv) >> 16u32
    }
    /// The blurred interval [-h, h] seen from `p`: CDF(p + h) - CDF(p - h).
    pub fn span3(p: i32, h: i32, w: i32, inv: i32) -> u32 {
        cdf3(in_w(p + h, w, inv)) - cdf3(in_w(p - h, w, inv))
    }
    /// Coverage of the blurred rounded box `c.b` at (cx, cy). A square-cornered box is separable and exact
    /// for this kernel; a rounded one integrates the blurred chord over four rows of the kernel's support
    /// (Evan Wallace's construction with the spline in place of the Gaussian).
    pub fn shadow(c: Cmd, cx: i32, cy: i32) -> u32 {
        let hx = (c.b.z() - c.b.x()) >> 1u32;
        let hy = (c.b.w() - c.b.y()) >> 1u32;
        let px = cx - (c.b.x() + hx);
        let py = cy - (c.b.y() + hy);
        let r = c.rad.x();
        let w = c.rad.y();
        let inv = c.rad.z();
        if r == 0i32 {
            return mul15(span3(px, hx, w, inv), span3(py, hy, w, inv));
        }
        let k = c.par.y();
        let lim = w + (w >> 1u32);
        let lo = clamp(-lim, py - hy, py + hy);
        let hi = clamp(lim, py - hy, py + hy);
        let step = (hi - lo) >> 2u32;
        let su = ((step * inv) >> 16u32) as u32;
        let rr = (r as u32) >> k;
        let mut v = 0u32;
        let mut y = lo + (step >> 1u32);
        let mut i = 0u32;
        while i < 4u32 {
            let delta = min((max(abs(py - y) - (hy - r), 0i32) as u32) >> k, rr);
            let curved = hx - r + ((isqrt(rr * rr - delta * delta) << k) as i32);
            v += (mul15(span3(px, curved, w, inv), pdf3(in_w(y, w, inv))) * su) >> 8u32;
            y += step;
            i += 1u32;
        }
        min(v, ONE)
    }
    pub fn sh_shadow(c: Cmd, cx: i32, cy: i32) -> Vec4u {
        scale(c.pm, shadow(c, cx, cy))
    }

    /// The atlas index of the texel a glyph command reads at pixel (x, y); NO_TEXEL for anything else.
    pub fn texel(c: Cmd, x: i32, y: i32, aw: u32) -> u32 {
        if c.par.x() != K_GLYPH {
            return NO_TEXEL;
        }
        let tx = (x * 256i32 + 128i32 - c.b.x()) >> 8u32;
        let ty = (y * 256i32 + 128i32 - c.b.y()) >> 8u32;
        if tx < 0i32 || ty < 0i32 || tx >= c.rad.z() || ty >= c.rad.w() {
            return NO_TEXEL;
        }
        c.par.z() + (ty as u32) * aw + (tx as u32)
    }

    /// The command's premultiplied source at pixel (x, y). `t` is the atlas coverage (0..255) at `texel`.
    pub fn shade(c: Cmd, x: i32, y: i32, t: u32, now: u32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        let kind = c.par.x();
        if kind == K_SOLID {
            return c.pm;
        }
        if kind == K_GLYPH {
            return scale(c.pm, q15(t));
        }
        let cx = x * 256i32 + 128i32;
        let cy = y * 256i32 + 128i32;
        if kind == K_RRECT {
            return sh_rrect(c, cx, cy);
        }
        if kind == K_BORDER {
            return sh_border(c, cx, cy);
        }
        if kind == K_SHADOW {
            return sh_shadow(c, cx, cy);
        }
        if kind == K_GRAD {
            return sh_grad(c, cx, cy);
        }
        if kind == K_USER {
            return sh_user(c, cx, cy, now);
        }
        splat(0u32)
    }
    /// A user node: the user's function returns a premultiplied Q15 colour for a point in the box's own
    /// frame (Q8 from its top-left corner); the kernel clips it to the node's rounded box.
    pub fn sh_user(c: Cmd, cx: i32, cy: i32, now: u32) -> Vec4u {
        let col = user_shade(c.par.z(), cx - c.b.x(), cy - c.b.y(), c.b.z() - c.b.x(), c.b.w() - c.b.y(), now, c.pm, c.pm2);
        scale(col, cov(sd(cx, cy, c.b, c.rad.x(), c.par.y())))
    }

    // ---- transitions: a command is two states and a clock; the kernel evaluates it at the presenter's `now` ----

    /// Progress (Q15) of a transition started at `t0` lasting `dur` ticks (>= 1), eased (0 linear, 1 smooth).
    pub fn progress(now: u32, t0: u32, dur: u32, ease: u32) -> u32 {
        if now <= t0 {
            return 0u32;
        }
        let e = min(now - t0, dur);
        if e == dur {
            return ONE;
        }
        // dur < 2^16 ticks: e * ONE < 2^31
        let t = (min(e, 65535u32) * ONE) / min(dur, 65535u32);
        if ease == 1u32 {
            return mul15(mul15(t, t), 3u32 * ONE - 2u32 * t);
        }
        t
    }
    /// a + (b - a) t for Q8 geometry without a 64-bit product (the difference is split at bit 10).
    pub fn mix_i(a: i32, b: i32, t: u32) -> i32 {
        let d = b - a;
        let ti = t as i32;
        a + (((d >> 10u32) * ti) >> 5u32) + (((d & 1023i32) * ti) >> 15u32)
    }
    pub fn mix4i(a: Vec4i, b: Vec4i, t: u32) -> Vec4i {
        vec4i(mix_i(a.x(), b.x(), t), mix_i(a.y(), b.y(), t), mix_i(a.z(), b.z(), t), mix_i(a.w(), b.w(), t))
    }
    pub fn mix4u(a: Vec4u, b: Vec4u, t: u32) -> Vec4u {
        scale(a, ONE - t) + scale(b, t)
    }
    /// The state of the transition from `a` to `b` at progress `t`. Window, core, kind and shift are `a`'s:
    /// staging gives `a` the swept window and the common core, so the tile program is the same for the whole
    /// transition (a superset program is exact) and only `now` changes from frame to frame.
    pub fn at(a: Cmd, b: Cmd, t: u32) -> Cmd {
        Cmd {
            win: a.win,
            core: a.core,
            b: mix4i(a.b, b.b, t),
            par: a.par,
            rad: vec4i(mix_i(a.rad.x(), b.rad.x(), t), mix_i(a.rad.y(), b.rad.y(), t), a.rad.z(), a.rad.w()),
            pm: mix4u(a.pm, b.pm, t),
            pm2: mix4u(a.pm2, b.pm2, t),
        }
    }
    /// What `shade` returns at every pixel of `c.core`.
    pub fn core_value(c: Cmd) -> Vec4u {
        if c.par.x() == K_BORDER {
            return c.pm2;
        }
        c.pm
    }

    // ---- the GPU composition: one workgroup per scheduled tile, 16 x 16 invocations x 2 x 2 pixels ----

    pub fn unpack_i16x4(a: u32, b: u32) -> Vec4i {
        vec4i(((a << 16u32) as i32) >> 16u32, (a as i32) >> 16u32, ((b << 16u32) as i32) >> 16u32, (b as i32) >> 16u32)
    }
    pub fn unpack_u16x4(a: u32, b: u32) -> Vec4u {
        vec4u(a & 65535u32, a >> 16u32, b & 65535u32, b >> 16u32)
    }
    /// A command from its 20 words (five groups of four): head (kind | shift << 8 | flags, aux0, aux1, time),
    /// window and core (i16 x 8), geometry, radii and aux2/aux3, the two colours (u16 x 8).
    pub fn decode(h: Vec4u, wc: Vec4u, g: Vec4u, r: Vec4u, p: Vec4u) -> Cmd {
        Cmd {
            win: unpack_i16x4(wc.x(), wc.y()),
            core: unpack_i16x4(wc.z(), wc.w()),
            b: vec4i(g.x() as i32, g.y() as i32, g.z() as i32, g.w() as i32),
            par: vec4u(h.x() & 255u32, (h.x() >> 8u32) & 255u32, h.y(), h.z()),
            rad: vec4i(r.x() as i32, r.y() as i32, r.z() as i32, r.w() as i32),
            pm: unpack_u16x4(p.x(), p.y()),
            pm2: unpack_u16x4(p.z(), p.w()),
        }
    }
    /// The transition whose two states are `a` and `b` (heads `ha`, `hb`) as it is at `now`.
    pub fn timed(a: Cmd, b: Cmd, ha: Vec4u, hb: Vec4u, now: u32) -> Cmd {
        at(a, b, progress(now, ha.w(), hb.w() >> 2u32, hb.w() & 3u32))
    }
    pub fn word4(w: u32) -> Vec4u {
        vec4u(get!(CMDS)[w], get!(CMDS)[w + 1u32], get!(CMDS)[w + 2u32], get!(CMDS)[w + 3u32])
    }
    pub fn load(i: u32) -> Cmd {
        let w = i * CMD_WORDS;
        decode(word4(w), word4(w + 4u32), word4(w + 8u32), word4(w + 12u32), word4(w + 16u32))
    }
    /// Command `i` as it is at `now`.
    pub fn load_at(i: u32, now: u32) -> Cmd {
        let a = load(i);
        let ha = word4(i * CMD_WORDS);
        if (ha.x() & F_ANIM) == 0u32 {
            return a;
        }
        timed(a, load(i + 1u32), ha, word4((i + 1u32) * CMD_WORDS), now)
    }
    pub fn fetch(c: Cmd, x: i32, y: i32, aw: u32) -> u32 {
        let t = texel(c, x, y, aw);
        if t == NO_TEXEL {
            return 0u32;
        }
        (get!(ATLAS)[t >> 2u32] >> ((t & 3u32) * 8u32)) & 255u32
    }

    #[compute]
    #[workgroup_size(16, 16, 1)]
    pub fn fs_ktile(#[builtin(workgroup_id)] wg: Vec3u, #[builtin(local_invocation_id)] lid: Vec3u) {
        let fw = get!(PROG)[0u32];
        let fh = get!(PROG)[1u32];
        let stride = get!(PROG)[2u32];
        let aw = get!(PROG)[3u32];
        let now = get!(PROG)[4u32];
        let tw = PROG_HDR + wg.x() * 4u32;
        let off = get!(PROG)[tw + 2u32];
        let n = get!(PROG)[tw + 3u32];
        let ux = get!(PROG)[tw] + 2u32 * lid.x();
        let uy = get!(PROG)[tw + 1u32] + 2u32 * lid.y();
        let x = ux as i32;
        let y = uy as i32;
        let mut a0 = splat(0u32);
        let mut a1 = splat(0u32);
        let mut a2 = splat(0u32);
        let mut a3 = splat(0u32);
        let mut i = 0u32;
        while i < n {
            let c = load_at(get!(PROG)[off + i], now);
            if x + 1i32 >= c.win.x() && x < c.win.z() && y + 1i32 >= c.win.y() && y < c.win.w() {
                if x >= c.core.x() && x + 1i32 < c.core.z() && y >= c.core.y() && y + 1i32 < c.core.w() {
                    let cv = core_value(c);
                    a0 = blend(a0, cv);
                    a1 = blend(a1, cv);
                    a2 = blend(a2, cv);
                    a3 = blend(a3, cv);
                } else {
                    a0 = blend(a0, shade(c, x, y, fetch(c, x, y, aw), now));
                    a1 = blend(a1, shade(c, x + 1i32, y, fetch(c, x + 1i32, y, aw), now));
                    a2 = blend(a2, shade(c, x, y + 1i32, fetch(c, x, y + 1i32, aw), now));
                    a3 = blend(a3, shade(c, x + 1i32, y + 1i32, fetch(c, x + 1i32, y + 1i32, aw), now));
                }
            }
            i += 1u32;
        }
        if ux < fw && uy < fh {
            get_mut!(OUT)[uy * stride + ux] = store8(a0);
        }
        if ux + 1u32 < fw && uy < fh {
            get_mut!(OUT)[uy * stride + ux + 1u32] = store8(a1);
        }
        if ux < fw && uy + 1u32 < fh {
            get_mut!(OUT)[(uy + 1u32) * stride + ux] = store8(a2);
        }
        if ux + 1u32 < fw && uy + 1u32 < fh {
            get_mut!(OUT)[(uy + 1u32) * stride + ux + 1u32] = store8(a3);
        }
    }
}
