use bough_text::*;
fn main() {
    let data = std::fs::read("/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf").unwrap();
    let t0 = std::time::Instant::now();
    let s = SimpleScript::build(&data, 0).expect("build");
    println!("built in {:?}", t0.elapsed());
    let r = &s.report;
    println!("{:#?}", r);
    println!("single triggers: {:?}", r.single_triggers.iter().map(|&c| c as char).collect::<String>());
    println!("pair examples: {:?}", r.pair_triggers_example.iter().map(|&(a,b)| format!("{}{}", a as char, b as char)).collect::<Vec<_>>());
    println!("n1={} n2={} max_step={}", s.n1, s.n2, s.max_step);
}
