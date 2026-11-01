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
    uchar  kind;           // 42  0 solid, 1 rrect, 2 border, 3 glyph
    uchar  flags;          // 43
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
