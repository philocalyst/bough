//! zgui's kitchen-sink / cold-start document: the shipped component gallery at size s13 (1600 x 1000,
//! 2,380 DOM nodes, 2,695 layout boxes, 134 tab stops), rebuilt from the dump of zgui's own laid-out tree
//! (`build.rs`: DOM preorder, window boxes, computed colours, borders, radii, fonts, text, tab stops,
//! overflow clips, running animations). The document is a static template (`GALLERY`), the root a scroll
//! view over it (8,066 px of content), and the nodes zgui's kitchen interacts with are typed elements:
//! the four probe swatches (`picked`, click + hover), the text field the 22nd Tab reaches (`value`), and
//! the 21 running animations (spinner, skeleton pulses, progress slide, OTP carets; `clock`).
//!
//! Not reproduced (see INPUT.md): SVG paths (icons are tinted squares), gradients (one solid stop), box
//! shadows, non-uniform borders, transforms (the spinner's rotation is a colour phase over the same box),
//! text wrapping (each text node is one line), font families and weights (one face).
use crate::*;

include!(concat!(env!("OUT_DIR"), "/gallery_nodes.rs"));

/// Root scroll extent (zgui `gallery_s13.meta.txt`): content 8,066 px in a 1,000 px port.
pub const CONTENT_H: f32 = 8066.0;
/// One wheel line over the root: zgui's 3 lines per notch x the root's 24 px line height.
pub const LINE: f32 = 72.0;

pub struct Gallery {
    pub scroll: Signal<f32>,
    pub picked: Signal<usize>,
    pub value: Signal<String>,
    pub clock: Signal<f32>,
}

impl Gallery {
    pub fn new() -> Gallery {
        Gallery { scroll: Signal::new(0.0), picked: Signal::new(2), value: Signal::new("Ada Lovelace".to_string()), clock: Signal::new(0.0) }
    }
}

impl Default for Gallery {
    fn default() -> Self {
        Self::new()
    }
}

const SWATCH: Color = rgb(0x232b3a);
const SWATCH_HOVER: Color = rgb(0x2c3546);
const PICKED_BG: Color = rgb(0x2f6bff);
const PICKED_RING: Color = rgb(0x7ee3ff);
const CLEAR: Color = Color(0);

fn with_alpha(c: u32, a: f32) -> Color {
    Color((c & 0x00ff_ffff) | (((c >> 24) as f32 * a.clamp(0.0, 1.0)).round() as u32) << 24)
}

fn hook(v: &Gallery, m: &mut Mounter<'_, Gallery>, _i: usize, n: &TNode) -> Hooked {
    let [x, y, w, h] = n.rect;
    match n.tag {
        100..=103 => {
            let k = (n.tag - 100) as usize;
            m.mount(
                quad(x, y, w, h)
                    .bg(SWATCH)
                    .rounded(n.radius)
                    .border(2.0, CLEAR)
                    .paint_with(sig!(v: Gallery => picked), move |p| if *p == k { (PICKED_RING, PICKED_BG) } else { (CLEAR, SWATCH) })
                    .hover_bg(SWATCH_HOVER)
                    .on_click(move |g: &mut Gallery| {
                        g.picked.set(k);
                    }),
            );
            Hooked::Node
        }
        2 => {
            m.mount(
                text_field(sig!(v: Gallery => value))
                    .rect(x, y, w, h)
                    .font(14.0)
                    .colors(Color(n.bg), Color(n.border), rgb(0x0a0a0a))
                    .rounded(n.radius),
            );
            Hooked::Subtree
        }
        3..=6 => {
            let (bg, border, bw) = (n.bg, n.border, n.border_w);
            let base = if bw > 0.0 { border } else { bg };
            let q = quad(x, y, w.max(1.0), h.max(1.0)).rounded(n.radius);
            match n.tag {
                // spinner-turn 0.9 s: the ring's colour phase (no rotation in the instance format)
                3 => m.mount(q.bg(CLEAR).border(bw.max(2.0), Color(base)).paint_with(sig!(v: Gallery => clock), move |t| {
                    let ph = (t / 0.9).fract();
                    (with_alpha(base, 0.35 + 0.65 * ph), CLEAR)
                })),
                // skeleton-pulse 2 s: opacity 1 -> 0.5 -> 1
                4 => m.mount(q.bg(Color(bg)).paint_with(sig!(v: Gallery => clock), move |t| {
                    let ph = (t / 2.0).fract();
                    (CLEAR, with_alpha(bg, 1.0 - 0.5 * (1.0 - (2.0 * ph - 1.0).abs())))
                })),
                // progress-slide 1.2 s: the fill travels across its track (clipped by it)
                5 => m.mount(q.bg(Color(bg)).width(sig!(v: Gallery => clock).map(move |t| w * (0.2 + 0.8 * (t / 1.2).fract())))),
                // otp-caret-blink 1.25 s: on for the first half
                _ => m.mount(q.bg(Color(bg)).paint_with(sig!(v: Gallery => clock), move |t| {
                    (CLEAR, if (t / 1.25).fract() < 0.5 { Color(bg) } else { with_alpha(bg, 0.0) })
                })),
            }
            Hooked::Node
        }
        _ => Hooked::Default,
    }
}

impl View for Gallery {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        scroll_y(sig!(self.scroll), [0.0, 0.0, WIDTH, HEIGHT], CONTENT_H - HEIGHT, template(GALLERY, hook)).bg(rgb(0xffffff)).line(LINE)
    }
}

pub const TICK_60: f32 = 0.016_667;

/// One 60 Hz turn of the gallery's loop: the animations' clock and any glide, then the frame.
pub fn turn(app: &mut App<Gallery>, dt: f32) -> bool {
    app.write(|g| *g.clock += dt);
    if !app.tick(dt) {
        app.flush();
    }
    app.frame()
}

fn quiet(app: &mut App<Gallery>) {
    app.frame();
    for _ in 0..8 {
        turn(app, TICK_60);
    }
}

/// zgui `scenario/cold.rs`: from nothing to the first painted frame of the gallery, one sample per
/// process (the clock starts before the font is read and the document mounted).
pub fn cold_start(gpu: bool) {
    let i0 = bough_metal::perf::snap().instrs;
    let t0 = std::time::Instant::now();
    let mut app = App::new(Gallery::new(), config(WIDTH, HEIGHT), gpu);
    let drew = app.frame();
    let dt = t0.elapsed();
    let i1 = bough_metal::perf::snap().instrs;
    assert!(drew, "the first frame painted");
    println!("SCENARIO cold-start gallery s13, {} records ({GALLERY_NODES} template nodes), first painted frame; {} tiles", app.rt.bough.len(), app.repaired_tiles);
    println!("MEASURE\tcold-start\tcold.first_frame\tms\t{:.4}\t-", dt.as_secs_f64() * 1e3);
    println!("INSTR\tcold-start\tcold.first_frame\tp50={}\tn=1", i1 - i0);
}

/// zgui `scenario/kitchen.rs`: tab traversal to the first text field, 32 keystrokes (8 backspaces after
/// every 8th), 32 clicks on the probe swatches, 8 wheel notches with every glide tick, 32 resizes, 240
/// untouched turns, in that order.
pub fn kitchen(gpu: bool) {
    let mut app = App::new(Gallery::new(), config(WIDTH, HEIGHT), gpu);
    quiet(&mut app);
    // keystroke: tab to the field first (each press: key down + up, both settled)
    let mut tabs = Samples::new();
    let mut presses = 0;
    loop {
        tabs.time(|| {
            app.key_down(Key::Tab, Mods::default());
            app.frame();
            app.frame(); // key up: nothing to do
        });
        presses += 1;
        if app.rt.input.focused_field() || presses >= 400 {
            break;
        }
    }
    let mut keys = Samples::new();
    let letters = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
    for step in 0..32 {
        let c = letters[step % 8];
        keys.time(|| {
            app.key_down(Key::Char(c), Mods::default());
            app.frame()
        });
        app.frame(); // key up
        if step % 8 == 7 {
            for _ in 0..8 {
                app.key_down(Key::Backspace, Mods::default());
                app.frame();
                app.frame();
            }
        }
    }
    let value_after = app.owner.value.clone();
    // click: the pointer is put on the swatch first; the press is timed, then the release
    let centres = [(41.0, 41.0), (85.0, 41.0), (129.0, 41.0), (173.0, 41.0)];
    let mut clicks = Samples::new();
    let mut release = Samples::new();
    for step in 0..32 {
        let (x, y) = centres[step % 4];
        app.pointer_move(x, y);
        app.frame();
        clicks.time(|| {
            app.pointer_down(x, y, Button::Primary);
            app.frame()
        });
        release.time(|| {
            app.pointer_up(x, y, Button::Primary);
            app.frame()
        });
    }
    assert_eq!(*app.owner.picked, 3, "the clicks picked swatches");
    // glide: one notch, then every tick the animation carries after it
    let mut ticks = Samples::new();
    app.pointer_move(WIDTH / 2.0, HEIGHT / 2.0);
    app.frame();
    for step in 0..8 {
        let lines = if step % 2 == 0 { 3.0 } else { -3.0 };
        app.wheel(WIDTH / 2.0, HEIGHT / 2.0, 0.0, lines);
        app.frame();
        let mut carried = 0;
        while app.animating() && carried < 60 {
            ticks.time(|| turn(&mut app, TICK_60));
            carried += 1;
        }
    }
    // resize: each step a different width
    let mut resizes = Samples::new();
    for step in 0..32 {
        let width = WIDTH + (step % 24) as f32 * 8.0;
        resizes.time(|| {
            app.resize(width, HEIGHT);
            app.frame()
        });
    }
    // untouched, but animating
    let mut untouched = Samples::new();
    for _ in 0..240 {
        untouched.time(|| turn(&mut app, TICK_60));
    }
    println!(
        "SCENARIO kitchen-sink gallery s13, {} records, a text field {presses} tab stops in (value now {:?}), 32 of each interaction; {} glide ticks",
        app.rt.bough.len(),
        value_after,
        ticks.len()
    );
    // not zgui's kitchen.resize: the template has absolute geometry, so a resize repaints without reflowing
    report("kitchen-sink", "kitchen.resize_repaint_only", "ms", &resizes, 1e-3);
    report("kitchen-sink", "kitchen.click", "us", &clicks, 1.0);
    report("kitchen-sink", "kitchen.click_release", "us", &release, 1.0);
    report("kitchen-sink", "kitchen.keystroke", "us", &keys, 1.0);
    report("kitchen-sink", "kitchen.untouched_turn", "us", &untouched, 1.0);
    report("kitchen-sink", "kitchen.glide_tick", "us", &ticks, 1.0);
    report("kitchen-sink", "kitchen.tab_traversal", "us", &tabs, 1.0);
}
