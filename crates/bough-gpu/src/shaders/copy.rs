/// Scroll by copy (bough-metal RESULTS §15, INTERFACE.md "Scroll by copy"): one workgroup per 32 x 32 tile of the
/// copy's destination tile span copies the tile's pixels inside `[dst0, dst1)` from surface `src` at `p - d`, unless
/// the frame's damaged rows mark the tile (the tile pass writes those whole). 32 x 8 invocations x 4 rows.
#[wgsl_rs::wgsl]
pub mod copy {
    use wgsl_rs::std::*;

    storage!(group(0), binding(0), POOL: RuntimeArray<u32>);
    texture!(group(0), binding(3), T0: TextureStorage2D<Bgra8unorm, Write>);
    texture!(group(0), binding(4), T1: TextureStorage2D<Bgra8unorm, Write>);
    texture!(group(0), binding(5), T2: TextureStorage2D<Bgra8unorm, Write>);
    texture!(group(0), binding(6), T3: TextureStorage2D<Bgra8unorm, Write>);
    texture!(group(0), binding(7), R0: Texture2D<f32>);
    texture!(group(0), binding(8), R1: Texture2D<f32>);
    texture!(group(0), binding(9), R2: Texture2D<f32>);
    texture!(group(0), binding(10), R3: Texture2D<f32>);
    uniform!(group(1), binding(0), SLOT: Vec4u);

    /// Words of the frame slot's header (`FSlot`, 64 B); damaged-row records (`FRow`, 8 words) follow it.
    pub const SLOT_ROWS: u32 = 16u32;

    #[compute]
    #[workgroup_size(32, 8, 1)]
    pub fn fs_copy(#[builtin(workgroup_id)] wg: Vec3u, #[builtin(local_invocation_id)] lt: Vec3u) {
        let s = get!(SLOT).x() / 4u32;
        let tgt = get!(POOL)[s + 6u32] & 3u32;
        let nrows = get!(POOL)[s + 7u32];
        let src = get!(POOL)[s + 11u32] & 3u32;
        let dst0 = get!(POOL)[s + 12u32];
        let dst1 = get!(POOL)[s + 13u32];
        let d = get!(POOL)[s + 14u32];
        let cspan = get!(POOL)[s + 15u32];
        let tw = max(cspan >> 16u32, 1u32);
        let tx = (cspan & 255u32) + wg.x() % tw;
        let ty = ((cspan >> 8u32) & 255u32) + wg.x() / tw;
        // the repair's rows are sorted by tile row: is this tile one of them?
        let mut lo = 0u32;
        let mut hi = nrows;
        while lo < hi {
            let mid = (lo + hi) >> 1u32;
            if get!(POOL)[s + SLOT_ROWS + mid * 8u32] < ty {
                lo = mid + 1u32;
            } else {
                hi = mid;
            }
        }
        if lo < nrows && get!(POOL)[s + SLOT_ROWS + lo * 8u32] == ty {
            let bits = get!(POOL)[s + SLOT_ROWS + lo * 8u32 + 4u32 + ((tx >> 5u32) & 3u32)];
            if ((bits >> (tx & 31u32)) & 1u32) != 0u32 {
                return;
            }
        }
        let dx = ((d << 16u32) as i32) >> 16u32;
        let dy = (d as i32) >> 16u32;
        let x0 = dst0 & 65535u32;
        let y0 = dst0 >> 16u32;
        let x1 = dst1 & 65535u32;
        let y1 = dst1 >> 16u32;
        let mut j = 0u32;
        while j < 4u32 {
            let px = tx * 32u32 + lt.x();
            let py = ty * 32u32 + lt.y() + 8u32 * j;
            if px >= x0 && px < x1 && py >= y0 && py < y1 {
                let sp = vec2i(px as i32 - dx, py as i32 - dy);
                let mut v = texture_load(R0, sp, 0u32);
                if src == 1u32 {
                    v = texture_load(R1, sp, 0u32);
                } else if src == 2u32 {
                    v = texture_load(R2, sp, 0u32);
                } else if src == 3u32 {
                    v = texture_load(R3, sp, 0u32);
                }
                let dp = vec2u(px, py);
                if tgt == 0u32 {
                    texture_store(T0, dp, v);
                } else if tgt == 1u32 {
                    texture_store(T1, dp, v);
                } else if tgt == 2u32 {
                    texture_store(T2, dp, v);
                } else {
                    texture_store(T3, dp, v);
                }
            }
            j += 1u32;
        }
    }
}
