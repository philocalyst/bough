//! Shared pieces of the text round-3 benchmark ports (`src/bin/*`): counters, medians, fonts, and the scenes
//! whose pixels the tests check.
pub mod churn;
pub mod doc;
pub mod i18n;

/// Instructions retired by this process so far (`proc_pid_rusage` `ri_instructions`).
#[inline]
pub fn instrs() -> u64 {
    bough::sys::perf::snap().instrs
}

pub fn median(v: &mut [f64]) -> f64 {
    assert!(!v.is_empty(), "no samples were taken");
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

pub fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// The font every bough scene uses (zgx's DejaVu Sans, `ZGX_FONT_FILE` or the workload's bundled copy).
pub fn font_bytes() -> Vec<u8> {
    zgx_workload::font_file().unwrap_or_else(|| zgx_workload::FONT_BYTES.to_vec())
}

/// DejaVu's other faces (bold, mono) for the ports whose workload mixes fonts: from `BOUGH_DEJAVU_DIR` or the
/// system's DejaVu 2.37 (the same files the zgx `*_dejavu` drivers register).
pub fn dejavu(file: &str) -> Vec<u8> {
    let dir = std::env::var("BOUGH_DEJAVU_DIR")
        .unwrap_or_else(|_| "/Library/Fonts/Nix Fonts/vl9mbzk97wmh1s1yj3766b4zai8msjq6-dejavu-fonts-2.37/share/fonts/truetype".into());
    std::fs::read(format!("{dir}/{file}")).unwrap_or_else(|e| panic!("{dir}/{file}: {e} (set BOUGH_DEJAVU_DIR)"))
}
