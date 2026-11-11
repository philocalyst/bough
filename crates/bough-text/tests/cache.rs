//! Round 3: the table cache returns exactly the table `build` derives (every field the shaper reads), for each
//! font the benchmarks load; a corrupt file is rebuilt. And `build`'s GPOS pair table (`script/pairfast.rs`)
//! equals the one-pair probe for every covered pair of every one of those fonts (release builds included).
use bough_text::SimpleScript;

#[test]
fn cached_table_equals_the_built_one() {
    let dir = std::env::temp_dir().join(format!("bough-cache-test-{}", std::process::id()));
    unsafe { std::env::set_var("BOUGH_CACHE_DIR", &dir) };
    let mut fonts = vec![zgx_workload::FONT_BYTES.to_vec()];
    let d = std::env::var("BOUGH_DEJAVU_DIR")
        .unwrap_or_else(|_| "/Library/Fonts/Nix Fonts/vl9mbzk97wmh1s1yj3766b4zai8msjq6-dejavu-fonts-2.37/share/fonts/truetype".into());
    for f in ["DejaVuSans-Bold.ttf", "DejaVuSansMono.ttf"] {
        if let Ok(b) = std::fs::read(format!("{d}/{f}")) {
            fonts.push(b);
        }
    }
    if let Ok(b) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
        fonts.push(b);
    }
    bough_text::script::PAIR_CHECK.store(true, std::sync::atomic::Ordering::Relaxed);
    for font in &fonts {
        let built = SimpleScript::build(font, 0).unwrap();
        assert_eq!(built.report.pair_table_mismatches, 0, "pairfast table != the one-pair probe");
        for round in 0..3 {
            if round == 2 {
                // corrupt every cached file: it must be rebuilt, not trusted
                for e in std::fs::read_dir(&dir).unwrap() {
                    std::fs::write(e.unwrap().path(), b"BOUGHSS2 garbage").unwrap();
                }
            }
            let c = SimpleScript::build_cached(font, 0).unwrap();
            assert!((c.upem, c.gid, c.adv, c.class1, c.class2, c.n1, c.n2, &c.matrix, c.covered, c.max_step) == (built.upem, built.gid, built.adv, built.class1, built.class2, built.n1, built.n2, &built.matrix, built.covered, built.max_step), "table differs");
            match (&c.tokens, &built.tokens) {
                (Some(a), Some(b)) => assert!((&a.step, a.gid, &a.ligs, a.lig_first, a.max_step) == (&b.step, b.gid, &b.ligs, b.lig_first, b.max_step), "tokens differ"),
                (None, None) => {}
                _ => panic!("token model differs"),
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
