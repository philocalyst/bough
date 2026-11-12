//! zgui's kitchen-sink / cold-start document: the shipped component gallery at size s13 (1600 x 1000,
//! 2,380 DOM nodes, 2,695 layout boxes, 134 tab stops), rebuilt from the dump of zgui's own laid-out tree
//! (`build.rs`: DOM preorder, window boxes, computed colours, borders, radii, fonts, text, tab stops,
//! overflow clips, running animations). The document is a static template (`GALLERY`), the root a scroll
//! view over it (8,066 px of content), and the nodes zgui's kitchen interacts with are typed elements:
//! the four probe swatches (`picked`, click + hover), the text field the 22nd Tab reaches (`value`), and
//! the 21 running animations (spinner, skeleton pulses, progress slide, OTP carets; `clock`).
//!
//! `Gallery` (the dump-geometry variant) does not reproduce: SVG paths (icons are tinted squares),
//! gradients (one solid stop), box shadows, transforms (the spinner's rotation is a colour phase), text
//! wrapping (each text node is one line), faces (one). `GalleryL` (the laid-out document the scenarios
//! measure) draws all of those (deco.rs; faces in text.rs). Neither reproduces: non-uniform borders,
//! transforms other than the spinner's turn (rotated / scaled text, the tilted card), gradient-filled text
//! (`background-clip: text`: drawn in the text colour).
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
    app.frame_paced()
}

/// What the kitchen and cold-start drivers need of a gallery document (the dump-placed one or the laid-out one).
pub trait Doc: View {
    const LAID_OUT: bool;
    fn fresh() -> Self;
    fn turn(app: &mut App<Self>, dt: f32) -> bool;
    fn picked(&self) -> usize;
    fn value(&self) -> String;
    /// The document's static text (size in px at scale 1, text, logical face) in the order the mount meets
    /// it, and its vector images (document, w, h points): what `bough::app::prepare` rasterises from
    /// process entry.
    fn strikes() -> Vec<(f32, &'static str, u8)>;
    fn svgs() -> Vec<(&'static str, f32, f32)> {
        Vec::new()
    }
}

/// Every printable ASCII character: the label path readies the whole alphabet of a strike it meets.
const ASCII: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";
impl Doc for Gallery {
    const LAID_OUT: bool = false;
    fn strikes() -> Vec<(f32, &'static str, u8)> {
        GALLERY.iter().filter(|n| !n.text.is_empty()).map(|n| (n.font, ASCII, 0)).collect()
    }
    fn fresh() -> Self {
        Gallery::new()
    }
    fn turn(app: &mut App<Self>, dt: f32) -> bool {
        turn(app, dt)
    }
    fn picked(&self) -> usize {
        *self.picked
    }
    fn value(&self) -> String {
        (*self.value).clone()
    }
}

fn quiet<D: Doc>(app: &mut App<D>) {
    app.frame();
    for _ in 0..8 {
        D::turn(app, TICK_60);
    }
}

/// zgui `scenario/cold.rs`: from nothing to the first painted frame of the gallery, one sample per
/// process (the clock starts before the font is read and the document mounted).
pub fn cold_start<D: Doc>(gpu: bool) {
    let i0 = bough::sys::perf::snap().instrs;
    let t0 = std::time::Instant::now();
    let mut app = App::new(D::fresh(), config(WIDTH, HEIGHT), gpu);
    let drew = app.frame();
    let dt = t0.elapsed();
    let i1 = bough::sys::perf::snap().instrs;
    assert!(drew, "the first frame painted");
    let name = if D::LAID_OUT { "cold.first_frame" } else { "cold.first_frame_dump_geometry" };
    println!(
        "SCENARIO cold-start gallery s13 ({}), {} records, first painted frame; {} tiles",
        if D::LAID_OUT { "laid out by cephal from zgui's computed declarations, text measured and wrapped" } else { "zgui's dumped geometry, no layout" },
        app.rt.bough.len(),
        app.repaired_tiles
    );
    println!("DECO\ticons_rasterised={}\tsvg_instr={}\tglyphs_rasterised={}\traster_ms={:.3}", app.rt.images.rasterised, app.rt.images.svg_instr, app.rt.text.cache.rasterized, app.rt.text.cache.raster_ns as f64 / 1e6);
    println!("MEASURE\tcold-start\t{name}\tms\t{:.4}\t-", dt.as_secs_f64() * 1e3);
    println!("INSTR\tcold-start\t{name}\tp50={}\tn=1", i1 - i0);
}

/// Process entry to the first presented pixel (the GPU has completed the first frame), with the work that
/// does not depend on the document started at entry on other threads (`overlap`: Metal device, queues and
/// pipelines on one; the document's strikes rasterised on `raster` more) or all of it on the main thread.
/// Instructions per thread: main (xnu's per-thread counter), the GPU preparation thread, each raster
/// worker, and the rest of the process (Metal's and the runtime's own threads: process total minus those).
pub fn first_pixel<D: Doc>(entry: std::time::Instant, proc0: u64, main0: u64, overlap: bool, raster: usize) {
    let cfg = config(WIDTH, HEIGHT);
    if overlap {
        bough::app::prepare(&cfg, &D::strikes(), &D::svgs(), raster);
    }
    let mut app = App::new(D::fresh(), cfg, true);
    let mounted = entry.elapsed();
    assert!(app.frame(), "the first frame painted");
    let submitted = entry.elapsed();
    app.wait_presented();
    let wall = entry.elapsed();
    let exec = bough::sys::perf::since_exec_ns();
    let main = bough::sys::perf::thread_instructions() - main0;
    let proc = bough::sys::perf::snap().instrs - proc0;
    let st = app.startup;
    let raster_stats = bough::app::raster_stats();
    let rast: u64 = raster_stats.iter().map(|r| r[2]).sum();
    let mode = if overlap { "overlapped" } else { "serial" };
    let tag = if D::LAID_OUT { "" } else { "_dump_geometry" };
    println!(
        "SCENARIO first-pixel gallery s13 ({mode}{}), process entry to the GPU's completion of the first frame; {} records, {} tiles",
        if D::LAID_OUT { ", laid out" } else { ", dumped geometry" },
        app.rt.bough.len(),
        app.repaired_tiles
    );
    println!(
        "PHASES\tmount_done_ms={:.3}\tmain_mount_ms={:.3}\tmain_mount_instr={}\tgpu_prep_ms={:.3}\tgpu_wait_ms={:.3}\tframe_submitted_ms={:.3}\tpresented_ms={:.3}\texec_to_presented_ms={:.3}",
        mounted.as_secs_f64() * 1e3 - st.gpu_wait_ns as f64 / 1e6,
        st.mount_ns as f64 / 1e6,
        st.mount_instr,
        st.gpu_prep_ns as f64 / 1e6,
        st.gpu_wait_ns as f64 / 1e6,
        submitted.as_secs_f64() * 1e3,
        wall.as_secs_f64() * 1e3,
        exec as f64 / 1e6
    );
    let rm = bough::app::raster_main();
    println!("THREAD\tmain\t{main}\tstrikes_rasterised_here={}\tstrikes_waited={}\twaited_ms={:.3}", rm[0], rm[1], rm[2] as f64 / 1e6);
    if overlap {
        println!("THREAD\tgpu-prep\t{}", st.gpu_prep_instr);
        for (i, r) in raster_stats.iter().enumerate() {
            println!("THREAD\traster-{i}\t{}\tstrikes={}\tglyphs={}\tms={:.3}", r[2], r[0], r[1], r[3] as f64 / 1e6);
        }
    }
    let sv = bough::deco::prerender_svg_stats();
    if overlap {
        println!("THREAD\tsvg-workers\t{}\timages={}\tms={:.3}", sv[1], sv[0], sv[2] as f64 / 1e6);
    }
    println!("DECO\ticons_rasterised_on_main={}\tsvg_instr_on_main={}", app.rt.images.rasterised, app.rt.images.svg_instr);
    let fp = bough::text::FACE_PREP_INSTR.load(std::sync::atomic::Ordering::Relaxed);
    if overlap {
        println!("THREAD\tface-prep\t{fp}");
    }
    let named = main + if overlap { st.gpu_prep_instr + rast + sv[1] + fp } else { 0 };
    println!("GLYPHS\trasterised_on_main={}\traster_ms_on_main={:.3}\ttext_nodes={}", app.rt.text.cache.rasterized, app.rt.text.cache.raster_ns as f64 / 1e6, D::strikes().len());
    println!("THREAD\tother\t{}", proc.saturating_sub(named));
    println!("THREAD\tprocess\t{proc}");
    println!("MEASURE\tfirst-pixel\tcold.first_pixel_{mode}{tag}\tms\t{:.4}\t-", wall.as_secs_f64() * 1e3);
    println!("INSTR\tfirst-pixel\tcold.first_pixel_{mode}{tag}\tp50={proc}\tn=1");
}

/// zgui `scenario/kitchen.rs`: tab traversal to the first text field, 32 keystrokes (8 backspaces after
/// every 8th), 32 clicks on the probe swatches, 8 wheel notches with every glide tick, 32 resizes, 240
/// untouched turns, in that order.
pub fn kitchen<D: Doc>(gpu: bool) {
    let mut app = App::new(D::fresh(), config(WIDTH, HEIGHT), gpu);
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
    let value_after = app.owner.value();
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
    assert_eq!(app.owner.picked(), 3, "the clicks picked swatches");
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
            ticks.time(|| D::turn(&mut app, TICK_60));
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
        untouched.time(|| D::turn(&mut app, TICK_60));
    }
    println!(
        "SCENARIO kitchen-sink gallery s13, {} records, a text field {presses} tab stops in (value now {:?}), 32 of each interaction; {} glide ticks",
        app.rt.bough.len(),
        value_after,
        ticks.len()
    );
    if D::LAID_OUT {
        report("kitchen-sink", "kitchen.resize", "ms", &resizes, 1e-3);
    } else {
        // the dump-placed template has absolute geometry: a resize repaints without reflowing (not comparable)
        report("kitchen-sink", "kitchen.resize_repaint_only", "ms", &resizes, 1e-3);
    }
    report("kitchen-sink", "kitchen.click", "us", &clicks, 1.0);
    report("kitchen-sink", "kitchen.click_release", "us", &release, 1.0);
    report("kitchen-sink", "kitchen.keystroke", "us", &keys, 1.0);
    report("kitchen-sink", "kitchen.untouched_turn", "us", &untouched, 1.0);
    report("kitchen-sink", "kitchen.glide_tick", "us", &ticks, 1.0);
    report("kitchen-sink", "kitchen.tab_traversal", "us", &tabs, 1.0);
}

// ================================================================================ the laid-out gallery

/// zgui's spinner: 16 px, its mark a 4 px dot centred on the 2 px track at the top (the mark's colour from
/// the dump, preorder 125).
const SPINNER: f32 = 16.0;
const SPINNER_MARK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="2" r="2" fill="#fafafa"/></svg>"##;

/// The same gallery as a laid-out document: zgui's tree with each element's computed layout declarations
/// (`GALLERY_L`, `GALLERY_L_DECLS`; zgui's coordinates are not used), resolved at mount by bough's style
/// path, laid out by cephal from scratch inside the root scroll view (1,585 points wide: the port less
/// zgui's scrollbar), text measured and wrapped. Interactive nodes as in `Gallery`; the text field is
/// anchored to its laid-out box, the progress slide is an offset group in its track. Decorations from the
/// dump (`GALLERY_L_DECOS`): gradients, box shadows, the SVG documents of icons and charts; faces: weight
/// >= 600 in DejaVu Sans Bold, monospace in DejaVu Sans Mono.
pub struct GalleryL {
    pub scroll: Signal<f32>,
    pub picked: Signal<usize>,
    pub value: Signal<String>,
    pub clock: Signal<f32>,
    pub slide: Signal<(f32, f32)>,
}

impl GalleryL {
    pub fn new() -> GalleryL {
        GalleryL { scroll: Signal::new(0.0), picked: Signal::new(2), value: Signal::new("Ada Lovelace".to_string()), clock: Signal::new(0.0), slide: Signal::new((0.0, 0.0)) }
    }
}
impl Default for GalleryL {
    fn default() -> Self {
        Self::new()
    }
}

fn lhook(v: &GalleryL, m: &mut Mounter<'_, GalleryL>, ctx: &mut LCtx<'_>) -> LHooked {
    use bough::layout::prelude::*;
    let n = *ctx.node;
    let [w, h] = n.size;
    match n.tag {
        100..=103 => {
            let k = (n.tag - 100) as usize;
            m.mount(
                block(w, h)
                    .bg(SWATCH)
                    .rounded(n.radius)
                    .border(2.0, CLEAR)
                    .paint_with(sig!(v: GalleryL => picked), move |p| if *p == k { (PICKED_RING, PICKED_BG) } else { (CLEAR, SWATCH) })
                    .hover_bg(SWATCH_HOVER)
                    .on_click(move |g: &mut GalleryL| {
                        g.picked.set(k);
                    }),
            );
            LHooked::Default
        }
        2 => {
            m.mount(anchor(w, h, text_field(sig!(v: GalleryL => value)).rect(0.0, 0.0, w, h).font(14.0).colors(Color(n.bg), Color(n.border), rgb(0x0a0a0a)).rounded(n.radius)));
            LHooked::Subtree
        }
        3 => {
            // zgui's spinner (zgui-ui spinner/style.rs): a 16 px ring with a 2 px track and a 4 px mark on the
            // track, the whole turning (zui-spinner-turn, 900 ms, linear). The ring is rotation-invariant; the
            // ring-and-mark image turns about its centre (a paint write of the instance's angle per frame).
            let _ = (w, h);
            m.mount(anchor(
                SPINNER,
                SPINNER,
                (
                    quad(0.0, 0.0, SPINNER, SPINNER).bg(CLEAR).rounded(SPINNER * 0.5).border(n.border_w.max(2.0), Color(n.border)),
                    svg_image(0.0, 0.0, SPINNER, SPINNER, SPINNER_MARK).angle(sig!(v: GalleryL => clock).map(|t| (t / 0.9).fract() * std::f32::consts::TAU)),
                ),
            ));
            LHooked::Subtree
        }
        4 => {
            let bg = n.bg;
            m.mount(block(w, h).bg(Color(bg)).rounded(n.radius).paint_with(sig!(v: GalleryL => clock), move |t| {
                let ph = (t / 2.0).fract();
                (CLEAR, with_alpha(bg, 1.0 - 0.5 * (1.0 - (2.0 * ph - 1.0).abs())))
            }));
            LHooked::Default
        }
        5 => {
            m.mount(anchor(w, h, offset(sig!(v: GalleryL => slide), quad(0.0, 0.0, w, h).bg(Color(n.bg)).rounded(n.radius))));
            LHooked::Subtree
        }
        6 => {
            let bg = n.bg;
            m.mount(block(w.max(1.0), h).bg(Color(bg)).paint_with(sig!(v: GalleryL => clock), move |t| (CLEAR, if (t / 1.25).fract() < 0.5 { Color(bg) } else { with_alpha(bg, 0.0) })));
            LHooked::Default
        }
        _ => LHooked::Default,
    }
}

/// The root's content width: zgui's port less its 15 px scrollbar.
pub const CONTENT_W: f32 = 1585.0;

impl View for GalleryL {
    fn view(&self) -> impl Element<Self> + '_ {
        scroll_y(sig!(self.scroll), [0.0, 0.0, WIDTH, HEIGHT], 0.0, ltemplate(GALLERY_L, GALLERY_L_DECLS, [CONTENT_W, f32::NAN], lhook).decos(GALLERY_L_DECOS)).bg(rgb(0xffffff)).line(LINE)
    }
}

/// One 60 Hz turn of the laid-out gallery: the clock (and the progress slide's travel), any glide, a frame.
pub fn turn_l(app: &mut App<GalleryL>, dt: f32) -> bool {
    app.write(|g| {
        *g.clock += dt;
        let t = *g.clock;
        g.slide.set(((214.0 * ((t / 1.2).fract() * 2.0 - 1.0)).round(), 0.0));
    });
    if !app.tick(dt) {
        app.flush();
    }
    app.frame_paced()
}

impl Doc for GalleryL {
    const LAID_OUT: bool = true;
    fn strikes() -> Vec<(f32, &'static str, u8)> {
        GALLERY_L.iter().filter(|n| !n.text.is_empty()).map(|n| (n.font, n.text, n.face)).collect()
    }
    fn svgs() -> Vec<(&'static str, f32, f32)> {
        let mut v: Vec<(&'static str, f32, f32)> = GALLERY_L.iter().filter(|n| n.deco != 0 && !GALLERY_L_DECOS[n.deco as usize].svg.is_empty()).map(|n| (GALLERY_L_DECOS[n.deco as usize].svg, n.size[0], n.size[1])).collect();
        v.push((SPINNER_MARK, SPINNER, SPINNER));
        v
    }
    fn fresh() -> Self {
        GalleryL::new()
    }
    fn turn(app: &mut App<Self>, dt: f32) -> bool {
        turn_l(app, dt)
    }
    fn picked(&self) -> usize {
        *self.picked
    }
    fn value(&self) -> String {
        (*self.value).clone()
    }
}
