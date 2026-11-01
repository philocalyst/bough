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
    float2 local;                       // pixel offset from instance centre
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
    o.local = p - centre;
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
    switch (v.kind) {
    case 0: {
        col = float4(v.color.rgb * v.color.a, v.color.a);
        break; }
    case 1: {
        float d = sd_rbox(v.local, v.params.xy, v.params.z);
        float cov = clamp(0.5 - d, 0.0, 1.0);
        col = float4(v.color.rgb * v.color.a, v.color.a) * cov;
        break; }
    case 2: {
        float d = sd_rbox(v.local, v.params.xy, v.params.z);
        float outer = clamp(0.5 - d, 0.0, 1.0);
        float inner = clamp(0.5 - (d + v.params.w), 0.0, 1.0);
        float ring = max(outer - inner, 0.0);
        float4 bc = float4(v.color.rgb * v.color.a, v.color.a);
        float4 fc = float4(v.color2.rgb * v.color2.a, v.color2.a);
        col = bc * ring + fc * inner;
        break; }
    default: {
        float2 t = v.local + v.params.xy;           // 0..size in pixels
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
