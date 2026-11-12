/// The binned 32 x 32 compute repair (bough-metal RESULTS 13.1, Part IV; INTERFACE.md "Fused layout"), ported from
/// shader.metal's `fs_bin` / `fs_tile` to wgsl-rs. Region data is read as u32 words; an `NRect` (i16 x 4: x0, y0,
/// -x1, -y1) is two words; every index read from region data is ANDed with its column's mask (bounds by
/// construction). Colour is accumulated in f32 (the direct kernel uses half).
#[wgsl_rs::wgsl]
pub mod raster {
    use wgsl_rs::std::*;

    storage!(group(0), binding(0), POOL: RuntimeArray<u32>);
    storage!(group(0), binding(1), read_write, BINS: RuntimeArray<Vec4u>);
    storage!(group(0), binding(2), read_write, COUNTS: RuntimeArray<Atomic<u32>>);
    texture!(group(0), binding(3), T0: TextureStorage2D<Bgra8unorm, Write>);
    texture!(group(0), binding(4), T1: TextureStorage2D<Bgra8unorm, Write>);
    texture!(group(0), binding(5), T2: TextureStorage2D<Bgra8unorm, Write>);
    texture!(group(0), binding(6), T3: TextureStorage2D<Bgra8unorm, Write>);
    texture!(group(0), binding(11), ATLAS: Texture2D<f32>);
    uniform!(group(1), binding(0), SLOT: Vec4u);

    workgroup!(SGC: [u32; 8]);
    workgroup!(FLO: [u32; 8]);
    workgroup!(DL: [u32; 256]);
    workgroup!(GL: [u32; 256]);
    workgroup!(HL: [u32; 256]);
    workgroup!(HK: [Vec2u; 256]);
    workgroup!(HC: [u32; 256]);
    workgroup!(RQ: [Vec4f; 128]);
    workgroup!(RCORE: [Vec4f; 128]);
    workgroup!(RB: [Vec4f; 128]);
    workgroup!(RPM: [Vec4f; 128]);
    workgroup!(RPM2: [Vec4f; 128]);
    workgroup!(RPAR: [Vec4u; 128]);

    pub const F_RUN: u32 = 0x80000000u32;
    pub const F_IRUN: u32 = 0x40000000u32;
    /// Words of a page (1472 B) and of its columns' starts.
    pub const PG_WORDS: u32 = 368u32;
    pub const PG_INK: u32 = 32u32;
    pub const PG_GINK: u32 = 160u32;
    pub const PG_PAYLOAD: u32 = 192u32;
    pub const PG_CLIP: u32 = 320u32;
    pub const PG_OFFL: u32 = 359u32;
    /// Header words (`FHdr`).
    pub const H_TILES_Y: u32 = 3u32;
    pub const H_INST: u32 = 4u32;
    pub const H_SPACES: u32 = 5u32;
    pub const H_CLIPS: u32 = 6u32;
    pub const H_DIR_ID: u32 = 7u32;
    pub const H_DIR_INK: u32 = 8u32;
    pub const H_N_DIR: u32 = 9u32;
    pub const H_PAGES: u32 = 10u32;
    pub const H_GX: u32 = 11u32;
    pub const H_GENT: u32 = 12u32;
    pub const H_RUNS: u32 = 13u32;
    pub const H_TOKS: u32 = 14u32;
    pub const H_TOK_STRIDE: u32 = 15u32;
    pub const H_BIN_CAP: u32 = 16u32;
    pub const H_DEBUG: u32 = 17u32;
    pub const H_M_INST: u32 = 18u32;
    pub const H_M_PAGE: u32 = 19u32;
    pub const H_M_GLYPH: u32 = 20u32;
    pub const H_M_RUN: u32 = 21u32;
    pub const H_M_SPACE: u32 = 22u32;
    pub const H_M_CLIP: u32 = 23u32;
    pub const H_M_STRIKE: u32 = 24u32;

    pub fn hw(i: u32) -> u32 {
        get!(POOL)[i]
    }
    /// An NRect at word `w` as i32 lanes (x0, y0, -x1, -y1).
    pub fn nrect(w: u32) -> Vec4i {
        let a = get!(POOL)[w];
        let b = get!(POOL)[w + 1u32];
        vec4i(((a << 16u32) as i32) >> 16u32, (a as i32) >> 16u32, ((b << 16u32) as i32) >> 16u32, (b as i32) >> 16u32)
    }
    pub fn pack2(x: i32, y: i32) -> u32 {
        ((x as u32) & 65535u32) | ((y as u32) << 16u32)
    }
    pub fn unpack_rect(a: u32, b: u32) -> Vec4i {
        vec4i(((a << 16u32) as i32) >> 16u32, (a as i32) >> 16u32, ((b << 16u32) as i32) >> 16u32, (b as i32) >> 16u32)
    }
    /// Overlap test of the core's `Thresh::matches`: all(r < th).
    pub fn lt4(r: Vec4i, th: Vec4i) -> bool {
        r.x() < th.x() && r.y() < th.y() && r.z() < th.z() && r.w() < th.w()
    }
    /// Saturating i16 subtraction, lane-wise.
    pub fn subsat(a: Vec4i, b: Vec4i) -> Vec4i {
        clamp(a - b, vec4i(-32768i32, -32768i32, -32768i32, -32768i32), vec4i(32767i32, 32767i32, 32767i32, 32767i32))
    }
    pub fn clip_rect(c: u32) -> Vec4f {
        let r = nrect(hw(H_CLIPS) / 4u32 + 2u32 * (c & hw(H_M_CLIP)));
        vec4f(r.x() as f32, r.y() as f32, -(r.z() as f32), -(r.w() as f32))
    }
    pub fn space(s: u32) -> Vec2f {
        let w = hw(H_SPACES) / 4u32 + 2u32 * (s & hw(H_M_SPACE));
        vec2f((get!(POOL)[w] as i32) as f32, (get!(POOL)[w + 1u32] as i32) as f32)
    }
    pub fn unpack_color(c: u32) -> Vec4f {
        vec4f((c & 255u32) as f32, ((c >> 8u32) & 255u32) as f32, ((c >> 16u32) & 255u32) as f32, (c >> 24u32) as f32) * (1.0 / 255.0)
    }
    pub fn premul(c: u32) -> Vec4f {
        let v = unpack_color(c);
        vec4f(v.x() * v.w(), v.y() * v.w(), v.z() * v.w(), v.w())
    }
    pub fn qink(qa: Vec2f, qb: Vec2f) -> Vec4i {
        vec4i(floor(qa.x()) as i32, floor(qa.y()) as i32, -(ceil(qb.x()) as i32), -(ceil(qb.y()) as i32))
    }

    /// A glyph of run `run`, slot `slot` (bough-text's pen formula): (p0.x, p0.y, w, h) and in `meta` (color, uv
    /// packed, visible). Returns w = -1 when the glyph has no bitmap.
    #[derive(Clone, Copy, Wgsl)]
    pub struct Glyph {
        pub geo: Vec4f,
        pub info: Vec2u,
    }
    pub fn glyph(run: u32, slot: u32) -> Glyph {
        let rw = hw(H_RUNS) / 4u32 + 8u32 * (run & hw(H_M_RUN));
        let s = slot & hw(H_M_GLYPH);
        let x = get!(POOL)[hw(H_GX) / 4u32 + s] as i32;
        let gw = get!(POOL)[hw(H_GENT) / 4u32 + (s >> 1u32)];
        let e = (gw >> ((s & 1u32) * 16u32)) & 255u32;
        let sw = get!(POOL)[rw + 4u32];
        let strike = (sw >> 16u32) & hw(H_M_STRIKE);
        let ew = (hw(H_TOKS) + strike * hw(H_TOK_STRIDE)) / 4u32 + e * 8u32;
        let uvz = get!(POOL)[ew + 5u32] & 65535u32;
        if uvz == 0u32 {
            return Glyph { geo: vec4f(0.0, 0.0, -1.0, -1.0), info: vec2u(0u32, 0u32) };
        }
        let dx = x - (get!(POOL)[rw + 7u32] as i32);
        let scale = bitcast_f32(get!(POOL)[rw + 2u32]);
        let ox = bitcast_f32(get!(POOL)[rw]);
        let oy = bitcast_f32(get!(POOL)[rw + 1u32]);
        let gx = bitcast_f32(get!(POOL)[ew]);
        let gy = bitcast_f32(get!(POOL)[ew + 1u32]);
        let gwid = bitcast_f32(get!(POOL)[ew + 2u32]);
        let ght = bitcast_f32(get!(POOL)[ew + 3u32]);
        let px = floor(fma(dx as f32, scale, ox + 0.5)) + gx;
        let sp = space(sw & 65535u32);
        Glyph { geo: vec4f(px + sp.x(), oy + gy + sp.y(), gwid, ght), info: vec2u(get!(POOL)[rw + 3u32], get!(POOL)[ew + 4u32]) }
    }

    /// Ordered compaction of `hit` across the workgroup: (this invocation's slot, total).
    pub fn compact(hit: bool, lane: u32, sg: u32) -> Vec2u {
        let h = select(0u32, 1u32, hit);
        let pre = subgroup_exclusive_add(h);
        let cnt = subgroup_add(h);
        if lane == 0u32 {
            get_mut!(SGC)[sg as usize] = cnt;
        }
        workgroup_barrier();
        let mut off = 0u32;
        let mut total = 0u32;
        let mut s = 0u32;
        while s < 8u32 {
            let v = get!(SGC)[s as usize];
            if s < sg {
                off += v;
            }
            total += v;
            s += 1u32;
        }
        vec2u(off + pre, total)
    }

    pub fn bin_put(at: u32, cap: u32, base: u32, v: Vec4u) {
        if at < cap {
            get_mut!(BINS)[base + at] = v;
        }
    }

    /// The bin pass: one workgroup per damaged tile row. Cull the directory -> 8-record groups -> records against
    /// the row's damaged span, append hits to the row's bin in paint order, expanding text runs and instance runs
    /// to their glyphs / instances that meet the row (256 at a time).
    #[compute]
    #[workgroup_size(256, 1, 1)]
    pub fn fs_bin(
        #[builtin(workgroup_id)] wg: Vec3u,
        #[builtin(local_invocation_index)] li: u32,
        #[builtin(subgroup_invocation_id)] lane: u32,
        #[builtin(subgroup_id)] sg: u32,
    ) {
        let rw = get!(SLOT).x() / 4u32 + 16u32 + 8u32 * wg.x();
        let ty = get!(POOL)[rw];
        let span = get!(POOL)[rw + 1u32];
        let tx0 = span & 255u32;
        let tx1 = (span >> 8u32) & 255u32;
        let th = vec4i((tx1 * 32u32) as i32, (ty * 32u32 + 32u32) as i32, -((tx0 * 32u32) as i32), -((ty * 32u32) as i32));
        let cap = hw(H_BIN_CAP);
        let base = ty * cap;
        let ndir = min(hw(H_N_DIR), 2u32 * (hw(H_M_PAGE) + 1u32)); // directory columns hold 2x pages
        let pages = hw(H_PAGES) / 4u32;
        let mut nb = 0u32;
        let mut d0 = 0u32;
        while d0 < ndir {
            let di = d0 + li;
            let mut dh = false;
            if di < ndir {
                dh = lt4(nrect(hw(H_DIR_INK) / 4u32 + 2u32 * di), th);
            }
            workgroup_barrier();
            let dc = compact(dh, lane, sg);
            if dh {
                get_mut!(DL)[dc.x() as usize] = get!(POOL)[hw(H_DIR_ID) / 4u32 + di] & hw(H_M_PAGE);
            }
            workgroup_barrier();
            let nd = dc.y();
            let mut p0 = 0u32;
            while p0 < nd {
                let pk = p0 + (li >> 3u32);
                let g = li & 7u32;
                let mut gh = false;
                let mut gid = 0u32;
                if pk < nd {
                    let pid = get!(DL)[pk as usize];
                    gid = pid * 8u32 + g;
                    let pg = pages + pid * PG_WORDS;
                    let thp = subsat(th, nrect(pg + PG_OFFL));
                    gh = lt4(nrect(pg + PG_GINK + 2u32 * g), thp);
                }
                workgroup_barrier();
                let gc = compact(gh, lane, sg);
                if gh {
                    get_mut!(GL)[gc.x() as usize] = gid;
                }
                workgroup_barrier();
                let ng = gc.y();
                let mut q0 = 0u32;
                while q0 < ng {
                    let gk = q0 + (li >> 3u32);
                    let s = li & 7u32;
                    let mut rh = false;
                    let mut ii = 0u32;
                    let mut cid = 0u32;
                    let mut ink = vec4i(0i32, 0i32, 0i32, 0i32);
                    if gk < ng {
                        let gidv = get!(GL)[gk as usize];
                        let pid = gidv >> 3u32;
                        let sl = (gidv & 7u32) * 8u32 + s;
                        let pg = pages + pid * PG_WORDS;
                        let offl = nrect(pg + PG_OFFL);
                        let rel = nrect(pg + PG_INK + 2u32 * sl);
                        rh = lt4(rel, subsat(th, offl));
                        ink = rel + offl;
                        ii = get!(POOL)[pg + PG_PAYLOAD + sl];
                        cid = ((get!(POOL)[pg + PG_CLIP + (sl >> 1u32)] >> ((sl & 1u32) * 16u32)) & 65535u32) & hw(H_M_CLIP);
                    }
                    workgroup_barrier();
                    let rc = compact(rh, lane, sg);
                    if rh {
                        get_mut!(HL)[rc.x() as usize] = ii;
                        get_mut!(HK)[rc.x() as usize] = vec2u(pack2(ink.x(), ink.y()), pack2(ink.z(), ink.w()));
                        get_mut!(HC)[rc.x() as usize] = cid;
                    }
                    workgroup_barrier();
                    let nr = rc.y();
                    let mut k = 0u32;
                    while k < nr {
                        let mut j = k;
                        while j < nr && (get!(HL)[j as usize] & (F_RUN | F_IRUN)) == 0u32 {
                            j += 1u32;
                        }
                        if li < j - k {
                            let idx = k + li;
                            let hk = get!(HK)[idx as usize];
                            bin_put(nb + li, cap, base, vec4u(get!(HL)[idx as usize] & hw(H_M_INST), hk.x(), hk.y(), get!(HC)[idx as usize] << 20u32));
                        }
                        nb += j - k;
                        if j < nr {
                            let p = get!(HL)[j as usize];
                            let cid2 = get!(HC)[j as usize];
                            let cl = clip_rect(cid2);
                            if (p & F_RUN) != 0u32 {
                                let r0 = p & 1048575u32;
                                let rn = ((p >> 20u32) & 2047u32) + 1u32;
                                let mut run0 = r0;
                                while run0 < r0 + rn {
                                    let run = run0 & hw(H_M_RUN);
                                    let rhw = hw(H_RUNS) / 4u32 + 8u32 * run;
                                    let rlen = min(get!(POOL)[rhw + 6u32], hw(H_M_GLYPH) + 1u32);
                                    let rbase = get!(POOL)[rhw + 5u32];
                                    let mut g0 = 0u32;
                                    while g0 < rlen {
                                        let gk2 = g0 + li;
                                        let mut gh2 = false;
                                        let mut gi = vec4i(0i32, 0i32, 0i32, 0i32);
                                        if gk2 < rlen {
                                            let gl = glyph(run, rbase + gk2);
                                            if gl.geo.z() >= 0.0 {
                                                let q0v = gl.geo.xy();
                                                let qa = clamp(q0v - vec2f(1.0, 1.0), cl.xy(), cl.zw());
                                                let qb = clamp(q0v - vec2f(1.0, 1.0) + (gl.geo.zw() + vec2f(2.0, 2.0)), cl.xy(), cl.zw());
                                                if qa.x() < qb.x() && qa.y() < qb.y() {
                                                    gi = qink(qa, qb);
                                                    gh2 = lt4(gi, th);
                                                }
                                            }
                                        }
                                        workgroup_barrier();
                                        let c2 = compact(gh2, lane, sg);
                                        if gh2 {
                                            bin_put(nb + c2.x(), cap, base, vec4u(run | F_RUN, pack2(gi.x(), gi.y()), pack2(gi.z(), gi.w()), (cid2 << 20u32) | ((rbase + gk2) & hw(H_M_GLYPH))));
                                        }
                                        nb += c2.y();
                                        g0 += 256u32;
                                    }
                                    run0 += 1u32;
                                }
                            } else {
                                let i0 = p & 262143u32;
                                let inn = ((p >> 18u32) & 4095u32) + 1u32;
                                let mut g0 = 0u32;
                                while g0 < inn {
                                    let gk2 = g0 + li;
                                    let mut gh2 = false;
                                    let mut gi = vec4i(0i32, 0i32, 0i32, 0i32);
                                    if gk2 < inn {
                                        let iw = hw(H_INST) / 4u32 + 12u32 * ((i0 + gk2) & hw(H_M_INST));
                                        let q0v = vec2f(bitcast_f32(get!(POOL)[iw]), bitcast_f32(get!(POOL)[iw + 1u32])) + space(get!(POOL)[iw + 10u32] & 65535u32);
                                        let size = vec2f(bitcast_f32(get!(POOL)[iw + 2u32]), bitcast_f32(get!(POOL)[iw + 3u32]));
                                        let kind = (get!(POOL)[iw + 10u32] >> 16u32) & 255u32;
                                        let m = select(1.0, 0.0, kind == 0u32);
                                        let qa = clamp(q0v - vec2f(m, m), cl.xy(), cl.zw());
                                        let qb = clamp(q0v + size + vec2f(m, m), cl.xy(), cl.zw());
                                        if qa.x() < qb.x() && qa.y() < qb.y() {
                                            gi = qink(qa, qb);
                                            gh2 = lt4(gi, th);
                                        }
                                    }
                                    workgroup_barrier();
                                    let c2 = compact(gh2, lane, sg);
                                    if gh2 {
                                        bin_put(nb + c2.x(), cap, base, vec4u((i0 + gk2) & hw(H_M_INST), pack2(gi.x(), gi.y()), pack2(gi.z(), gi.w()), cid2 << 20u32));
                                    }
                                    nb += c2.y();
                                    g0 += 256u32;
                                }
                            }
                        }
                        k = j + 1u32;
                    }
                    workgroup_barrier();
                    q0 += 32u32;
                }
                p0 += 32u32;
            }
            d0 += 256u32;
        }
        if li == 0u32 {
            atomic_store(&get!(COUNTS)[ty], min(nb, cap));
            if nb > cap {
                atomic_max(&get!(COUNTS)[hw(H_TILES_Y)], nb);
            }
        }
    }

    /// A staged record (`FRec` of the direct kernel, edge form): q = clip-clamped coverage window, core = the
    /// region of provably full coverage, b = the four edges, pm/pm2 = premultiplied colours, par = (radius bits,
    /// border width bits, atlas uv, kind | hit << 8 | cover << 9).
    #[derive(Clone, Copy, Wgsl)]
    pub struct Rec {
        pub q: Vec4f,
        pub core: Vec4f,
        pub b: Vec4f,
        pub pm: Vec4f,
        pub pm2: Vec4f,
        pub par: Vec4u,
    }

    pub fn stage_inst(i: u32, cl: Vec4f, o: Vec2u) -> Rec {
        let iw = hw(H_INST) / 4u32 + 12u32 * (i & hw(H_M_INST));
        let k10 = get!(POOL)[iw + 10u32];
        let p0 = vec2f(bitcast_f32(get!(POOL)[iw]), bitcast_f32(get!(POOL)[iw + 1u32])) + space(k10 & 65535u32);
        let p1 = p0 + vec2f(bitcast_f32(get!(POOL)[iw + 2u32]), bitcast_f32(get!(POOL)[iw + 3u32]));
        let kind = (k10 >> 16u32) & 255u32;
        let color = get!(POOL)[iw + 4u32];
        let radius = bitcast_f32(get!(POOL)[iw + 6u32]);
        let bw = bitcast_f32(get!(POOL)[iw + 7u32]);
        let m = select(1.0, 0.0, kind == 0u32);
        let qa = clamp(p0 - vec2f(m, m), cl.xy(), cl.zw());
        let qb = clamp(p1 + vec2f(m, m), cl.xy(), cl.zw());
        let kk = select(max(radius, 0.5 + bw) + 1.0, max(radius, 0.5) + 1.0, kind == 1u32);
        let cb0 = max(p0 + vec2f(kk, kk), qa);
        let cb1 = min(p1 - vec2f(kk, kk), qb);
        let mut core = vec4f(cb0.x(), cb0.y(), cb1.x(), cb1.y());
        if kind == 0u32 {
            core = vec4f(qa.x(), qa.y(), qb.x(), qb.y());
        }
        if kind == 3u32 {
            core = vec4f(0.0, 0.0, 0.0, 0.0);
        }
        let ox = o.x() as f32;
        let oy = o.y() as f32;
        let cover = kind == 0u32 && (color >> 24u32) == 255u32 && qa.x() <= ox + 0.5 && qa.y() <= oy + 0.5 && qb.x() > ox + 31.5 && qb.y() > oy + 31.5;
        let hit = qa.x() < qb.x() && qa.y() < qb.y();
        let flags = kind | (select(0u32, 1u32, hit) << 8u32) | (select(0u32, 1u32, cover) << 9u32);
        Rec {
            q: vec4f(qa.x(), qa.y(), qb.x(), qb.y()),
            core: core,
            b: vec4f(p0.x(), p0.y(), p1.x(), p1.y()),
            pm: premul(color),
            pm2: premul(get!(POOL)[iw + 5u32]),
            par: vec4u(bitcast_u32(radius), bitcast_u32(bw), get!(POOL)[iw + 8u32], flags),
        }
    }

    pub fn stage_glyph(run: u32, slot: u32, cl: Vec4f) -> Rec {
        let gl = glyph(run, slot);
        let p0 = gl.geo.xy();
        let qa = clamp(p0 - vec2f(1.0, 1.0), cl.xy(), cl.zw());
        let qb = clamp(p0 - vec2f(1.0, 1.0) + (gl.geo.zw() + vec2f(2.0, 2.0)), cl.xy(), cl.zw());
        let hit = gl.geo.z() >= 0.0 && qa.x() < qb.x() && qa.y() < qb.y();
        Rec {
            q: vec4f(qa.x(), qa.y(), qb.x(), qb.y()),
            core: vec4f(0.0, 0.0, 0.0, 0.0),
            b: vec4f(p0.x(), p0.y(), p0.x() + gl.geo.z(), p0.y() + gl.geo.w()),
            pm: premul(gl.info.x()),
            pm2: vec4f(0.0, 0.0, 0.0, 0.0),
            par: vec4u(0u32, 0u32, gl.info.y(), 3u32 | (select(0u32, 1u32, hit) << 8u32)),
        }
    }

    /// Rounded-box SDF in edge form.
    pub fn sd(c: Vec2f, b: Vec4f, r: f32) -> f32 {
        let q = max(c - b.zw(), b.xy() - c) + vec2f(r, r);
        length(max(q, vec2f(0.0, 0.0))) + min(max(q.x(), q.y()), 0.0) - r
    }

    pub fn shade(k: u32, c: Vec2f) -> Vec4f {
        let q = get!(RQ)[k as usize];
        if !(c.x() >= q.x() && c.x() < q.z() && c.y() >= q.y() && c.y() < q.w()) {
            return vec4f(0.0, 0.0, 0.0, 0.0);
        }
        let par = get!(RPAR)[k as usize];
        let kind = par.w() & 255u32;
        let pm = get!(RPM)[k as usize];
        let b = get!(RB)[k as usize];
        if kind == 0u32 {
            return pm;
        }
        if kind == 1u32 {
            let d = sd(c, b, bitcast_f32(par.x()));
            return pm * clamp(0.5 - d, 0.0, 1.0);
        }
        if kind == 2u32 {
            let d = sd(c, b, bitcast_f32(par.x()));
            let outer = clamp(0.5 - d, 0.0, 1.0);
            let inner = clamp(0.5 - (d + bitcast_f32(par.y())), 0.0, 1.0);
            return pm * max(outer - inner, 0.0) + get!(RPM2)[k as usize] * inner;
        }
        let t = c - b.xy();
        let mut cov = 0.0;
        if t.x() >= 0.0 && t.y() >= 0.0 && t.x() < b.z() - b.x() && t.y() < b.w() - b.y() {
            let dims = texture_dimensions(ATLAS);
            let ux = min((par.z() & 65535u32) + (t.x() as u32), dims.x() - 1u32);
            let uy = min((par.z() >> 16u32) + (t.y() as u32), dims.y() - 1u32);
            cov = texture_load(ATLAS, vec2i(ux as i32, uy as i32), 0u32).x();
        }
        pm * cov
    }

    pub fn store(tgt: u32, p: Vec2u, v: Vec4f) {
        if tgt == 0u32 {
            texture_store(T0, p, v);
        } else if tgt == 1u32 {
            texture_store(T1, p, v);
        } else if tgt == 2u32 {
            texture_store(T2, p, v);
        } else {
            texture_store(T3, p, v);
        }
    }

    /// The tile pass: one workgroup per damaged tile (found by a binary search over the rows' prefix counts and a
    /// select of the k-th set bit); 16 x 16 invocations x 2 x 2 pixels; records staged 128 at a time in paint
    /// order; the last opaque record covering the whole tile starts the shading.
    #[compute]
    #[workgroup_size(256, 1, 1)]
    pub fn fs_tile(
        #[builtin(workgroup_id)] wg: Vec3u,
        #[builtin(local_invocation_index)] li: u32,
        #[builtin(subgroup_invocation_id)] lane: u32,
        #[builtin(subgroup_id)] sg: u32,
    ) {
        // one-dimensional workgroup (subgroup builtins require it); invocation (x, y) of the 16 x 16 block
        let lt = vec2u(li & 15u32, li >> 4u32);
        let dbg = hw(H_DEBUG);
        if dbg == 2u32 {
            return;
        }
        let s = get!(SLOT).x() / 4u32;
        let tgt = get!(POOL)[s + 6u32] & 3u32;
        let nrows = get!(POOL)[s + 7u32];
        let rows = s + 16u32;
        let i = wg.x();
        let mut lo = 0u32;
        let mut hi = nrows;
        while hi - lo > 1u32 {
            let mid = (lo + hi) >> 1u32;
            if get!(POOL)[rows + 8u32 * mid + 2u32] <= i {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let rw = rows + 8u32 * lo;
        let mut k = i - get!(POOL)[rw + 2u32];
        let mut w = 0u32;
        while w < 3u32 {
            let c = count_one_bits(get!(POOL)[rw + 4u32 + w]);
            if k < c {
                break;
            }
            k -= c;
            w += 1u32;
        }
        let mut v = get!(POOL)[rw + 4u32 + w];
        let mut jj = 0u32;
        while jj < k {
            v = v & (v - 1u32);
            jj += 1u32;
        }
        let tile = vec2u(w * 32u32 + first_trailing_bit(v), get!(POOL)[rw]);
        let o = tile * 32u32;
        let th = vec4i((o.x() + 32u32) as i32, (o.y() + 32u32) as i32, -(o.x() as i32), -(o.y() as i32));
        let c00 = vec2f((o.x() + 2u32 * lt.x()) as f32 + 0.5, (o.y() + 2u32 * lt.y()) as f32 + 0.5);
        let blk = vec4f(c00.x(), c00.y(), c00.x() + 1.0, c00.y() + 1.0);
        let mut a0 = vec4f(0.0, 0.0, 0.0, 0.0);
        let mut a1 = vec4f(0.0, 0.0, 0.0, 0.0);
        let mut a2 = vec4f(0.0, 0.0, 0.0, 0.0);
        let mut a3 = vec4f(0.0, 0.0, 0.0, 0.0);
        let base = tile.y() * hw(H_BIN_CAP);
        let nb = atomic_load(&get!(COUNTS)[tile.y()]);
        let mut b0 = 0u32;
        while b0 < nb {
            let mut r = Rec {
                q: vec4f(0.0, 0.0, 0.0, 0.0),
                core: vec4f(0.0, 0.0, 0.0, 0.0),
                b: vec4f(0.0, 0.0, 0.0, 0.0),
                pm: vec4f(0.0, 0.0, 0.0, 0.0),
                pm2: vec4f(0.0, 0.0, 0.0, 0.0),
                par: vec4u(0u32, 0u32, 0u32, 0u32),
            };
            let bi = b0 + li;
            if li < 128u32 && bi < nb {
                let en = get!(BINS)[base + bi];
                if lt4(unpack_rect(en.y(), en.z()), th) {
                    let cl = clip_rect(en.w() >> 20u32);
                    if (en.x() & F_RUN) != 0u32 {
                        r = stage_glyph(en.x() & hw(H_M_RUN), en.w() & hw(H_M_GLYPH), cl);
                    } else {
                        r = stage_inst(en.x(), cl, o);
                    }
                }
            }
            let rh = ((r.par.w() >> 8u32) & 1u32) != 0u32;
            let cover = ((r.par.w() >> 9u32) & 1u32) != 0u32;
            workgroup_barrier();
            let rc = compact(rh, lane, sg);
            if rh {
                let at = rc.x() as usize;
                get_mut!(RQ)[at] = r.q;
                get_mut!(RCORE)[at] = r.core;
                get_mut!(RB)[at] = r.b;
                get_mut!(RPM)[at] = r.pm;
                get_mut!(RPM2)[at] = r.pm2;
                get_mut!(RPAR)[at] = r.par;
            }
            let fl = subgroup_max(select(0u32, rc.x() + 1u32, rh && cover));
            if lane == 0u32 {
                get_mut!(FLO)[sg as usize] = fl;
            }
            workgroup_barrier();
            let mut k0 = 0u32;
            let mut s2 = 0u32;
            while s2 < 8u32 {
                k0 = max(k0, get!(FLO)[s2 as usize]);
                s2 += 1u32;
            }
            if k0 > 0u32 {
                k0 -= 1u32;
            }
            let mut kend = rc.y();
            if dbg == 1u32 {
                kend = k0;
            }
            let mut kk = k0;
            while kk < kend {
                let q = get!(RQ)[kk as usize];
                let skip = q.x() > blk.z() || q.z() <= blk.x() || q.y() > blk.w() || q.w() <= blk.y();
                if !skip {
                    let core = get!(RCORE)[kk as usize];
                    if blk.x() >= core.x() && blk.z() < core.z() && blk.y() >= core.y() && blk.w() < core.w() {
                        let mut cv = get!(RPM)[kk as usize];
                        if (get!(RPAR)[kk as usize].w() & 255u32) == 2u32 {
                            cv = get!(RPM2)[kk as usize];
                        }
                        a0 = cv + a0 * (1.0 - cv.w());
                        a1 = cv + a1 * (1.0 - cv.w());
                        a2 = cv + a2 * (1.0 - cv.w());
                        a3 = cv + a3 * (1.0 - cv.w());
                    } else {
                        let s0 = shade(kk, c00);
                        a0 = s0 + a0 * (1.0 - s0.w());
                        let s1 = shade(kk, c00 + vec2f(1.0, 0.0));
                        a1 = s1 + a1 * (1.0 - s1.w());
                        let s2v = shade(kk, c00 + vec2f(0.0, 1.0));
                        a2 = s2v + a2 * (1.0 - s2v.w());
                        let s3 = shade(kk, c00 + vec2f(1.0, 1.0));
                        a3 = s3 + a3 * (1.0 - s3.w());
                    }
                }
                kk += 1u32;
            }
            b0 += 128u32;
        }
        let vw = bitcast_f32(hw(0u32)) as u32;
        let vh = bitcast_f32(hw(1u32)) as u32;
        let p = vec2u(o.x() + 2u32 * lt.x(), o.y() + 2u32 * lt.y());
        if p.x() < vw && p.y() < vh {
            store(tgt, p, a0);
        }
        if p.x() + 1u32 < vw && p.y() < vh {
            store(tgt, p + vec2u(1u32, 0u32), a1);
        }
        if p.x() < vw && p.y() + 1u32 < vh {
            store(tgt, p + vec2u(0u32, 1u32), a2);
        }
        if p.x() + 1u32 < vw && p.y() + 1u32 < vh {
            store(tgt, p + vec2u(1u32, 1u32), a3);
        }
    }
}
