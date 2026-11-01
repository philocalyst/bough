// bough backend uber-shader: one vertex function, one fragment function, one draw.
// The 48-byte instance is bough-text's `atlas::Instance` (glyph instances are written by the text
// crate straight into the pool); quads use the aux words: aux0 = second colour, aux1 = radius (f32),
// aux2 = border width (f32), aux3 = clip index.
#include <metal_stdlib>
using namespace metal;

struct Instance {          // 48 bytes, mirrors bough_text::atlas::Instance
    float2 pos;            //  0  position relative to `space`, device px
    float2 size;           //  8
    ushort4 uv;            // 16  glyph atlas texels x,y,w,h
    uint   color;          // 24  RGBA8 (r in low byte), straight alpha
    ushort kind;           // 28  0 solid, 1 rrect, 2 border, 4 glyph
    ushort space;          // 30  index into spaces[]
    uint   color2;         // 32  fill colour for border kind
    float  radius;         // 36
    float  bw;             // 40  border width
    uint   clip;           // 44  clip index
};

struct Hdr {
    float2 viewport;
    uint spaces_off, clips_off, rects_off, inst_off, list_off;
};

struct VOut {
    float4 position [[position]];
    float2 local;
    float4 color [[flat]];
    float4 color2 [[flat]];
    float4 params [[flat]];
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
    float4 clip = ((device const float4*)(pool + h.clips_off))[in.clip];

    float2 c = float2(vid & 1u, vid >> 1);
    float2 p0 = in.pos + off;
    float2 half_ = in.size * 0.5;
    float2 centre = p0 + half_;
    float m = (in.kind == 0) ? 0.0 : 1.0;           // AA margin (solid quads are pixel-exact)
    float2 p = p0 - m + c * (in.size + 2.0 * m);
    p = clamp(p, clip.xy, clip.zw);
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
    float4 col;
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
        float2 t = v.local + v.params.xy;
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

vertex float4 vs_rects(uint vid [[vertex_id]], uint iid [[instance_id]],
                       device const uchar* pool [[buffer(0)]]) {
    device const Hdr& h = *(device const Hdr*)pool;
    float4 r = ((device const float4*)(pool + h.rects_off))[iid];
    float2 c = float2(vid & 1u, vid >> 1);
    float2 p = mix(r.xy, r.zw, c);
    return float4(p.x / h.viewport.x * 2.0 - 1.0, 1.0 - p.y / h.viewport.y * 2.0, 0.0, 1.0);
}
