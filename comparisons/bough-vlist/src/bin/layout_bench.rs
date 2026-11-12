//! Layout engine costs at tree scale, in retired instructions (proc_pid_rusage), for the layout path
//! (cephal over bough's storage, child ids in the side column):
//!   * mount: building ~NODES layout nodes (builders -> records / engine nodes), the cold layout and the
//!     write-back of every node, per node;
//!   * idle: a turn with nothing changed;
//!   * contained edit: a text edit in a fixed-height card (the answer the card gives its parent does not
//!     change: cephal stops there);
//!   * propagating edit: the same edit in an auto-height card (the card's height changes, the cards below
//!     in its column move, their subtrees are re-placed).
//! `NODES` (default 10000), `EDITS` (200), `FIXED` (1|0, which edit; default both are run).
use bough::layout::prelude::*;
use bough::raw::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Big {
    s: Signal<String>,
    cols: usize,
    per: usize,
    fixed: bool,
}

/// The edited card: a label, a wrapping paragraph (the dynamic text) and a block.
fn card<'a, S: TextSrc<Big> + 'a>(fixed: bool, text: S) -> impl Element<Big> + 'a {
    let mut r = row((label("ab").font(2.0, 3.0), para(text).font(2.0, 3.0).w(20.0), block(2.0, 2.0).bg(rgb(0x2b3a52)))).gap(1.0);
    if fixed {
        r = r.h(6.0).clip_overflow();
    }
    r
}
/// Every other card: three paint-less blocks (4 layout nodes).
fn quiet(fixed: bool) -> impl Element<Big> {
    let mut r = row((block(1.0, 3.0), block(20.0, 2.0), block(1.0, 3.0))).gap(1.0);
    if fixed {
        r = r.h(6.0).clip_overflow();
    }
    r
}

impl View for Big {
    fn view(&self) -> impl Element<Self> + '_ {
        let (cols, per, fixed) = (self.cols, self.per, self.fixed);
        let mid = cols / 2;
        row((
            each((0..mid).map(move |_| column(each((0..per).map(move |_| quiet(fixed)))).w(30.0))),
            column((card(fixed, sig!(self.s)), each((1..per).map(move |_| quiet(fixed))))).w(30.0),
            each((mid + 1..cols).map(move |_| column(each((0..per).map(move |_| quiet(fixed)))).w(30.0))),
        ))
        .items(Align::Start)
    }
}

fn instrs() -> u64 {
    bough::sys::perf::snap().instrs
}

fn run(nodes: usize, edits: usize, fixed: bool) {
    let per: usize = std::env::var("PER").ok().and_then(|v| v.parse().ok()).unwrap_or(500);
    let cols = (nodes / 4 / per).max(1);
    let total_nodes = cols * per * 4 + cols + 1;
    let mut cfg = Config::new(4000.0, 3000.0, 1.0, FONT.to_vec()).once();
    cfg.caps.pages = (total_nodes / 8).max(512);
    let t0 = instrs();
    let mut app = App::new(Big { s: Signal::new("aaaa bbbb".into()), cols, per, fixed }, cfg, false);
    let t1 = instrs();
    if std::env::var_os("PROF").is_some() {
        app.rt.lay.prof.enable();
    }
    app.frame();
    let mount = (t1 - t0) as f64 / total_nodes as f64;
    // idle turns
    let n = 1000;
    let a = instrs();
    for _ in 0..n {
        app.turn(|_| {});
    }
    let idle = (instrs() - a) as f64 / n as f64;
    // edits alternate a one-line and a two-line text
    let texts = ["aaaa bbbb", "aaaa bbbb cccc dddd eeee ffff gggg"];
    for w in 0..20 {
        app.turn(|v| {
            v.s.set(texts[w & 1].to_string());
        });
    }
    let a = instrs();
    let w0 = app.rt.lay.stats.nodes_visited;
    let p0 = app.rt.lay.prof.acc;
    for e in 0..edits {
        app.turn(|v| {
            v.s.set(texts[e & 1].to_string());
        });
    }
    let per_edit = (instrs() - a) as f64 / edits as f64;
    let written = (app.rt.lay.stats.nodes_visited - w0) as f64 / edits as f64;
    if std::env::var_os("PROF").is_some() {
        print!("[per edit:");
        for (i, name) in bough::layout::LP_NAMES.iter().enumerate() {
            print!(" {name} {:.0}", (app.rt.lay.prof.acc[i] - p0[i]) as f64 / edits as f64);
        }
        println!("]");
    }
    println!(
        "{} nodes ({} cols x {} cards), {}: mount+layout+write {:.0} instr/node, idle turn {:.0}, edit {:.0} instr (layout nodes visited by the write-back per edit {:.1})",
        total_nodes,
        cols,
        per,
        if fixed { "contained edit (fixed-height card)" } else { "propagating edit (auto-height card)" },
        mount,
        idle,
        per_edit,
        written,
    );
}

fn main() {
    let nodes: usize = std::env::var("NODES").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000);
    let edits: usize = std::env::var("EDITS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    match std::env::var("FIXED").ok().as_deref() {
        Some("1") => run(nodes, edits, true),
        Some("0") => run(nodes, edits, false),
        _ => {
            run(nodes, edits, true);
            run(nodes, edits, false);
        }
    }
}
