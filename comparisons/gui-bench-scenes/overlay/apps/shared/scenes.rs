//! The heavy scenes (bough's standing workloads, ported to gui-bench): the look, the data, the keys and the
//! command line that every app shares, so all frameworks draw the same layout and react to the same input.
//! The runner mirrors the geometry it probes in `runner/src/scenes.rs`.
//!
//! An app calls [`from_args`] before gui-bench's own `cli::from_env`; it returns a scene when the command line
//! names one (`--scenario sheet|canvas --rows N`), and the app then shows that scene instead.
//!
//! Every scene has:
//! * a light and a dark theme; the key `t` toggles between them (`THEME_KEY`);
//! * a hover fan-out: one hover target that changes the look of thousands of elements;
//! * a steady edit on a key (`e` in the sheet, `m` in the canvas) that the same key undoes;
//! * a layout that follows the window: the panels keep their sizes and the scrolled or drawn area takes the rest.
//!
//! All sizes are logical pixels at scale 1; the window is `WINDOW`. Text is `FONT_FAMILY`, left aligned and
//! vertically centred in its box unless a constant says otherwise. A "fill" is a solid rectangle colour. Colours
//! are `0xrrggbb`, or `0xaarrggbb` where a constant says so.
//!
//! Write the scene the way the framework's own users would: its normal widgets, layout, scrolling (and its
//! virtualised list where it has one), hover and key handling, theming. Where the framework cannot draw what the
//! spec says, draw the closest thing and record the deviation in `DEVIATIONS.md` (bough's repository,
//! `comparisons/gui-bench-scenes/`).

#![allow(dead_code)]

/// The window size.
pub const WINDOW: (f32, f32) = (1280.0, 800.0);
/// The font family of every scene's text.
pub const FONT_FAMILY: &str = "sans-serif";
/// The key that toggles the theme.
pub const THEME_KEY: &str = "t";

/// A scene and its size, from the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    /// `--scenario sheet --rows N`: a spreadsheet of `rows` x `sheet::COLS` cells.
    Sheet { rows: usize },
    /// `--scenario canvas --rows N`: a design canvas of `nodes` layered nodes (N rounded down to a multiple of 20).
    Canvas { nodes: usize },
}

impl Scene {
    pub fn title(&self) -> &'static str {
        match self {
            Scene::Sheet { .. } => "gui-bench sheet",
            Scene::Canvas { .. } => "gui-bench canvas",
        }
    }
}

/// The scene the command line names, if it names one of these (`--scenario X` or `--scenario=X`, `--rows N` or
/// `--rows=N`). Any other scenario returns `None`: the app goes on to gui-bench's own command line.
pub fn from_args(args: impl IntoIterator<Item = String>) -> Option<Scene> {
    let mut scenario = None;
    let mut rows: Option<usize> = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let (k, v) = match a.split_once('=') {
            Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
            None => (a.clone(), None),
        };
        match k.as_str() {
            "--scenario" => scenario = v.or_else(|| it.next()),
            "--rows" => rows = v.or_else(|| it.next()).and_then(|v| v.parse().ok()),
            _ => {}
        }
    }
    match scenario.as_deref() {
        Some("sheet") => Some(Scene::Sheet { rows: rows.unwrap_or(1000) }),
        Some("canvas") => Some(Scene::Canvas { nodes: (rows.unwrap_or(5000) / canvas::FRAMES).max(5) * canvas::FRAMES }),
        _ => None,
    }
}

/// `from_args` over the process's arguments.
pub fn from_env() -> Option<Scene> {
    from_args(std::env::args().skip(1))
}

/// The red, green and blue bytes of `0xrrggbb`.
pub const fn rgb(c: u32) -> (u8, u8, u8) {
    ((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// SplitMix64: the deterministic generator every scene's data comes from.
pub const fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The spreadsheet.
///
/// ```text
/// y 0..40     toolbar: CHROME; "Sheet 1" (14 px, INK) at x 12; "Σ totals" (12 px, GUTTER_INK) at x 100
/// y 40..64    header: a gutter cell (0..56) and COLS header cells (x = 56 + c * 110, 110 wide), CHROME, or
///             HEADER_HOT for the hot column; the column letter (12 px, INK) at 6 px from the cell's left
/// y 64..778   the body, scrolled vertically: `rows` rows of 22: the gutter (56 wide, CHROME, the row number r + 1
///             in 11 px GUTTER_INK) and COLS cells (110 x 22, `fill(theme, hot, r)`, the value with two decimals
///             in 12 px INK at 6 px from the left); right of the last cell, the body is PAGE
/// y 778..800  footer: CHROME; "Σ" in the gutter; each column's total (two decimals, 12 px INK)
/// ```
///
/// The cell at row 0, column 0 is selected: a 2 px SELECT border inside its rectangle. The body's scrollbar, if the
/// framework shows one, sits at the right edge of the window, over PAGE.
///
/// Input: the pointer over header cell `c` makes column `c` hot (its header cell HEADER_HOT, all its body cells
/// `fill(theme, true, r)`); anywhere else, no column is hot. `t` toggles the theme. `e` toggles the value of row
/// `EDIT.0`, column `EDIT.1` between `value(EDIT)` and `value(EDIT) + EDIT_DELTA`, and its column's total follows.
pub mod sheet {
    /// Columns.
    pub const COLS: usize = 10;
    pub const TOOLBAR_H: f32 = 40.0;
    pub const HEADER_H: f32 = 24.0;
    pub const FOOTER_H: f32 = 22.0;
    pub const GUTTER_W: f32 = 56.0;
    pub const CELL_W: f32 = 110.0;
    pub const CELL_H: f32 = 22.0;
    /// The left padding of every cell's and header cell's text.
    pub const TEXT_PAD: f32 = 6.0;
    /// The top of the body (the first row at scroll 0).
    pub const BODY_TOP: f32 = TOOLBAR_H + HEADER_H;
    /// The cell the `e` key edits (row, column) and what it adds.
    pub const EDIT: (usize, usize) = (2, 1);
    pub const EDIT_DELTA: f64 = 1111.11;
    pub const EDIT_KEY: &str = "e";
    pub const SELECT_BORDER: f32 = 2.0;

    pub const COLUMN_NAMES: [&str; COLS] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];

    /// Light and dark colours.
    pub const CHROME: [u32; 2] = [0xeceef2, 0x23262d];
    pub const INK: [u32; 2] = [0x1d2129, 0xe6e9ef];
    pub const GUTTER_INK: u32 = 0x7a808c;
    pub const FILL_EVEN: [u32; 2] = [0xffffff, 0x15171c];
    pub const FILL_ODD: [u32; 2] = [0xf6f7f9, 0x1b1e24];
    pub const FILL_HOT: [u32; 2] = [0xdbe8ff, 0x26406b];
    pub const PAGE: [u32; 2] = FILL_EVEN;
    pub const HEADER_HOT: u32 = 0x9db8f0;
    pub const SELECT: u32 = 0x2f6bff;

    /// A body cell's fill: `theme` 0 light, 1 dark.
    pub const fn fill(theme: usize, hot: bool, row: usize) -> u32 {
        if hot {
            FILL_HOT[theme]
        } else if row % 2 == 0 {
            FILL_EVEN[theme]
        } else {
            FILL_ODD[theme]
        }
    }

    /// The value of a cell before any edit: 0.00 to 9999.99.
    pub const fn value(row: usize, col: usize) -> f64 {
        (super::mix((row * COLS + col) as u64) % 1_000_000) as f64 / 100.0
    }

    /// The text of a value: two decimals (`format!("{v:.2}")`).
    pub fn text(v: f64) -> String {
        format!("{v:.2}")
    }

    /// The x of a column's left edge.
    pub const fn col_x(col: usize) -> f32 {
        GUTTER_W + col as f32 * CELL_W
    }
}

/// The design canvas.
///
/// ```text
/// y 0..44            toolbar: CHROME; "Canvas" (14 px, INK) at x 12
/// x 0..240, y 44..   layers panel: CHROME, scrolled vertically: rows of 24: first the COMPONENTS component rows
///                    ("Component k", k = 1..5, with a 12 x 12 swatch of `comp(theme, k - 1, false)` at x 12 and
///                    the name at x 32), then for every frame f a row "Frame f+1" and LAYER_ROWS rows "Layer <g>"
///                    for the frame's first LAYER_ROWS nodes (g the node's global index), indented to x 24;
///                    text 12 px INK; a hovered component row takes ROW_HOT
/// x 1020..1280       inspector: CHROME; INSPECTOR rows of 24 from y 56: the label at x 1032 (12 px, MUTED) and the
///                    value at x 1140 (12 px, INK, whole numbers) of the selected node
/// x 240..1020        the canvas: CANVAS_BG; FRAMES frames (`frame_rect`) with their titles and nodes
/// ```
///
/// A frame: its title "Frame f+1" (11 px, MUTED) with its baseline area at `frame_rect.y - 16`; a FRAME_RADIUS
/// rounded rect of FRAME_BG with a FRAME_SHADOW under it; its nodes clipped to the frame's rectangle (a rounded
/// clip where the framework has one), drawn in index order, later on top, except the selected node (node 0 of
/// frame 0), which is drawn after all of frame 0's other nodes, on top of them (in index order it would be covered
/// by up to 249 later nodes and the `m` probe could not see it move). Node `i` of frame `f` is `node(f, i)`; group
/// nodes are also clipped to their group's rectangle inside the frame (`group_rect`).
///
/// Shadows: `blur` is the CSS blur radius (the Gaussian's standard deviation is `blur / 2`); map it to the
/// framework's own parameter and say how in DEVIATIONS.
///
/// Input: the pointer over component row `k` (`component_row_y(k)`) makes component `k` hot: all its instances take
/// HOT_FILL. `t` toggles the theme. `m` toggles the selected node (node 0 of frame 0, MOVE_A or MOVE_B) between its
/// two x positions; the inspector's "x" shows its x within the frame.
pub mod canvas {
    use super::mix;

    pub const FRAMES: usize = 20;
    pub const FRAME_COLS: usize = 5;
    pub const TOOLBAR_H: f32 = 44.0;
    pub const PANEL_W: f32 = 240.0;
    pub const INSPECTOR_X: f32 = 1020.0;
    pub const ROW_H: f32 = 24.0;
    pub const COMPONENTS: usize = 5;
    pub const LAYER_ROWS: usize = 25;
    pub const INSPECTOR: [&str; 12] = ["x", "y", "w", "h", "radius", "opacity", "blur", "shadow y", "fill r", "fill g", "fill b", "layer"];

    pub const FRAME_W: f32 = 140.0;
    pub const FRAME_H: f32 = 160.0;
    pub const FRAME_X0: f32 = 258.0;
    pub const FRAME_Y0: f32 = 74.0;
    pub const FRAME_STEP_X: f32 = 152.0;
    pub const FRAME_STEP_Y: f32 = 182.0;
    pub const FRAME_RADIUS: f32 = 6.0;
    /// A drop shadow: offset y, blur radius, colour `0xaarrggbb`.
    pub const FRAME_SHADOW: (f32, f32, u32) = (2.0, 8.0, 0x40000000);

    /// Light and dark colours.
    pub const CHROME: [u32; 2] = [0xeceef2, 0x23262d];
    pub const INK: [u32; 2] = [0x1d2129, 0xe6e9ef];
    pub const MUTED: u32 = 0x7a808c;
    pub const ROW_HOT: [u32; 2] = [0xcfe3ff, 0x2a3b55];
    pub const CANVAS_BG: [u32; 2] = [0xe5e5e5, 0x1e1e1e];
    pub const FRAME_BG: [u32; 2] = [0xffffff, 0x2c2c2c];
    pub const COMP: [[u32; COMPONENTS]; 2] = [[0x5b8def, 0x34c759, 0xff9f0a, 0xaf52de, 0xff375f], [0x3a6fd8, 0x248a3d, 0xc77c02, 0x8944ab, 0xd70f45]];
    pub const HOT_FILL: u32 = 0xffd60a;
    /// The node palette (the same in both themes).
    pub const PALETTE: [u32; 8] = [0x4e79a7, 0xf28e2b, 0xe15759, 0x76b7b2, 0x59a14f, 0xedc948, 0xb07aa1, 0xff9da7];
    /// The selected node: fill, and its 1 px outline 2 px outside it.
    pub const SELECT_FILL: u32 = 0x2f6bff;
    pub const SELECT_OUTLINE: u32 = 0x0a84ff;
    /// The selected node's rectangle in frame 0 (frame-relative x, y, w, h) and its two x positions.
    pub const MOVE_RECT: (f32, f32, f32, f32) = (10.0, 10.0, 40.0, 40.0);
    pub const MOVE_A: f32 = 10.0;
    pub const MOVE_B: f32 = 58.0;
    pub const MOVE_KEY: &str = "m";

    /// A component's fill: `theme` 0 light, 1 dark.
    pub const fn comp(theme: usize, k: usize, hot: bool) -> u32 {
        if hot { HOT_FILL } else { COMP[theme][k] }
    }

    /// Frame `f`'s rectangle (window coordinates: x, y, w, h).
    pub fn frame_rect(f: usize) -> (f32, f32, f32, f32) {
        let (c, r) = (f % FRAME_COLS, f / FRAME_COLS);
        (FRAME_X0 + c as f32 * FRAME_STEP_X, FRAME_Y0 + r as f32 * FRAME_STEP_Y, FRAME_W, FRAME_H)
    }

    /// Group `q` (0..4) of a frame: a quadrant, frame-relative (x, y, w, h).
    pub fn group_rect(q: usize) -> (f32, f32, f32, f32) {
        ((q % 2) as f32 * 70.0, (q / 2) as f32 * 80.0, 70.0, 80.0)
    }

    /// The y of component row `k`'s centre (k = 0..5).
    pub fn component_row_y(k: usize) -> f32 {
        TOOLBAR_H + k as f32 * ROW_H + ROW_H / 2.0
    }

    /// What a node is.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Kind {
        /// A rounded rect with a drop shadow: radius, fill, shadow (offset y, blur radius); shadow colour SHADOW.
        Shadowed { radius: f32, fill: u32, dy: f32, blur: f32 },
        /// A rect with a vertical linear gradient from `top` to `bottom`.
        Gradient { top: u32, bottom: u32 },
        /// A translucent box: `fill` at alpha 0.4 (0x66), with a 1 px border of `border`.
        Bordered { fill: u32, border: u32 },
        /// A text label "L<g>" (10 px, INK) at the rect's top left; the rect is only its place.
        Label,
        /// An instance of component `k`: a radius 4 rect of `comp(theme, k, hot)` with a 1 px border of 0x33000000.
        Instance { k: usize },
        /// The selected node (node 0 of frame 0): a radius 6 rect of SELECT_FILL with its SELECT_OUTLINE.
        Selected,
    }

    /// The shadow colour of `Kind::Shadowed`, `0xaarrggbb`.
    pub const SHADOW: u32 = 0x4d000000;

    /// One node: global index, its rectangle (frame-relative, or group-relative when `group` is set), its kind.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Node {
        pub g: usize,
        pub rect: (f32, f32, f32, f32),
        pub group: Option<usize>,
        pub kind: Kind,
    }

    /// Node `i` of frame `f` in a canvas of `per_frame` nodes per frame.
    pub fn node(f: usize, i: usize, per_frame: usize) -> Node {
        let g = f * per_frame + i;
        if f == 0 && i == 0 {
            return Node { g, rect: MOVE_RECT, group: None, kind: Kind::Selected };
        }
        let h = |k: u64| mix((g as u64) << 4 | k);
        let w = 12.0 + (h(1) % 49) as f32;
        let ht = 10.0 + (h(2) % 41) as f32;
        let group = (i % 10 == 9).then_some((i / 10) % 4);
        let (aw, ah) = match group {
            Some(_) => (70.0 + 30.0, 80.0 + 30.0),
            None => (FRAME_W, FRAME_H),
        };
        let x = (h(3) % ((aw - w).max(1.0) as u64)) as f32;
        let y = (h(4) % ((ah - ht).max(1.0) as u64)) as f32;
        let fill = PALETTE[(h(5) % 8) as usize];
        let kind = match g % 5 {
            0 => {
                let blur = 2.0 + (h(6) % 39) as f32;
                Kind::Shadowed { radius: (h(7) % 11) as f32, fill, dy: (blur / 4.0).round(), blur }
            }
            1 => Kind::Gradient { top: fill, bottom: PALETTE[(h(8) % 8) as usize] },
            2 => Kind::Bordered { fill, border: PALETTE[(h(9) % 8) as usize] },
            3 => Kind::Label,
            _ => Kind::Instance { k: (g / 5) % COMPONENTS },
        };
        Node { g, rect: (x, y, w, ht), group, kind }
    }

    /// The inspector's values for the selected node at x `x` (frame-relative).
    pub fn inspector(x: f32) -> [i64; 12] {
        let (r, gr, b) = super::rgb(SELECT_FILL);
        [x as i64, MOVE_RECT.1 as i64, MOVE_RECT.2 as i64, MOVE_RECT.3 as i64, 6, 100, 0, 0, r as i64, gr as i64, b as i64, 0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let a = |s: &str| s.split(' ').map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(from_args(a("--scenario sheet --rows 50")), Some(Scene::Sheet { rows: 50 }));
        assert_eq!(from_args(a("--scenario=canvas --rows=5003")), Some(Scene::Canvas { nodes: 5000 }));
        assert_eq!(from_args(a("--scenario rows --rows 50")), None);
    }

    #[test]
    fn deterministic() {
        assert_eq!(sheet::value(0, 0), sheet::value(0, 0));
        assert!(sheet::value(7, 3) < 10_000.0);
        let n = canvas::node(3, 17, 250);
        assert_eq!(n, canvas::node(3, 17, 250));
        assert!(n.rect.0 >= 0.0 && n.rect.1 >= 0.0);
    }
}
