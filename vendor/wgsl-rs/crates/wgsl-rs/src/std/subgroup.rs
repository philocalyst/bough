//! Subgroup operations (bough addition, vendor/README.md). On the CPU a subgroup is one invocation, so each op is
//! its single-lane value; the WGSL is `subgroupExclusiveAdd` / `subgroupAdd` / `subgroupMax` / `subgroupBallot`
//! (naga: `simd_prefix_exclusive_sum` / `simd_sum` / `simd_max` / `simd_ballot` in MSL).

/// Sum of `e` over the active invocations of the subgroup with a lower id (single lane: 0).
pub fn subgroup_exclusive_add<T: Default>(_e: T) -> T {
    T::default()
}
/// Sum of `e` over the active invocations of the subgroup (single lane: `e`).
pub fn subgroup_add<T>(e: T) -> T {
    e
}
/// Maximum of `e` over the active invocations of the subgroup (single lane: `e`).
pub fn subgroup_max<T>(e: T) -> T {
    e
}
/// Bit i of the result set iff invocation i's predicate holds (single lane: bit 0).
pub fn subgroup_ballot(p: bool) -> crate::std::Vec4u {
    crate::std::vec4u(p as u32, 0, 0, 0)
}
