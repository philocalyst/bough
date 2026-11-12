// bough-metal uber-shader: one vertex function, one fragment function, one draw.
// Instance = 48 bytes. Everything the vertex stage needs lives in a single shared
// "pool" buffer: header | spaces | clips | stencil rects | instances | draw list.
#include <metal_stdlib>
using namespace metal;

struct Instance {          // 48 bytes, mirrors instance.rs
    float2 pos;            //  0  position relative to `space`
    float2 size;           //  8
    uint   color;          // 16  RGBA8 (r in low byte), straight alpha
    uint   color2;         // 20  second colour (fill for border kind)
    float  radius;         // 24
    float  bw;             // 28  border width
    ushort4 uv;            // 32  glyph atlas texels x,y,w,h
    ushort space;          // 40  index into spaces[]
    uchar  kind;           // 42  0 solid, 1 rrect, 2 border, 3 glyph, 4 image, 5 gradient, 6 shadow
    uchar  flags;          // 43  ink margin in px beyond the box (0: the kind's default)
    uint   ext;            // 44  clip index
};

struct Hdr {
    float2 viewport;
    uint spaces_off, clips_off, rects_off, inst_off, list_off;
};

struct VOut {
    float4 position [[position]];
    float2 centre [[flat]];             // instance centre (px); local = pixel centre - centre, exact under any quad split
    float4 color [[flat]];
    float4 color2 [[flat]];
    float4 params [[flat]];             // half w, half h, radius, bw
    float4 uv [[flat]];
    uint   kind [[flat]];
};

static float4 unpack(uint c) { return float4(as_type<uchar4>(c)) * (1.0 / 255.0); }

vertex VOut vs_main(uint vid [[vertex_id]], uint iid [[instance_id]],
                    device const uchar* pool [[buffer(0)]]) {
    device const Hdr& h = *(device const Hdr*)pool;
    uint idx = ((device const uint*)(pool + h.list_off))[iid];
    device const Instance& in = ((device const Instance*)(pool + h.inst_off))[idx];
    float2 off = ((device const float2*)(pool + h.spaces_off))[in.space];
    float4 clip = ((device const float4*)(pool + h.clips_off))[in.ext];

    float2 c = float2(vid & 1u, vid >> 1);
    float2 p0 = in.pos + off;
    float2 half_ = in.size * 0.5;
    float2 centre = p0 + half_;
    float m = 1.0;                                   // AA margin
    float2 p = p0 - m + c * (in.size + 2.0 * m);
    p = clamp(p, clip.xy, clip.zw);                  // axis-aligned clip == intersection
    VOut o;
    o.position = float4(p.x / h.viewport.x * 2.0 - 1.0, 1.0 - p.y / h.viewport.y * 2.0, 0.0, 1.0);
    o.centre = centre;
    o.color = unpack(in.color);
    o.color2 = unpack(in.color2);
    o.params = float4(half_, in.radius, in.bw);
    o.uv = float4(in.uv);
    o.kind = in.kind;
    return o;
}

static float sd_rbox(float2 p, float2 hs, float r) {
    float2 q = abs(p) - hs + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

fragment float4 fs_main(VOut v [[stage_in]], texture2d<float> atlas [[texture(0)]]) {
    float4 col; // premultiplied
    float2 local = v.position.xy - v.centre;
    switch (v.kind) {
    case 0: {
        col = float4(v.color.rgb * v.color.a, v.color.a);
        break; }
    case 1: {
        float d = sd_rbox(local, v.params.xy, v.params.z);
        float cov = clamp(0.5 - d, 0.0, 1.0);
        col = float4(v.color.rgb * v.color.a, v.color.a) * cov;
        break; }
    case 2: {
        float d = sd_rbox(local, v.params.xy, v.params.z);
        float outer = clamp(0.5 - d, 0.0, 1.0);
        float inner = clamp(0.5 - (d + v.params.w), 0.0, 1.0);
        float ring = max(outer - inner, 0.0);
        float4 bc = float4(v.color.rgb * v.color.a, v.color.a);
        float4 fc = float4(v.color2.rgb * v.color2.a, v.color2.a);
        col = bc * ring + fc * inner;
        break; }
    default: {
        float2 t = local + v.params.xy;           // 0..size in pixels
        float cov = 0.0;
        if (all(t >= 0.0) && all(t < v.params.xy * 2.0)) {
            uint2 tex = uint2(v.uv.xy) + uint2(t);
            cov = atlas.read(tex).r;
        }
        col = float4(v.color.rgb * v.color.a, v.color.a) * cov;
        break; }
    }
    return col;
}

// Stencil mask pass: instance k draws rect k from the stencil-rect table (x0,y0,x1,y1 as float4).
vertex float4 vs_rects(uint vid [[vertex_id]], uint iid [[instance_id]],
                       device const uchar* pool [[buffer(0)]]) {
    device const Hdr& h = *(device const Hdr*)pool;
    float4 r = ((device const float4*)(pool + h.rects_off))[iid];
    float2 c = float2(vid & 1u, vid >> 1);
    float2 p = mix(r.xy, r.zw, c);
    return float4(p.x / h.viewport.x * 2.0 - 1.0, 1.0 - p.y / h.viewport.y * 2.0, 0.0, 1.0);
}

// ============================================================================ COMPUTE TILE REPAIR
// One threadgroup = one damaged 16x16 tile (256 threads = 256 pixels). The tile list, its count, the indirect
// dispatch arguments and the target index live in a "frame slot" in the pool, so the command stream is constant
// (it can be pre-encoded and released later by a shared event). Each threadgroup culls the instance array itself
// (256 instances per step, compacted in paint order with a simd prefix sum) and blends in registers; every pixel
// of the tile is written exactly once. No render pass, no load/store of the old pixels, no rasterizer.
struct Hdr2 {
    float2 viewport;
    uint spaces_off, clips_off, rects_off, inst_off, list_off;
    uint n_inst;
    uint plane_off;    // 16-px tile damage plane: 90 rows x 4 u32 (128 bits, tiles 120..127 phantom)
    uint pages_off;    // int4 ink union per 64-instance page
};
struct FrameSlot {
    uint tg[3];        // +0  indirect dispatch args: (ntiles, 1, 1)  (MTLDispatchThreadgroupsIndirectArguments)
    uint target;       // +12 which of the 4 bound textures to write
    uint ntiles;       // +16
    uint full;         // +20 1 = every tile, threadgroup index IS the tile (no list written)
    uint pad[2];
    // uint tiles[] follows at +32: (ty << 16) | tx
};

static float4 shade(device const Instance& in, float2 off, float4 clip, float2 c, texture2d<float> atlas, thread bool& inside) {
    float2 p0 = in.pos + off;
    float m = 1.0;
    float2 q0 = clamp(p0 - m, clip.xy, clip.zw);
    float2 q1 = clamp(p0 - m + (in.size + 2.0 * m), clip.xy, clip.zw);
    inside = c.x >= q0.x && c.x < q1.x && c.y >= q0.y && c.y < q1.y;
    float2 half_ = in.size * 0.5;
    float2 local = c - (p0 + half_);
    float4 col = unpack(in.color);
    switch (in.kind) {
    case 0:
        return float4(col.rgb * col.a, col.a);
    case 1: {
        float d = sd_rbox(local, half_, in.radius);
        float cov = clamp(0.5 - d, 0.0, 1.0);
        return float4(col.rgb * col.a, col.a) * cov; }
    case 2: {
        float d = sd_rbox(local, half_, in.radius);
        float outer = clamp(0.5 - d, 0.0, 1.0);
        float inner = clamp(0.5 - (d + in.bw), 0.0, 1.0);
        float ring = max(outer - inner, 0.0);
        float4 c2 = unpack(in.color2);
        return float4(col.rgb * col.a, col.a) * ring + float4(c2.rgb * c2.a, c2.a) * inner; }
    default: {
        float2 t = local + half_;
        float cov = 0.0;
        if (all(t >= 0.0) && all(t < in.size)) {
            cov = atlas.read(uint2(float2(in.uv.xy)) + uint2(t)).r;
        }
        return float4(col.rgb * col.a, col.a) * cov; }
    }
}

// Per-threadgroup instance record, derived once by the thread that culled it (clip-clamped quad, centre, colours),
// so the per-pixel loop reads threadgroup memory only.
struct TgInst {
    float4 q;          // clip-clamped quad incl. AA margin
    float2 centre;
    float2 half_;
    uint color, color2;
    float radius, bw;
    ushort2 uv;
    uint kind;
};

static float4 shade_rec(threadgroup const TgInst& r, float2 c, texture2d<float> atlas) {
    if (!(c.x >= r.q.x && c.x < r.q.z && c.y >= r.q.y && c.y < r.q.w)) return float4(0.0);
    float2 local = c - r.centre;
    float4 col = unpack(r.color);
    switch (r.kind) {
    case 0:
        return float4(col.rgb * col.a, col.a);
    case 1: {
        float d = sd_rbox(local, r.half_, r.radius);
        float cov = clamp(0.5 - d, 0.0, 1.0);
        return float4(col.rgb * col.a, col.a) * cov; }
    case 2: {
        float d = sd_rbox(local, r.half_, r.radius);
        float outer = clamp(0.5 - d, 0.0, 1.0);
        float inner = clamp(0.5 - (d + r.bw), 0.0, 1.0);
        float ring = max(outer - inner, 0.0);
        float4 c2 = unpack(r.color2);
        return float4(col.rgb * col.a, col.a) * ring + float4(c2.rgb * c2.a, c2.a) * inner; }
    default: {
        float2 t = local + r.half_;
        float cov = 0.0;
        if (all(t >= 0.0) && all(t < r.half_ * 2.0)) {
            cov = atlas.read(uint2(float2(r.uv)) + uint2(t)).r;
        }
        return float4(col.rgb * col.a, col.a) * cov; }
    }
}

kernel void cs_repair(uint2 tgid2 [[threadgroup_position_in_grid]],
                      uint li [[thread_index_in_threadgroup]],
                      uint2 lt [[thread_position_in_threadgroup]],
                      uint lane [[thread_index_in_simdgroup]],
                      uint sg [[simdgroup_index_in_threadgroup]],
                      device const uchar* pool [[buffer(0)]],
                      constant uint& slot_off [[buffer(1)]],
                      texture2d<float> atlas [[texture(0)]],
                      texture2d<float, access::write> t0 [[texture(1)]],
                      texture2d<float, access::write> t1 [[texture(2)]],
                      texture2d<float, access::write> t2 [[texture(3)]],
                      texture2d<float, access::write> t3 [[texture(4)]]) {
    device const Hdr2& h = *(device const Hdr2*)pool;
    device const FrameSlot& fs = *(device const FrameSlot*)(pool + slot_off);
    device const uint* tiles = (device const uint*)(pool + slot_off + 32);
    device const Instance* inst = (device const Instance*)(pool + h.inst_off);
    device const float2* spaces = (device const float2*)(pool + h.spaces_off);
    device const float4* clips = (device const float4*)(pool + h.clips_off);
    uint t = fs.full ? ((tgid2.x / 120u) << 16 | (tgid2.x % 120u)) : tiles[tgid2.x];
    uint2 tile = uint2(t & 0xffffu, t >> 16);
    uint2 px = tile * 16u + lt;
    float2 c = float2(px) + 0.5;
    float4 tr = float4(float2(tile * 16u), float2(tile * 16u + 16u));
    int4 tri = int4(int2(tile * 16u), int2(tile * 16u + 16u));
    device const int4* pages = (device const int4*)(pool + h.pages_off);
    threadgroup uint sgc[8];
    threadgroup uint pg_list[64];
    threadgroup uint npg;
    threadgroup TgInst rec[256];
    float4 acc = float4(0.0);
    uint n = h.n_inst;
    uint np = (n + 63) / 64;
    // level 1: which 64-instance pages touch this tile (one thread per page; order kept by a prefix sum)
    {
        bool ph = false;
        if (li < np) {
            int4 pr = pages[li];
            ph = pr.x < pr.z && pr.x < tri.z && pr.z > tri.x && pr.y < tri.w && pr.w > tri.y;
        }
        uint pre = simd_prefix_exclusive_sum(uint(ph));
        uint cnt = simd_sum(uint(ph));
        if (lane == 0) sgc[sg] = cnt;
        threadgroup_barrier(mem_flags::mem_threadgroup);
        uint off = 0, total = 0;
        for (uint s = 0; s < 8; s++) { uint v = sgc[s]; off += (s < sg) ? v : 0; total += v; }
        if (ph) pg_list[off + pre] = li;
        if (li == 0) { npg = total; }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    uint npages = npg;  // written by thread 0 before the barrier above
    // level 2: 4 pages (256 instances) per step, compacted in paint order, then shaded in registers
    for (uint pb = 0; pb < npages; pb += 4) {
        uint slot = pb + (li >> 6);
        uint i = slot < npages ? pg_list[slot] * 64 + (li & 63) : n;
        bool hit = false;
        TgInst r;
        if (i < n) {
            device const Instance& in = inst[i];
            float2 p0 = in.pos + spaces[in.space];
            float4 cl = clips[in.ext];
            float2 q0 = clamp(p0 - 1.0, cl.xy, cl.zw);
            float2 q1 = clamp(p0 - 1.0 + (in.size + 2.0), cl.xy, cl.zw);
            hit = q0.x < tr.z && q1.x > tr.x && q0.y < tr.w && q1.y > tr.y && q0.x < q1.x && q0.y < q1.y;
            r.q = float4(q0, q1);
            r.half_ = in.size * 0.5;
            r.centre = p0 + r.half_;
            r.color = in.color;
            r.color2 = in.color2;
            r.radius = in.radius;
            r.bw = in.bw;
            r.uv = in.uv.xy;
            r.kind = in.kind;
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);   // previous step's readers of rec/sgc are done
        uint pre = simd_prefix_exclusive_sum(uint(hit));
        uint cnt = simd_sum(uint(hit));
        if (lane == 0) sgc[sg] = cnt;
        threadgroup_barrier(mem_flags::mem_threadgroup);
        uint off = 0, total = 0;
        for (uint s = 0; s < 8; s++) { uint v = sgc[s]; off += (s < sg) ? v : 0; total += v; }
        if (hit) rec[off + pre] = r;
        threadgroup_barrier(mem_flags::mem_threadgroup);
        for (uint k = 0; k < total; k++) {
            float4 s = shade_rec(rec[k], c, atlas);
            acc = s + acc * (1.0 - s.a);
        }
    }
    switch (fs.target) {
    case 0: t0.write(acc, px); break;
    case 1: t1.write(acc, px); break;
    case 2: t2.write(acc, px); break;
    default: t3.write(acc, px); break;
    }
}

// ============================================================================ VERTEX-CLIPPED INSTANCE STREAM
// Format C produced on the GPU: the CPU emits culled entries (instance | first row << 16), each covering up to
// VR tile rows of the instance; every entry is drawn as VR x VK sub-quads. Sub-quad (r, k) is the instance quad
// intersected with tile row (first+r) and with the k-th run of damaged tiles of that row inside the instance's
// x-span, read from the damage plane by bit scan. The CPU canonicalises each plane ROW to <= VK runs (filling the
// smallest gaps) before upload; a superset is exact only if it is a function of the row, never of the instance
// (a per-instance merge lets a panel repaint a gap tile that the button above it does not). Rows / runs that do not exist give a degenerate quad. Sub-quads of one instance are disjoint,
// and entries stay in paint order, so blending order per pixel is unchanged; the GPU only bins geometry over
// damaged tiles, so it only loads/stores hardware tiles that are actually damaged.
constant uint VR = 8;
constant uint VK = 4;

static uint next_set(uint4 b, uint p) {          // first set bit >= p, or 128
    for (uint w = p >> 5; w < 4; w++) {
        uint m = b[w] & (w == (p >> 5) ? (0xffffffffu << (p & 31)) : 0xffffffffu);
        if (m) return w * 32 + ctz(m);
    }
    return 128;
}
static uint next_clear(uint4 b, uint p) {        // first clear bit >= p, or 128
    for (uint w = p >> 5; w < 4; w++) {
        uint m = ~b[w] & (w == (p >> 5) ? (0xffffffffu << (p & 31)) : 0xffffffffu);
        if (m) return w * 32 + ctz(m);
    }
    return 128;
}
static uint last_set(uint4 b) {                  // highest set bit (b != 0)
    for (int w = 3; w >= 0; w--) {
        if (b[w]) return uint(w) * 32 + 31 - clz(b[w]);
    }
    return 0;
}
static uint4 span_mask(uint a, uint b) {        // bits [a, b)
    uint4 m;
    for (uint w = 0; w < 4; w++) {
        uint lo = w * 32, hi = lo + 32;
        uint s = max(a, lo), e = min(b, hi);
        m[w] = s >= e ? 0u : ((e - s == 32 ? 0xffffffffu : ((1u << (e - s)) - 1u)) << (s - lo));
    }
    return m;
}

vertex VOut vs_vclip(uint vid [[vertex_id]], uint iid [[instance_id]], device const uchar* pool [[buffer(0)]]) {
    device const Hdr2& h = *(device const Hdr2*)pool;
    uint e = ((device const uint*)(pool + h.list_off))[iid / (VR * VK)];
    uint sub = iid % (VR * VK);
    uint r = sub / VK, k = sub % VK;
    uint idx = e & 0xffffu;
    uint row = (e >> 16) + r;
    device const Instance& in = ((device const Instance*)(pool + h.inst_off))[idx];
    float2 off = ((device const float2*)(pool + h.spaces_off))[in.space];
    float4 clip = ((device const float4*)(pool + h.clips_off))[in.ext];
    float2 p0 = in.pos + off;
    float2 half_ = in.size * 0.5;
    float m = 1.0;
    float2 q0 = clamp(p0 - m, clip.xy, clip.zw);
    float2 q1 = clamp(p0 - m + (in.size + 2.0 * m), clip.xy, clip.zw);
    float ry0 = float(row * 16u), ry1 = ry0 + 16.0;
    float2 lo = float2(0.0), hi = float2(0.0);       // degenerate unless a run is found
    if (row < 90u && ry0 < q1.y && ry1 > q0.y && q0.x < q1.x) {
        uint tx0 = uint(clamp(floor(q0.x / 16.0), 0.0, 120.0));
        uint tx1 = uint(clamp(ceil(q1.x / 16.0), 0.0, 120.0));
        uint4 b = ((device const uint4*)(pool + h.plane_off))[row] & span_mask(tx0, tx1);
        uint s = next_set(b, tx0);
        uint t = s;
        for (uint j = 0; j < k && s < tx1; j++) {
            t = next_clear(b, s);
            s = next_set(b, t);
        }
        if (s < tx1) {
            uint end = next_clear(b, s);              // the CPU canonicalised every row to <= VK runs
            lo = float2(max(q0.x, float(s * 16u)), max(q0.y, ry0));
            hi = float2(min(q1.x, float(min(end, tx1) * 16u)), min(q1.y, ry1));
        }
    }
    float2 c = float2(vid & 1u, vid >> 1);
    float2 p = mix(lo, hi, c);
    VOut o;
    o.position = float4(p.x / h.viewport.x * 2.0 - 1.0, 1.0 - p.y / h.viewport.y * 2.0, 0.0, 1.0);
    o.centre = p0 + half_;
    o.color = unpack(in.color);
    o.color2 = unpack(in.color2);
    o.params = float4(half_, in.radius, in.bw);
    o.uv = float4(in.uv);
    o.kind = in.kind;
    return o;
}

// ============================================================================ COMPUTE REPAIR v3: 32x32 tiles
// Reads the bough-core page layout directly (INTERFACE.md): a directory (page id + page union ink per entry, in
// paint order) and 1344-byte SoA pages (ink: NRect i16x4 negated-max (x0, y0, -x1, -y1) per slot, gink: union of
// each 8-slot group, payload: instance index). Culling is three NRect-vs-threshold compares (exactly the core's
// `Thresh::matches`: overlap <=> all(r < (X1, Y1, -X0, -Y0)); EMPTY = MAX padding fails by itself):
//   L1 directory entries -> L2 groups of the hit pages -> L3 records of the hit groups.
// One threadgroup = one 32x32 damaged tile (the Apple GPU's hardware tile and the core's damage unit); 256 threads
// x 4 pixels each (thread (x, y) owns (x + 16i, y + 16j)), so every cull is amortised over 1024 pixels.
// Colour is accumulated in half precision (positions and SDFs stay float). Every pixel is written once.
struct Hdr3 {
    float2 viewport;
    uint spaces_off, clips_off, rects_off, inst_off, list_off;
    uint n_inst, plane_off, pages_off;
    uint dir_page_off;   // u32 page id per directory entry
    uint dir_ink_off;    // short4 page union ink per directory entry
    uint n_dir;
    uint pagepool_off;   // base of the 1344-byte pages (page id * 1344)
    uint debug;          // 0 normal; 1 = cull only (no shading); 2 = count staged records into stats
    uint stats_off;
    uint gx_off, gent_off, runs_off, entries_off, space_clip_off;   // bough-text format (INTERFACE.md)
};

// bough-text run header (32 B) and strike entry (32 B)
struct RunHeader {
    float2 origin;
    float scale;
    uint color;
    ushort space, strike;
    uint base, len;
    int x0;
};
struct GEntry {
    float4 geo;        // [left, -top, w, h] px
    ushort4 uv;
    uint state, pad;
};
constant uint RUN_BIT = 0x80000000u;

// Glyph `slot` of `run` -> clip-space inputs. The pen formula is bough-text's, verified bit-exact there:
// px = floor(fma(float(x - x0), scale, origin.x + 0.5)) + geo.x
static bool glyph_quad(device const uchar* pool, device const Hdr3& h, uint run, uint slot,
                       thread float2& p0, thread float2& size, thread float4& cl, thread uint& color, thread ushort2& uv) {
    device const RunHeader& rh = ((device const RunHeader*)(pool + h.runs_off))[run];
    int x = ((device const int*)(pool + h.gx_off))[slot];
    uint e = ((device const ushort*)(pool + h.gent_off))[slot];
    device const GEntry& en = ((device const GEntry*)(pool + h.entries_off))[uint(rh.strike) * 256u + e];
    if (en.uv.z == 0) return false;
    float2 off = ((device const float2*)(pool + h.spaces_off))[rh.space];
    cl = ((device const float4*)(pool + h.clips_off))[((device const uint*)(pool + h.space_clip_off))[rh.space]];
    float px = floor(fma(float(x - rh.x0), rh.scale, rh.origin.x + 0.5)) + en.geo.x;
    p0 = float2(px, rh.origin.y + en.geo.y) + off;
    size = en.geo.zw;
    color = rh.color;
    uv = en.uv.xy;
    return true;
}
constant uint PG_BYTES = 1344;
constant uint PG_INK = 128;
constant uint PG_GINK = 640;
constant uint PG_PAYLOAD = 768;

struct TgRec {
    float4 q;          // clip-clamped quad incl. AA margin (pixel-centre test)
    float4 core;       // pixel centres inside `core` take `cv` with no per-pixel work (coverage provably 1)
    half4 cv;
    float2 centre;
    float2 half_;
    half4 pm;          // premultiplied colour, unpacked once per record (not per pixel)
    half4 pm2;
    float radius, bw;
    ushort2 uv;
    ushort kind, pad;
};

static half4 shade_h(threadgroup const TgRec& r, float2 c, texture2d<float> atlas) {
    if (!(c.x >= r.q.x && c.x < r.q.z && c.y >= r.q.y && c.y < r.q.w)) return half4(0.0h);
    switch (r.kind) {
    case 0:
        return r.pm;
    case 1: {
        float d = sd_rbox(c - r.centre, r.half_, r.radius);
        return r.pm * half(clamp(0.5 - d, 0.0, 1.0)); }
    case 2: {
        float d = sd_rbox(c - r.centre, r.half_, r.radius);
        float outer = clamp(0.5 - d, 0.0, 1.0);
        float inner = clamp(0.5 - (d + r.bw), 0.0, 1.0);
        return r.pm * half(max(outer - inner, 0.0)) + r.pm2 * half(inner); }
    default: {
        float2 t = c - r.centre + r.half_;
        half cov = 0.0h;
        if (all(t >= 0.0) && all(t < r.half_ * 2.0)) {
            cov = half(atlas.read(uint2(float2(r.uv)) + uint2(t)).r);
        }
        return r.pm * cov; }
    }
}

// Ordered compaction of `hit` across the threadgroup: returns this thread's slot (if hit) and the total.
static uint compact(bool hit, uint lane, uint sg, threadgroup uint* sgc, thread uint& total) {
    uint pre = simd_prefix_exclusive_sum(uint(hit));
    uint cnt = simd_sum(uint(hit));
    if (lane == 0) sgc[sg] = cnt;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    uint off = 0;
    total = 0;
    for (uint s = 0; s < 8; s++) { uint v = sgc[s]; off += (s < sg) ? v : 0; total += v; }
    return off + pre;
}

// Ordered exclusive prefix sum of `v` across the threadgroup: this thread's sum of the threads before it, and
// the total (`compact` is this with v = 0 or 1).
static uint prefix(uint v, uint lane, uint sg, threadgroup uint* sgc, thread uint& total) {
    uint pre = simd_prefix_exclusive_sum(v);
    uint cnt = simd_sum(v);
    if (lane == 0) sgc[sg] = cnt;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    uint off = 0;
    total = 0;
    for (uint s = 0; s < 8; s++) { uint x = sgc[s]; off += (s < sg) ? x : 0; total += x; }
    return off + pre;
}

kernel void cs_repair32(uint2 tgid2 [[threadgroup_position_in_grid]],
                        uint li [[thread_index_in_threadgroup]],
                        uint2 lt [[thread_position_in_threadgroup]],
                        uint lane [[thread_index_in_simdgroup]],
                        uint sg [[simdgroup_index_in_threadgroup]],
                        device uchar* pool [[buffer(0)]],
                        constant uint& slot_off [[buffer(1)]],
                        texture2d<float> atlas [[texture(0)]],
                        texture2d<float, access::write> t0 [[texture(1)]],
                        texture2d<float, access::write> t1 [[texture(2)]],
                        texture2d<float, access::write> t2 [[texture(3)]],
                        texture2d<float, access::write> t3 [[texture(4)]]) {
    device const Hdr3& h = *(device const Hdr3*)pool;
    device const FrameSlot& fs = *(device const FrameSlot*)(pool + slot_off);
    device const uint* tiles = (device const uint*)(pool + slot_off + 32);
    device const Instance* inst = (device const Instance*)(pool + h.inst_off);
    device const float2* spaces = (device const float2*)(pool + h.spaces_off);
    device const float4* clips = (device const float4*)(pool + h.clips_off);
    device const uint* dpage = (device const uint*)(pool + h.dir_page_off);
    device const short4* dink = (device const short4*)(pool + h.dir_ink_off);
    device const uchar* pages = pool + h.pagepool_off;
    uint t = fs.full ? ((tgid2.x / 60u) << 16 | (tgid2.x % 60u)) : tiles[tgid2.x];
    uint2 tile = uint2(t & 0xffffu, t >> 16);
    uint2 o = tile * 32u;
    short4 th = short4(short(o.x + 32u), short(o.y + 32u), short(-int(o.x)), short(-int(o.y)));
    // thread (x, y) owns the 2x2 block at (2x, 2y): one block test rejects a record for all 4 pixels
    float2 c00 = float2(o + 2u * lt) + 0.5;
    float4 blk = float4(c00, c00 + 1.0);   // centres of the block's first and last pixel

    threadgroup uint sgc[8];
    threadgroup uint flo[8];
    threadgroup uint dl[256];      // hit directory entries (page ids), paint order
    threadgroup uint gl[256];      // hit groups (page id * 8 + group), paint order
    threadgroup TgRec rec[64];     // 8 groups x 8 records per L3 step: ~6 KB of threadgroup memory -> occupancy
    half4 acc[4] = { half4(0.0h), half4(0.0h), half4(0.0h), half4(0.0h) };

    uint ndir = h.debug == 3 ? 0u : h.n_dir;   // debug 3: write-only floor (no cull, no shading)
    for (uint d0 = 0; d0 < ndir; d0 += 256) {
        // L1: directory
        uint di = d0 + li;
        bool dh = di < h.n_dir && all(dink[di] < th);
        threadgroup_barrier(mem_flags::mem_threadgroup);
        uint nd;
        uint ds = compact(dh, lane, sg, sgc, nd);
        if (dh) dl[ds] = dpage[di];
        threadgroup_barrier(mem_flags::mem_threadgroup);
        // L2: the 8 groups of each hit page, 32 pages per step
        for (uint p0 = 0; p0 < nd; p0 += 32) {
            uint pk = p0 + (li >> 3), g = li & 7u;
            bool gh = false;
            uint gid = 0;
            if (pk < nd) {
                uint pid = dl[pk];
                gid = pid * 8u + g;
                gh = all(((device const short4*)(pages + pid * PG_BYTES + PG_GINK))[g] < th);
            }
            threadgroup_barrier(mem_flags::mem_threadgroup);
            uint ng;
            uint gs = compact(gh, lane, sg, sgc, ng);
            if (gh) gl[gs] = gid;
            threadgroup_barrier(mem_flags::mem_threadgroup);
            // L3: the 8 records of each hit group, 32 groups per step; stage hits, then shade
            for (uint q0 = 0; q0 < ng; q0 += 8) {
                uint gk = q0 + (li >> 3), s = li & 7u;
                bool rh = false, cover = false;
                TgRec r;
                if (li < 64 && gk < ng) {
                    uint gidv = gl[gk];
                    uint pid = gidv >> 3, slot = (gidv & 7u) * 8u + s;
                    device const uchar* pg = pages + pid * PG_BYTES;
                    if (all(((device const short4*)(pg + PG_INK))[slot] < th)) {
                        uint ii = ((device const uint*)(pg + PG_PAYLOAD))[slot];
                        device const Instance& in = inst[ii];
                        float2 p0 = in.pos + spaces[in.space];
                        float4 cl = clips[in.ext];
                        float2 qa = clamp(p0 - 1.0, cl.xy, cl.zw);
                        float2 qb = clamp(p0 - 1.0 + (in.size + 2.0), cl.xy, cl.zw);
                        rh = qa.x < qb.x && qa.y < qb.y;
                        r.q = float4(qa, qb);
                        r.half_ = in.size * 0.5;
                        r.centre = p0 + r.half_;
                        half4 c1 = half4(unpack(in.color)), c2 = half4(unpack(in.color2));
                        r.pm = half4(c1.rgb * c1.a, c1.a);
                        r.pm2 = half4(c2.rgb * c2.a, c2.a);
                        r.radius = in.radius;
                        r.bw = in.bw;
                        r.uv = in.uv.xy;
                        r.kind = in.kind;
                        // core box (exact shortcut, 1-px safety margin beyond the SDF's own 0.5)
                        float k = in.kind == 1 ? max(in.radius, 0.5) + 1.0 : max(in.radius, 0.5 + in.bw) + 1.0;
                        float4 cb = float4(r.centre - (r.half_ - k), r.centre + (r.half_ - k));
                        cb = float4(max(cb.xy, qa), min(cb.zw, qb));
                        r.core = in.kind == 0 ? float4(qa, qb) : (in.kind == 3 ? float4(0.0) : cb);
                        r.cv = in.kind == 2 ? r.pm2 : r.pm;
                        // opaque floor: a solid, alpha-1 record whose quad holds every pixel centre of the tile
                        cover = in.kind == 0 && (in.color >> 24) == 255u && qa.x <= float(o.x) + 0.5 && qa.y <= float(o.y) + 0.5
                                && qb.x > float(o.x) + 31.5 && qb.y > float(o.y) + 31.5;
                    }
                }
                threadgroup_barrier(mem_flags::mem_threadgroup);
                uint nr;
                uint rs = compact(rh, lane, sg, sgc, nr);
                if (rh) rec[rs] = r;
                uint fl = simd_max((rh && cover) ? rs + 1u : 0u);
                if (lane == 0) flo[sg] = fl;
                threadgroup_barrier(mem_flags::mem_threadgroup);
                uint k0 = 0;
                for (uint s2 = 0; s2 < 8; s2++) k0 = max(k0, flo[s2]);
                k0 = k0 > 0 ? k0 - 1u : 0u;   // records below an opaque full cover cannot show: exact skip
                if (h.debug == 2 && li == 0) {
                    atomic_fetch_add_explicit((device atomic_uint*)(pool + h.stats_off), nr - k0, memory_order_relaxed);
                    atomic_fetch_add_explicit((device atomic_uint*)(pool + h.stats_off + 4), 1u, memory_order_relaxed);
                }
                uint nshade = h.debug == 1 ? 0u : nr;
                for (uint k = k0; k < nshade; k++) {
                    threadgroup const TgRec& rr = rec[k];
                    if (rr.q.x > blk.z || rr.q.z <= blk.x || rr.q.y > blk.w || rr.q.w <= blk.y) continue;
                    if (blk.x >= rr.core.x && blk.z < rr.core.z && blk.y >= rr.core.y && blk.w < rr.core.w) {
                        half4 cv = rr.cv;
                        for (uint j = 0; j < 4; j++) acc[j] = cv + acc[j] * (1.0h - cv.a);
                        continue;
                    }
                    for (uint j = 0; j < 4; j++) {
                        half4 sh = shade_h(rr, c00 + float2(float(j & 1u), float(j >> 1)), atlas);
                        acc[j] = sh + acc[j] * (1.0h - sh.a);
                    }
                }
            }
        }
    }
    for (uint j = 0; j < 4; j++) {
        uint2 px = o + 2u * lt + uint2(j & 1u, j >> 1);
        float4 v = float4(acc[j]);
        switch (fs.target) {
        case 0: t0.write(v, px); break;
        case 1: t1.write(v, px); break;
        case 2: t2.write(v, px); break;
        default: t3.write(v, px); break;
        }
    }
}

// ============================================================================ COMPUTE REPAIR v4: binned
// Pass 1 (cs_bin32): one threadgroup per damaged 32-px tile ROW (restricted to that row's damaged x-span) runs the
// hierarchical core-page cull ONCE and appends the hit records (instance index + ink), in paint order, to the
// row's bin. Pass 2 (cs_tile32b): each damaged tile scans only its row's bin (128 entries per step), stages hits
// and shades exactly like cs_repair32. Culling work per tile drops from directory+groups+records to one list scan.
// Slot tail (last 256 B of the frame slot): [nrows, 1, 1, pad] indirect args, then rows[]: ty | tx0 << 8 | tx1 << 16.
constant uint BIN_CAP = 8192;           // a record or glyph appears at most once per row: never overflows

static bool stage_rec(device const Instance& in, float2 off, float4 cl, uint2 o, thread TgRec& r, thread bool& cover) {
    float2 p0 = in.pos + off;
    float2 qa = clamp(p0 - 1.0, cl.xy, cl.zw);
    float2 qb = clamp(p0 - 1.0 + (in.size + 2.0), cl.xy, cl.zw);
    r.q = float4(qa, qb);
    r.half_ = in.size * 0.5;
    r.centre = p0 + r.half_;
    half4 c1 = half4(unpack(in.color)), c2 = half4(unpack(in.color2));
    r.pm = half4(c1.rgb * c1.a, c1.a);
    r.pm2 = half4(c2.rgb * c2.a, c2.a);
    r.radius = in.radius;
    r.bw = in.bw;
    r.uv = in.uv.xy;
    r.kind = in.kind;
    float k = in.kind == 1 ? max(in.radius, 0.5) + 1.0 : max(in.radius, 0.5 + in.bw) + 1.0;
    float4 cb = float4(r.centre - (r.half_ - k), r.centre + (r.half_ - k));
    cb = float4(max(cb.xy, qa), min(cb.zw, qb));
    r.core = in.kind == 0 ? float4(qa, qb) : (in.kind == 3 ? float4(0.0) : cb);
    r.cv = in.kind == 2 ? r.pm2 : r.pm;
    cover = in.kind == 0 && (in.color >> 24) == 255u && qa.x <= float(o.x) + 0.5 && qa.y <= float(o.y) + 0.5
            && qb.x > float(o.x) + 31.5 && qb.y > float(o.y) + 31.5;
    return qa.x < qb.x && qa.y < qb.y;
}

static bool stage_glyph(device const uchar* pool, device const Hdr3& h, uint run, uint slot, thread TgRec& r) {
    float2 p0, size;
    float4 cl;
    uint color;
    ushort2 uv;
    if (!glyph_quad(pool, h, run, slot, p0, size, cl, color, uv)) return false;
    float2 qa = clamp(p0 - 1.0, cl.xy, cl.zw);
    float2 qb = clamp(p0 - 1.0 + (size + 2.0), cl.xy, cl.zw);
    r.q = float4(qa, qb);
    r.half_ = size * 0.5;
    r.centre = p0 + r.half_;
    half4 c1 = half4(unpack(color));
    r.pm = half4(c1.rgb * c1.a, c1.a);
    r.pm2 = half4(0.0h);
    r.radius = 0.0;
    r.bw = 0.0;
    r.uv = uv;
    r.kind = 3;
    r.core = float4(0.0);
    r.cv = r.pm;
    return qa.x < qb.x && qa.y < qb.y;
}


kernel void cs_bin32(uint2 tgid2 [[threadgroup_position_in_grid]],
                     uint li [[thread_index_in_threadgroup]],
                     uint lane [[thread_index_in_simdgroup]],
                     uint sg [[simdgroup_index_in_threadgroup]],
                     device const uchar* pool [[buffer(0)]],
                     constant uint& slot_off [[buffer(1)]],
                     device uint4* bins [[buffer(2)]],
                     device uint* counts [[buffer(3)]]) {
    device const Hdr3& h = *(device const Hdr3*)pool;
    device const uint* rows = (device const uint*)(pool + slot_off + 65536 - 256 + 16);
    device const uint* dpage = (device const uint*)(pool + h.dir_page_off);
    device const short4* dink = (device const short4*)(pool + h.dir_ink_off);
    device const uchar* pages = pool + h.pagepool_off;
    uint e = rows[tgid2.x];
    uint ty = e & 0xffu, tx0 = (e >> 8) & 0xffu, tx1 = (e >> 16) & 0xffu;
    short4 th = short4(short(tx1 * 32u), short(ty * 32u + 32u), short(-int(tx0 * 32u)), short(-int(ty * 32u)));
    threadgroup uint sgc[8];
    threadgroup uint dl[256];
    threadgroup uint gl[256];
    threadgroup uint hl[256];      // L3 hits (payload) and their inks, paint order
    threadgroup uint2 hk[256];
    uint nb = 0;
    device uint4* bin = bins + ty * BIN_CAP;
    for (uint d0 = 0; d0 < h.n_dir; d0 += 256) {
        uint di = d0 + li;
        bool dh = di < h.n_dir && all(dink[di] < th);
        threadgroup_barrier(mem_flags::mem_threadgroup);
        uint nd;
        uint ds = compact(dh, lane, sg, sgc, nd);
        if (dh) dl[ds] = dpage[di];
        threadgroup_barrier(mem_flags::mem_threadgroup);
        for (uint p0 = 0; p0 < nd; p0 += 32) {
            uint pk = p0 + (li >> 3), g = li & 7u;
            bool gh = false;
            uint gid = 0;
            if (pk < nd) {
                uint pid = dl[pk];
                gid = pid * 8u + g;
                gh = all(((device const short4*)(pages + pid * PG_BYTES + PG_GINK))[g] < th);
            }
            threadgroup_barrier(mem_flags::mem_threadgroup);
            uint ng;
            uint gs = compact(gh, lane, sg, sgc, ng);
            if (gh) gl[gs] = gid;
            threadgroup_barrier(mem_flags::mem_threadgroup);
            for (uint q0 = 0; q0 < ng; q0 += 32) {
                uint gk = q0 + (li >> 3), s = li & 7u;
                bool rh = false;
                uint ii = 0;
                short4 ink = short4(0);
                if (gk < ng) {
                    uint gidv = gl[gk];
                    uint pid = gidv >> 3, slot = (gidv & 7u) * 8u + s;
                    device const uchar* pg = pages + pid * PG_BYTES;
                    ink = ((device const short4*)(pg + PG_INK))[slot];
                    rh = all(ink < th);
                    ii = ((device const uint*)(pg + PG_PAYLOAD))[slot];
                }
                threadgroup_barrier(mem_flags::mem_threadgroup);
                uint nr;
                uint rs = compact(rh, lane, sg, sgc, nr);
                if (rh) { hl[rs] = ii; hk[rs] = uint2(as_type<uint>(ink.xy), as_type<uint>(ink.zw)); }
                threadgroup_barrier(mem_flags::mem_threadgroup);
                // append in paint order: runs of instance hits in parallel; each text run expanded cooperatively
                uint k = 0;
                while (k < nr) {
                    uint j = k;
                    while (j < nr && (hl[j] & RUN_BIT) == 0u) j++;
                    if (li < j - k) {
                        uint idx = k + li;
                        bin[nb + li] = uint4(hl[idx], hk[idx].x, hk[idx].y, 0u);
                    }
                    nb += j - k;
                    if (j < nr) {
                        uint run = hl[j] & ~RUN_BIT;
                        device const RunHeader& rhd = ((device const RunHeader*)(pool + h.runs_off))[run];
                        uint rlen = rhd.len, rbase = rhd.base;
                        for (uint g0 = 0; g0 < rlen; g0 += 256) {
                            uint gk2 = g0 + li;
                            bool gh = false;
                            short4 gi = short4(0);
                            if (gk2 < rlen) {
                                float2 p0, size;
                                float4 cl;
                                uint color;
                                ushort2 uv;
                                if (glyph_quad(pool, h, run, rbase + gk2, p0, size, cl, color, uv)) {
                                    float2 qa = clamp(p0 - 1.0, cl.xy, cl.zw);
                                    float2 qb = clamp(p0 - 1.0 + (size + 2.0), cl.xy, cl.zw);
                                    if (qa.x < qb.x && qa.y < qb.y) {
                                        gi = short4(short(floor(qa.x)), short(floor(qa.y)), short(-ceil(qb.x)), short(-ceil(qb.y)));
                                        gh = all(gi < th);
                                    }
                                }
                            }
                            threadgroup_barrier(mem_flags::mem_threadgroup);
                            uint ng2;
                            uint gs = compact(gh, lane, sg, sgc, ng2);
                            if (gh) bin[nb + gs] = uint4(run | RUN_BIT, as_type<uint>(gi.xy), as_type<uint>(gi.zw), rbase + gk2);
                            nb += ng2;
                        }
                    }
                    k = j + 1;
                }
                threadgroup_barrier(mem_flags::mem_threadgroup);
            }
        }
    }
    if (li == 0) counts[ty] = nb;
    // (Round 2 attempt, REJECTED, RESULTS 13.1: per-tile lists built here by a sequential scan per tile column made
    //  the bin pass a serial bottleneck: full 0.21 vs 0.18 ms p10, text-heavy 1.5x slower. Row bins are scanned
    //  directly by the tile pass instead.)
}

kernel void cs_tile32b(uint2 tgid2 [[threadgroup_position_in_grid]],
                       uint li [[thread_index_in_threadgroup]],
                       uint2 lt [[thread_position_in_threadgroup]],
                       uint lane [[thread_index_in_simdgroup]],
                       uint sg [[simdgroup_index_in_threadgroup]],
                       device const uchar* pool [[buffer(0)]],
                       constant uint& slot_off [[buffer(1)]],
                       device const uint4* bins [[buffer(2)]],
                       device const uint* counts [[buffer(3)]],
                       texture2d<float> atlas [[texture(0)]],
                       texture2d<float, access::write> t0 [[texture(1)]],
                       texture2d<float, access::write> t1 [[texture(2)]],
                       texture2d<float, access::write> t2 [[texture(3)]],
                       texture2d<float, access::write> t3 [[texture(4)]]) {
    device const Hdr3& h = *(device const Hdr3*)pool;
    device const FrameSlot& fs = *(device const FrameSlot*)(pool + slot_off);
    device const uint* tiles = (device const uint*)(pool + slot_off + 32);
    device const Instance* inst = (device const Instance*)(pool + h.inst_off);
    device const float2* spaces = (device const float2*)(pool + h.spaces_off);
    device const float4* clips = (device const float4*)(pool + h.clips_off);
    uint t = fs.full ? ((tgid2.x / 60u) << 16 | (tgid2.x % 60u)) : tiles[tgid2.x];
    uint2 tile = uint2(t & 0xffffu, t >> 16);
    uint2 o = tile * 32u;
    short4 th = short4(short(o.x + 32u), short(o.y + 32u), short(-int(o.x)), short(-int(o.y)));
    float2 c00 = float2(o + 2u * lt) + 0.5;
    float4 blk = float4(c00, c00 + 1.0);
    threadgroup uint sgc[8];
    threadgroup uint flo[8];
    threadgroup TgRec rec[128];
    half4 acc[4] = { half4(0.0h), half4(0.0h), half4(0.0h), half4(0.0h) };
    device const uint4* bin = bins + tile.y * BIN_CAP;
    uint nb = counts[tile.y];
    for (uint b0 = 0; b0 < nb; b0 += 128) {
        bool rh = false, cover = false;
        TgRec r;
        uint bi = b0 + li;
        if (li < 128 && bi < nb) {
            uint4 en = bin[bi];
            short4 ink = short4(as_type<short2>(en.y), as_type<short2>(en.z));
            if (all(ink < th)) {
                if ((en.x & RUN_BIT) != 0u) {
                    rh = stage_glyph(pool, h, en.x & ~RUN_BIT, en.w, r);
                } else {
                    device const Instance& in = inst[en.x];
                    rh = stage_rec(in, spaces[in.space], clips[in.ext], o, r, cover);
                }
            }
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);   // previous step's shading reads of rec are done
        uint nr;
        uint rs = compact(rh, lane, sg, sgc, nr);
        if (rh) rec[rs] = r;
        uint fl = simd_max((rh && cover) ? rs + 1u : 0u);
        if (lane == 0) flo[sg] = fl;
        threadgroup_barrier(mem_flags::mem_threadgroup);
        uint k0 = 0;
        for (uint s2 = 0; s2 < 8; s2++) k0 = max(k0, flo[s2]);
        k0 = k0 > 0 ? k0 - 1u : 0u;
        for (uint k = k0; k < nr; k++) {
            threadgroup const TgRec& rr = rec[k];
            if (rr.q.x > blk.z || rr.q.z <= blk.x || rr.q.y > blk.w || rr.q.w <= blk.y) continue;
            if (blk.x >= rr.core.x && blk.z < rr.core.z && blk.y >= rr.core.y && blk.w < rr.core.w) {
                half4 cv = rr.cv;
                for (uint j = 0; j < 4; j++) acc[j] = cv + acc[j] * (1.0h - cv.a);
                continue;
            }
            for (uint j = 0; j < 4; j++) {
                half4 sh = shade_h(rr, c00 + float2(float(j & 1u), float(j >> 1)), atlas);
                acc[j] = sh + acc[j] * (1.0h - sh.a);
            }
        }
    }
    for (uint j = 0; j < 4; j++) {
        uint2 px = o + 2u * lt + uint2(j & 1u, j >> 1);
        float4 v = float4(acc[j]);
        switch (fs.target) {
        case 0: t0.write(v, px); break;
        case 1: t1.write(v, px); break;
        case 2: t2.write(v, px); break;
        default: t3.write(v, px); break;
        }
    }
}

// ============================================================================ FUSED REPAIR (the façade's presenter)
// The binned 32x32 repair (cs_bin32 + cs_tile32b) reading the fused system's ONE shared region in place
// (INTERFACE.md, "fused layout"): bough-core's pages (page-relative inks + offset lanes, clip column), its
// directory columns and clip table, bough-text's glyph slots / run headers / strike token tables, integral
// space translations, and the façade's 48-byte quad instances. Window size is a runtime value.
// Record payload: bit 31 = text (runs [p & 0xFFFFF, + ((p >> 20) & 0x7FF) + 1)), bit 30 = instance run
// (instances [p & 0x3FFFF, + ((p >> 18) & 0xFFF) + 1)), else one instance index.
//
// The damage IS the dispatch: the frame slot holds the damaged tile rows themselves (one 32-byte record per
// damaged row: its index, x-span, the count of damaged tiles in the rows before it, and its 128 tile bits),
// written by the CPU in O(damaged rows). The bin pass runs one threadgroup per record; the tile pass's
// threadgroup i finds its tile by a binary search over the prefix counts and a select of the k-th set bit.
struct FSlot {
    uint bin_tg[3];    // +0  indirect args of the bin pass: (nrows, 1, 1)
    uint tile_tg[3];   // +12 indirect args of the tile pass: (ntiles, 1, 1)
    uint target;       // +24
    uint nrows;        // +28
    uint copy_tg[3];   // +32 indirect args of the scroll copy: (tiles of the destination's tile span, 1, 1), or 0
    uint src;          // +44 source surface of the copy (the one showing the previous frame)
    uint dst0, dst1;   // +48 destination rect [x0, y0, x1, y1) as u16 pairs
    uint d;            // +56 the move (dx, dy) as i16 pair: destination p reads source p - d
    uint cspan;        // +60 the copy's tile span: x0 | y0 << 8 | width << 16
    // FRow rows[] at +64
};
constant uint FSLOT_ROWS = 64;
struct FRow {
    uint ty;
    uint span;         // x0 | x1 << 8: tiles [x0, x1) hold every damaged tile of the row
    uint prefix;       // damaged tiles in the rows before this one
    uint pad;
    uint4 bits;        // bit x = tile (x, ty) damaged
};
struct FHdr {
    float2 viewport;
    uint tiles_x, tiles_y;
    uint inst_off, spaces_off, clips_off, dir_id_off, dir_ink_off, n_dir, pages_off;
    uint gx_off, gent_off, runs_off, toks_off, tok_stride;
    uint bin_cap, debug;
    // capacity - 1 of each column (all capacities are powers of two): every index read from region data is
    // ANDed with its column's mask, every loop bound read from it is min-ed with a capacity, so no value in
    // the region can make a kernel read outside its column or loop unboundedly. No branch, ~1 ALU op each.
    uint m_inst, m_page, m_glyph, m_run, m_space, m_clip, m_strike;
    uint xforms_off;
};
// The bind table (header + FBIND_OFF): every resource the kernels touch besides the region itself, as Metal 3
// resource ids / GPU addresses written by the CPU. Nothing is bound per encoder but the region and the frame
// slot offset, so a pre-encoded frame never names a surface (no per-segment surface tracking in the driver, no
// IOSurface use counts held by pending frames) and growing the bins is a header write. Everything here is in
// the queue's residency set.
struct FCert {
    uint2 c[8];
    uchar m[8];
};
struct FBind {
    texture2d<float, access::write> t[4];
    texture2d<float> atlas;
    device uint4* bins;
    device atomic_uint* counts;
    texture2d<float, access::read> r[4];   // round 3c: the same surfaces, read (the scroll copy's source)
    texture2d<float> img;      // premultiplied RGBA images and gradient ramps (the glyph atlas until one exists)
    device FCert* cert[4];     // round 6: each surface's certificates (below)
};
constant uint FBIND_OFF = 128;
// The column table (header + FCOL_OFF, round 6, lane memory): every GPU-read column's GPU address. Each column
// is either a range of the region buffer or its own no-copy view of the region's memory covering its used
// prefix (bough-metal `Fused::new_views`): a column's capacity is then address space and only what the masks
// can reach is wired. The kernels read every column through this table, never `region + offset`.
struct FCols {
    device const uchar* inst;
    device const uchar* spaces;
    device const uchar* clips;
    device const uchar* dir_id;
    device const uchar* dir_ink;
    device const uchar* pages;
    device const uchar* gx;
    device const uchar* gent;
    device const uchar* runs;
    device const uchar* toks;
    device const uchar* xforms;
};
constant uint FCOL_OFF = 256;
static device const FCols& fcols(device const uchar* pool) { return *(device const FCols*)(pool + FCOL_OFF); }
// Row certificates (round 6). A band is 32 x 4 px of a tile (the tile pass shades one band per simdgroup).
// Every surface has a plane of one `FCert` per tile (`cert[ty * tiles_x + tx]`): per band a colour `c` (the
// bits of a half4; 0 = none) and a mask of the band's 4 pixel rows.
// INVARIANT: mask bit j of a band is set only if every pixel of row j of that band (32 px) holds, in that
// surface, the bytes that `write(float4(c))` produces: each was written so, or copied from such a pixel.
// Whoever writes pixels keeps it: the tile pass certifies a band it has just shaded when all 128 of its
// accumulators are bit-equal; the scroll copy derives each destination row's certificate from the source
// rows it would read (rows, not bands: a move by any dy keeps what is known, nothing erodes). That makes a
// frame's work per row of a band DATA, not a code path chosen ahead:
//   PAINT  the damage row marks the tile                                  -> fs_tile shades it
//   SKIP   source row certified c, destination row certified c            -> nothing is read or written
//   FILL   source row certified c, destination not                        -> 32 writes of c, no read
//   MOVE   otherwise                                                      -> read at p - d, write
// A zero plane (a fresh, resized or trimmed surface) is all MOVE: the copy as it was before certificates.
constant uint CERT_FROZEN = 0x100u;   // FSlot.src bit: the copy leaves the destination's certificates as they are
                                      // (kernel timing: every repetition of a frame decides as the first)
typedef Instance FInst;
constant uint F_RUN = 0x80000000u;
constant uint F_IRUN = 0x40000000u;
constant uint FPG_BYTES = 1472;
constant uint FPG_INK = 128;
constant uint FPG_GINK = 640;
constant uint FPG_PAYLOAD = 768;
constant uint FPG_CLIP = 1280;
constant uint FPG_OFFL = 1436;
constant uint FPG_XF = 1448;   // the page's affine: float4 M - I, float2 translation (all zero: none)

// A transformed page (bough-core `Page::xm/xv`, round 3c): its stored inks are untransformed; absolute =
// M s + xv + off. The bin pass culls with the transformed bounding box (grown by a pixel), as the CPU's
// `Page::abs`; the instances themselves are transformed per instance by their space (`FXf`).
static bool f_pg_xf(device const uchar* pg) {
    float4 xm = *(device const float4*)(pg + FPG_XF);
    float2 xv = *(device const float2*)(pg + FPG_XF + 16);
    return any(xm != 0.0) || any(xv != 0.0);
}
static short4 f_pg_xink(device const uchar* pg, short4 rel) {
    if (rel.x >= -rel.z || rel.y >= -rel.w) return rel;   // empty stays empty (absorbing)
    float4 xm = *(device const float4*)(pg + FPG_XF);
    float2 xv = *(device const float2*)(pg + FPG_XF + 16);
    short4 o = *(device const short4*)(pg + FPG_OFFL);
    float4 m = xm + float4(1.0, 0.0, 0.0, 1.0);
    float2 t = xv + float2(float(o.x), float(o.y));
    float2 a = float2(float(rel.x), float(rel.y)), b = float2(-float(rel.z), -float(rel.w));
    float2 p0 = float2(m.x * a.x + m.y * a.y, m.z * a.x + m.w * a.y) + t;
    float2 p1 = float2(m.x * b.x + m.y * a.y, m.z * b.x + m.w * a.y) + t;
    float2 p2 = float2(m.x * a.x + m.y * b.y, m.z * a.x + m.w * b.y) + t;
    float2 p3 = float2(m.x * b.x + m.y * b.y, m.z * b.x + m.w * b.y) + t;
    float2 lo = clamp(floor(min(min(p0, p1), min(p2, p3))) - 1.0, -30000.0, 30000.0);
    float2 hi = clamp(ceil(max(max(p0, p1), max(p2, p3))) + 1.0, -30000.0, 30000.0);
    return short4(short(lo.x), short(lo.y), short(-hi.x), short(-hi.y));
}

// The ink margin beyond an instance's box: none for a solid quad, 1 px for an SDF edge, or the instance's
// own (`flags`: a shadow's blur reach, a rotated image's circumscribed circle). The CPU's ink rects use the
// same value (bough deco.rs), so a record's ink always holds every pixel the kernel writes for it.
static float f_margin(uint kind, uint flags) {
    return kind == 0u ? 0.0 : (flags != 0u ? float(flags) : 1.0);
}

static float4 f_clip(device const uchar* pool, device const FHdr& h, uint c) {
    short4 r = ((device const short4*)(fcols(pool).clips))[c & h.m_clip];
    return float4(float(r.x), float(r.y), -float(r.z), -float(r.w));
}
static float2 f_space(device const uchar* pool, device const FHdr& h, uint s) {
    return float2(((device const int2*)(fcols(pool).spaces))[s & h.m_space]);
}
// bough-text's pen formula, verified bit-exact there: px = floor(fma(float(x - x0), scale, origin.x + 0.5)) + geo.x
static bool f_glyph_rh(device const uchar* pool, device const FHdr& h, device const RunHeader& rh, uint slot,
                    thread float2& p0, thread float2& size, thread uint& color, thread ushort2& uv);
static bool f_glyph(device const uchar* pool, device const FHdr& h, uint run, uint slot,
                    thread float2& p0, thread float2& size, thread uint& color, thread ushort2& uv) {
    device const RunHeader& rh = ((device const RunHeader*)(fcols(pool).runs))[run & h.m_run];
    return f_glyph_rh(pool, h, rh, slot, p0, size, color, uv);
}
static uint f_run_space(device const uchar* pool, device const FHdr& h, uint run) {
    return uint(((device const RunHeader*)(fcols(pool).runs))[run & h.m_run].space);
}
static bool f_glyph_rh(device const uchar* pool, device const FHdr& h, device const RunHeader& rh, uint slot,
                    thread float2& p0, thread float2& size, thread uint& color, thread ushort2& uv) {
    int x = ((device const int*)(fcols(pool).gx))[slot & h.m_glyph];
    uint e = ((device const ushort*)(fcols(pool).gent))[slot & h.m_glyph] & 255u;
    device const GEntry& en = *(device const GEntry*)(fcols(pool).toks + (uint(rh.strike) & h.m_strike) * h.tok_stride + e * 32u);
    if (en.uv.z == 0) return false;
    int dx = as_type<int>(as_type<uint>(x) - as_type<uint>(rh.x0));
    float px = floor(fma(float(dx), rh.scale, rh.origin.x + 0.5)) + en.geo.x;
    p0 = float2(px, rh.origin.y + en.geo.y) + f_space(pool, h, rh.space);
    size = en.geo.zw;
    color = rh.color;
    uv = en.uv.xy;
    return true;
}
// A space's affine (round 3c, input agent: transformed subtrees). Stored as M - I, the pivot c (space-local
// px) and a translation t, so the zeroed column is the identity: a point u in untransformed absolute px
// (space-local + T) is drawn at K + t + M (u - K), K = T + c.
struct FXf { float4 dm; float2 c; float2 t; };
static bool f_xf(device const uchar* pool, device const FHdr& h, uint s, thread float4& m, thread float2& k, thread float2& t) {
    device const FXf& x = ((device const FXf*)(fcols(pool).xforms))[s & h.m_space];
    m = x.dm + float4(1.0, 0.0, 0.0, 1.0);
    k = f_space(pool, h, s) + x.c;
    t = x.t;
    return any(x.dm != 0.0) || any(x.t != 0.0);
}
static float2 f_xmul(float4 m, float2 v) { return float2(m.x * v.x + m.y * v.y, m.z * v.x + m.w * v.y); }
// The transformed box [a, b] (untransformed absolute px): its bounding box, grown by a pixel.
static float4 f_xbox(float2 a, float2 b, float4 m, float2 k, float2 t) {
    float2 p0 = k + t + f_xmul(m, a - k), p1 = k + t + f_xmul(m, float2(b.x, a.y) - k);
    float2 p2 = k + t + f_xmul(m, float2(a.x, b.y) - k), p3 = k + t + f_xmul(m, b - k);
    return float4(min(min(p0, p1), min(p2, p3)) - 1.0, max(max(p0, p1), max(p2, p3)) + 1.0);
}

static short4 f_qink(float2 qa, float2 qb) {
    return short4(short(floor(qa.x)), short(floor(qa.y)), short(-ceil(qb.x)), short(-ceil(qb.y)));
}

kernel void fs_bin(uint2 tgid2 [[threadgroup_position_in_grid]],
                   uint li [[thread_index_in_threadgroup]],
                   uint lane [[thread_index_in_simdgroup]],
                   uint sg [[simdgroup_index_in_threadgroup]],
                   device const uchar* pool [[buffer(0)]],
                   constant uint2& slot [[buffer(1)]]) {
    device const FHdr& h = *(device const FHdr*)pool;
    device const FBind& fb = *(device const FBind*)(pool + FBIND_OFF);
    device uint4* bins = fb.bins;
    device const FRow* frows = (device const FRow*)(pool + slot.x + FSLOT_ROWS);
    device const uint* dpage = (device const uint*)(fcols(pool).dir_id);
    device const short4* dink = (device const short4*)(fcols(pool).dir_ink);
    device const uchar* pages = fcols(pool).pages;
    device const FInst* inst = (device const FInst*)(fcols(pool).inst);
    FRow fr = frows[tgid2.x];
    uint ty = fr.ty, tx0 = fr.span & 0xffu, tx1 = (fr.span >> 8) & 0xffu;
    short4 th = short4(short(tx1 * 32u), short(ty * 32u + 32u), short(-int(tx0 * 32u)), short(-int(ty * 32u)));
    threadgroup uint sgc[8];
    threadgroup uint dl[256];
    threadgroup uint gl[256];
    threadgroup uint hl[256];      // L3 hits: payload, absolute ink, clip id; paint order
    threadgroup uint2 hk[256];
    threadgroup uint hc[256];
    threadgroup uint sp[256];      // hit -> its first segment
    threadgroup uint sd[256];      // segment: an instance, an instance run (payload) or a glyph run (run | F_RUN)
    threadgroup uint sh[256];      // segment -> its hit
    threadgroup uint ip[256];      // segment -> its first item
    uint nb = 0;
    uint cap = h.bin_cap;
    device uint4* bin = bins + ty * cap;
    uint ndir = min(h.n_dir, 2u * (h.m_page + 1u));   // the directory columns hold 2x pages (blocks with holes)
    for (uint d0 = 0; d0 < ndir; d0 += 256) {
        uint di = d0 + li;
        bool dh = di < ndir && all(dink[di] < th);
        threadgroup_barrier(mem_flags::mem_threadgroup);
        uint nd;
        uint ds = compact(dh, lane, sg, sgc, nd);
        if (dh) dl[ds] = dpage[di] & h.m_page;
        threadgroup_barrier(mem_flags::mem_threadgroup);
        for (uint p0 = 0; p0 < nd; p0 += 32) {
            uint pk = p0 + (li >> 3), g = li & 7u;
            bool gh = false;
            uint gid = 0;
            if (pk < nd) {
                uint pid = dl[pk];
                gid = pid * 8u + g;
                device const uchar* pg = pages + pid * FPG_BYTES;
                short4 thp = subsat(th, *(device const short4*)(pg + FPG_OFFL));   // page-relative threshold
                gh = f_pg_xf(pg) ? all(f_pg_xink(pg, ((device const short4*)(pg + FPG_GINK))[g]) < th)
                                 : all(((device const short4*)(pg + FPG_GINK))[g] < thp);
            }
            threadgroup_barrier(mem_flags::mem_threadgroup);
            uint ng;
            uint gs = compact(gh, lane, sg, sgc, ng);
            if (gh) gl[gs] = gid;
            threadgroup_barrier(mem_flags::mem_threadgroup);
            for (uint q0 = 0; q0 < ng; q0 += 32) {
                uint gk = q0 + (li >> 3), s = li & 7u;
                bool rh = false;
                uint ii = 0, cid = 0;
                short4 ink = short4(0);
                if (gk < ng) {
                    uint gidv = gl[gk];
                    uint pid = gidv >> 3, sl = (gidv & 7u) * 8u + s;
                    device const uchar* pg = pages + pid * FPG_BYTES;
                    short4 offl = *(device const short4*)(pg + FPG_OFFL);
                    short4 rel = ((device const short4*)(pg + FPG_INK))[sl];
                    if (f_pg_xf(pg)) {
                        ink = f_pg_xink(pg, rel);                                    // transformed bounding box
                        rh = all(ink < th);
                    } else {
                        rh = all(rel < subsat(th, offl));
                        ink = rel + offl;                                            // absolute (in domain when hit)
                    }
                    ii = ((device const uint*)(pg + FPG_PAYLOAD))[sl];
                    cid = ((device const ushort*)(pg + FPG_CLIP))[sl] & h.m_clip;
                }
                threadgroup_barrier(mem_flags::mem_threadgroup);
                uint nr;
                uint rs = compact(rh, lane, sg, sgc, nr);
                if (rh) { hl[rs] = ii; hk[rs] = uint2(as_type<uint>(ink.xy), as_type<uint>(ink.zw)); hc[rs] = cid; }
                threadgroup_barrier(mem_flags::mem_threadgroup);
                // Append in paint order (round 6). A hit is one instance, an instance run or a text record of
                // several glyph runs. Hits expand to SEGMENTS (an instance, an instance run, one glyph run)
                // and segments to ITEMS (an instance, a glyph); both expansions are prefix sums, and a
                // thread finds its segment or item by a binary search over them, so 256 items are tested
                // per step whatever runs they belong to. (Before, every run took its own steps: a row of
                // 30 short labels took 30, and run expansion was 60 % of the bin pass on text.) Segments
                // and items are numbered in paint order, and compaction keeps it.
                uint v = 0;
                if (li < nr) {
                    uint p = hl[li];
                    v = (p & F_RUN) != 0u ? ((((p >> 20) & 0x7FFu) + 1u) | 1u << 20) : ((p & F_IRUN) != 0u ? (1u | 1u << 20) : 1u);
                }
                uint vt;
                uint vp = prefix(v, lane, sg, sgc, vt);
                uint nseg = vt & 0xFFFFFu;
                if ((vt >> 20) == 0u) {
                    // no run among the hits: the hits are the items
                    if (li < nr && nb + li < cap) bin[nb + li] = uint4(hl[li] & h.m_inst, hk[li].x, hk[li].y, hc[li] << 20);
                    nb += nr;
                } else {
                    if (li < nr) sp[li] = vp & 0xFFFFFu;
                    threadgroup_barrier(mem_flags::mem_threadgroup);
                    for (uint s0 = 0; s0 < nseg; s0 += 256) {
                        // this thread's segment: its hit (the last whose first segment is <= s), then what it is
                        uint sgi = s0 + li, c = 0, desc = 0, hit = 0;
                        if (sgi < nseg) {
                            uint lo = 0, hi = nr;
                            while (hi - lo > 1u) {
                                uint mid = (lo + hi) >> 1;
                                if (sp[mid] <= sgi) lo = mid; else hi = mid;
                            }
                            hit = lo;
                            uint p = hl[lo];
                            if ((p & F_RUN) != 0u) {
                                uint run = ((p & 0xFFFFFu) + (sgi - sp[lo])) & h.m_run;
                                c = min(((device const RunHeader*)(fcols(pool).runs))[run].len, h.m_glyph + 1u);
                                desc = run | F_RUN;
                            } else if ((p & F_IRUN) != 0u) {
                                c = ((p >> 18) & 0xFFFu) + 1u;
                                desc = p;
                            } else {
                                c = 1u;
                                desc = p & h.m_inst;
                            }
                        }
                        threadgroup_barrier(mem_flags::mem_threadgroup);   // the previous chunk's readers of sd, sh, ip are done
                        uint nitem;
                        uint cp = prefix(c, lane, sg, sgc, nitem);
                        sd[li] = desc;
                        sh[li] = hit;
                        ip[li] = cp;
                        uint ns = min(nseg - s0, 256u);
                        for (uint t0 = 0; t0 < nitem; t0 += 256) {
                            threadgroup_barrier(mem_flags::mem_threadgroup);   // sd, sh, ip are written; the previous step's readers of sgc are done
                            uint t = t0 + li;
                            bool ok = false;
                            uint4 e = uint4(0u);
                            if (t < nitem) {
                                // this thread's item: its segment (the last whose first item is <= t), then its place in it
                                uint lo = 0, hi = ns;
                                while (hi - lo > 1u) {
                                    uint mid = (lo + hi) >> 1;
                                    if (ip[mid] <= t) lo = mid; else hi = mid;
                                }
                                uint u = t - ip[lo], d = sd[lo], hx = sh[lo];
                                uint cid2 = hc[hx];
                                if ((d & (F_RUN | F_IRUN)) == 0u) {
                                    ok = true;
                                    e = uint4(d, hk[hx].x, hk[hx].y, cid2 << 20);
                                } else {
                                    float4 cl = f_clip(pool, h, cid2);
                                    float2 qa = float2(0.0), qb = float2(0.0);
                                    uint spc = 0u;
                                    bool have = false;
                                    if ((d & F_RUN) != 0u) {
                                        uint run = d & ~F_RUN;
                                        uint slot = ((device const RunHeader*)(fcols(pool).runs))[run].base + u;
                                        float2 q0, size;
                                        uint color;
                                        ushort2 uv;
                                        if (f_glyph(pool, h, run, slot, q0, size, color, uv)) {
                                            qa = q0 - 1.0;
                                            qb = q0 - 1.0 + (size + 2.0);
                                            spc = f_run_space(pool, h, run);
                                            have = true;
                                        }
                                        e = uint4(d, 0u, 0u, (cid2 << 20) | (slot & h.m_glyph));
                                    } else {
                                        uint idx = ((d & 0x3FFFFu) + u) & h.m_inst;
                                        device const FInst& q = inst[idx];
                                        float2 q0 = q.pos + f_space(pool, h, q.space);
                                        float m = f_margin(q.kind, q.flags);
                                        qa = q0 - m;
                                        qb = q0 + q.size + m;
                                        spc = q.space;
                                        have = true;
                                        e = uint4(idx, 0u, 0u, cid2 << 20);
                                    }
                                    if (have) {
                                        float4 xm; float2 xk, xt;
                                        if (f_xf(pool, h, spc, xm, xk, xt)) {
                                            float4 bb = f_xbox(qa, qb, xm, xk, xt);
                                            qa = bb.xy; qb = bb.zw;
                                        }
                                        qa = clamp(qa, cl.xy, cl.zw);
                                        qb = clamp(qb, cl.xy, cl.zw);
                                        if (qa.x < qb.x && qa.y < qb.y) {
                                            short4 gi = f_qink(qa, qb);
                                            ok = all(gi < th);
                                            e.y = as_type<uint>(gi.xy);
                                            e.z = as_type<uint>(gi.zw);
                                        }
                                    }
                                }
                            }
                            uint na;
                            uint as_ = compact(ok, lane, sg, sgc, na);
                            if (ok && nb + as_ < cap) bin[nb + as_] = e;
                            nb += na;
                        }
                    }
                }
                threadgroup_barrier(mem_flags::mem_threadgroup);
            }
        }
    }
    if (li == 0) {
        atomic_store_explicit(fb.counts + ty, min(nb, cap), memory_order_relaxed);
        // overflow: the largest row count the CPU must grow the bins to (it redraws the frame then)
        if (nb > cap) atomic_fetch_max_explicit(fb.counts + h.tiles_y, nb, memory_order_relaxed);
    }
}

// Staged record, EDGE FORM: the geometry is kept as its four edges `b` (x0, y0, x1, y1), never as centre and
// half size, and every distance is taken to an edge directly (`max(c - b1, b0 - c)`). With the façade's
// geometry on a 1/256-px grid every one of these differences is exact in f32, so a pixel's value depends
// bit for bit only on the edges within its influence band: an edge that did not move leaves every pixel
// outside the moved edges' bands unchanged, which is what lets the CPU damage only those bands
// (bough-core `damage_edges`, RESULTS Part IV).
struct FRec {
    float4 q;          // clip-clamped coverage window incl. AA margin (pixel-centre test)
    float4 core;       // pixel centres inside `core` take `cv` with no per-pixel work (coverage provably 1)
    float4 b;          // geometry edges x0, y0, x1, y1 (unclamped)
    half4 cv;
    half4 pm;          // premultiplied colour, unpacked once per record (not per pixel)
    half4 pm2;
    float radius, bw;
    ushort2 uv;
    ushort kind, pad;
    float2 gc;         // gradient: radial centre as fractions of the box
    float4 xi;         // transformed space: M^-1 (u = xk + M^-1 (c - xk - xt)); xs = 0: untransformed
    float2 xk, xt;
    float xs;          // the transform's scale (sqrt |det M|): SDF distances in u are xs times smaller in px
};

// Rounded-box SDF in edge form (== sd_rbox(c - centre, half, r) mathematically).
static float f_sd(float2 c, float4 b, float r) {
    float2 q = max(c - b.zw, b.xy - c) + r;
    float2 m = max(q, 0.0);
    // the length with every operation specified (round 6, lane memory), so the CPU repair backend can compute the
    // same bits: `length` under fast math is x * rsqrt(x) with a hardware rsqrt that is not correctly rounded (4 % of
    // inputs, examples/mathprobe.rs). A correctly rounded square root of the dot as one fma costs nothing measurable
    // (kernel_bench, lab: 0.2784 vs 0.2787 ms full redraw); a branch around it (edges skipping the root) cost 4.6 %.
    return precise::sqrt(fma(m.y, m.y, m.x * m.x)) + min(max(q.x, q.y), 0.0) - r;
}

// `lim`: the atlas's last texel (its size - 1), read once per kernel by the caller.
static half4 f_shade(threadgroup const FRec& r, float2 c, texture2d<float> atlas, uint2 lim) {
    if (!(c.x >= r.q.x && c.x < r.q.z && c.y >= r.q.y && c.y < r.q.w)) return half4(0.0h);
    switch (r.kind) {
    case 0:
        return r.pm;
    case 1: {
        float d = f_sd(c, r.b, r.radius);
        return r.pm * half(clamp(0.5 - d, 0.0, 1.0)); }
    case 2: {
        float d = f_sd(c, r.b, r.radius);
        float outer = clamp(0.5 - d, 0.0, 1.0);
        float inner = clamp(0.5 - (d + r.bw), 0.0, 1.0);
        return fma(r.pm, half4(half(max(outer - inner, 0.0))), r.pm2 * half(inner)); }   // the contraction the compiler chose, written out
    default: {
        float2 t = c - r.b.xy;
        half cov = 0.0h;
        if (all(t >= 0.0) && all(t < r.b.zw - r.b.xy)) {
            cov = half(atlas.read(min(uint2(float2(r.uv)) + uint2(t), lim)).r);
        }
        return r.pm * cov; }
    }
}

static float2 f_erf2(float2 x) {
    float2 s = sign(x), a = min(abs(x), 2.8284271);   // saturated beyond 4 sigma: exactly +-1 there
    x = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    x *= x;
    return select(s - s / (x * x), s, a >= 2.8284271);
}
// A rounded box's Gaussian shadow (Evan Wallace's closed form along x, 4 samples along y), in edge form. A
// pixel depends on an edge only within 4 sigma of it (+ the corner radius): the erf saturates there and the
// y window is +-3 sigma, so the edge bands of a moved shadow (bough rt.rs `edge_band`) hold every change.
static float f_shadow_x(float x, float y, float sigma, float corner, float2 hs) {
    float delta = min(hs.y - corner - abs(y), 0.0);
    float curved = hs.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    float2 integral = 0.5 + 0.5 * f_erf2((x + float2(-curved, curved)) * (0.70710678 / sigma));
    return integral.y - integral.x;
}
static float f_shadow(float2 p, float2 hs, float sigma, float corner) {
    float low = p.y - hs.y, high = p.y + hs.y;
    float start = clamp(-3.0 * sigma, low, high), end = clamp(3.0 * sigma, low, high);
    float step = (end - start) * 0.25;
    float y = start + step * 0.5, v = 0.0;
    for (int i = 0; i < 4; i++) {
        float g = exp(-(y * y) / (2.0 * sigma * sigma)) / (2.5066283 * sigma);
        v += f_shadow_x(p.x, p.y - y, sigma, corner, hs) * g * step;
        y += step;
    }
    return v;
}

// The fused tile pass's shading: the base kinds as `f_shade`, plus images (premultiplied RGBA, rotated about
// their centre by `radius` rad, bilinear), gradients (a 256-texel premultiplied ramp in the image atlas,
// linear at angle `bw` (CSS: 0 = to top, clockwise) or a radial circle about `gc` (fractions of the box)
// reaching its farthest corner, in a rounded box) and box shadows (blur sigma `bw`). Every texel read is
// clamped to the texture (bounds by construction, as the masks).
// A record in a transformed space: the pixel centre mapped into the untransformed frame, distances scaled.
static half4 f_shade_t(threadgroup const FRec& r, float2 u, texture2d<float> atlas) {
    float s = r.xs;
    switch (r.kind) {
    case 0: {
        float d = f_sd(u, r.b, 0.0) * s;
        return r.pm * half(clamp(0.5 - d, 0.0, 1.0)); }
    case 1: {
        float d = f_sd(u, r.b, r.radius) * s;
        return r.pm * half(clamp(0.5 - d, 0.0, 1.0)); }
    case 2: {
        float d = f_sd(u, r.b, r.radius);
        float outer = clamp(0.5 - d * s, 0.0, 1.0);
        float inner = clamp(0.5 - (d + r.bw) * s, 0.0, 1.0);
        return r.pm * half(max(outer - inner, 0.0)) + r.pm2 * half(inner); }
    default: {
        // the glyph's mask, bilinear (texels outside the glyph's rect are empty)
        float2 sz = r.b.zw - r.b.xy;
        float2 t = u - r.b.xy - 0.5;
        float2 f0 = floor(t), fr = t - f0;
        int2 i0 = int2(f0);
        int2 lim = int2(int(atlas.get_width()) - 1, int(atlas.get_height()) - 1);
        float cov = 0.0;
        for (int k = 0; k < 4; k++) {
            int2 d = int2(k & 1, k >> 1);
            int2 q = i0 + d;
            if (q.x < 0 || q.y < 0 || float(q.x) >= sz.x || float(q.y) >= sz.y) continue;
            float wgt = (d.x == 1 ? fr.x : 1.0 - fr.x) * (d.y == 1 ? fr.y : 1.0 - fr.y);
            cov += atlas.read(uint2(clamp(int2(r.uv) + q, int2(0), lim))).r * wgt;
        }
        return r.pm * half(cov); }
    }
}

static half4 f_shade_x(threadgroup const FRec& r, float2 c, texture2d<float> atlas, texture2d<float> img) {
    if (r.xs != 0.0) {
        if (!(c.x >= r.q.x && c.x < r.q.z && c.y >= r.q.y && c.y < r.q.w)) return half4(0.0h);
        c = r.xk + float2(r.xi.x * (c.x - r.xt.x) + r.xi.y * (c.y - r.xt.y), r.xi.z * (c.x - r.xt.x) + r.xi.w * (c.y - r.xt.y));
        if (r.kind < 4u) return f_shade_t(r, c, atlas);
    } else {
        if (r.kind < 4u) return f_shade(r, c, atlas, uint2(atlas.get_width() - 1u, atlas.get_height() - 1u));
        if (!(c.x >= r.q.x && c.x < r.q.z && c.y >= r.q.y && c.y < r.q.w)) return half4(0.0h);
    }
    float2 hs = (r.b.zw - r.b.xy) * 0.5;
    float2 p = c - (r.b.xy + hs);
    int2 lim = int2(int(img.get_width()) - 1, int(img.get_height()) - 1);
    switch (r.kind) {
    case 4: {
        float cs = cos(r.radius), sn = sin(r.radius);
        float2 t = float2(p.x * cs + p.y * sn, -p.x * sn + p.y * cs) + hs - 0.5;
        float2 sz = hs * 2.0;
        if (t.x <= -1.0 || t.y <= -1.0 || t.x >= sz.x || t.y >= sz.y) return half4(0.0h);
        float2 f0 = floor(t), fr = t - f0;
        int2 i0 = int2(f0);
        float4 acc = float4(0.0);
        for (int k = 0; k < 4; k++) {
            int2 d = int2(k & 1, k >> 1);
            int2 q = i0 + d;
            if (q.x < 0 || q.y < 0 || q.x >= int(sz.x) || q.y >= int(sz.y)) continue;
            float wgt = (d.x == 1 ? fr.x : 1.0 - fr.x) * (d.y == 1 ? fr.y : 1.0 - fr.y);
            acc += img.read(uint2(clamp(int2(r.uv) + q, int2(0), lim))) * wgt;
        }
        return half4(acc) * r.pm.a; }
    case 5: {
        float t;
        if (r.pad == 0u) {
            float2 dir = float2(sin(r.bw), -cos(r.bw));
            float len = abs(2.0 * hs.x * dir.x) + abs(2.0 * hs.y * dir.y);
            t = dot(p, dir) / max(len, 1e-6) + 0.5;
        } else {
            float2 g = (r.gc - 0.5) * 2.0 * hs;
            t = length(p - g) / max(length(abs(g) + hs), 1e-6);
        }
        float x = clamp(t, 0.0, 1.0) * 255.0;
        float i = min(floor(x), 254.0), f = x - i;
        int2 a0 = clamp(int2(r.uv) + int2(int(i), 0), int2(0), lim), a1 = clamp(int2(r.uv) + int2(int(i) + 1, 0), int2(0), lim);
        float4 c0 = img.read(uint2(a0)), c1 = img.read(uint2(a1));
        float cov = clamp(0.5 - f_sd(c, r.b, r.radius) * (r.xs != 0.0 ? r.xs : 1.0), 0.0, 1.0);
        return half4(mix(c0, c1, f) * cov); }
    default:
        return r.pm * half(clamp(f_shadow(p, hs, max(r.bw, 1e-3), r.radius), 0.0, 1.0));
    }
}

// Solid quads are pixel-exact (no AA margin); the others carry a 1 px margin for their SDF edge, or their own
// (`f_margin`).
// Stage the inverse of a transformed space into `r` and return the coverage window of [a, b] under it.
static void f_stage_xf(thread FRec& r, float4 xm, float2 xk, float2 xt, thread float2& qa, thread float2& qb) {
    float4 bb = f_xbox(qa, qb, xm, xk, xt);
    qa = bb.xy;
    qb = bb.zw;
    float det = xm.x * xm.w - xm.y * xm.z;
    float id = abs(det) > 1e-12 ? 1.0 / det : 0.0;
    r.xi = float4(xm.w, -xm.y, -xm.z, xm.x) * id;
    r.xk = xk;
    r.xt = xk + xt;
    r.xs = max(sqrt(abs(det)), 1e-6);
}

static bool f_stage_rec(device const FInst& in, float2 off, float4 cl, uint2 o, thread FRec& r, thread bool& cover,
                        device const uchar* pool, device const FHdr& h) {
    float2 p0 = in.pos + off;
    float2 p1 = p0 + in.size;
    float m = f_margin(in.kind, in.flags);
    float2 qa = p0 - m, qb = p1 + m;
    float4 xm; float2 xk, xt;
    bool xf = f_xf(pool, h, in.space, xm, xk, xt);
    r.xs = 0.0;
    if (xf) f_stage_xf(r, xm, xk, xt, qa, qb);
    qa = clamp(qa, cl.xy, cl.zw);
    qb = clamp(qb, cl.xy, cl.zw);
    r.q = float4(qa, qb);
    r.b = float4(p0, p1);
    half4 c1 = half4(unpack(in.color)), c2 = half4(unpack(in.color2));
    r.pm = half4(c1.rgb * c1.a, c1.a);
    r.pm2 = half4(c2.rgb * c2.a, c2.a);
    // CSS clamps a corner radius to the box (a pill's 9999 px is half its height); an image's word is its angle
    float rad = in.kind == 4 ? in.radius : min(in.radius, 0.5 * min(in.size.x, in.size.y));
    r.radius = rad;
    r.bw = in.bw;
    r.uv = in.uv.xy;
    r.kind = in.kind;
    r.pad = in.uv.z;
    r.gc = float2(as_type<float>(in.color), as_type<float>(in.color2));
    float k = in.kind == 1 ? max(rad, 0.5) + 1.0 : max(rad, 0.5 + in.bw) + 1.0;
    float4 cb = float4(p0 + k, p1 - k);
    cb = float4(max(cb.xy, qa), min(cb.zw, qb));
    r.core = xf ? float4(0.0) : (in.kind == 0 ? float4(qa, qb) : (in.kind >= 3 ? float4(0.0) : cb));
    r.cv = in.kind == 2 ? r.pm2 : r.pm;
    cover = !xf && in.kind == 0 && (in.color >> 24) == 255u && qa.x <= float(o.x) + 0.5 && qa.y <= float(o.y) + 0.5
            && qb.x > float(o.x) + 31.5 && qb.y > float(o.y) + 31.5;
    return qa.x < qb.x && qa.y < qb.y;
}

static bool f_stage_glyph(device const uchar* pool, device const FHdr& h, uint run, uint slot, float4 cl, thread FRec& r) {
    float2 p0, size;
    uint color;
    ushort2 uv;
    if (!f_glyph(pool, h, run, slot, p0, size, color, uv)) return false;
    float2 qa = p0 - 1.0, qb = p0 - 1.0 + (size + 2.0);
    float4 xm; float2 xk, xt;
    r.xs = 0.0;
    if (f_xf(pool, h, f_run_space(pool, h, run), xm, xk, xt)) f_stage_xf(r, xm, xk, xt, qa, qb);
    qa = clamp(qa, cl.xy, cl.zw);
    qb = clamp(qb, cl.xy, cl.zw);
    r.q = float4(qa, qb);
    r.b = float4(p0, p0 + size);
    half4 c1 = half4(unpack(color));
    r.pm = half4(c1.rgb * c1.a, c1.a);
    r.pm2 = half4(0.0h);
    r.radius = 0.0;
    r.bw = 0.0;
    r.uv = uv;
    r.kind = 3;
    r.core = float4(0.0);
    r.cv = r.pm;
    return qa.x < qb.x && qa.y < qb.y;
}

kernel void fs_tile(uint2 tgid2 [[threadgroup_position_in_grid]],
                    uint li [[thread_index_in_threadgroup]],
                    uint2 lt [[thread_position_in_threadgroup]],
                    uint lane [[thread_index_in_simdgroup]],
                    uint sg [[simdgroup_index_in_threadgroup]],
                    device const uchar* pool [[buffer(0)]],
                    constant uint2& slot [[buffer(1)]]) {
    device const FHdr& h = *(device const FHdr*)pool;
    if (h.debug == 2u) return;   // debug 2: the bin pass alone
    device const FBind& fb = *(device const FBind*)(pool + FBIND_OFF);
    device const uint4* bins = fb.bins;
    texture2d<float> atlas = fb.atlas;
    texture2d<float> img = fb.img;
    device const FSlot& fs = *(device const FSlot*)(pool + slot.x);
    device const FRow* frows = (device const FRow*)(pool + slot.x + FSLOT_ROWS);
    device const FInst* inst = (device const FInst*)(fcols(pool).inst);
    // this threadgroup's tile: the last row whose prefix <= i, then the k-th set bit of that row
    uint i = tgid2.x;
    uint lo = 0, hi = fs.nrows;
    while (hi - lo > 1) {
        uint mid = (lo + hi) >> 1;
        if (frows[mid].prefix <= i) lo = mid; else hi = mid;
    }
    FRow fr = frows[lo];
    uint k = i - fr.prefix;
    uint w = 0;
    for (; w < 3u; w++) {
        uint c = popcount(fr.bits[w]);
        if (k < c) break;
        k -= c;
    }
    uint v = fr.bits[w];
    for (uint j = 0; j < k; j++) v &= v - 1u;
    uint2 tile = uint2(w * 32u + ctz(v), fr.ty);
    uint2 o = tile * 32u;
    short4 th = short4(short(o.x + 32u), short(o.y + 32u), short(-int(o.x)), short(-int(o.y)));
    float2 c00 = float2(o + 2u * lt) + 0.5;
    float4 blk = float4(c00, c00 + 1.0);
    threadgroup uint sgc[8];
    threadgroup uint flo[8];
    threadgroup FRec rec[128];
    half4 acc[4] = { half4(0.0h), half4(0.0h), half4(0.0h), half4(0.0h) };
    uint2 alim = uint2(atlas.get_width() - 1u, atlas.get_height() - 1u);
    device const uint4* bin = bins + tile.y * h.bin_cap;
    uint nb = atomic_load_explicit(fb.counts + tile.y, memory_order_relaxed);
    // 256 entries of the row list per step. All 256 threads test one entry each against the tile and the hits
    // are compacted, in order, BEFORE they are staged (round 6; before, 128 threads tested and staged in one
    // go). A step costs its barriers and the simdgroups that run the staging code, whatever it finds: a
    // glyph-dense row (170-210 entries, ~10 hits) now takes one step, not two, and its hits are staged by one
    // simdgroup, not four (measured: lab stream 44 -> 39 us per frame, same bytes).
    threadgroup uint hl[256];
    for (uint b0 = 0; b0 < nb; b0 += 256) {
        uint bi = b0 + li;
        bool ih = false;
        if (bi < nb) {
            uint4 en = bin[bi];
            ih = all(short4(as_type<short2>(en.y), as_type<short2>(en.z)) < th);
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);   // the previous step's readers of hl and rec are done
        uint nh;
        uint hs = compact(ih, lane, sg, sgc, nh);
        if (ih) hl[hs] = bi;
        // the hits are staged 128 at a time by the first threads (one simdgroup stages up to 32 hits)
        for (uint h0 = 0; h0 < nh; h0 += 128) {
        threadgroup_barrier(mem_flags::mem_threadgroup);   // hl is written; the previous chunk's readers of rec are done
        bool cover = false;
        uint nr = min(nh - h0, 128u);
        if (li < nr) {
            uint4 en = bin[hl[h0 + li]];
            FRec r;
            bool rh;
            float4 cl = f_clip(pool, h, en.w >> 20);
            if ((en.x & F_RUN) != 0u) {
                rh = f_stage_glyph(pool, h, en.x & h.m_run, en.w & h.m_glyph, cl, r);
            } else {
                device const FInst& in = inst[en.x & h.m_inst];
                rh = f_stage_rec(in, f_space(pool, h, in.space), cl, o, r, cover, pool, h);
            }
            if (!rh) {   // clipped away: an empty window, which no block meets
                r.q = float4(1e30, 1e30, -1e30, -1e30);
                r.core = float4(0.0);
                cover = false;
            }
            rec[li] = r;
        }
        uint fl = simd_max(cover ? li + 1u : 0u);
        if (lane == 0) flo[sg] = fl;
        threadgroup_barrier(mem_flags::mem_threadgroup);
        uint k0 = 0;
        for (uint s2 = 0; s2 < 8; s2++) k0 = max(k0, flo[s2]);
        k0 = k0 > 0 ? k0 - 1u : 0u;
        uint kend = h.debug == 1u ? k0 : nr;   // debug 1: cull and stage only (no shading)
        // Survivors first, then shading (round 6). Each thread marks the staged records whose window meets
        // its 2x2 block (bit = index in `rec` = paint order; one word per 32 records), then walks its own
        // set bits. The threads of a simdgroup step together, so a step shades a DIFFERENT record in every
        // lane that still has one, and a band takes as many steps as its busiest block has records (2-4 on
        // text), not one per record that touches the band anywhere (measured: lab stream 54 -> 44 us per
        // frame, same bytes).
        // (Tried and rejected, same bytes: the marks as the AND of per-block-row and per-block-column sets
        // built by 32 threads. Its extra threadgroup barrier per step cost more than the 224 tests it
        // saved: heavy stream 0.90 -> 0.98 of the original, lab stream 0.62 -> 0.67.)
        for (uint w = k0 >> 5; w * 32u < kend; w++) {
        uint m = 0u;
        for (uint k = max(k0, w * 32u); k < min(kend, w * 32u + 32u); k++) {
            float4 q = rec[k].q;
            m |= uint(!(q.x > blk.z || q.z <= blk.x || q.y > blk.w || q.w <= blk.y)) << (k & 31u);
        }
        for (; m != 0u; m &= m - 1u) {
            threadgroup const FRec& rr = rec[w * 32u + ctz(m)];
            if (blk.x >= rr.core.x && blk.z < rr.core.z && blk.y >= rr.core.y && blk.w < rr.core.w) {
                half4 cv = rr.cv;
                for (uint j = 0; j < 4; j++) acc[j] = fma(acc[j], half4(1.0h - cv.a), cv);
                continue;
            }
            // The untransformed base kinds (solid, rounded, border, glyph: all text and almost every box) are
            // shaded here, not through `f_shade_x`: with the transformed, image, gradient and shadow paths in
            // the same per-pixel call a glyph sample cost 4x what it does alone (round 6, measured: lab
            // stream 71 -> 43 us per frame, same bytes). The arithmetic is `f_shade`'s, unchanged.
            if (rr.xs == 0.0 && rr.kind == 3u) {
                float4 gq = rr.q, gb = rr.b;
                half4 pm = rr.pm;
                uint2 uv = uint2(float2(rr.uv));
                for (uint j = 0; j < 4; j++) {
                    float2 c = c00 + float2(float(j & 1u), float(j >> 1));
                    float2 t = c - gb.xy;
                    // `sh` is the product or exactly zero, as `f_shade` returns it: written as `pm * cov`
                    // alone the compiler contracts it into the blend (an fma), and 35 pixels of the animate
                    // scene round differently from the original kernels
                    half4 sh = half4(0.0h);
                    if (c.x >= gq.x && c.x < gq.z && c.y >= gq.y && c.y < gq.w && all(t >= 0.0) && all(t < gb.zw - gb.xy)) {
                        sh = pm * half(atlas.read(min(uv + uint2(t), alim)).r);
                    }
                    acc[j] = fma(acc[j], half4(1.0h - sh.a), sh);   // one rounding (lane memory): the CPU executor relies on it
                }
                continue;
            }
            if (rr.xs == 0.0 && rr.kind < 3u) {
                for (uint j = 0; j < 4; j++) {
                    half4 sh = f_shade(rr, c00 + float2(float(j & 1u), float(j >> 1)), atlas, alim);
                    acc[j] = fma(acc[j], half4(1.0h - sh.a), sh);   // one rounding (lane memory): the CPU executor relies on it
                }
                continue;
            }
            for (uint j = 0; j < 4; j++) {
                half4 sh = f_shade_x(rr, c00 + float2(float(j & 1u), float(j >> 1)), atlas, img);
                acc[j] = fma(acc[j], half4(1.0h - sh.a), sh);   // one rounding: the CPU repair backend relies on it
            }
        }
        }
        }
    }
    for (uint j = 0; j < 4; j++) {
        uint2 px = o + 2u * lt + uint2(j & 1u, j >> 1);
        if (px.x >= uint(h.viewport.x) || px.y >= uint(h.viewport.y)) continue;
        fb.t[fs.target & 3u].write(float4(acc[j]), px);
    }
    // Certify this simdgroup's band: one colour iff the 4 accumulators of all its 32 threads are bit-equal
    // (min == max of their bit patterns), and the simdgroup IS a band (32 threads of one band row). The
    // band's first thread stores the verdict, so every band of a shaded tile is rewritten, never left stale.
    uint2 a0 = as_type<uint2>(acc[0]), a1 = as_type<uint2>(acc[1]), a2 = as_type<uint2>(acc[2]), a3 = as_type<uint2>(acc[3]);
    uint2 lo2 = min(min(a0, a1), min(a2, a3)), hi2 = max(max(a0, a1), max(a2, a3));
    uint band = lt.y >> 1;
    uint2 clo = uint2(simd_min(lo2.x), simd_min(lo2.y)), chi = uint2(simd_max(hi2.x), simd_max(hi2.y));
    bool whole = simd_sum(1u) == 32u && simd_min(band) == simd_max(band);
#ifdef MUTANT_CERT_TILE
    chi = clo;   // mutant: a band is certified by its smallest pixel alone
#endif
    if (lt.x == 0u && (lt.y & 1u) == 0u && tile.x < h.tiles_x && tile.y < h.tiles_y) {
        device FCert& ct = fb.cert[fs.target & 3u][tile.y * h.tiles_x + tile.x];
        bool one = whole && all(clo == chi) && any(clo != uint2(0u));
        ct.c[band] = one ? clo : uint2(0u);
        ct.m[band] = one ? uchar(15) : uchar(0);
    }
}

// Scroll by copy (RESULTS §15): the pixels of a scrolled clip already exist in the surface showing the previous
// frame. One threadgroup per 32x32 tile of the destination rect's tile span moves the tile's pixels inside the
// rect from the source at p - d, unless the frame's damaged rows mark the tile (the tile pass writes those whole),
// so copy and repair write disjoint tiles and no barrier orders them. 32 x 8 threads x 4 rows: thread row y is
// band y of the tile (32 x 4 px), which is SKIPped, FILLed or MOVEd by its certificates (see `FBind`); the
// copy is bound by memory traffic (measured: 2.3 Mpx read + written in ~165 us, written alone in ~65 us), so
// the bands it does not read or write are its only saving.
kernel void fs_copy(uint2 tgid2 [[threadgroup_position_in_grid]],
                    uint2 lt [[thread_position_in_threadgroup]],
                    device const uchar* pool [[buffer(0)]],
                    constant uint2& slot [[buffer(1)]]) {
    device const FHdr& h = *(device const FHdr*)pool;
    device const FSlot& fs = *(device const FSlot*)(pool + slot.x);
    device const FBind& fb = *(device const FBind*)(pool + FBIND_OFF);
    device const FRow* frows = (device const FRow*)(pool + slot.x + FSLOT_ROWS);
    uint i = tgid2.x, tw = max(fs.cspan >> 16, 1u);
    uint2 tile = uint2((fs.cspan & 0xffu) + i % tw, ((fs.cspan >> 8) & 0xffu) + i / tw);
    // the repair's rows are sorted by tile row: is this tile one of them?
    uint lo = 0, hi = fs.nrows;
    while (lo < hi) {
        uint mid = (lo + hi) >> 1;
        if (frows[mid].ty < tile.y) lo = mid + 1; else hi = mid;
    }
    if (lo < fs.nrows && frows[lo].ty == tile.y && ((frows[lo].bits[(tile.x >> 5) & 3u] >> (tile.x & 31u)) & 1u) != 0u) return;
    int2 d = int2(as_type<short2>(fs.d));
    uint2 r0 = uint2(fs.dst0 & 0xffffu, fs.dst0 >> 16), r1 = uint2(fs.dst1 & 0xffffu, fs.dst1 >> 16);
    uint tx = h.tiles_x, tys = h.tiles_y;
    if (tile.x >= tx || tile.y >= tys) return;
    uint2 org = tile * 32u;
    // The tile's 8 band decisions are made by 8 threads of its first row (thread x decides band x: one
    // simdgroup executes the decision once, 8 lanes wide) and handed to the tile through threadgroup memory.
    // (Measured: decided by every thread, by every band's first thread, or by one thread for all bands, the
    // decision cost more than moving the pixels: the kernel is bound by instructions per simdgroup.)
    // Band b's certificate is read and replaced by that one thread, so nothing races.
    threadgroup uint tg_k[8];     // per band: rows of one colour (K) | rows the destination holds (keep) << 4 | rows in the rect (W) << 8
    threadgroup uint2 tg_c[8];    // per band: that colour
    if (lt.y == 0u && lt.x < 8u) {
        uint band = lt.x;
        uint y0 = org.y + 4u * band;
        bool xall = org.x >= r0.x && org.x + 32u <= r1.x, xany = org.x < r1.x && org.x + 32u > r0.x;
        device FCert& dc = fb.cert[fs.target & 3u][tile.y * tx + tile.x];
        uint2 O = dc.c[band];
        uint M = uint(dc.m[band]);
        // The source rows: pixel rows y0 - dy .. + 3 lie in at most two source bands (A: row 0's, B: row 3's)
        // of at most two tile columns (0, 1); a destination row is one colour iff its source row is certified
        // in both columns with the same colour, and all 32 of its pixels are copied (`xall`).
        device const FCert* sc = fb.cert[fs.src & 3u];
        int sx = int(org.x) - d.x, sy = int(y0) - d.y;
        uint sx0 = min(uint(max(sx, 0)) >> 5, tx - 1u), sx1 = min(uint(max(sx + 31, 0)) >> 5, tx - 1u);
        uint ya = min(uint(max(sy, 0)), tys * 32u - 1u), yb = min(uint(max(sy + 3, 0)), tys * 32u - 1u);
        uint ia = (ya >> 5) * tx, ib = (yb >> 5) * tx, ba = (ya >> 2) & 7u, bb = (yb >> 2) & 7u;
        uint2 cA = sc[ia + sx0].c[ba], cB = sc[ib + sx0].c[bb];
        uint mA = uint(sc[ia + sx0].m[ba]) & uint(sc[ia + sx1].m[ba]), mB = uint(sc[ib + sx0].m[bb]) & uint(sc[ib + sx1].m[bb]);
        if (!xall || any(cA != sc[ia + sx1].c[ba]) || all(cA == uint2(0u))) mA = 0u;
        if (!xall || any(cB != sc[ib + sx1].c[bb]) || all(cB == uint2(0u))) mB = 0u;
        uint2 C = uint2(0u);
        uint K = 0u, W = 0u;
        for (uint j = 0; j < 4u; j++) {
            uint y = y0 + j;
            if (!(xany && y >= r0.y && y < r1.y)) continue;
            W |= 1u << j;
            uint ys = uint(max(int(y) - d.y, 0));
            bool inA = (ys >> 2) == (ya >> 2);
            if ((((inA ? mA : mB) >> (ys & 3u)) & 1u) == 0u) continue;
            uint2 cj = inA ? cA : cB;
            if (all(C == uint2(0u))) C = cj;
            if (all(cj == C)) K |= 1u << j;
        }
        uint2 Cn = K != 0u ? C : O;
        uint keep = all(O == Cn) ? M : 0u;   // rows the destination already holds in colour Cn
#ifdef MUTANT_CERT_SKIP
        keep = 15u;   // mutant: the destination's own certificate is not consulted
#endif
        uint mask = K | (keep & ~W);         // moved rows lose their bit; rows off the rect keep theirs in the same colour
#ifdef MUTANT_CERT_STALE
        mask |= keep;   // mutant: a moved row keeps the certificate of what it held before
#endif
        if (W != 0u && (fs.src & CERT_FROZEN) == 0u) {
            dc.c[band] = Cn;
            dc.m[band] = uchar(mask);
        }
        tg_k[band] = K | keep << 4 | W << 8;
        tg_c[band] = Cn;
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);
    // this thread: column lt.x of band lt.y, pixel rows y0 .. y0 + 3
    uint kk = tg_k[lt.y];
    uint K = kk & 15u, keep = (kk >> 4) & 15u, W = kk >> 8;
    if ((W & ~(K & keep)) == 0u) return;                    // no row of the band is filled or moved
    uint y0 = org.y + 4u * lt.y;
    texture2d<float, access::write> dst = fb.t[fs.target & 3u];
    texture2d<float, access::read> src = fb.r[fs.src & 3u];
    float4 v = float4(as_type<half4>(tg_c[lt.y]));
    for (uint j = 0; j < 4u; j++) {
        if (((W >> j) & 1u) == 0u) continue;
        uint2 px = uint2(org.x + lt.x, y0 + j);
        if (((K >> j) & 1u) != 0u) {
            if (((keep >> j) & 1u) == 0u) dst.write(v, px);                                 // FILL (else SKIP)
        } else if (px.x >= r0.x && px.x < r1.x) {
            dst.write(src.read(uint2(int2(px) - d)), px);                                   // MOVE
        }
    }
}

// ============================================================================ FLOOR PROBE (examples/fprobe.rs)
// The smallest real GPU work: one thread bumps a counter. Used to price kicks, encoders and events alone.
kernel void fl_nop(device atomic_uint* b [[buffer(0)]], uint i [[thread_position_in_grid]]) {
    if (i == 0) atomic_fetch_add_explicit(b, 1u, memory_order_relaxed);
}
