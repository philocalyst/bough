// Second calibration: hardware tile size, per-instance cost, and whether
// scissor / fragment discard keep untouched tiles from loading/storing.
import Metal
import Foundation

let W = 2560, H = 1600
let dev = MTLCreateSystemDefaultDevice()!
let q = dev.makeCommandQueue()!
let src = """
#include <metal_stdlib>
using namespace metal;
struct Inst { float4 rect; float4 color; };
struct VOut { float4 pos [[position]]; float4 color; };
vertex VOut vs(uint vid [[vertex_id]], uint iid [[instance_id]],
               const device Inst* inst [[buffer(0)]], constant float2& vp [[buffer(1)]]) {
  Inst I = inst[iid];
  float2 c = float2(vid & 1, vid >> 1);
  float2 p = I.rect.xy + c * I.rect.zw;
  VOut o; o.pos = float4(p / vp * float2(2, -2) + float2(-1, 1), 0, 1); o.color = I.color; return o;
}
fragment float4 fs(VOut in [[stage_in]]) { return in.color; }
fragment float4 fs_noise(VOut in [[stage_in]]) {
  uint2 p = uint2(in.pos.xy); uint h = p.x * 0x9E3779B1u ^ p.y * 0x85EBCA6Bu; h ^= h >> 15; h *= 0x2C1B3C6Du; h ^= h >> 12;
  return float4(float(h & 255) / 255.0, float((h >> 8) & 255) / 255.0, float((h >> 16) & 255) / 255.0, 1.0);
}
fragment float4 fs_discard(VOut in [[stage_in]]) { discard_fragment(); return in.color; }
"""
let lib = try! dev.makeLibrary(source: src, options: nil)
func pipe(_ f: String) -> MTLRenderPipelineState {
  let pd = MTLRenderPipelineDescriptor()
  pd.vertexFunction = lib.makeFunction(name: "vs")
  pd.fragmentFunction = lib.makeFunction(name: f)
  pd.colorAttachments[0].pixelFormat = .bgra8Unorm
  pd.colorAttachments[0].isBlendingEnabled = true
  pd.colorAttachments[0].sourceRGBBlendFactor = .one
  pd.colorAttachments[0].destinationRGBBlendFactor = .oneMinusSourceAlpha
  return try! dev.makeRenderPipelineState(descriptor: pd)
}
let pso = pipe("fs"), psoDiscard = pipe("fs_discard"), psoNoise = pipe("fs_noise")
let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm, width: W, height: H, mipmapped: false)
d.usage = [.renderTarget, .shaderRead]; d.storageMode = .private
let A = dev.makeTexture(descriptor: d)!
struct Inst { var rect: SIMD4<Float>; var color: SIMD4<Float> }
func buf(_ v: [Inst]) -> MTLBuffer {
  dev.makeBuffer(bytes: v, length: max(1, v.count) * MemoryLayout<Inst>.stride, options: .storageModeShared)!
}
var vp: SIMD2<Float> = [Float(W), Float(H)]
func median(_ xs: [Double]) -> Double { let s = xs.sorted(); return s[s.count / 2] }
func gpu(_ n: Int = 80, _ body: (MTLCommandBuffer) -> Void) -> Double {
  var ts = [Double]()
  for i in 0..<(n + 10) {
    let cb = q.makeCommandBuffer()!; body(cb); cb.commit(); cb.waitUntilCompleted()
    if i >= 10 { ts.append((cb.gpuEndTime - cb.gpuStartTime) * 1e6) }
  }
  return median(ts)
}
func pass(_ cb: MTLCommandBuffer, _ b: MTLBuffer, _ n: Int, p: MTLRenderPipelineState = pso, scissor: MTLScissorRect? = nil) {
  let rp = MTLRenderPassDescriptor()
  rp.colorAttachments[0].texture = A; rp.colorAttachments[0].loadAction = .load; rp.colorAttachments[0].storeAction = .store
  let e = cb.makeRenderCommandEncoder(descriptor: rp)!
  e.setRenderPipelineState(p); e.setVertexBytes(&vp, length: 8, index: 1)
  if let s = scissor { e.setScissorRect(s) }
  e.setVertexBuffer(b, offset: 0, index: 0)
  if n > 0 { e.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4, instanceCount: n) }
  e.endEncoding()
}
let col: SIMD4<Float> = [0.2, 0.4, 0.6, 0.8]
// Warm-up.
let full = buf([Inst(rect: [0, 0, Float(W), Float(H)], color: col)])
for _ in 0..<300 { let cb = q.makeCommandBuffer()!; pass(cb, full, 1); cb.commit(); cb.waitUntilCompleted() }

var out = [(String, Double)]()
func rec(_ k: String, _ v: Double) { out.append((k, v)); print(String(format: "%-44@ %9.2f us", k as NSString, v)) }
rec("empty_pass", gpu { pass($0, full, 0) })
rec("fullscreen", gpu { pass($0, full, 1) })
// 1) Tile size: M quads of size s on a 64 px grid (aligned to 64).
for s in [1, 8, 16, 32, 64] {
  var v = [Inst]()
  for gy in 0..<(H / 64) { for gx in 0..<(W / 64) { v.append(Inst(rect: [Float(gx * 64), Float(gy * 64), Float(s), Float(s)], color: col)) } }
  let b = buf(v)
  rec("grid64_quads\(v.count)_size\(s)", gpu { pass($0, b, v.count) })
}
// Same with quads spaced 32 apart, size 1 and 16 and 32.
for s in [1, 16, 32] {
  var v = [Inst]()
  for gy in 0..<(H / 32) { for gx in 0..<(W / 32) { v.append(Inst(rect: [Float(gx * 32), Float(gy * 32), Float(s), Float(s)], color: col)) } }
  let b = buf(v)
  rec("grid32_quads\(v.count)_size\(s)", gpu { pass($0, b, v.count) })
}
// Spaced 16 apart, size 1.
do {
  var v = [Inst]()
  for gy in 0..<(H / 16) { for gx in 0..<(W / 16) { v.append(Inst(rect: [Float(gx * 16), Float(gy * 16), 1, 1], color: col)) } }
  let b = buf(v)
  rec("grid16_quads\(v.count)_size1", gpu { pass($0, b, v.count) })
}
// 2) Per-instance cost, all in one 32x32 tile.
for n in [100, 1000, 10000, 100000] {
  let v = (0..<n).map { i in Inst(rect: [Float(i % 28), Float((i / 28) % 28), 4, 4], color: col) }
  let b = buf(v)
  rec("one_tile_inst\(n)", gpu { pass($0, b, n) })
}
// Offscreen / zero-area instances (culled before raster).
for n in [10000, 100000] {
  let v = (0..<n).map { _ in Inst(rect: [-100, -100, 4, 4], color: col) }
  let b = buf(v)
  rec("offscreen_inst\(n)", gpu { pass($0, b, n) })
}
// 3) Fullscreen quad under a small scissor, and with all fragments discarded.
rec("fullscreen_scissor64", gpu { pass($0, full, 1, scissor: MTLScissorRect(x: 512, y: 512, width: 64, height: 64)) })
rec("fullscreen_scissor_half", gpu { pass($0, full, 1, scissor: MTLScissorRect(x: 0, y: 0, width: W, height: H / 2)) })
rec("fullscreen_discard_all", gpu { pass($0, full, 1, p: psoDiscard) })
// 4) A row of 100 tall-thin quads (like bars) vs one bbox quad over them.
do {
  let v = (0..<100).map { i in Inst(rect: [Float(100 + i * 8), 400, 6, 84], color: col) }
  let b = buf(v)
  rec("bars100_6x84", gpu { pass($0, b, 100) })
}

// 5) Incompressible content: fill with noise, then touch tiles.
func noiseFill() { let cb = q.makeCommandBuffer()!; pass(cb, full, 1, p: psoNoise); cb.commit(); cb.waitUntilCompleted() }
for s in [1, 32] {
  var v = [Inst]()
  for gy in 0..<(H / 32) { for gx in 0..<(W / 32) { v.append(Inst(rect: [Float(gx * 32), Float(gy * 32), Float(s), Float(s)], color: [0.2, 0.4, 0.6, 0.0]) ) } }
  let b = buf(v)
  rec("noise_grid32_quads\(v.count)_size\(s)", gpu { noiseFill(); pass($0, b, v.count) })
}
rec("noise_fullscreen_alpha0", gpu { noiseFill(); pass($0, buf([Inst(rect: [0, 0, Float(W), Float(H)], color: [0, 0, 0, 0])]), 1) })
rec("noise_empty_pass", gpu { noiseFill(); pass($0, full, 0) })
