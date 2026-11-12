//! The application surface the design specifies (`designs/reactivity/final/02-elements.md`, `07-platform.md`
//! §12): text, buttons and boxes in flow, laid out by cephal; `const` layout recipes; `Application::run`.
//! Nothing here takes a coordinate: geometry comes from layout.
use crate::app::{Config, View};
use crate::color::{rgb, Color};
use crate::input::{HoverBg, Interact, On};
use crate::elem::{Element, Mounter};
use crate::layout::{attach, column, label, row, Align, Dim, Justify, LBox, Para, Spec, Styled};
use crate::signal::Wired;
use bough_core::{NRect, RecordInit};

/// Text in flow: one line, as wide as its text. A `sig!(..)` operand makes it a direct port, so publishing the
/// signal re-shapes only the changed glyphs, without rerunning `view`.
pub fn text<S>(s: S) -> Para<S> {
    label(s).font(14.0, 20.0).color(rgb(0x000000))
}

const BUTTON: Color = rgb(0x2f6bff);
const BUTTON_HOVER: Color = rgb(0x4a80ff);

/// A push button: its label in a padded, rounded box. Give it a route with `.on_click(Owner::method)`.
pub fn button<S>(s: S) -> On<LBox<Color, (Para<S>,)>, HoverBg> {
    row((text(s).color(rgb(0xffffff)),)).center().px(14.0).py(6.0).bg(BUTTON).rounded(6.0).hover_bg(BUTTON_HOVER)
}

/// The styled-box primitive, born empty: add content with `.child(x)`, arrange it with `.layout(recipe)`.
pub fn div() -> LBox<Color, ()> {
    column(())
}

/// A flex layout recipe. Every method is `const`, so a recipe can be a `const` item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flex {
    pub(crate) column: bool,
    pub(crate) gap: f32,
    pub(crate) align: Option<Align>,
    pub(crate) justify: Option<Justify>,
}

impl Flex {
    pub const fn row() -> Flex {
        Flex { column: false, gap: 0.0, align: None, justify: None }
    }
    pub const fn column() -> Flex {
        Flex { column: true, ..Flex::row() }
    }
    pub const fn gap(self, v: f32) -> Flex {
        Flex { gap: v, ..self }
    }
    pub const fn align(self, a: Align) -> Flex {
        Flex { align: Some(a), ..self }
    }
    pub const fn justify(self, j: Justify) -> Flex {
        Flex { justify: Some(j), ..self }
    }
}

/// A window around a root view: `Window::document(view).title("…").size(w, h)`.
pub struct Window<V> {
    view: V,
    title: String,
    size: (f32, f32),
}

impl<V: View> Window<V> {
    pub fn document(view: V) -> Window<V> {
        Window { view, title: String::new(), size: (480.0, 320.0) }
    }
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }
    /// The content size in points.
    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.size = (w, h);
        self
    }
}

/// What `Application::run` accepts: a bare view (one window, default chrome) or a configured `Window`.
pub trait IntoWindow {
    type View: View;
    fn into_window(self) -> Window<Self::View>;
}
impl<V: View> IntoWindow for V {
    type View = V;
    fn into_window(self) -> Window<V> {
        Window::document(self)
    }
}
impl<V: View> IntoWindow for Window<V> {
    type View = V;
    fn into_window(self) -> Window<V> {
        self
    }
}

/// The one application: `Application::run(root)`.
pub struct Application;

static WINDOW: std::sync::OnceLock<(f32, f32, Vec<u8>)> = std::sync::OnceLock::new();

/// Region capacities for applications (the comparison scenes size their own): room for documents of tens of
/// thousands of records and a few hundred thousand glyphs. Untouched capacity is never written, so it costs no
/// memory; a column that still fills up stops with its name (growth is not built yet).
const APP_CAPS: crate::rt::Caps = crate::rt::Caps { instances: 1 << 16, pages: 1 << 13, glyphs: 1 << 19, runs: 1 << 16, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };

fn window_config(scale: f32) -> Config {
    let (w, h, font) = WINDOW.get().expect("Application::run sets the window first");
    let mut cfg = Config::new(*w, *h, scale, font.clone());
    cfg.caps = APP_CAPS;
    // the most surfaces the window may draw (default 4, as contention needs): measurement knob, read once
    cfg.ring = std::env::var("BOUGH_RING").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    cfg
}

/// The default face: `BOUGH_FONT`, then `ZGX_FONT_FILE`, then the system's Arial.
fn default_font() -> Vec<u8> {
    let candidates = [std::env::var("BOUGH_FONT").ok(), std::env::var("ZGX_FONT_FILE").ok(), Some("/System/Library/Fonts/Supplemental/Arial.ttf".into())];
    candidates.into_iter().flatten().find_map(|p| std::fs::read(p).ok()).expect("a TrueType font (set BOUGH_FONT)")
}

impl Application {
    /// Open the window, mount the root view, and run until the window closes.
    #[cfg(target_os = "macos")]
    pub fn run<D: IntoWindow>(root: D) -> ! {
        let w = root.into_window();
        let title = if w.title.is_empty() { "bough".to_string() } else { w.title };
        let _ = WINDOW.set((w.size.0, w.size.1, default_font()));
        crate::backend::host::run_interactive(&title, window_config, w.view, 0.0, |_, _| {}, |_| {})
    }

    /// `run` for measurement harnesses (not an application API): the same window, configuration and input
    /// path, plus `start(host, app)` once the first frame is up (a harness installs its timers there: an
    /// in-process scroll through `App::wheel_points`, a ledger sample) and an exit after `seconds`, after
    /// `report(app)`.
    #[cfg(target_os = "macos")]
    pub fn run_harness<D: IntoWindow>(
        root: D,
        seconds: f64,
        start: impl FnOnce(&bough_metal::app::Host, std::rc::Rc<std::cell::RefCell<crate::app::App<D::View>>>) + 'static,
        report: impl FnOnce(&mut crate::app::App<D::View>) + 'static,
    ) -> ! {
        let w = root.into_window();
        let title = if w.title.is_empty() { "bough".to_string() } else { w.title };
        let _ = WINDOW.set((w.size.0, w.size.1, default_font()));
        crate::backend::host::run_interactive(&title, window_config, w.view, seconds, start, report)
    }
}

/// A raster image (PNG bytes) in flow, sized by layout (by default its pixel size, in points). The pixels are
/// decoded once into the image atlas and drawn on the GPU wherever layout puts the box.
pub struct ImageBox {
    png: &'static [u8],
    spec: Spec,
    px: (u32, u32),
}

pub fn image(png: &'static [u8]) -> ImageBox {
    assert!(png.len() > 24 && &png[1..4] == b"PNG", "image(): PNG bytes");
    let (w, h) = (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()));
    let mut spec = Spec::new(false, false);
    spec.w = Dim::Pt(w as f32);
    spec.h = Dim::Pt(h as f32);
    spec.shrink = 0.0;
    ImageBox { png, spec, px: (w, h) }
}

impl Styled for ImageBox {
    fn spec_mut(&mut self) -> &mut Spec {
        &mut self.spec
    }
}

/// `png` decoded to premultiplied RGBA8 into `out` (`w * h * 4` bytes).
fn decode_premultiplied(png: &[u8], out: &mut [u8]) {
    let mut dec = png::Decoder::new(std::io::Cursor::new(png));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().expect("a PNG image");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("PNG pixels");
    let n = (info.width * info.height) as usize;
    let px = |i: usize| -> [u8; 4] {
        match info.color_type {
            png::ColorType::Rgba => [buf[4 * i], buf[4 * i + 1], buf[4 * i + 2], buf[4 * i + 3]],
            png::ColorType::Rgb => [buf[3 * i], buf[3 * i + 1], buf[3 * i + 2], 255],
            png::ColorType::GrayscaleAlpha => [buf[2 * i], buf[2 * i], buf[2 * i], buf[2 * i + 1]],
            _ => [buf[i], buf[i], buf[i], 255],
        }
    };
    for i in 0..n.min(out.len() / 4) {
        let [r, g, b, a] = px(i);
        let pm = |c: u8| ((c as u32 * a as u32 + 127) / 255) as u8;
        out[4 * i..4 * i + 4].copy_from_slice(&[pm(r), pm(g), pm(b), a]);
    }
}

/// Bilinear resample of premultiplied RGBA8 (`sw` x `sh` into `dw` x `dh`), texel centres aligned.
fn resample(src: &[u8], sw: u32, sh: u32, dst: &mut [u8], dw: u32, dh: u32) {
    let at = |x: i64, y: i64, c: usize| src[(((y.clamp(0, sh as i64 - 1) as u32) * sw + x.clamp(0, sw as i64 - 1) as u32) * 4) as usize + c] as f32;
    for y in 0..dh {
        let fy = (y as f32 + 0.5) * sh as f32 / dh as f32 - 0.5;
        let (y0, ty) = (fy.floor() as i64, fy - fy.floor());
        for x in 0..dw {
            let fx = (x as f32 + 0.5) * sw as f32 / dw as f32 - 0.5;
            let (x0, tx) = (fx.floor() as i64, fx - fx.floor());
            for c in 0..4 {
                let top = at(x0, y0, c) * (1.0 - tx) + at(x0 + 1, y0, c) * tx;
                let bot = at(x0, y0 + 1, c) * (1.0 - tx) + at(x0 + 1, y0 + 1, c) * tx;
                dst[((y * dw + x) * 4) as usize + c] = (top * (1.0 - ty) + bot * ty).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

impl<V: 'static> Element<V> for ImageBox {
    fn mount(self, m: &mut Mounter<'_, V>) {
        // state lives in signals, the atlas or the region: a re-description steps over it
        crate::describe::mount_opaque(m, move |m: &mut Mounter<'_, V>| {
        let s = m.rt.scale;
        let hd = m.rt.bough.new_handle();
        let lid = m.rt.lay.eng.add(hd, &self.spec, s, None);
        attach(m, lid);
        m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: crate::rt::REC_BOX, payload: 0, handle: hd.idx, clip: m.clip });
        let key = self.png.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
        let png = self.png;
        // the image kernel draws atlas texels 1:1 on device pixels: resample to the box's device size
        let dev = |d: Dim, n: u32| (d.pt().unwrap_or(n as f32) * s).round().max(1.0) as u32;
        let (tw, th) = (dev(self.spec.w, self.px.0), dev(self.spec.h, self.px.1));
        let (pw, ph) = self.px;
        let uv = m.rt.images.put(key, tw, th, |buf| {
            let mut src = vec![0u8; (pw * ph * 4) as usize];
            decode_premultiplied(png, &mut src);
            resample(&src, pw, ph, buf, tw, th);
        });
        let ins = bough_metal::instance::Instance { color: u32::MAX, uv, kind: crate::deco::KIND_IMAGE, ..Default::default() };
        m.depth += 1;
        let node = crate::deco::mount_deco(m, ins, 0.0, 0.0, 0.0, 0.0, 1.0);
        m.depth -= 1;
        m.rt.add_place_hook(lid, crate::ltemplate::PlaceHook::Deco { node, dx: 0.0, dy: 0.0, grow: 0.0, sized: true });
    })
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
    /// Its re-description steps over it (`update_opaque`): the description has nothing to compare.
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x40);
    }
}

/// A one-line text input in flow, bound two-way to a `Signal<String>` (`text_input(sig!(self.notes))`).
pub struct TextInput<'a, V> {
    sig: Wired<'a, String, V>,
    w: f32,
    h: f32,
}

pub fn text_input<V>(sig: Wired<'_, String, V>) -> TextInput<'_, V> {
    TextInput { sig, w: 240.0, h: 32.0 }
}

impl<V> TextInput<'_, V> {
    /// The input's size in points.
    pub fn size(mut self, w: f32, h: f32) -> Self {
        (self.w, self.h) = (w, h);
        self
    }
}

impl<V: 'static> Element<V> for TextInput<'_, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        // state lives in signals, the atlas or the region: a re-description steps over it
        crate::describe::mount_opaque(m, move |m: &mut Mounter<'_, V>| {
        let field = crate::input::text_field(self.sig).rect(0.0, 0.0, self.w, self.h).font(14.0).colors(rgb(0xffffff), rgb(0xb4b4b4), rgb(0x000000)).rounded(6.0);
        crate::template::anchor(self.w, self.h, field).mount(m)
    })
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
    /// Its re-description steps over it (`update_opaque`): the description has nothing to compare.
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x40);
    }
}
