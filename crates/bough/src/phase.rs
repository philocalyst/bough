//! Phase marks for cost breakdowns: with feature `phase_cost` the turn records the calling thread's instruction
//! count at the end of each phase (`examples/async_phases.rs` takes them apart); without it a mark is nothing.

#[cfg(feature = "phase_cost")]
thread_local! {
    static MARKS: std::cell::RefCell<Vec<(&'static str, u64)>> = std::cell::RefCell::new(Vec::with_capacity(4096));
}

/// The phase `what` ended here.
#[inline(always)]
pub(crate) fn mark(_what: &'static str) {
    #[cfg(feature = "phase_cost")]
    {
        let t = bough_metal::perf::tinstr();
        MARKS.with(|m| m.borrow_mut().push((_what, t)));
    }
}

/// The marks recorded since the last call: (phase, instruction count at its end).
#[cfg(feature = "phase_cost")]
pub fn take() -> Vec<(&'static str, u64)> {
    MARKS.with(|m| {
        let mut m = m.borrow_mut();
        let v = m.clone();
        m.clear();
        v
    })
}
