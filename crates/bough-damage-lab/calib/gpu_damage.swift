// "Damage is free because the GPU already knows": per-frame GPU signature
// pass over the whole instance buffer, compare with the surface's stored
// signatures, compact (instance, damaged-tile-run) pairs in paint order, and
// draw them with one indirect draw. Zero CPU damage tracking and culling.
// Input: results/dump_<trace>.bin from `lab dump`. Checks the GPU's damaged
// tile set bit-for-bit against the CPU sig32 formulation (rings 1 and 2).
// Build: swiftc -O calib/gpu_damage.swift -o /tmp/gpu_damage && /tmp/gpu_damage results/dump_heavy-both.bin
import Metal
import Foundation

let path = CommandLine.arguments[1]
let data = try! Data(contentsOf: URL(fileURLWithPath: path))
func u32(_ o: Int) -> UInt32 { data.withUnsafeBytes { $0.load(fromByteOffset: o, as: UInt32.self) } }
let W = Int(u32(0)), H = Int(u32(4)), SLOTS = Int(u32(8)), FRAMES = Int(u32(12))
let COLS = (W + 31) / 32, ROWS = (H + 31) / 32, NT = COLS * ROWS
let frameBytes = SLOTS * 24
let ISZ = 32
let bitsBase = 16 + FRAMES * frameBytes
let bitsPerFrame = 2 * ROWS * 8

let dev = MTLCreateSystemDefaultDevice()!
let q = dev.makeCommandQueue()!
let src = """
#include <metal_stdlib>
using namespace metal;
struct P { short4 ink; short4 cov; uint key; uchar kind; uchar rad; ushort pad; uint slot; uint pad2; };
struct Dims { uint slots; uint cols; uint rows; uint ntiles; };
inline ulong fin(ulong z) { z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9ul; z = (z ^ (z >> 27)) * 0x94d049bb133111ebul; return z ^ (z >> 31); }
inline ulong m(ulong h, ulong v) { return rotate((h ^ v) * 0x9E3779B97F4A7C15ul, 27ul); }
inline ulong pk(int a, int b, int c, int d) { return ulong(ushort(a)) | (ulong(ushort(b)) << 16) | (ulong(ushort(c)) << 32) | (ulong(ushort(d)) << 48); }
inline bool meets(int4 a, int4 b) { return max(a.x, b.x) < min(a.z, b.z) && max(a.y, b.y) < min(a.w, b.w); }
inline ulong contrib(P s, uint slot, int4 t) {
  int4 cv = int4(s.cov); int4 c = int4(max(cv.x, t.x), max(cv.y, t.y), min(cv.z, t.z), min(cv.w, t.w));
  ulong h = m(ulong(slot) | (ulong(s.kind) << 32) | (ulong(s.rad) << 40), ulong(s.key));
  h = m(h, pk(c.x - t.x, c.y - t.y, c.z - t.x, c.w - t.y));
  int4 i = int4(s.ink);
  if (s.kind == 3) { h = m(h, pk(i.x - t.x, i.y - t.y, 0, 0)); }
  else if (s.kind == 2) {
    int r = s.rad; int rx = min(r, i.z - i.x), ry = min(r, i.w - i.y);
    int4 q[4] = { int4(i.x, i.y, i.x + rx, i.y + ry), int4(i.z - rx, i.y, i.z, i.y + ry),
                  int4(i.x, i.w - ry, i.x + rx, i.w), int4(i.z - rx, i.w - ry, i.z, i.w) };
    for (int k = 0; k < 4; k++) if (meets(q[k], c)) h = m(h, pk(q[k].x - t.x, q[k].y - t.y, q[k].z - t.x, q[k].w - t.y) ^ (ulong(k) << 62));
  }
  return fin(h);
}
// One thread per (slot, tile row).
kernel void accum(const device P* inst [[buffer(0)]], device atomic_uint* sig [[buffer(1)]], constant Dims& d [[buffer(2)]],
                  uint gid [[thread_position_in_grid]]) {
  uint ii = gid / d.rows, ty = gid % d.rows;
  if (ii >= d.slots) return;
  P s = inst[ii]; int4 cv = int4(s.cov); uint slot = s.slot;
  if (cv.x >= cv.z || cv.y >= cv.w) return;
  int ty0 = cv.y >> 5, ty1 = ((cv.w - 1) >> 5) + 1;
  if (int(ty) < ty0 || int(ty) >= ty1) return;
  int tx0 = cv.x >> 5, tx1 = ((cv.z - 1) >> 5) + 1;
  for (int tx = tx0; tx < tx1; tx++) {
    int4 t = int4(tx << 5, int(ty) << 5, (tx + 1) << 5, (int(ty) + 1) << 5);
    ulong h = contrib(s, slot, t);
    uint idx = ty * d.cols + tx;
    atomic_fetch_add_explicit(&sig[2 * idx], uint(h), memory_order_relaxed);
    atomic_fetch_add_explicit(&sig[2 * idx + 1], uint(h >> 32), memory_order_relaxed);
  }
}
// One thread per tile: damaged = signature differs from what this surface drew.
kernel void compare(device uint* sig [[buffer(0)]], device uint* stored [[buffer(1)]], device atomic_uint* dmg [[buffer(2)]],
                    constant Dims& d [[buffer(3)]], uint t [[thread_position_in_grid]]) {
  if (t >= d.ntiles) return;
  uint lo = sig[2 * t], hi = sig[2 * t + 1];
  if (lo != stored[2 * t] || hi != stored[2 * t + 1]) {
    stored[2 * t] = lo; stored[2 * t + 1] = hi;
    uint ty = t / d.cols, tx = t % d.cols;
    atomic_fetch_or_explicit(&dmg[ty * 4 + tx / 32], 1u << (tx % 32), memory_order_relaxed);
  }
  sig[2 * t] = 0; sig[2 * t + 1] = 0;
}
inline bool dbit(const device uint* dmg, int tx, int ty) { return (dmg[ty * 4 + tx / 32] >> (tx % 32)) & 1; }
// Runs of damaged tiles under each instance, per (instance, tile row).
kernel void count(const device P* inst [[buffer(0)]], const device uint* dmg [[buffer(1)]], device uint* cnt [[buffer(2)]],
                  constant Dims& d [[buffer(3)]], uint gid [[thread_position_in_grid]]) {
  uint ii = gid / d.rows; int ty = int(gid % d.rows);
  if (ii >= d.slots) return;
  P s = inst[ii]; int4 cv = int4(s.cov); uint n = 0;
  if (cv.x < cv.z && cv.y < cv.w && ty >= (cv.y >> 5) && ty <= ((cv.w - 1) >> 5)) {
    int tx0 = cv.x >> 5, tx1 = ((cv.z - 1) >> 5) + 1; bool prev = false;
    for (int tx = tx0; tx < tx1; tx++) { bool b = dbit(dmg, tx, ty); n += (b && !prev) ? 1 : 0; prev = b; }
  }
  cnt[gid] = n;
}
// Exclusive scan of counts, one threadgroup of 1024 threads; writes the indirect draw args.
kernel void scan(device uint* cnt [[buffer(0)]], device uint* off [[buffer(1)]], device uint* args [[buffer(2)]],
                 constant Dims& d [[buffer(3)]], uint lid [[thread_position_in_threadgroup]],
                 uint sl [[thread_index_in_simdgroup]], uint sg [[simdgroup_index_in_threadgroup]]) {
  threadgroup uint sums[32];
  uint total = d.slots * d.rows, per = (total + 1023) / 1024, a = lid * per, b = min(a + per, total), local = 0;
  for (uint i = a; i < b; i++) local += cnt[i];
  uint pre = simd_prefix_exclusive_sum(local);
  if (sl == 31) sums[sg] = pre + local;
  threadgroup_barrier(mem_flags::mem_threadgroup);
  if (sg == 0) { uint v = sums[sl]; sums[sl] = simd_prefix_exclusive_sum(v); }
  threadgroup_barrier(mem_flags::mem_threadgroup);
  uint base = sums[sg] + pre;
  for (uint i = a; i < b; i++) { off[i] = base; base += cnt[i]; }
  if (lid == 1023) { args[0] = 4; args[1] = base; args[2] = 0; args[3] = 0; }
}
struct Pair { uint inst; short4 clip; };
kernel void emit(const device P* inst [[buffer(0)]], const device uint* dmg [[buffer(1)]], const device uint* off [[buffer(2)]],
                 device Pair* pairs [[buffer(3)]], constant Dims& d [[buffer(4)]], uint gid [[thread_position_in_grid]]) {
  uint ii = gid / d.rows; int ty = int(gid % d.rows);
  if (ii >= d.slots) return;
  P s = inst[ii]; int4 cv = int4(s.cov); uint o = off[gid];
  if (cv.x >= cv.z || cv.y >= cv.w || ty < (cv.y >> 5) || ty > ((cv.w - 1) >> 5)) return;
  int tx0 = cv.x >> 5, tx1 = ((cv.z - 1) >> 5) + 1, tx = tx0;
  while (tx < tx1) {
    if (!dbit(dmg, tx, ty)) { tx++; continue; }
    int a = tx; while (tx < tx1 && dbit(dmg, tx, ty)) tx++;
    Pair p; p.inst = ii;
    p.clip = short4(max(cv.x, a << 5), max(cv.y, ty << 5), min(cv.z, tx << 5), min(cv.w, (ty + 1) << 5));
    pairs[o++] = p;
  }
}
struct VOut { float4 pos [[position]]; float4 color; };
vertex VOut vs(uint vid [[vertex_id]], uint iid [[instance_id]], const device Pair* pairs [[buffer(0)]],
               const device P* inst [[buffer(1)]], constant float2& vp [[buffer(2)]]) {
  Pair p = pairs[iid]; float4 r = float4(p.clip);
  float2 c = float2(vid & 1, vid >> 1); float2 xy = mix(r.xy, r.zw, c);
  uint k = inst[p.inst].key;
  VOut o; o.pos = float4(xy / vp * float2(2, -2) + float2(-1, 1), 0, 1);
  o.color = float4(float(k & 255) / 255.0, float((k >> 8) & 255) / 255.0, float((k >> 16) & 255) / 255.0, 0.9);
  return o;
}
fragment float4 fs(VOut in [[stage_in]]) { return float4(in.color.rgb * in.color.a, in.color.a); }
"""
let lib = try! dev.makeLibrary(source: src, options: nil)
func cp(_ n: String) -> MTLComputePipelineState { try! dev.makeComputePipelineState(function: lib.makeFunction(name: n)!) }
let pAccum = cp("accum"), pCompare = cp("compare"), pCount = cp("count"), pScan = cp("scan"), pEmit = cp("emit")
let pd = MTLRenderPipelineDescriptor()
pd.vertexFunction = lib.makeFunction(name: "vs"); pd.fragmentFunction = lib.makeFunction(name: "fs")
pd.colorAttachments[0].pixelFormat = .bgra8Unorm
pd.colorAttachments[0].isBlendingEnabled = true
pd.colorAttachments[0].sourceRGBBlendFactor = .one; pd.colorAttachments[0].destinationRGBBlendFactor = .oneMinusSourceAlpha
let pso = try! dev.makeRenderPipelineState(descriptor: pd)
let td = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm, width: W, height: H, mipmapped: false)
td.usage = [.renderTarget]; td.storageMode = .private

func buf(_ n: Int) -> MTLBuffer { dev.makeBuffer(length: max(16, n), options: .storageModeShared)! }
let inst = buf(SLOTS * ISZ), sig = buf(NT * 8), dmg = buf(ROWS * 16), cnt = buf(SLOTS * ROWS * 4), off = buf(SLOTS * ROWS * 4)
let args = buf(16), pairs = buf(SLOTS * 64 * 12)
var dims: (UInt32, UInt32, UInt32, UInt32) = (UInt32(SLOTS), UInt32(COLS), UInt32(ROWS), UInt32(NT))
var vp: SIMD2<Float> = [Float(W), Float(H)]
memset(sig.contents(), 0, NT * 8)

for ring in [1, 2] {
  let surfaces = (0..<ring).map { _ in dev.makeTexture(descriptor: td)! }
  let stored = (0..<ring).map { _ in buf(NT * 8) }
  for b in stored { memset(b.contents(), 0, NT * 8) }
  var liveN = [Double](), tDamage = [Double](), tTotal = [Double](), pairsN = [Double](), mism = 0, checked = 0
  for f in 0..<FRAMES {
    let s = f % ring
    var live = 0
    data.withUnsafeBytes { raw in
      let base = raw.baseAddress! + 16 + f * frameBytes
      for sl in 0..<SLOTS {
        let p = base + sl * 24
        let x0 = p.load(fromByteOffset: 8, as: Int16.self), x1 = p.load(fromByteOffset: 12, as: Int16.self)
        let y0 = p.load(fromByteOffset: 10, as: Int16.self), y1 = p.load(fromByteOffset: 14, as: Int16.self)
        if x0 >= x1 || y0 >= y1 { continue }
        let d = inst.contents() + live * ISZ
        memcpy(d, p, 24)
        d.storeBytes(of: UInt32(sl), toByteOffset: 24, as: UInt32.self)
        live += 1
      }
    }
    dims.0 = UInt32(live)
    liveN.append(Double(live))
    memset(dmg.contents(), 0, ROWS * 16)
    let cb = q.makeCommandBuffer()!
    // Damage: accumulate + compare.
    let e = cb.makeComputeCommandEncoder()!
    e.setComputePipelineState(pAccum); e.setBuffer(inst, offset: 0, index: 0); e.setBuffer(sig, offset: 0, index: 1)
    e.setBytes(&dims, length: 16, index: 2)
    e.dispatchThreads(MTLSize(width: live * ROWS, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 256, height: 1, depth: 1))
    e.memoryBarrier(scope: .buffers)
    e.setComputePipelineState(pCompare); e.setBuffer(sig, offset: 0, index: 0); e.setBuffer(stored[s], offset: 0, index: 1)
    e.setBuffer(dmg, offset: 0, index: 2); e.setBytes(&dims, length: 16, index: 3)
    e.dispatchThreads(MTLSize(width: NT, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 256, height: 1, depth: 1))
    e.endEncoding()
    let e2 = cb.makeComputeCommandEncoder()!
    e2.setComputePipelineState(pCount); e2.setBuffer(inst, offset: 0, index: 0); e2.setBuffer(dmg, offset: 0, index: 1)
    e2.setBuffer(cnt, offset: 0, index: 2); e2.setBytes(&dims, length: 16, index: 3)
    e2.dispatchThreads(MTLSize(width: live * ROWS, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 256, height: 1, depth: 1))
    e2.memoryBarrier(scope: .buffers)
    e2.setComputePipelineState(pScan); e2.setBuffer(cnt, offset: 0, index: 0); e2.setBuffer(off, offset: 0, index: 1)
    e2.setBuffer(args, offset: 0, index: 2); e2.setBytes(&dims, length: 16, index: 3)
    e2.dispatchThreadgroups(MTLSize(width: 1, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 1024, height: 1, depth: 1))
    e2.memoryBarrier(scope: .buffers)
    e2.setComputePipelineState(pEmit); e2.setBuffer(inst, offset: 0, index: 0); e2.setBuffer(dmg, offset: 0, index: 1)
    e2.setBuffer(off, offset: 0, index: 2); e2.setBuffer(pairs, offset: 0, index: 3); e2.setBytes(&dims, length: 16, index: 4)
    e2.dispatchThreads(MTLSize(width: live * ROWS, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 256, height: 1, depth: 1))
    e2.endEncoding()
    let rp = MTLRenderPassDescriptor()
    rp.colorAttachments[0].texture = surfaces[s]; rp.colorAttachments[0].loadAction = f < ring ? .clear : .load
    rp.colorAttachments[0].storeAction = .store
    let r = cb.makeRenderCommandEncoder(descriptor: rp)!
    r.setRenderPipelineState(pso); r.setVertexBuffer(pairs, offset: 0, index: 0); r.setVertexBuffer(inst, offset: 0, index: 1)
    r.setVertexBytes(&vp, length: 8, index: 2)
    r.drawPrimitives(type: .triangleStrip, indirectBuffer: args, indirectBufferOffset: 0)
    r.endEncoding()
    cb.commit(); cb.waitUntilCompleted()
    let total = (cb.gpuEndTime - cb.gpuStartTime) * 1e6
    // Damage-only timing: a second command buffer with just accum + compare (on a scratch copy).
    if f >= ring {
      tTotal.append(total)
      pairsN.append(Double(args.contents().load(fromByteOffset: 4, as: UInt32.self)))
      // Compare damaged tiles with the CPU sig32 bitmap.
      let cpuOff = bitsBase + (ring == 1 ? 0 : (FRAMES - 1) * bitsPerFrame) + (f - ring) * bitsPerFrame
      let dp = dmg.contents().assumingMemoryBound(to: UInt32.self)
      for ty in 0..<ROWS {
        let cw0 = u32(cpuOff + ty * 16), cw1 = u32(cpuOff + ty * 16 + 4), cw2 = u32(cpuOff + ty * 16 + 8)
        if cw0 != dp[ty * 4] || cw1 != dp[ty * 4 + 1] || cw2 != dp[ty * 4 + 2] { mism += 1 }
        checked += 1
      }
    }
    _ = tDamage
  }
  // Damage-only cost: rerun accum+compare on the last frame repeatedly.
  var dOnly = [Double]()
  for _ in 0..<60 {
    let cb = q.makeCommandBuffer()!
    let e = cb.makeComputeCommandEncoder()!
    e.setComputePipelineState(pAccum); e.setBuffer(inst, offset: 0, index: 0); e.setBuffer(sig, offset: 0, index: 1)
    e.setBytes(&dims, length: 16, index: 2)
    e.dispatchThreads(MTLSize(width: Int(dims.0) * ROWS, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 256, height: 1, depth: 1))
    e.memoryBarrier(scope: .buffers)
    e.setComputePipelineState(pCompare); e.setBuffer(sig, offset: 0, index: 0); e.setBuffer(stored[0], offset: 0, index: 1)
    e.setBuffer(dmg, offset: 0, index: 2); e.setBytes(&dims, length: 16, index: 3)
    e.dispatchThreads(MTLSize(width: NT, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 256, height: 1, depth: 1))
    e.endEncoding(); cb.commit(); cb.waitUntilCompleted()
    dOnly.append((cb.gpuEndTime - cb.gpuStartTime) * 1e6)
  }
  // Damage + binning (no render pass) on the last frame, repeated.
  var dBin = [Double]()
  for _ in 0..<60 {
    let cb = q.makeCommandBuffer()!
    let e = cb.makeComputeCommandEncoder()!
    e.setComputePipelineState(pAccum); e.setBuffer(inst, offset: 0, index: 0); e.setBuffer(sig, offset: 0, index: 1)
    e.setBytes(&dims, length: 16, index: 2)
    e.dispatchThreads(MTLSize(width: Int(dims.0) * ROWS, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 256, height: 1, depth: 1))
    e.memoryBarrier(scope: .buffers)
    e.setComputePipelineState(pCompare); e.setBuffer(sig, offset: 0, index: 0); e.setBuffer(stored[0], offset: 0, index: 1)
    e.setBuffer(dmg, offset: 0, index: 2); e.setBytes(&dims, length: 16, index: 3)
    e.dispatchThreads(MTLSize(width: NT, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 256, height: 1, depth: 1))
    e.memoryBarrier(scope: .buffers)
    e.setComputePipelineState(pCount); e.setBuffer(inst, offset: 0, index: 0); e.setBuffer(dmg, offset: 0, index: 1)
    e.setBuffer(cnt, offset: 0, index: 2); e.setBytes(&dims, length: 16, index: 3)
    e.dispatchThreads(MTLSize(width: Int(dims.0) * ROWS, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 256, height: 1, depth: 1))
    e.memoryBarrier(scope: .buffers)
    e.setComputePipelineState(pScan); e.setBuffer(cnt, offset: 0, index: 0); e.setBuffer(off, offset: 0, index: 1)
    e.setBuffer(args, offset: 0, index: 2); e.setBytes(&dims, length: 16, index: 3)
    e.dispatchThreadgroups(MTLSize(width: 1, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 1024, height: 1, depth: 1))
    e.memoryBarrier(scope: .buffers)
    e.setComputePipelineState(pEmit); e.setBuffer(inst, offset: 0, index: 0); e.setBuffer(dmg, offset: 0, index: 1)
    e.setBuffer(off, offset: 0, index: 2); e.setBuffer(pairs, offset: 0, index: 3); e.setBytes(&dims, length: 16, index: 4)
    e.dispatchThreads(MTLSize(width: Int(dims.0) * ROWS, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 256, height: 1, depth: 1))
    e.endEncoding(); cb.commit(); cb.waitUntilCompleted()
    dBin.append((cb.gpuEndTime - cb.gpuStartTime) * 1e6)
  }
  let med = { (x: [Double]) -> Double in let s = x.sorted(); return s.isEmpty ? 0 : s[s.count / 2] }
  print(String(format: "%@ ring %d: frames %d, live inst %.0f, GPU total median %.1f us (damage-only %.1f us, damage+bin %.1f us), pairs median %.0f, tile-row mismatches vs CPU sig32: %d/%d",
               (path as NSString).lastPathComponent, ring, tTotal.count, med(liveN), med(tTotal), med(dOnly), med(dBin), med(pairsN), mism, checked))
}
