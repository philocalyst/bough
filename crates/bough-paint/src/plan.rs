//! The compute structure, chosen from types. Every input of the plan is an associated constant of a paint
//! type (`Desc` holds them): what it reads, what moves its window, whether it covers, what a pixel of it
//! costs. `plan` is a `const fn` over a list of archetypes, so a scene whose archetype list is a constant
//! (a store's row type gives one: SEAM 6.2) has its plan in rodata before `main` runs; the same function
//! folds a run-time list for the whole application at start-up. The only thing left to a frame is area:
//! how many tiles are damaged, against a threshold the plan fixed.
use crate::paint::Cost;
use crate::scene::Desc;

/// Where an archetype's pixels are best computed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exec {
    /// Spans on a CPU core: constant interiors, glyph rows, a distance only along edges.
    CpuSpans,
    /// A distance or an integral at every pixel: a GPU pass.
    GpuSdf,
}

#[derive(Clone, Copy, Debug)]
pub struct ArchPlan {
    pub name: &'static str,
    pub exec: Exec,
    pub cost: Cost,
    pub words: usize,
    pub layers: usize,
    pub reads: u64,
    pub geom_reads: u64,
    pub opaque_core: bool,
    pub hit: bool,
}
pub const fn arch(d: &Desc) -> ArchPlan {
    ArchPlan {
        name: d.name,
        exec: match d.cost {
            Cost::Flat | Cost::Edge => Exec::CpuSpans,
            Cost::PerPixel | Cost::Wide => Exec::GpuSdf,
        },
        cost: d.cost,
        words: d.words,
        layers: d.layers,
        reads: d.reads,
        geom_reads: d.geom_reads,
        opaque_core: d.opaque_core,
        hit: d.hit,
    }
}

/// The whole-scene half.
#[derive(Clone, Copy, Debug)]
pub struct Whole {
    /// Some archetype is a GPU pass: the pipeline is built and the scene is GPU-resident. When false no
    /// Metal object is ever created.
    pub gpu: bool,
    /// The block words any archetype reads: the uniform block's layout (a write to any other word reaches no
    /// pixel and is not even compared).
    pub block: u64,
    /// The block words whose change can move a window: a write to one of them runs the bin pass. Zero: the
    /// bins are built once, at mount.
    pub rebin_on: u64,
    /// Some archetype covers: the occlusion cut applies.
    pub cut: bool,
    /// Some archetype answers the pick: the pick pass exists.
    pub pick: bool,
    /// A frame that damages at most this many tiles is shaded by one CPU core even when the scene is
    /// GPU-resident (a GPU frame has a fixed cost; below this the core is sooner). From the worst cost class.
    pub cpu_tiles: u32,
    /// Words of record constants per record, summed over archetypes (the columns' width).
    pub words: usize,
}

/// A plan over `N` archetypes.
#[derive(Clone, Copy, Debug)]
pub struct Plan<const N: usize> {
    pub arch: [ArchPlan; N],
    pub whole: Whole,
    /// For each block word, the archetypes (bit k) whose window pass a write to it runs: a signal's
    /// dependents, in rodata.
    pub deps: [u32; 64],
}

/// One-core tile budgets by the worst cost class present (round 7's measured crossovers: a core beats the
/// GPU's round trip up to ~256 tiles of flat or edge content, ~32 of per-pixel content, ~8 of blurred).
pub const fn cpu_tiles(worst: Cost) -> u32 {
    match worst {
        Cost::Flat => 1024,
        Cost::Edge => 256,
        Cost::PerPixel => 32,
        Cost::Wide => 8,
    }
}

pub const fn whole(a: &[ArchPlan]) -> (Whole, [u32; 64]) {
    let mut w = Whole { gpu: false, block: 0, rebin_on: 0, cut: false, pick: false, cpu_tiles: 0, words: 0 };
    let mut deps = [0u32; 64];
    let mut worst = Cost::Flat;
    let mut k = 0;
    while k < a.len() {
        let p = &a[k];
        w.gpu |= matches!(p.exec, Exec::GpuSdf);
        w.block |= p.reads;
        w.rebin_on |= p.geom_reads;
        w.cut |= p.opaque_core;
        w.pick |= p.hit;
        w.words += p.words;
        if p.cost as u8 > worst as u8 {
            worst = p.cost;
        }
        let mut s = 0;
        while s < 64 {
            if p.reads >> s & 1 != 0 {
                deps[s] |= 1 << k;
            }
            s += 1;
        }
        k += 1;
    }
    w.cpu_tiles = cpu_tiles(worst);
    (w, deps)
}

pub const fn plan<const N: usize>(d: [&'static Desc; N]) -> Plan<N> {
    let mut a = [arch(d[0]); N];
    let mut k = 1;
    while k < N {
        a[k] = arch(d[k]);
        k += 1;
    }
    let (whole, deps) = whole(&a);
    Plan { arch: a, whole, deps }
}

/// The same plan from a list only known at run time (the whole application, at start-up).
pub fn plan_dyn(d: &[&'static Desc]) -> (Vec<ArchPlan>, Whole, [u32; 64]) {
    let a: Vec<ArchPlan> = d.iter().map(|d| arch(d)).collect();
    let (w, deps) = whole(&a);
    (a, w, deps)
}

fn mask(m: u64) -> String {
    if m == 0 {
        return "-".into();
    }
    (0..64).filter(|s| m >> s & 1 != 0).map(|s| s.to_string()).collect::<Vec<_>>().join(",")
}
/// A paint type's name without paths.
pub fn short(name: &str) -> String {
    let mut out = String::new();
    let mut seg = String::new();
    for ch in name.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            seg.push(ch);
        } else {
            if ch == ':' {
                seg.clear();
            } else {
                out.push_str(&seg);
                seg.clear();
                out.push(ch);
            }
        }
    }
    out.push_str(&seg);
    out.replace(' ', "")
}

pub fn print(title: &str, a: &[ArchPlan], w: &Whole, deps: &[u32; 64]) {
    println!("plan: {title}");
    println!("  {:>2}  {:9} {:8} {:>5} {:>6}  {:10} {:10} {:5} {:4}  paint", "k", "exec", "cost", "words", "layers", "reads", "moves on", "cover", "pick");
    for (k, p) in a.iter().enumerate() {
        let n = short(p.name);
        let n = if n.len() > 150 { format!("{}...", &n[..150]) } else { n };
        println!("  {k:>2}  {:9} {:8} {:>5} {:>6}  {:10} {:10} {:5} {:4}  {n}", format!("{:?}", p.exec), format!("{:?}", p.cost), p.words, p.layers, mask(p.reads), mask(p.geom_reads), p.opaque_core, p.hit);
    }
    println!("  whole: gpu pipeline {}; block words {{{}}}; bin pass on {{{}}}; cut {}; pick {}; one CPU core up to {} damaged tiles; {} record words over {} archetypes", w.gpu, mask(w.block), mask(w.rebin_on), w.cut, w.pick, w.cpu_tiles, w.words, a.len());
    let d: Vec<String> = (0..64).filter(|s| deps[*s] != 0).map(|s| format!("{s}->{:#b}", deps[s])).collect();
    println!("  dependents by block word (archetype bits): {}", d.join(" "));
}
