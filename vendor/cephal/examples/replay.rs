//! Replays a bough scene's layout trees (dumped by bough-workloads' `lay_probe` with `DUMP=path`, feature
//! `dump`) through cephal's own `Tree`, so the engine's cost on a real application tree can be measured and
//! profiled on any host (bough lane `layout`, round 7).
//!
//!     cargo run --release --example replay -- sheet.json [mount|resize|all] [reps]
//!
//! Prints instructions on the calling thread (bough-mem's counters: Linux `perf_event_open`, macOS
//! `thread_selfcounts`) for the cold layout of every group root ("mount") and for each step of the recorded
//! resize sweep (every root whose available space changed is laid out again), and cephal's query counters
//! when built with `--features stats`. `reps` repeats the chosen phase (for `perf record`).
//!
//! Measured leaves (wrapping text) are replayed from their word table with the same greedy wrap and the
//! same measure rules as bough's `LeafCtx`; one-line labels are sized leaves whose size is in their style.
use cephal::geometry::{AvailableSpace, Size};
use cephal::style::Style;
use cephal::tree::{MeasureInput, MeasureOutput, NodeId};
use cephal::Tree;
use serde_json::Value;
use std::collections::HashMap;

/// A measured text leaf: font-unit word extents and the scale to device px.
struct Leaf {
    k: f32,
    lh: f32,
    wrap: bool,
    line_w: i32,
    words: Vec<(i32, i32)>,
}

impl Leaf {
    fn wraps(&self) -> bool {
        self.wrap && !self.words.is_empty()
    }
    fn longest_word(&self) -> i32 {
        self.words.iter().map(|w| w.1 - w.0).max().unwrap_or(0)
    }
    /// bough-text's `Para::greedy`: lines and the widest line at `max_units`.
    fn wrap_at(&self, max_units: i32) -> (usize, i32) {
        let w = &self.words;
        if w.is_empty() {
            return (1, 0);
        }
        let (mut x0, mut last, mut lines, mut widest) = (0, w[0].1, 0usize, i32::MIN);
        for wd in &w[1..] {
            if wd.1 - x0 > max_units {
                lines += 1;
                widest = widest.max(last - x0);
                x0 = wd.0;
            }
            last = wd.1;
        }
        (lines + 1, widest.max(last - x0))
    }
    /// bough's `LeafCtx::measure`.
    fn measure(&self, input: MeasureInput) -> MeasureOutput {
        let k = self.k.max(1e-6);
        let known = input.known_dimensions;
        if !self.wraps() {
            let w = self.line_w as f32 * k;
            return MeasureOutput::from_size(Size { width: known.width.unwrap_or(w), height: known.height.unwrap_or(self.lh) });
        }
        let width = match (known.width, input.available_space.width) {
            (Some(w), _) => Some(w),
            (None, AvailableSpace::Definite(w)) => Some(w),
            (None, AvailableSpace::MinContent) => Some(self.longest_word() as f32 * k),
            (None, AvailableSpace::MaxContent) => None,
        };
        let (lines, widest) = match width {
            Some(w) => self.wrap_at(((w + 1e-3) / k).floor() as i32),
            None => (1, self.line_w),
        };
        MeasureOutput::from_size(Size { width: known.width.unwrap_or(widest as f32 * k), height: known.height.unwrap_or(lines as f32 * self.lh) })
    }
}

fn avail(v: &Value) -> AvailableSpace {
    match v.as_f64() {
        Some(x) => AvailableSpace::Definite(x as f32),
        None => AvailableSpace::MaxContent,
    }
}

fn instr() -> u64 {
    bough_mem::pmu::instructions_cycles().0
}

fn stats() -> String {
    #[cfg(feature = "stats")]
    {
        cephal::compute::stats::report().split(' ').filter(|kv| !kv.ends_with("=0")).collect::<Vec<_>>().join(" ")
    }
    #[cfg(not(feature = "stats"))]
    String::new()
}

struct Scene {
    tree: Tree<Leaf>,
    roots: Vec<(NodeId, Size<AvailableSpace>)>,
    /// Per step: (root index, available space) for every root whose space changed.
    steps: Vec<(f64, f64, Vec<(usize, Size<AvailableSpace>)>)>,
}

fn load(path: &str) -> Scene {
    let d: Value = serde_json::from_slice(&std::fs::read(path).expect("read dump")).expect("json");
    let styles: Vec<Style> = d["styles"].as_array().unwrap().iter().map(|s| serde_json::from_value(s.clone()).expect("style")).collect();
    let nodes = d["nodes"].as_array().unwrap();
    let by_id: HashMap<u64, &Value> = nodes.iter().map(|n| (n[0].as_u64().unwrap(), n)).collect();
    let mut tree: Tree<Leaf> = Tree::new();
    tree.disable_rounding();
    tree.set_measure_ignores_available_height(true);
    let mut made: HashMap<u64, NodeId> = HashMap::new();
    // children before parents: an explicit post-order walk from each root
    let roots_v = d["roots"].as_array().unwrap();
    for r in roots_v {
        let mut stack = vec![(r[0].as_u64().unwrap(), false)];
        while let Some((id, done)) = stack.pop() {
            let n = by_id[&id];
            let kids: Vec<u64> = n[2].as_array().unwrap().iter().map(|k| k.as_u64().unwrap()).collect();
            if !done && !kids.is_empty() {
                stack.push((id, true));
                stack.extend(kids.iter().rev().map(|&k| (k, false)));
                continue;
            }
            let style = styles[n[1].as_u64().unwrap() as usize].clone();
            let node = if kids.is_empty() {
                match &n[3] {
                    Value::Null => tree.new_leaf(style),
                    l => {
                        let words = l["words"].as_array().unwrap().iter().map(|w| (w[0].as_i64().unwrap() as i32, w[1].as_i64().unwrap() as i32)).collect();
                        let leaf = Leaf { k: l["k"].as_f64().unwrap() as f32, lh: l["lh"].as_f64().unwrap() as f32, wrap: l["wrap"].as_bool().unwrap(), line_w: l["line_w"].as_i64().unwrap() as i32, words };
                        tree.new_leaf_with_context(style, leaf)
                    }
                }
            } else {
                let ch: Vec<NodeId> = kids.iter().map(|k| made[k]).collect();
                tree.new_with_children(style, &ch)
            };
            made.insert(id, node);
        }
    }
    let ridx: HashMap<u64, usize> = roots_v.iter().enumerate().map(|(i, r)| (r[0].as_u64().unwrap(), i)).collect();
    let roots: Vec<(NodeId, Size<AvailableSpace>)> = roots_v.iter().map(|r| (made[&r[0].as_u64().unwrap()], Size { width: avail(&r[1]), height: avail(&r[2]) })).collect();
    let mut last: Vec<Size<AvailableSpace>> = roots.iter().map(|r| r.1).collect();
    let mut steps = Vec::new();
    for s in d["steps"].as_array().map(|v| v.as_slice()).unwrap_or(&[]) {
        let mut ch = Vec::new();
        for r in s[2].as_array().unwrap() {
            let Some(&i) = ridx.get(&r[0].as_u64().unwrap()) else { continue };
            let a = Size { width: avail(&r[1]), height: avail(&r[2]) };
            if a != last[i] {
                ch.push((i, a));
                last[i] = a;
            }
        }
        steps.push((s[0].as_f64().unwrap(), s[1].as_f64().unwrap(), ch));
    }
    Scene { tree, roots, steps }
}

fn measure(leaf: &mut Leaf, input: MeasureInput, _: &Style) -> MeasureOutput {
    leaf.measure(input)
}

fn mount(sc: &mut Scene) -> u64 {
    let i0 = instr();
    for &(r, a) in &sc.roots {
        sc.tree.compute_layout_with_measure(r, a, measure);
    }
    instr() - i0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("replay DUMP.json [mount|resize|all] [reps]");
    let what = args.get(2).map(String::as_str).unwrap_or("all");
    let reps: usize = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1);
    let t = std::time::Instant::now();
    let mut sc = load(path);
    eprintln!("loaded {} nodes, {} roots, {} steps in {:.0} ms", sc.tree.node_count(), sc.roots.len(), sc.steps.len(), t.elapsed().as_secs_f64() * 1e3);
    if what == "mount" {
        for k in 0..reps {
            if k > 0 {
                sc.tree.clear_layout();
            }
            let n = mount(&mut sc);
            println!("mount\t{n}\t{}", stats());
        }
        return;
    }
    let _ = stats();
    let n = mount(&mut sc);
    println!("mount\t{n}\tper_node={:.0}\t{}", n as f64 / sc.tree.node_count() as f64, stats());
    if what == "mount_only" {
        return;
    }
    for rep in 0..reps {
        let mut total = 0;
        let steps = sc.steps.clone();
        for (k, (w, h, ch)) in steps.iter().enumerate() {
            let i0 = instr();
            for &(i, a) in ch {
                let r = sc.roots[i].0;
                sc.roots[i].1 = a;
                sc.tree.compute_layout_with_measure(r, a, measure);
            }
            let n = instr() - i0;
            total += n;
            if reps == 1 {
                println!("step {k:2}\t{w}x{h}\troots={}\t{n}\t{}", ch.len(), stats());
            }
            // `SHOW_WIDTH=<px>`: the laid-out widths of every node whose style fixes that width, as a histogram
            if let Some(px) = std::env::var("SHOW_WIDTH").ok().and_then(|v| v.parse::<f32>().ok()) {
                let mut hist: std::collections::BTreeMap<u32, usize> = Default::default();
                for &(r, _) in &sc.roots {
                    let mut st = vec![r];
                    while let Some(n) = st.pop() {
                        if sc.tree.style(n).size.width == cephal::style::Dimension::length(px) {
                            *hist.entry(sc.tree.unrounded_layout(n).size.width.to_bits()).or_default() += 1;
                        }
                        st.extend(sc.tree.children(n));
                    }
                }
                let v: Vec<String> = hist.iter().map(|(b, c)| format!("{}x{c}", f32::from_bits(*b))).collect();
                println!("   widths of {px}px boxes: {}", v.join(" "));
            }
        }
        if reps > 1 {
            println!("resize rep {rep}\t{total}");
        }
    }
}
