use bough_text::*;
fn main() {
    // The font lives in the zgx reference checkout: `BOUGH_REF` (default `$HOME/projects/_ref`), or pass a path.
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        let r = std::env::var("BOUGH_REF").unwrap_or_else(|_| format!("{}/projects/_ref", std::env::var("HOME").unwrap_or_default()));
        format!("{r}/zgx/comparisons/fonts/DejaVuSans.ttf")
    });
    let data = std::fs::read(&path).unwrap();
    let t0 = std::time::Instant::now();
    let s = SimpleScript::build(&data, 0).expect("build");
    println!("built in {:?}", t0.elapsed());
    let r = &s.report;
    println!("{:#?}", r);
    println!("single triggers: {:?}", r.single_triggers.iter().map(|&c| c as char).collect::<String>());
    println!("pair examples: {:?}", r.pair_triggers_example.iter().map(|&(a,b)| format!("{}{}", a as char, b as char)).collect::<Vec<_>>());
    println!("n1={} n2={} max_step={}", s.n1, s.n2, s.max_step);
}
