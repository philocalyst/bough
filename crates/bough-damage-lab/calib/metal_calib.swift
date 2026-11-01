// GPU cost-model calibration for the damage lab (Apple M3 Pro).
// Build: swiftc -O calib/metal_calib.swift -o /tmp/metal_calib && /tmp/metal_calib
// Times command buffers with gpuStartTime/gpuEndTime (median of N) on a
// 2560x1600 BGRA8 private texture (the heavy window @2x).
import Metal
import Foundation

let W = 2560, H = 1600
let dev = MTLCreateSystemDefaultDevice()!
let q = dev.makeCommandQueue()!
let src = """
#include <metal_stdlib>
using namespace metal;
struct Inst { float4 rect; float4 color; float radius; float3 pad; };
struct VOut { float4 pos [[position]]; float4 color; float2 local; float2 half_; float radius; };
vertex VOut vs(uint vid [[vertex_id]], uint iid [[instance_id]],
               const device Inst* inst [[buffer(0)]], constant float2& vp [[buffer(1)]]) {
  Inst I = inst[iid];
  float2 c = float2(vid & 1, vid >> 1);
  float2 p = I.rect.xy + c * I.rect.zw;
  VOut o;
  o.pos = float4(p / vp * float2(2, -2) + float2(-1, 1), 0, 1);
  o.color = I.color; o.local = (c - 0.5) * I.rect.zw; o.half_ = I.rect.zw * 0.5; o.radius = I.radius;
  return o;
}
// Uber-shader-like: rounded-rect SDF coverage, premultiplied output.
fragment float4 fs(VOut in [[stage_in]]) {
  float2 q = abs(in.local) - in.half_ + in.radius;
  float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - in.radius;
  float a = clamp(0.5 - d, 0.0, 1.0) * in.color.a;
  return float4(in.color.rgb * a, a);
}
kernel void tiles(texture2d<float, access::write> t [[texture(0)]], const device uint2* tl [[buffer(0)]],
                  uint2 lid [[thread_position_in_threadgroup]], uint2 tg [[threadgroup_position_in_grid]]) {
  uint2 base = tl[tg.x] * 16;
  t.write(float4(0.2, 0.3, 0.4, 1), base + lid);
}
"""
let lib = try! dev.makeLibrary(source: src, options: nil)
let pd = MTLRenderPipelineDescriptor()
pd.vertexFunction = lib.makeFunction(name: "vs")
pd.fragmentFunction = lib.makeFunction(name: "fs")
pd.colorAttachments[0].pixelFormat = .bgra8Unorm
pd.colorAttachments[0].isBlendingEnabled = true
pd.colorAttachments[0].sourceRGBBlendFactor = .one
pd.colorAttachments[0].destinationRGBBlendFactor = .oneMinusSourceAlpha
pd.colorAttachments[0].sourceAlphaBlendFactor = .one
pd.colorAttachments[0].destinationAlphaBlendFactor = .oneMinusSourceAlpha
let pso = try! dev.makeRenderPipelineState(descriptor: pd)
let cps = try! dev.makeComputePipelineState(function: lib.makeFunction(name: "tiles")!)

func tex(_ w: Int, _ h: Int) -> MTLTexture {
  let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm, width: w, height: h, mipmapped: false)
  d.usage = [.renderTarget, .shaderRead, .shaderWrite]
  d.storageMode = .private
  return dev.makeTexture(descriptor: d)!
}
let A = tex(W, H), B = tex(W, H)

struct Inst { var rect: SIMD4<Float>; var color: SIMD4<Float>; var radius: Float; var pad: SIMD3<Float> }
func instances(_ n: Int, size: Float, radius: Float = 4) -> MTLBuffer {
  var v = [Inst]()
  var rng: UInt64 = 0x1234
  for _ in 0..<n {
    rng = rng &* 6364136223846793005 &+ 1442695040888963407
    let x = Float((rng >> 33) % UInt64(max(1, W - Int(size))))
    let y = Float((rng >> 13) % UInt64(max(1, H - Int(size))))
    v.append(Inst(rect: [x, y, size, size], color: [0.3, 0.5, 0.7, 0.9], radius: radius, pad: [0, 0, 0]))
  }
  return dev.makeBuffer(bytes: v, length: max(1, v.count) * MemoryLayout<Inst>.stride, options: .storageModeShared)!
}
var vp: SIMD2<Float> = [Float(W), Float(H)]

func median(_ xs: [Double]) -> Double { let s = xs.sorted(); return s[s.count / 2] }

/// GPU µs of one command buffer built by `body`, median over `n` runs.
func gpu(_ n: Int = 60, _ body: (MTLCommandBuffer) -> Void) -> Double {
  var ts = [Double]()
  for i in 0..<(n + 10) {
    let cb = q.makeCommandBuffer()!
    body(cb)
    cb.commit()
    cb.waitUntilCompleted()
    if i >= 10 { ts.append((cb.gpuEndTime - cb.gpuStartTime) * 1e6) }
  }
  return median(ts)
}

func pass(_ cb: MTLCommandBuffer, load: MTLLoadAction = .load, scissor: MTLScissorRect? = nil,
          _ draw: ((MTLRenderCommandEncoder) -> Void)? = nil) {
  let rp = MTLRenderPassDescriptor()
  rp.colorAttachments[0].texture = A
  rp.colorAttachments[0].loadAction = load
  rp.colorAttachments[0].storeAction = .store
  rp.colorAttachments[0].clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 1)
  let e = cb.makeRenderCommandEncoder(descriptor: rp)!
  e.setRenderPipelineState(pso)
  e.setVertexBytes(&vp, length: 8, index: 1)
  if let s = scissor { e.setScissorRect(s) }
  draw?(e)
  e.endEncoding()
}
func drawInst(_ e: MTLRenderCommandEncoder, _ buf: MTLBuffer, _ n: Int) {
  e.setVertexBuffer(buf, offset: 0, index: 0)
  e.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4, instanceCount: n)
}

// Warm the GPU up.
let warm = instances(2000, size: 64)
for _ in 0..<200 { let cb = q.makeCommandBuffer()!; pass(cb) { drawInst($0, warm, 2000) }; cb.commit(); cb.waitUntilCompleted() }

var out = [String: Double]()
out["noop_cb"] = gpu { _ in }
out["pass_load_store_nodraw"] = gpu { pass($0) }
out["pass_clear_store_nodraw"] = gpu { pass($0, load: .clear) }
out["pass_load_store_scissor64_nodraw"] = gpu { pass($0, scissor: MTLScissorRect(x: 0, y: 0, width: 64, height: 64)) }
let one16 = instances(1, size: 16)
out["pass_load_store_one16px"] = gpu { cb in pass(cb) { drawInst($0, one16, 1) } }
let fullq = dev.makeBuffer(bytes: [Inst(rect: [0, 0, Float(W), Float(H)], color: [0.3, 0.5, 0.7, 0.9], radius: 0, pad: [0, 0, 0])],
                           length: MemoryLayout<Inst>.stride, options: .storageModeShared)!
out["pass_fullscreen_quad"] = gpu { cb in pass(cb) { drawInst($0, fullq, 1) } }
out["pass_fullscreen_quad_x4"] = gpu { cb in pass(cb) { drawInst($0, fullq, 4) } }
// Per-instance: many tiny (4x4 px) quads.
for n in [1000, 10000, 50000] {
  let b = instances(n, size: 4)
  out["pass_inst\(n)_4px"] = gpu { cb in pass(cb) { drawInst($0, b, n) } }
}
// Per-fragment: fixed count, growing size.
for s in [16, 64, 256] {
  let b = instances(1000, size: Float(s))
  out["pass_inst1000_\(s)px"] = gpu { cb in pass(cb) { drawInst($0, b, 1000) } }
}
// Small render target: does pass cost scale with attachment size?
let small = tex(256, 256)
out["pass_256x256_target_nodraw"] = gpu { cb in
  let rp = MTLRenderPassDescriptor()
  rp.colorAttachments[0].texture = small; rp.colorAttachments[0].loadAction = .load; rp.colorAttachments[0].storeAction = .store
  let e = cb.makeRenderCommandEncoder(descriptor: rp)!; e.endEncoding()
}
// Blits.
out["blit_full_surface"] = gpu { cb in
  let b = cb.makeBlitCommandEncoder()!
  b.copy(from: A, sourceSlice: 0, sourceLevel: 0, sourceOrigin: MTLOrigin(x: 0, y: 0, z: 0), sourceSize: MTLSize(width: W, height: H, depth: 1),
         to: B, destinationSlice: 0, destinationLevel: 0, destinationOrigin: MTLOrigin(x: 0, y: 0, z: 0))
  b.endEncoding()
}
out["blit_256x256"] = gpu { cb in
  let b = cb.makeBlitCommandEncoder()!
  b.copy(from: A, sourceSlice: 0, sourceLevel: 0, sourceOrigin: MTLOrigin(x: 0, y: 0, z: 0), sourceSize: MTLSize(width: 256, height: 256, depth: 1),
         to: B, destinationSlice: 0, destinationLevel: 0, destinationOrigin: MTLOrigin(x: 0, y: 0, z: 0))
  b.endEncoding()
}
out["blit_64_rects_32x32"] = gpu { cb in
  let b = cb.makeBlitCommandEncoder()!
  for i in 0..<64 {
    let o = MTLOrigin(x: (i % 16) * 160, y: (i / 16) * 400, z: 0)
    b.copy(from: A, sourceSlice: 0, sourceLevel: 0, sourceOrigin: o, sourceSize: MTLSize(width: 32, height: 32, depth: 1),
           to: B, destinationSlice: 0, destinationLevel: 0, destinationOrigin: o)
  }
  b.endEncoding()
}
// Compute writes to K scattered 16x16 tiles.
for k in [64, 1024, 16000] {
  var tl = [SIMD2<UInt32>]()
  for i in 0..<k { tl.append([UInt32((i * 37) % 160), UInt32((i * 11 + i / 160) % 100)]) }
  let tb = dev.makeBuffer(bytes: tl, length: tl.count * 8, options: .storageModeShared)!
  out["compute_tiles\(k)"] = gpu { cb in
    let e = cb.makeComputeCommandEncoder()!
    e.setComputePipelineState(cps); e.setTexture(A, index: 0); e.setBuffer(tb, offset: 0, index: 0)
    e.dispatchThreadgroups(MTLSize(width: k, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 16, height: 16, depth: 1))
    e.endEncoding()
  }
}
for (k, v) in out.sorted(by: { $0.key < $1.key }) { print(String(format: "%-36@ %10.2f us", k as NSString, v)) }
