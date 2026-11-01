/// The ordered compaction every binned kernel is built on (`compact` in bough-metal's shader.metal): a hit's slot
/// among the workgroup's hits in invocation order, from subgroup prefix sums and one workgroup pass over the 8
/// subgroup counts. `fs_compact_test` compacts the set bits of the region words at `SLOT.y` into `BINS` (a
/// self-test of the subgroup path: wgsl-rs -> naga -> `simd_prefix_exclusive_sum` / `simd_sum`).
#[wgsl_rs::wgsl]
pub mod compact {
    use wgsl_rs::std::*;

    storage!(group(0), binding(0), POOL: RuntimeArray<u32>);
    storage!(group(0), binding(1), read_write, BINS: RuntimeArray<Vec4u>);
    storage!(group(0), binding(2), read_write, COUNTS: RuntimeArray<Atomic<u32>>);
    uniform!(group(1), binding(0), SLOT: Vec4u);
    workgroup!(SGC: [u32; 8]);

    #[compute]
    #[workgroup_size(256, 1, 1)]
    pub fn fs_compact_test(
        #[builtin(local_invocation_index)] li: u32,
        #[builtin(subgroup_invocation_id)] lane: u32,
        #[builtin(subgroup_id)] sg: u32,
    ) {
        let word = get!(POOL)[get!(SLOT).y() / 4u32 + (li >> 5u32)];
        let hit = ((word >> (li & 31u32)) & 1u32) != 0u32;
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
        if hit {
            get_mut!(BINS)[off + pre] = vec4u(li, total, 0u32, 0u32);
        }
    }
}
