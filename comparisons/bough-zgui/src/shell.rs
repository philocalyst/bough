//! zgui's app-shell document (`scenario/shell.rs`): a head of twelve icon buttons that name themselves in
//! tooltips, a page of 48 rows of library controls, and a dock in a vertical resizable group holding a
//! 5,000-line virtualised log. Rebuilt from the dump of zgui's laid-out tree (`build.rs`, `SHELL`), with
//! the interactive parts typed: tooltip triggers (`tooltip`: hover shows a pre-mounted bubble behind a
//! clip and fades it in), row hover backgrounds, the handle (`on_drag` writes `split`), the page panel's
//! clip (`wired_clip` on `split`) and the log (`port_list`: its port's top edge is `dock_top`).
use crate::gallery::SHELL;
use crate::*;

pub struct Shell {
    /// The resize line's y (points): the page panel ends there, the dock starts one point below.
    pub split: Signal<f32>,
    pub dock_top: Signal<f32>,
    pub log_scroll: Signal<f32>,
    split0: f32,
}

impl Shell {
    pub fn new() -> Shell {
        Shell { split: Signal::new(692.0), dock_top: Signal::new(693.0), log_scroll: Signal::new(0.0), split0: 692.0 }
    }
}
impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

/// zgui `tooltip_sweep.tsv`: each tooltip's left edge and width (the bubble sits at y 48, 28 tall).
const TIP_X: [f32; 12] = [8.0, 8.0, 43.0, 81.0, 118.0, 157.0, 194.0, 233.0, 270.0, 308.0, 344.0, 383.0];
const TIP_W: [f32; 12] = [118.0, 116.0, 117.0, 117.0, 118.0, 117.0, 118.0, 117.0, 118.0, 118.0, 123.0, 121.0];
pub const LINES: usize = 5_000;
const CLEAR: Color = Color(0);
const ACCENT: Color = rgb(0x262626);
const MUTED: Color = rgb(0x262626);
const LINE: Color = Color::rgba(255, 255, 255, 26);
/// The group's bottom edge (the dock panel runs past the window: 693 + 350).
const DOCK_BOTTOM: f32 = 1043.0;
/// Page panel at least 10 % of the group, dock at least 120 px.
const MIN_SPLIT: f32 = 42.0 + 96.0;
const MAX_SPLIT: f32 = 1000.0 - 120.0;

fn log_line(_: &Shell, i: usize, m: &mut Mounter<'_, Shell>) {
    m.mount(text(format!("2026-10-09T12:00:{:02}Z line {i} request served", i % 60)).at(8.0, 0.0).size(13.0).line_height(18.0).color(rgb(0xfafafa)));
}

fn hook(v: &Shell, m: &mut Mounter<'_, Shell>, _i: usize, n: &TNode) -> Hooked {
    let [x, y, w, h] = n.rect;
    match n.tag {
        10..=21 => {
            let k = (n.tag - 10) as usize;
            let label = format!("{k}");
            let tx = 17.0 - 4.5 * label.len() as f32;
            let button = quad(x, y, w, h).bg(CLEAR).rounded(8.0).border(1.0, CLEAR).children(text(label).at(tx, 7.0).size(14.0).line_height(20.0).color(rgb(0xfafafa)));
            let button = button.hover_bg(ACCENT).focusable();
            m.mount(tooltip(button, format!("Action number {k}"), [TIP_X[k], 48.0, TIP_W[k], 28.0]));
            Hooked::Subtree
        }
        40 => {
            m.mount(quad(x, y, w, h).bg(CLEAR).hover_bg(MUTED));
            Hooked::Node
        }
        50 => {
            m.push_box();
            let id = wired_clip(m, sig!(v: Shell => split), |l| [0.0, 42.0, WIDTH, *l - 42.0]);
            Hooked::NodeClip(id)
        }
        51 => {
            m.mount(
                quad(0.0, 0.0, WIDTH, 9.0)
                    .bg(CLEAR)
                    .span(sig!(v: Shell => split).map(|l| (*l - 4.0, 9.0)))
                    .on_press(|s: &mut Shell, _| s.split0 = *s.split)
                    .on_drag(|s: &mut Shell, e| {
                        let l = (s.split0 + e.dy).clamp(MIN_SPLIT, MAX_SPLIT).round();
                        s.split.set(l);
                        s.dock_top.set(l + 1.0);
                    }),
            );
            m.mount(no_hit(quad(0.0, 0.0, WIDTH, 1.0).bg(LINE).span(sig!(v: Shell => split).map(|l| (*l, 1.0)))));
            let _ = (x, y, w, h);
            Hooked::Subtree
        }
        52 => {
            m.push_box();
            m.mount(port_list(sig!(v: Shell => log_scroll), sig!(v: Shell => dock_top), 0.0, WIDTH, DOCK_BOTTOM, 18.0, LINES, log_line));
            Hooked::Subtree
        }
        _ => Hooked::Default,
    }
}

impl View for Shell {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        (quad(0.0, 0.0, WIDTH, HEIGHT).bg(rgb(0x0a0a0a)), template(&SHELL[..], hook))
    }
}

const TICK_60: f32 = 0.016_667;
const TICK_120: f32 = 0.008_333;

fn idle(app: &mut App<Shell>, frames: usize) {
    for _ in 0..frames {
        app.tick(TICK_60);
        app.frame();
    }
}

/// zgui `scenario/shell.rs`: three sweeps of the head (each crossing: the move, its frames and six idle
/// 60 Hz turns), the move away; the dock handle dragged 30 steps of 5 px up and 30 down at 120 Hz.
pub fn run(gpu: bool) {
    let mut app = App::new(Shell::new(), config(WIDTH, HEIGHT), gpu);
    app.frame();
    idle(&mut app, 8);
    let aims: Vec<(f32, f32)> = (0..12).map(|i| (25.0 + 38.0 * i as f32, 21.0)).collect();
    app.pointer_move(aims[0].0, aims[0].1);
    app.frame();
    let st0 = app.rt.input.stats;
    let mut crossings = Samples::new();
    let mut frames = 0u64;
    for _ in 0..3 {
        for &(x, y) in &aims {
            let f0 = app.frames_presented();
            crossings.time(|| {
                app.pointer_move(x, y);
                app.frame();
                idle(&mut app, 6);
            });
            frames += app.frames_presented() - f0;
        }
    }
    crossings.time(|| {
        app.pointer_move(WIDTH / 2.0, 200.0);
        app.frame()
    });
    idle(&mut app, 6);
    let tips_st = app.rt.input.stats;
    // drag the dock's edge
    let (sx, mut sy) = (800.0, 692.5);
    app.pointer_move(sx, sy);
    app.frame();
    app.pointer_down(sx, sy, Button::Primary);
    app.frame();
    let mut steps = Samples::new();
    let mut tiles = 0u64;
    let rows0 = app.rt.ctr.rows_mounted + app.rt.ctr.rows_unmounted;
    for step in 0..60 {
        sy += if step < 30 { -5.0 } else { 5.0 };
        steps.time(|| {
            app.pointer_move(sx, sy);
            app.frame()
        });
        tiles += app.repaired_tiles as u64;
        app.tick(TICK_120);
        app.frame();
    }
    let rows = app.rt.ctr.rows_mounted + app.rt.ctr.rows_unmounted - rows0;
    app.pointer_up(sx, sy, Button::Primary);
    app.frame();
    assert_eq!(*app.owner.split, 692.0, "the drag came back");
    println!(
        "SCENARIO app-shell {} records, 12 tooltip triggers, 48 rows, a dock of {LINES} lines; {} frames over 36 crossings; hits {} cell hits {}; drag mounted/unmounted {rows} log rows",
        app.rt.bough.len(),
        frames,
        tips_st.hits - st0.hits,
        tips_st.cell_hits - st0.cell_hits
    );
    report("app-shell", "shell.tooltip_crossing", "us", &crossings, 1.0);
    report("app-shell", "shell.drag_step", "us", &steps, 1.0);
    count("app-shell", "shell.drag_hit_rebuilds", "rebuilds", app.rt.input.stats.hit_rebuilds as f64);
    count("app-shell", "shell.tooltip_tree_builds", "builds", 0.0);
    count("app-shell", "shell.drag_tiles_per_step", "tiles", tiles as f64 / 60.0);
}
