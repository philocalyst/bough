//! The application surface the design specifies (`designs/reactivity/final/02-elements.md`, `07-platform.md`
//! §12): text, buttons and boxes in flow, laid out by cephal; `const` layout recipes; `Application::run`.
//! Nothing here takes a coordinate: geometry comes from layout.
use crate::app::{Config, View};
use crate::color::{rgb, Color};
use crate::input::{HoverBg, Interact, On};
use crate::layout::{column, label, row, Align, Justify, LBox, Para, Styled};

/// Text in flow: one line, as wide as its text. A `sig!(..)` operand makes it a direct port, so publishing the
/// signal re-shapes only the changed glyphs, without rerunning `view`.
pub fn text<S>(s: S) -> Para<S> {
    label(s).font(14.0, 20.0)
}

const BUTTON: Color = rgb(0x2f6bff);
const BUTTON_HOVER: Color = rgb(0x4a80ff);

/// A push button: its label in a padded, rounded box. Give it a route with `.on_click(Owner::method)`.
pub fn button<S>(s: S) -> On<LBox<Color, (Para<S>,)>, HoverBg> {
    row((text(s),)).px(14.0).py(6.0).bg(BUTTON).rounded(6.0).hover_bg(BUTTON_HOVER)
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

fn window_config(scale: f32) -> Config {
    let (w, h, font) = WINDOW.get().expect("Application::run sets the window first");
    Config::new(*w, *h, scale, font.clone())
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
}
