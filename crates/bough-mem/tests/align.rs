//! The independent review's O02: a column carved for an over-aligned type is aligned as an address, not only
//! as an offset from the region's base.
#[repr(align(1048576))]
#[allow(dead_code)]
struct VeryAligned([u8; 1048576]);

#[test]
fn region_carve_respects_element_alignment() {
    let mut keep = Vec::new();
    for _ in 0..4 {
        let mut r = bough_mem::Region::heap(3 * 1048576);
        let c = r.carve::<VeryAligned>("alignment probe", 1);
        assert_eq!(c.as_ptr() as usize % std::mem::align_of::<VeryAligned>(), 0, "the column's address is not aligned for its element type");
        keep.push((r, c));
    }
}
