//! zgx's Sketch example (`crates/zgx-script/examples/sketch`: `app.luau`, `state.luau`, `actions.luau`,
//! `history.luau`, `components/{board,inspector,toolbar}.luau`, `lib/{geometry,snap}.luau`) as a bough view:
//! a toolbar (tools, undo/redo, status, title, Snap, zoom, Open, Save), the board (a dotted grid, the shapes,
//! the selection's outlines and handles, snapping guides, the marquee) and the inspector (nothing / one shape
//! / many), with the board's gestures (select, move with snapping, resize by a handle, draw, marquee, pan by
//! the wheel or Space-drag, zoom by Cmd-wheel or a pinch) and keys.
//!
//! Representation (each a structural choice; the port's report says what each bought):
//! * the drawing is a column of plain-old-data shapes; the board draws it as one canvas run of instances
//!   in a space of its own: **a pan is one space write** (the run follows on the GPU), **a zoom is one pass
//!   over the column** (every instance re-derived: position, size, corner radius, stroke), **a drag writes
//!   one instance** (its old and new rects damaged);
//! * the grid's dots are two canvas runs in a space: a pan moves the space by the view modulo the grid step
//!   (no dot is rewritten), a zoom re-derives the dots in one pass (only the dots in view);
//! * the marks (outlines, handles, guides, marquee) are a second canvas, rewritten whole when they change
//!   (a dozen instances), as zgx's `marks` canvas draws them again;
//! * the inspector's panels are mounted once, each in a space, and shown or hidden by moving that space
//!   (no mount on selection: zgx's `ui.presence` mounts the panel); its fields are text fields wired to
//!   the selected shape's values.
pub mod model;

use crate::canvas::Canvas;
use crate::theme::*;
use bough::layout::prelude::*;
use bough::layout::Dim;
use bough::raw::*;
use bough::rt::{Rt, KIND_BORDER, KIND_RRECT, KIND_SOLID};
use bough::frame::Instance;
use model::*;

pub const WIDTH: f32 = 1240.0;
pub const HEIGHT: f32 = 800.0;
pub const SCALE: f32 = 2.0;
/// The toolbar (py 8 around a control), the inspector (`state.inspector`, 260) and the split's line.
pub const TOOLBAR_H: f32 = 50.0;
pub const INSPECTOR_W: f32 = 260.0;
pub const SPLIT_W: f32 = 1.0;
/// The board.
pub const BX: f32 = 0.0;
pub const BY: f32 = TOOLBAR_H;
pub const BW: f32 = WIDTH - INSPECTOR_W - SPLIT_W;
pub const BH: f32 = HEIGHT - TOOLBAR_H;
/// The inspector's content box (p 16).
const IX: f32 = BW + SPLIT_W + 16.0;
const IY: f32 = TOOLBAR_H + 16.0;
const IW: f32 = INSPECTOR_W - 32.0;
/// Handles and snapping's pull, in screen points (`board.luau`).
const HANDLE: f32 = 8.0;
const PULL: f32 = 6.0;
/// The grid: `bg_grid { step = GRID * 2, min_step = 12, dot = 1 }`: dots at most every 12 points, in two
/// canvas runs of rows (a run holds at most 4,096 instances).
const GRID_STEP: f32 = 2.0 * GRID;
const MIN_STEP: f32 = 12.0;
const GRID_COLS: usize = (BW / MIN_STEP) as usize + 3;
const GRID_ROWS_A: usize = 33;
const GRID_ROWS_B: usize = 34;
const GRID_CAP_A: usize = GRID_COLS * GRID_ROWS_A;
const GRID_CAP_B: usize = GRID_COLS * GRID_ROWS_B;
/// Where a hidden panel goes (off the window, right).
const AWAY: f32 = 4.0 * WIDTH;

/// `actions.FILLS` and the inspector's `STROKES`.
pub const FILLS: [&str; 7] = ["#4c8dff", "#33b679", "#f6bf26", "#f4511e", "#a142f4", "#e8eaed", "#202124"];
pub const STROKES: [&str; 5] = ["#00000000", "#202124", "#e8eaed", "#4c8dff", "#f4511e"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rect,
    Ellipse,
}

/// The grid's dots: the screen step and the sub-pixel part of the grid's origin (points).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Grid {
    pub step: f32,
    pub cols: u32,
    pub rows: u32,
    pub fx: f32,
    pub fy: f32,
}
fn blank(_: &u8, _: usize) -> Lane {
    Lane { x: 0.0, y: 0.0, w: 0.0, h: 0.0, color: CLEAR }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GestureKind {
    Move,
    Resize,
    Draw,
    Marquee,
    Pan,
}

struct Gesture {
    kind: GestureKind,
    /// Where it started, on the board and in the world.
    sx: f32,
    sy: f32,
    wx: f32,
    wy: f32,
    view: Viewport,
    /// The shapes it moves: index and where they were.
    moving: Vec<(usize, f32, f32)>,
    bounds: Option<Rect>,
    handle: Option<model::Handle>,
    index: Option<usize>,
    kept: Vec<u32>,
    /// The lines the shapes it leaves where they are offer, found once.
    lines: Option<(Vec<f32>, Vec<f32>)>,
}

/// The paint step's log: what changed since the canvases were last written.
#[derive(Default)]
struct Paint {
    shapes: Option<Canvas>,
    marks: Option<Canvas>,
    grid: Option<[Canvas; 2]>,
    /// The grid changed (a zoom), and how many dots each grid run shows.
    grid_dirty: bool,
    grid_used: [u32; 2],
    /// Every shape (a zoom, a load, a sub-pixel pan) or these.
    all: bool,
    some: Vec<u32>,
    marks_dirty: bool,
    marks_used: u32,
    /// The view the shapes were painted at: zoom and the drawing's sub-pixel offset (device px).
    zoom: f32,
    frac: (f32, f32),
}

pub struct Sketch {
    // what the screen shows (wired)
    pub pan: Signal<(f32, f32)>,
    pub grid_at: Signal<(f32, f32)>,
    pub grid: Grid,
    pub canvas_key: Signal<u8>,
    pub tool_sig: Signal<u8>,
    pub undo_on: Signal<bool>,
    pub redo_on: Signal<bool>,
    pub status: Signal<String>,
    pub title: Signal<String>,
    pub zoom_pct: Signal<f32>,
    pub snap_on: Signal<bool>,
    pub p_none: Signal<(f32, f32)>,
    pub p_one: Signal<(f32, f32)>,
    pub p_rect: Signal<(f32, f32)>,
    pub p_ell: Signal<(f32, f32)>,
    pub p_many: Signal<(f32, f32)>,
    pub count_text: Signal<String>,
    pub many_text: Signal<String>,
    pub f_name: Signal<String>,
    pub f_x: Signal<String>,
    pub f_y: Signal<String>,
    pub f_w: Signal<String>,
    pub f_h: Signal<String>,
    pub f_r: Signal<String>,
    pub f_fill: Signal<String>,
    pub f_sw: Signal<String>,
    pub fill_sel: Signal<u8>,
    pub stroke_sel: Signal<u8>,
    // the model (`state.luau`)
    pub shapes: Vec<Shape>,
    pub names: Vec<String>,
    pub selected: Vec<u32>,
    pub tool: Tool,
    pub view: Viewport,
    pub snapping: bool,
    pub guides: Vec<Guide>,
    pub marquee: Option<Rect>,
    pub file: Option<String>,
    pub dirty: bool,
    pub next_id: u32,
    pub history: History,
    gesture: Option<Gesture>,
    space: bool,
    /// Canvas capacity (shapes the board can hold).
    pub cap: usize,
    paint: Paint,
}

/// `ui.number(n, { decimals = n % 1 == 0 and 0 or 1, grouping = false })`.
pub fn shown(n: f32) -> String {
    if n.fract() == 0.0 { format!("{n:.0}") } else { format!("{n:.1}") }
}

impl Sketch {
    /// An empty drawing ("Untitled") whose board holds up to `cap` shapes.
    pub fn new(cap: usize) -> Sketch {
        let mut s = Sketch {
            pan: Signal::new((0.0, 0.0)),
            grid_at: Signal::new((0.0, 0.0)),
            grid: Grid::default(),
            canvas_key: Signal::new(0),
            tool_sig: Signal::new(0),
            undo_on: Signal::new(false),
            redo_on: Signal::new(false),
            status: Signal::new(String::new()),
            title: Signal::new("Untitled".into()),
            zoom_pct: Signal::new(100.0),
            snap_on: Signal::new(true),
            // every panel mounts in place (its text fields' clips are cut there) and is hidden by `init`
            p_none: Signal::new((0.0, 0.0)),
            p_one: Signal::new((0.0, 0.0)),
            p_rect: Signal::new((0.0, 0.0)),
            p_ell: Signal::new((0.0, 0.0)),
            p_many: Signal::new((0.0, 0.0)),
            count_text: Signal::new("0 shapes".into()),
            many_text: Signal::new(String::new()),
            f_name: Signal::new(String::new()),
            f_x: Signal::new(String::new()),
            f_y: Signal::new(String::new()),
            f_w: Signal::new(String::new()),
            f_h: Signal::new(String::new()),
            f_r: Signal::new(String::new()),
            f_fill: Signal::new(String::new()),
            f_sw: Signal::new(String::new()),
            fill_sel: Signal::new(u8::MAX),
            stroke_sel: Signal::new(u8::MAX),
            shapes: Vec::new(),
            names: Vec::new(),
            selected: Vec::new(),
            tool: Tool::Select,
            view: Viewport { x: 80.0, y: 60.0, zoom: 1.0 },
            snapping: true,
            guides: Vec::new(),
            marquee: None,
            file: None,
            dirty: false,
            next_id: 1,
            history: History::default(),
            gesture: None,
            space: false,
            cap,
            paint: Paint { zoom: f32::NAN, ..Default::default() },
        };
        s.view_changed();
        s
    }

    // ------------------------------------------------------------------ the view's derived values

    /// The view moved or zoomed: the drawing's space, the grid, the zoom text; what to repaint.
    fn view_changed(&mut self) {
        let s = SCALE;
        let v = self.view;
        // the drawing's space: the view's origin to the device pixel; the rest goes into the instances
        let (px, py) = ((v.x * s).round(), (v.y * s).round());
        self.pan.set((px / s, py / s));
        let frac = (v.x * s - px, v.y * s - py);
        if frac != self.paint.frac || v.zoom != self.paint.zoom {
            self.paint.all = true;
        }
        // the grid: dots at the world's multiples of the step, the step at least `MIN_STEP` on screen; the
        // dots are a function of the step alone, their space follows the view modulo the step
        let mut step = GRID_STEP * v.zoom;
        while step < MIN_STEP {
            step *= 2.0;
        }
        let (gx, gy) = (v.x.rem_euclid(step) - step, v.y.rem_euclid(step) - step);
        let (qx, qy) = ((gx * s).round(), (gy * s).round());
        self.grid_at.set((qx / s, qy / s));
        let cols = (((BW + step) / step).ceil() as u32 + 1).min(GRID_COLS as u32);
        let rows = (((BH + step) / step).ceil() as u32 + 1).min((GRID_ROWS_A + GRID_ROWS_B) as u32);
        let grid = Grid { step, cols, rows, fx: (gx * s - qx) / s, fy: (gy * s - qy) / s };
        if grid != self.grid {
            self.grid = grid;
            self.paint.grid_dirty = true;
        }
        self.zoom_pct.set((v.zoom * 100.0).round());
        if !self.selected.is_empty() || !self.guides.is_empty() || self.marquee.is_some() {
            self.paint.marks_dirty = true;
        }
    }

    fn index_of(&self, id: u32) -> Option<usize> {
        self.shapes.iter().position(|s| s.id == id)
    }
    fn is_selected(&self, id: u32) -> bool {
        self.selected.contains(&id)
    }

    fn set_selected(&mut self, ids: Vec<u32>) {
        if ids != self.selected {
            self.selected = ids;
            self.paint.marks_dirty = true;
            self.inspect();
        }
    }

    fn shapes_changed(&mut self, which: Option<usize>) {
        match which {
            Some(i) => self.paint.some.push(i as u32),
            None => self.paint.all = true,
        }
        if !self.selected.is_empty() {
            self.paint.marks_dirty = true;
        }
    }

    fn publish_history(&mut self) {
        self.undo_on.set(!self.history.past.is_empty());
        self.redo_on.set(!self.history.future.is_empty());
    }
    fn set_dirty(&mut self, d: bool) {
        self.dirty = d;
        self.update_title();
    }
    fn update_title(&mut self) {
        let name = self.file.clone().unwrap_or_else(|| "Untitled".into());
        self.title.set(if self.dirty { format!("{name} •") } else { name });
    }

    /// The inspector: which panel shows (`ui.presence` on the selection) and the selected shape's fields.
    fn inspect(&mut self) {
        let n = self.selected.len();
        let one = if n == 1 { self.index_of(self.selected[0]) } else { None };
        let (none, onep, rect, ell, many) = match (n, one.map(|i| self.shapes[i].kind)) {
            (0, _) => (true, false, false, false, false),
            (1, Some(Kind::Rect)) => (false, true, true, false, false),
            (1, Some(Kind::Ellipse)) => (false, true, false, true, false),
            _ => (false, false, false, false, true),
        };
        let at = |on: bool| if on { (0.0, 0.0) } else { (AWAY, 0.0) };
        self.p_none.set(at(none));
        self.p_one.set(at(onep));
        self.p_rect.set(at(rect));
        self.p_ell.set(at(ell));
        self.p_many.set(at(many));
        let k = self.shapes.len();
        self.count_text.set(if k == 1 { "1 shape".into() } else { format!("{k} shapes") });
        if many {
            self.many_text.set(format!("{n} shapes"));
        }
        if let Some(i) = one {
            let s = self.shapes[i];
            self.f_name.set(self.names[s.name as usize].clone());
            self.f_x.set(shown(s.x));
            self.f_y.set(shown(s.y));
            self.f_w.set(shown(s.w));
            self.f_h.set(shown(s.h));
            self.f_r.set(shown(s.radius));
            self.f_fill.set(color_hex(s.fill));
            self.f_sw.set(shown(s.stroke_width));
            self.fill_sel.set(FILLS.iter().position(|f| hex(f) == s.fill).map_or(u8::MAX, |p| p as u8));
            self.stroke_sel.set(STROKES.iter().position(|f| hex(f) == s.stroke).map_or(u8::MAX, |p| p as u8));
        }
    }

    /// The panels' places after the mount (all mount in place: see `new`).
    pub fn init(&mut self) {
        self.inspect();
        self.paint.marks_dirty = true;
    }

    // ------------------------------------------------------------------ actions (`actions.luau`)

    pub fn new_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn add(&mut self, kind: Kind, r: Rect) -> u32 {
        let count = self.shapes.len() + 1;
        let id = self.new_id();
        self.names.push(format!("{} {count}", if kind == Kind::Rect { "Rectangle" } else { "Ellipse" }));
        let name = (self.names.len() - 1) as u32;
        assert!(self.shapes.len() < self.cap, "the board holds at most {} shapes", self.cap);
        self.shapes.push(Shape { id, kind, name, x: r.x, y: r.y, w: r.w, h: r.h, fill: hex(FILLS[(count - 1) % FILLS.len()]), stroke: CLEAR, stroke_width: 0.0, radius: if kind == Kind::Rect { 6.0 } else { 0.0 } });
        self.shapes_changed(Some(self.shapes.len() - 1));
        self.inspect();
        id
    }

    pub fn delete_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        self.history.record(&self.shapes);
        let sel = std::mem::take(&mut self.selected);
        self.shapes.retain(|s| !sel.contains(&s.id));
        self.shapes_changed(None);
        self.paint.marks_dirty = true;
        self.set_dirty(true);
        self.publish_history();
        self.inspect();
    }

    pub fn duplicate(&mut self) {
        let chosen: Vec<Shape> = self.shapes.iter().filter(|s| self.is_selected(s.id)).copied().collect();
        if chosen.is_empty() || self.shapes.len() + chosen.len() > self.cap {
            return;
        }
        self.history.record(&self.shapes);
        let mut copies = Vec::new();
        for s in chosen {
            let id = self.new_id();
            self.names.push(format!("{} copy", self.names[s.name as usize]));
            let name = (self.names.len() - 1) as u32;
            self.shapes.push(Shape { id, name, x: s.x + 16.0, y: s.y + 16.0, ..s });
            self.shapes_changed(Some(self.shapes.len() - 1));
            copies.push(id);
        }
        self.set_dirty(true);
        self.publish_history();
        self.set_selected(copies);
        self.inspect();
    }

    /// Move the selection by (dx, dy) world points, as one change.
    pub fn nudge(&mut self, dx: f32, dy: f32) {
        if self.selected.is_empty() {
            return;
        }
        self.history.record(&self.shapes);
        for i in 0..self.shapes.len() {
            if self.is_selected(self.shapes[i].id) {
                self.shapes[i].x += dx;
                self.shapes[i].y += dy;
                self.shapes_changed(Some(i));
            }
        }
        self.set_dirty(true);
        self.publish_history();
        self.inspect();
    }

    pub fn select_all(&mut self) {
        let ids = self.shapes.iter().map(|s| s.id).collect();
        self.set_selected(ids);
    }

    /// To the top (`front`) or the bottom of the stack.
    pub fn arrange(&mut self, front: bool) {
        if self.selected.is_empty() {
            return;
        }
        self.history.record(&self.shapes);
        let (moved, rest): (Vec<Shape>, Vec<Shape>) = self.shapes.iter().partition(|s| self.is_selected(s.id));
        self.shapes = if front { [rest, moved].concat() } else { [moved, rest].concat() };
        self.shapes_changed(None);
        self.set_dirty(true);
        self.publish_history();
    }

    pub fn undo(&mut self) {
        if let Some(last) = self.history.past.pop() {
            self.history.future.push(std::mem::replace(&mut self.shapes, last));
            self.after_history();
        }
    }
    pub fn redo(&mut self) {
        if let Some(next) = self.history.future.pop() {
            self.history.past.push(std::mem::replace(&mut self.shapes, next));
            self.after_history();
        }
    }
    fn after_history(&mut self) {
        let kept: Vec<u32> = self.selected.iter().copied().filter(|&id| self.index_of(id).is_some()).collect();
        self.selected.clear();
        self.set_selected(kept);
        self.shapes_changed(None);
        self.set_dirty(true);
        self.publish_history();
        self.inspect();
    }

    /// Zoom by `factor` about the board point (sx, sy): what is under it stays under it.
    pub fn zoom(&mut self, factor: f32, sx: f32, sy: f32) {
        let v = self.view;
        let zoom = (v.zoom * factor).clamp(0.1, 8.0);
        let (wx, wy) = v.to_world(sx, sy);
        self.view = Viewport { x: sx - wx * zoom, y: sy - wy * zoom, zoom };
        self.view_changed();
    }
    pub fn zoom_middle(&mut self, factor: f32) {
        self.zoom(factor, BW / 2.0, BH / 2.0);
    }
    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.view.x += dx;
        self.view.y += dy;
        self.view_changed();
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.tool = t;
        self.tool_sig.set(t as u8);
    }

    /// The drawing read from JSON (`actions.load`); false if it is not one.
    pub fn load(&mut self, text: &str) -> bool {
        let Some(doc) = parse_json(text) else {
            self.status.set("Not a Sketch drawing".into());
            return false;
        };
        let Some(Json::Arr(list)) = doc.get("shapes") else {
            self.status.set("Not a Sketch drawing".into());
            return false;
        };
        let mut highest = 0;
        self.shapes.clear();
        self.names.clear();
        for j in list {
            if self.shapes.len() == self.cap {
                break;
            }
            let id: u32 = j.get("id").and_then(Json::str).and_then(|s| s.strip_prefix('s')).and_then(|n| n.parse().ok()).unwrap_or(0);
            highest = highest.max(id);
            let num = |k: &str| j.get(k).and_then(Json::num).unwrap_or(0.0) as f32;
            self.names.push(j.get("name").and_then(Json::str).unwrap_or("").to_owned());
            self.shapes.push(Shape {
                id,
                kind: if j.get("kind").and_then(Json::str) == Some("ellipse") { Kind::Ellipse } else { Kind::Rect },
                name: (self.names.len() - 1) as u32,
                x: num("x"),
                y: num("y"),
                w: num("w"),
                h: num("h"),
                fill: hex(j.get("fill").and_then(Json::str).unwrap_or("#000000")),
                stroke: hex(j.get("stroke").and_then(Json::str).unwrap_or("#00000000")),
                stroke_width: num("stroke_width"),
                radius: num("radius"),
            });
        }
        self.next_id = highest + 1;
        self.selected.clear();
        self.history.reset();
        self.publish_history();
        self.shapes_changed(None);
        self.paint.marks_dirty = true;
        self.dirty = false;
        true
    }

    /// `actions.open` of the file at `path` (the system's dialog picked it).
    pub fn open(&mut self, path: &std::path::Path) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read_to_string(path) {
            Ok(text) => {
                if self.load(&text) {
                    self.file = Some(name.clone());
                    self.status.set(format!("Opened {name}"));
                }
            }
            Err(_) => {
                self.status.set(format!("Couldn't open {name}"));
            }
        }
        self.update_title();
        self.inspect();
    }

    // ------------------------------------------------------------------ the board's gestures (`board.luau`)

    fn shape_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.shapes.len()).rev().find(|&i| contains(&self.shapes[i], x, y))
    }

    fn handle_at(&self, sx: f32, sy: f32) -> Option<(model::Handle, usize)> {
        if self.selected.len() != 1 {
            return None;
        }
        let i = self.index_of(self.selected[0])?;
        let s = self.shapes[i];
        HANDLES.iter().copied().find_map(|h| {
            let (hx, hy) = handle_at(s.x, s.y, s.w, s.h, h);
            let (x, y) = self.view.to_screen(hx, hy);
            ((sx - x).abs() <= HANDLE && (sy - y).abs() <= HANDLE).then_some((h, i))
        })
    }

    /// The lines the shapes not in `skip` offer: found once a gesture.
    fn lines(&mut self, skip: &[u32]) -> (Vec<f32>, Vec<f32>) {
        if self.gesture.as_ref().is_some_and(|g| g.lines.is_none()) {
            let others = || self.shapes.iter().filter(|s| !skip.contains(&s.id));
            let found = (lines(others(), true), lines(others(), false));
            self.gesture.as_mut().unwrap().lines = Some(found);
        }
        self.gesture.as_ref().and_then(|g| g.lines.clone()).unwrap_or_default()
    }

    fn snap_point(&mut self, x: f32, y: f32, skip: &[u32]) -> (f32, f32, Vec<Guide>) {
        if !self.snapping {
            return (x, y, Vec::new());
        }
        let zoom = self.view.zoom;
        let (xs, ys) = self.lines(skip);
        let (dx, gx) = shift(&[x], &xs, PULL / zoom, true);
        let (dy, gy) = shift(&[y], &ys, PULL / zoom, true);
        let mut guides = Vec::new();
        if let Some(l) = gx {
            guides.push(Guide::X(l));
        }
        if let Some(l) = gy {
            guides.push(Guide::Y(l));
        }
        (x + dx, y + dy, guides)
    }

    fn set_guides(&mut self, g: Vec<Guide>) {
        if g != self.guides {
            self.guides = g;
            self.paint.marks_dirty = true;
        }
    }

    /// `press`: the board's pointer went down at (window) `e.x, e.y`.
    pub fn press(&mut self, e: &Ev) {
        let (sx, sy) = (e.x - BX, e.y - BY);
        let view = self.view;
        let (wx, wy) = view.to_world(sx, sy);
        self.gesture = Some(Gesture { kind: GestureKind::Pan, sx, sy, wx, wy, view, moving: Vec::new(), bounds: None, handle: None, index: None, kept: Vec::new(), lines: None });
        if self.space {
            return;
        }
        if self.tool != Tool::Select {
            if self.shapes.len() >= self.cap {
                self.gesture = None;
                return;
            }
            self.history.begin(&self.shapes);
            let (x, y, guides) = self.snap_point(wx, wy, &[]);
            let kind = if self.tool == Tool::Rect { Kind::Rect } else { Kind::Ellipse };
            let id = self.add(kind, Rect { x, y, w: 1.0, h: 1.0 });
            let index = self.shapes.len() - 1;
            if let Some(g) = self.gesture.as_mut() {
                (g.kind, g.wx, g.wy, g.index) = (GestureKind::Draw, x, y, Some(index));
            }
            self.set_selected(vec![id]);
            self.set_guides(guides);
            return;
        }
        if let Some((h, i)) = self.handle_at(sx, sy) {
            self.history.begin(&self.shapes);
            let s = self.shapes[i];
            if let Some(g) = self.gesture.as_mut() {
                (g.kind, g.handle, g.index, g.bounds) = (GestureKind::Resize, Some(h), Some(i), Some(Rect { x: s.x, y: s.y, w: s.w, h: s.h }));
            }
            return;
        }
        if let Some(hit) = self.shape_at(wx, wy) {
            let id = self.shapes[hit].id;
            if e.mods.shift {
                // shift-click adds or takes away, and moves nothing
                let mut ids: Vec<u32> = self.selected.iter().copied().filter(|&x| x != id).collect();
                if !self.is_selected(id) {
                    ids.push(id);
                }
                self.set_selected(ids);
                self.gesture = None;
                return;
            }
            if !self.is_selected(id) {
                self.set_selected(vec![id]);
            }
            self.history.begin(&self.shapes);
            let moving: Vec<(usize, f32, f32)> = self.shapes.iter().enumerate().filter(|(_, s)| self.is_selected(s.id)).map(|(i, s)| (i, s.x, s.y)).collect();
            let b = bounds(self.shapes.iter().filter(|s| self.is_selected(s.id)));
            if let Some(g) = self.gesture.as_mut() {
                (g.kind, g.moving, g.bounds) = (GestureKind::Move, moving, b);
            }
            return;
        }
        let kept = if e.mods.shift { self.selected.clone() } else { Vec::new() };
        if !e.mods.shift {
            self.set_selected(Vec::new());
        }
        if let Some(g) = self.gesture.as_mut() {
            (g.kind, g.kept) = (GestureKind::Marquee, kept);
        }
    }

    /// `move` while pressed: the gesture follows the pointer.
    pub fn drag(&mut self, e: &Ev) {
        let Some(g) = self.gesture.as_ref() else { return };
        let (lx, ly) = (e.x - BX, e.y - BY);
        let view = self.view;
        let (wx, wy) = view.to_world(lx, ly);
        match g.kind {
            GestureKind::Pan => {
                let (gv, gsx, gsy) = (g.view, g.sx, g.sy);
                self.view = Viewport { x: gv.x + lx - gsx, y: gv.y + ly - gsy, zoom: view.zoom };
                self.view_changed();
            }
            GestureKind::Move => {
                let Some(b) = g.bounds else { return };
                let (mut dx, mut dy) = (wx - g.wx, wy - g.wy);
                let mut guides = Vec::new();
                if self.snapping {
                    let sel = self.selected.clone();
                    let (xs, ys) = self.lines(&sel);
                    let within = PULL / view.zoom;
                    let (ax, gx) = shift(&[b.x + dx, b.x + dx + b.w / 2.0, b.x + dx + b.w], &xs, within, true);
                    let (ay, gy) = shift(&[b.y + dy, b.y + dy + b.h / 2.0, b.y + dy + b.h], &ys, within, true);
                    dx += ax;
                    dy += ay;
                    if let Some(l) = gx {
                        guides.push(Guide::X(l));
                    }
                    if let Some(l) = gy {
                        guides.push(Guide::Y(l));
                    }
                }
                let moving = self.gesture.as_mut().map(|g| std::mem::take(&mut g.moving)).unwrap_or_default();
                let mut moved = false;
                for &(i, x, y) in &moving {
                    let (nx, ny) = (x + dx, y + dy);
                    if self.shapes[i].x != nx || self.shapes[i].y != ny {
                        self.shapes[i].x = nx;
                        self.shapes[i].y = ny;
                        self.shapes_changed(Some(i));
                        moved = true;
                    }
                }
                if let Some(g) = self.gesture.as_mut() {
                    g.moving = moving;
                }
                self.set_guides(guides);
                if moved && self.selected.len() == 1 {
                    if let Some(i) = self.index_of(self.selected[0]) {
                        let s = self.shapes[i];
                        self.f_x.set(shown(s.x));
                        self.f_y.set(shown(s.y));
                    }
                }
            }
            GestureKind::Resize => {
                let (Some(b), Some(h), Some(i)) = (g.bounds, g.handle, g.index) else { return };
                let id = self.shapes[i].id;
                let (x, y, guides) = self.snap_point(wx, wy, &[id]);
                let r = resize(b, h, x, y);
                let s = &mut self.shapes[i];
                (s.x, s.y, s.w, s.h) = (r.x, r.y, r.w, r.h);
                self.shapes_changed(Some(i));
                self.set_guides(guides);
                self.inspect();
            }
            GestureKind::Draw => {
                let (Some(i), gwx, gwy) = (g.index, g.wx, g.wy) else { return };
                let id = self.shapes[i].id;
                let (mut x, mut y, guides) = self.snap_point(wx, wy, &[id]);
                if e.mods.shift {
                    let side = (x - gwx).abs().max((y - gwy).abs());
                    x = gwx + if x < gwx { -side } else { side };
                    y = gwy + if y < gwy { -side } else { side };
                }
                let r = span(gwx, gwy, x, y);
                let s = &mut self.shapes[i];
                (s.x, s.y, s.w, s.h) = (r.x, r.y, r.w.max(1.0), r.h.max(1.0));
                self.shapes_changed(Some(i));
                self.set_guides(guides);
                self.inspect();
            }
            GestureKind::Marquee => {
                let r = span(g.wx, g.wy, wx, wy);
                let mut ids = g.kept.clone();
                for s in &self.shapes {
                    if !ids.contains(&s.id) && overlaps(&r, s) {
                        ids.push(s.id);
                    }
                }
                self.marquee = Some(r);
                self.paint.marks_dirty = true;
                self.set_selected(ids);
            }
        }
    }

    /// `release`.
    pub fn release(&mut self, _: &Ev) {
        let Some(g) = self.gesture.take() else { return };
        if g.kind == GestureKind::Draw {
            if let Some(i) = g.index {
                // a click, not a drag: a shape of the usual size
                if self.shapes[i].w < 4.0 && self.shapes[i].h < 4.0 {
                    self.shapes[i].w = 120.0;
                    self.shapes[i].h = 80.0;
                    self.shapes_changed(Some(i));
                    self.inspect();
                }
            }
            self.set_tool(Tool::Select);
        }
        if matches!(g.kind, GestureKind::Draw | GestureKind::Move | GestureKind::Resize) && self.history.finish(&self.shapes) {
            self.set_dirty(true);
            self.publish_history();
        }
        self.set_guides(Vec::new());
        if self.marquee.take().is_some() {
            self.paint.marks_dirty = true;
        }
    }

    /// The window's keys while no field is edited (`app.luau`'s `key`).
    pub fn key(&mut self, e: &Ev) {
        let Some(k) = e.key else { return };
        let cmd = e.mods.cmd || e.mods.ctrl;
        match k {
            Key::Char(c) if cmd && c.eq_ignore_ascii_case(&'z') => {
                if e.mods.shift {
                    self.redo()
                } else {
                    self.undo()
                }
            }
            Key::Char(c) if cmd && c.eq_ignore_ascii_case(&'a') => self.select_all(),
            _ if cmd => {}
            Key::Backspace | Key::Delete => self.delete_selected(),
            Key::Escape => {
                self.set_selected(Vec::new());
                self.set_tool(Tool::Select);
            }
            Key::Char(' ') => self.space = true,
            Key::Char(c) => match c.to_ascii_lowercase() {
                'v' => self.set_tool(Tool::Select),
                'r' => self.set_tool(Tool::Rect),
                'o' => self.set_tool(Tool::Ellipse),
                _ => {}
            },
            Key::Left | Key::Right | Key::Up | Key::Down => {
                let step = if e.mods.shift { 10.0 } else { 1.0 };
                let (dx, dy) = match k {
                    Key::Left => (-step, 0.0),
                    Key::Right => (step, 0.0),
                    Key::Up => (0.0, -step),
                    _ => (0.0, step),
                };
                self.nudge(dx, dy);
            }
            _ => {}
        }
    }

    /// `on_wheel` at (window) (x, y): Cmd/Ctrl zooms about the pointer, else the view pans.
    pub fn wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32, zoom: bool) {
        if zoom {
            self.zoom((-dy * 0.01).exp(), x - BX, y - BY);
        } else {
            self.pan_by(-dx, -dy);
        }
    }
    /// `on_pinch`: `1 + scale` about the pointer.
    pub fn pinch(&mut self, x: f32, y: f32, scale: f32) {
        self.zoom(1.0 + scale, x - BX, y - BY);
    }

    // ------------------------------------------------------------------ the paint step

    /// Write the canvases (the board's drawing and its marks) for what changed since the last paint; run
    /// between a turn and its frame.
    pub fn paint(&mut self, rt: &mut Rt) {
        if self.paint.shapes.is_none() {
            self.paint.shapes = Some(Canvas::locate(rt, self.cap as u32));
            self.paint.marks = Some(Canvas::locate(rt, self.marks_cap() as u32));
            self.paint.grid = Some([Canvas::locate(rt, GRID_CAP_A as u32), Canvas::locate(rt, GRID_CAP_B as u32)]);
            self.paint.all = true;
            self.paint.marks_dirty = true;
            self.paint.grid_dirty = true;
        }
        if self.paint.grid_dirty {
            self.paint.grid_dirty = false;
            self.paint_grid(rt);
        }
        let s = SCALE;
        let z = self.view.zoom;
        let frac = ((self.view.x * s) - (self.view.x * s).round(), (self.view.y * s) - (self.view.y * s).round());
        let mut cv = self.paint.shapes.take().expect("located");
        if self.paint.all {
            // one pass over the column: every instance re-derived (the old and the new union damaged)
            if let Some(r) = rt.bough.visible_ink_handle(cv.handle) {
                rt.bough.damage_rect(r);
            }
            for i in 0..self.cap {
                let ins = match self.shapes.get(i) {
                    Some(sh) => shape_instance(sh, z, frac),
                    None => Instance { kind: KIND_SOLID, ..Default::default() },
                };
                *rt.inst_mut(cv.first + i as u32) = Instance { space: cv.space, ..ins };
            }
            cv.reset_ink(rt);
            cv.commit(rt);
            if let Some(r) = rt.bough.visible_ink_handle(cv.handle) {
                rt.bough.damage_rect(r);
            }
            self.paint.zoom = z;
            self.paint.frac = frac;
        } else if !self.paint.some.is_empty() {
            for &i in &self.paint.some {
                if let Some(sh) = self.shapes.get(i as usize) {
                    cv.set(rt, i, shape_instance(sh, z, frac));
                }
            }
            cv.commit(rt);
        }
        self.paint.all = false;
        self.paint.some.clear();
        self.paint.shapes = Some(cv);
        if self.paint.marks_dirty {
            self.paint.marks_dirty = false;
            self.paint_marks(rt);
        }
    }

    fn marks_cap(&self) -> usize {
        self.cap + 12
    }

    /// The grid's dots for the current step: the dots in view written (one pass), the rest of what was
    /// shown hidden; the old and the new extent damaged.
    fn paint_grid(&mut self, rt: &mut Rt) {
        let Some(mut runs) = self.paint.grid.take() else { return };
        let s = SCALE;
        let g = self.grid;
        let mut row0 = 0u32;
        for (k, cv) in runs.iter_mut().enumerate() {
            let rows_here = if k == 0 { GRID_ROWS_A } else { GRID_ROWS_B } as u32;
            let rows = g.rows.saturating_sub(row0).min(rows_here);
            if let Some(r) = rt.bough.visible_ink_handle(cv.handle) {
                rt.bough.damage_rect(r);
            }
            let dot = Instance { size: [s, s], kind: KIND_SOLID, color: BORDER.0, space: cv.space, ..Default::default() };
            let mut n = 0u32;
            for r in 0..rows {
                let y = (g.fy + (row0 + r) as f32 * g.step - 0.5) * s;
                for c in 0..g.cols {
                    let x = (g.fx + c as f32 * g.step - 0.5) * s;
                    *rt.inst_mut(cv.first + n) = Instance { pos: [x, y], ..dot };
                    n += 1;
                }
            }
            for i in n..self.paint.grid_used[k] {
                rt.inst_mut(cv.first + i).size = [0.0, 0.0];
            }
            self.paint.grid_used[k] = n;
            if n > 0 {
                let x0 = (g.fx - 0.5) * s;
                let y0 = (g.fy + row0 as f32 * g.step - 0.5) * s;
                cv.set_ink_local([x0, y0, x0 + ((g.cols - 1) as f32 * g.step + 1.0) * s, y0 + ((rows - 1) as f32 * g.step + 1.0) * s]);
            } else {
                cv.reset_ink(rt);
            }
            cv.commit(rt);
            if let Some(r) = rt.bough.visible_ink_handle(cv.handle) {
                rt.bough.damage_rect(r);
            }
            row0 += rows_here;
        }
        self.paint.grid = Some(runs);
    }

    /// The marks (`board.luau`'s `marks` canvas), rewritten whole.
    fn paint_marks(&mut self, rt: &mut Rt) {
        let mut cv = self.paint.marks.take().expect("located");
        let s = SCALE;
        let v = self.view;
        let (ox, oy) = (BX + v.x, BY + v.y);
        let mut n = 0u32;
        let mut put = |rt: &mut Rt, ins: Instance| {
            cv.set(rt, n, ins);
            n += 1;
        };
        let rect = |x: f32, y: f32, w: f32, h: f32| ([x * s, y * s], [w * s, h * s]);
        let mut one = None;
        let mut count = 0;
        for sh in &self.shapes {
            if !self.selected.contains(&sh.id) {
                continue;
            }
            let (x, y, w, h) = (ox + sh.x * v.zoom, oy + sh.y * v.zoom, sh.w * v.zoom, sh.h * v.zoom);
            // a 1.5 stroke on the outline, centred on it
            let r = match sh.kind {
                Kind::Rect => sh.radius * v.zoom + 0.75,
                Kind::Ellipse => (w.min(h) + 1.5) / 2.0,
            };
            let (pos, size) = rect(x - 0.75, y - 0.75, w + 1.5, h + 1.5);
            put(rt, Instance { pos, size, kind: KIND_BORDER, color: ACCENT.0, color2: 0, bw: 1.5 * s, radius: r * s, ..Default::default() });
            one = Some(*sh);
            count += 1;
        }
        if count == 1 {
            if let Some(sh) = one {
                for h in HANDLES {
                    let (hx, hy) = handle_at(sh.x, sh.y, sh.w, sh.h, h);
                    let (x, y) = (ox + hx * v.zoom, oy + hy * v.zoom);
                    let (pos, size) = rect(x - 4.75, y - 4.75, 9.5, 9.5);
                    put(rt, Instance { pos, size, kind: KIND_BORDER, color: ACCENT.0, color2: rgb(0xffffff).0, bw: 1.5 * s, radius: 1.75 * s, ..Default::default() });
                }
            }
        }
        for g in &self.guides {
            let (pos, size) = match *g {
                Guide::X(at) => rect(ox + at * v.zoom - 0.5, BY, 1.0, BH),
                Guide::Y(at) => rect(BX, oy + at * v.zoom - 0.5, BW, 1.0),
            };
            put(rt, Instance { pos, size, kind: KIND_SOLID, color: hex("#ff3b8b").0, ..Default::default() });
        }
        if let Some(m) = self.marquee {
            let (x, y, w, h) = (ox + m.x * v.zoom, oy + m.y * v.zoom, m.w * v.zoom, m.h * v.zoom);
            let (pos, size) = rect(x - 0.5, y - 0.5, w + 1.0, h + 1.0);
            put(rt, Instance { pos, size, kind: KIND_BORDER, color: ACCENT.0, color2: hex("#4c8dff22").0, bw: s, radius: 0.0, ..Default::default() });
        }
        for i in n..self.paint.marks_used.max(n) {
            cv.clear(rt, i);
        }
        self.paint.marks_used = n;
        cv.reset_ink(rt);
        cv.commit(rt);
        self.paint.marks = Some(cv);
    }

    // ------------------------------------------------------------------ the description

    fn tool_button(&self, k: u8, t: &'static str) -> impl Element<Self> + '_ {
        row(label(t).font(SIZE_MD, LINE_MD).color(TEXT_PRIMARY))
            .h(CONTROL_H)
            .px(12.0)
            .items(Align::Center)
            .bg(sig!(self.tool_sig).map(move |t: &u8| if *t == k { ACCENT } else { SURFACE_BASE }))
            .rounded(RADIUS_MD)
            .on_click(move |s: &mut Sketch| {
                s.set_tool(match k {
                    0 => Tool::Select,
                    1 => Tool::Rect,
                    _ => Tool::Ellipse,
                })
            })
    }

    /// A button at `opacity` 0.4 while it does nothing: a veil of the bar's colour over it.
    fn veiled(&self, t: &'static str, redo: bool) -> impl Element<Self> + '_ {
        fn veil_of(on: &bool) -> Color {
            if *on { CLEAR } else { alpha(SURFACE_RAISED, 153) }
        }
        let on = if redo { sig!(self.redo_on) } else { sig!(self.undo_on) };
        let mut veil = block(0.0, 0.0).bg(on.map(veil_of));
        {
            let sp = veil.spec_mut();
            sp.set_abs(Some((0.0, 0.0)));
            sp.w = Dim::Pct(1.0);
            sp.h = Dim::Pct(1.0);
        }
        let mut b = row((label(t).font(SIZE_MD, LINE_MD).color(TEXT_PRIMARY), veil)).h(CONTROL_H).px(12.0).items(Align::Center).bg(SURFACE_BASE).rounded(RADIUS_MD);
        b.spec_mut().relative = true;
        b.on_click(move |s: &mut Sketch| if redo { s.redo() } else { s.undo() })
    }

    fn toolbar(&self) -> impl Element<Self> + '_ {
        let snap_box = row(label("✓").font(12.0, 14.0).color(ACCENT_TEXT)).w(16.0).h(16.0).justify(Justify::Center).items(Align::Center).bg(sig!(self.snap_on).map(|on: &bool| if *on { ACCENT } else { INPUT_BG })).rounded(4.0);
        row((
            (row((self.tool_button(0, "Select"), self.tool_button(1, "Rectangle"), self.tool_button(2, "Ellipse"))).gap(4.0), block(12.0, 1.0), self.veiled("Undo", false), self.veiled("Redo", true)),
            block(0.0, 0.0).grow(1.0),
            label(sig!(self.status)).font(SIZE_SM, LINE_SM).color(TEXT_SECONDARY),
            label(sig!(self.title)).font(SIZE_MD, LINE_MD).color(TEXT_PRIMARY),
            block(0.0, 0.0).grow(1.0),
            (
                row((snap_box, label("Snap").font(SIZE_MD, LINE_MD).color(TEXT_PRIMARY))).gap(6.0).items(Align::Center).on_click(|s: &mut Sketch| {
                    s.snapping = !s.snapping;
                    s.snap_on.set(s.snapping);
                }),
                tbutton("−").on_click(|s: &mut Sketch| s.zoom_middle(1.0 / 1.25)),
                row(label(sig!(self.zoom_pct).num(|z: &f32| *z, NumFmt::new("", None, "%"))).font(SIZE_MD, LINE_MD).color(TEXT_PRIMARY)).w(48.0).justify(Justify::Center),
                tbutton("+").on_click(|s: &mut Sketch| s.zoom_middle(1.25)),
                tbutton("Open…"),
                tbutton("Save"),
            ),
        ))
        .w(WIDTH)
        .h(TOOLBAR_H)
        .px(12.0)
        .gap(8.0)
        .items(Align::Center)
        .bg(SURFACE_RAISED)
    }

    fn board(&self) -> impl Element<Self> + '_ {
        quad(BX, BY, BW, BH)
            .bg(SURFACE_SUNKEN)
            .children(no_hit(clip(
                0.0,
                0.0,
                BW,
                BH,
                (
                    offset(sig!(self.grid_at), (lanes(GRID_CAP_A, sig!(self.canvas_key), blank), lanes(GRID_CAP_B, sig!(self.canvas_key), blank))),
                    offset(sig!(self.pan), lanes(self.cap, sig!(self.canvas_key), blank)),
                    lanes(self.marks_cap(), sig!(self.canvas_key), blank),
                ),
            )))
            .on_press(|s: &mut Sketch, e: &Ev| s.press(e))
            .on_drag(|s: &mut Sketch, e: &Ev| if e.on { s.drag(e) } else { s.release(e) })
            .on_key(|s: &mut Sketch, e: &Ev| s.key(e))
    }

    fn none_panel(&self) -> impl Element<Self> + '_ {
        const TIPS: [(&str, &str); 7] = [
            ("V", "Select and move"),
            ("R", "Rectangle"),
            ("O", "Ellipse"),
            ("Space", "Hold and drag to\npan"),
            ("⌘ + wheel", "Zoom"),
            ("Shift", "Square shapes, add\nto selection"),
            ("⌘Z", "Undo"),
        ];
        let mut y = IY + LINE_MD + 12.0 + LINE_MD + 12.0;
        let tips: Vec<_> = TIPS
            .iter()
            .map(|&(k, d)| {
                let lines = d.lines().count() as f32;
                let row = (text(k).at(IX, y).size(SIZE_SM).line_height(LINE_SM).color(TEXT_PRIMARY), text(d).at(IX + 104.0, y).size(SIZE_SM).line_height(LINE_SM).color(TEXT_SECONDARY));
                y += lines * LINE_SM + 6.0;
                row
            })
            .collect();
        offset(
            sig!(self.p_none),
            (
                text("Nothing selected").at(IX, IY).size(SIZE_MD).line_height(LINE_MD).color(TEXT_PRIMARY),
                text(sig!(self.count_text)).at(IX, IY + LINE_MD + 12.0).size(SIZE_MD).line_height(LINE_MD).color(TEXT_SECONDARY),
                each(tips.into_iter()),
            ),
        )
    }

    fn one_panel(&self) -> impl Element<Self> + '_ {
        let y0 = IY + CONTROL_H + 16.0;
        let gy = y0 + LINE_SM + 8.0;
        let half = (IW - 8.0) / 2.0;
        offset(
            sig!(self.p_one),
            (
                field(sig!(self.f_name), IX, IY, IW),
                section_title("Position and size", y0),
                num_field("X", sig!(self.f_x), IX, gy, half),
                num_field("Y", sig!(self.f_y), IX + half + 8.0, gy, half),
                num_field("W", sig!(self.f_w), IX, gy + CONTROL_H + 8.0, half),
                num_field("H", sig!(self.f_h), IX + half + 8.0, gy + CONTROL_H + 8.0, half),
            ),
        )
    }

    /// The part of the one-shape panel below "Position and size": Corners (rects only), Fill, Stroke, and
    /// the buttons.
    fn one_bottom(&self, corners: bool) -> impl Element<Self> + '_ {
        let mut y = IY + CONTROL_H + 16.0 + LINE_SM + 8.0 + 2.0 * CONTROL_H + 8.0 + 16.0;
        let corners_part = corners.then(|| {
            let part = (section_title("Corners", y), num_field("R", sig!(self.f_r), IX, y + LINE_SM + 8.0, IW));
            y += LINE_SM + 8.0 + CONTROL_H + 16.0;
            part
        });
        let fill_y = y;
        let fills: Vec<_> = (0..FILLS.len()).map(|k| swatch(sig!(self.fill_sel), k as u8, hex(FILLS[k]), IX + k as f32 * 28.0, fill_y + LINE_SM + 8.0)).collect();
        let fill_field = field(sig!(self.f_fill), IX, fill_y + LINE_SM + 8.0 + 22.0 + 8.0, IW);
        let stroke_y = fill_y + LINE_SM + 8.0 + 22.0 + 8.0 + CONTROL_H + 16.0;
        let strokes: Vec<_> = (0..STROKES.len()).map(|k| swatch(sig!(self.stroke_sel), k as u8, hex(STROKES[k]), IX + k as f32 * 28.0, stroke_y + LINE_SM + 8.0)).collect();
        let sw_y = stroke_y + LINE_SM + 8.0 + 22.0 + 8.0;
        let by = sw_y + CONTROL_H + 16.0;
        let buttons = (abutton("Duplicate", IX, by, 96.0).on_click(|s: &mut Sketch| s.duplicate()), abutton("Delete", IX + 104.0, by, 76.0).on_click(|s: &mut Sketch| s.delete_selected()));
        offset(
            if corners { sig!(self.p_rect) } else { sig!(self.p_ell) },
            (
                corners_part,
                section_title("Fill", fill_y),
                each(fills.into_iter()),
                fill_field,
                section_title("Stroke", stroke_y),
                each(strokes.into_iter()),
                num_field("W", sig!(self.f_sw), IX, sw_y, IW),
                buttons,
            ),
        )
    }

    fn many_panel(&self) -> impl Element<Self> + '_ {
        let y = IY + LINE_MD + 12.0;
        offset(
            sig!(self.p_many),
            (
                text(sig!(self.many_text)).at(IX, IY).size(SIZE_MD).line_height(LINE_MD).color(TEXT_PRIMARY),
                abutton("Bring to Front", IX, y, 128.0).on_click(|s: &mut Sketch| s.arrange(true)),
                abutton("Send to Back", IX + 136.0, y, 116.0).on_click(|s: &mut Sketch| s.arrange(false)),
                abutton("Duplicate", IX, y + CONTROL_H + 8.0, 96.0).on_click(|s: &mut Sketch| s.duplicate()),
                abutton("Delete", IX + 104.0, y + CONTROL_H + 8.0, 76.0).on_click(|s: &mut Sketch| s.delete_selected()),
            ),
        )
    }
}

/// An input (`ui.input`) editing the string `sig`.
fn field(sig: Wired<'_, String, Sketch>, x: f32, y: f32, w: f32) -> impl Element<Sketch> + '_ {
    text_field(sig).rect(x, y, w, CONTROL_H).font(SIZE_MD).colors(INPUT_BG, BORDER, TEXT_PRIMARY).rounded(RADIUS_MD)
}
/// `number(id, key, label)`: the label, then the input.
fn num_field<'a>(l: &'static str, sig: Wired<'a, String, Sketch>, x: f32, y: f32, w: f32) -> impl Element<Sketch> + 'a {
    (text(l).at(x, y + (CONTROL_H - LINE_SM) / 2.0).size(SIZE_SM).line_height(LINE_SM).color(TEXT_SECONDARY), field(sig, x + 22.0, y, w - 22.0))
}

/// A section's title (`section`: bold `sm` secondary text; bold is regular here: one face).
fn section_title<V: 'static>(t: &'static str, y: f32) -> impl Element<V> {
    text(t).at(IX, y).size(SIZE_SM).line_height(LINE_SM).color(TEXT_SECONDARY)
}

/// A colour swatch (`swatches`): a 22 pt round button, its ring the accent while it is the shape's colour.
fn swatch(sel: Wired<'_, u8, Sketch>, k: u8, c: Color, x: f32, y: f32) -> impl Element<Sketch> + '_ {
    quad(x, y, 22.0, 22.0).bg(c).rounded(11.0).border(2.0, BORDER).paint_with(sel, move |s: &u8| (if *s == k { ACCENT } else { BORDER }, c))
}

/// A toolbar button (`ui.button { label }`), laid out.
fn tbutton(t: &'static str) -> bough::layout::LBox<Color, bough::layout::Para<&'static str>> {
    row(label(t).font(SIZE_MD, LINE_MD).color(TEXT_PRIMARY)).h(CONTROL_H).px(12.0).items(Align::Center).bg(SURFACE_BASE).rounded(RADIUS_MD)
}

/// A button at a place in the inspector.
fn abutton(t: &'static str, x: f32, y: f32, w: f32) -> bough::elem::Quad<Color, f32, (f32, f32), bough::elem::Text<&'static str>> {
    quad(x, y, w, CONTROL_H).bg(SURFACE_BASE).rounded(RADIUS_MD).children(text(t).at(12.0, (CONTROL_H - LINE_MD) / 2.0).size(SIZE_MD).line_height(LINE_MD).color(TEXT_PRIMARY))
}

/// `#rrggbb` (or `#rrggbbaa` when not opaque).
fn color_hex(c: Color) -> String {
    let [r, g, b, a] = c.0.to_le_bytes();
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

/// A shape's instance at zoom `z` (the drawing's space adds the view's whole pixels; `frac` is the rest, in
/// device px): its fill and centred stroke as one bordered box, the corner radius (rect) or half the box
/// (ellipse: exact for a circle; a pill for an ellipse that is not one, bough's quad kinds having no
/// ellipse).
#[inline]
pub fn shape_instance(sh: &Shape, z: f32, frac: (f32, f32)) -> Instance {
    let s = SCALE;
    let stroked = sh.stroke_width > 0.0 && sh.stroke.0 >> 24 != 0;
    let sw = if stroked { sh.stroke_width * z } else { 0.0 };
    let h2 = sw / 2.0;
    let (x, y, w, h) = (sh.x * z - h2, sh.y * z - h2, sh.w * z + sw, sh.h * z + sw);
    let r = match sh.kind {
        Kind::Rect if sh.radius > 0.0 => (sh.radius * z + h2).min(w.min(h) / 2.0),
        Kind::Rect => 0.0,
        Kind::Ellipse => w.min(h) / 2.0,
    };
    let pos = [x * s + frac.0, y * s + frac.1];
    let size = [w * s, h * s];
    if stroked {
        Instance { pos, size, kind: KIND_BORDER, color: sh.stroke.0, color2: sh.fill.0, bw: sw * s, radius: r * s, ..Default::default() }
    } else {
        Instance { pos, size, kind: if r > 0.0 { KIND_RRECT } else { KIND_SOLID }, color: sh.fill.0, radius: r * s, ..Default::default() }
    }
}

impl View for Sketch {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            quad(0.0, 0.0, WIDTH, HEIGHT).bg(SURFACE_BASE),
            self.toolbar(),
            self.board(),
            quad(BW, BY, SPLIT_W, BH).bg(BORDER),
            quad(BW + SPLIT_W, BY, INSPECTOR_W, BH).bg(SURFACE_RAISED),
            (self.none_panel(), self.one_panel(), self.one_bottom(true), self.one_bottom(false), self.many_panel()),
        )
    }
}

// ====================================================================== mounting and driving

/// The window at scale 2; the instance column holds the grid's runs (2 x 4,096) and the two canvases.
pub fn config(font: Vec<u8>) -> Config {
    let mut c = Config::new(WIDTH, HEIGHT, SCALE, font).once();
    c.caps.instances = 16_384;
    c
}

/// Sketch mounted (`gpu`: drawn by the fused presenter; `ring`: surfaces, 0 the default), its panels placed
/// and its canvases painted.
pub fn mount(gpu: bool, ring: usize, cap: usize) -> App<Sketch> {
    mount_view(Sketch::new(cap), gpu, ring)
}

/// `v` mounted as `mount` does.
pub fn mount_view(v: Sketch, gpu: bool, ring: usize) -> App<Sketch> {
    let mut c = config(crate::font_sans());
    c.ring = ring;
    let mut app = App::new(v, c, gpu);
    app.turn(|s| s.init());
    paint(&mut app);
    app
}

/// The paint step: the canvases written for what the last turns changed.
pub fn paint(app: &mut App<Sketch>) {
    let rt = &mut app.rt;
    app.owner.paint(rt);
}

/// The board's record (the interactive record under its middle).
pub fn board_handle(app: &App<Sketch>) -> bough_core::Handle {
    crate::harness::hit_chain(app, BX + BW / 2.0, BY + BH / 2.0)[0]
}

/// zgx's `Scroll { x, y, delta_x, delta_y }`: hit-tested; the board's `on_wheel` runs if it is under (x, y).
pub fn wheel(app: &mut App<Sketch>, board: bough_core::Handle, x: f32, y: f32, dx: f32, dy: f32) -> bool {
    let r = crate::harness::route_to(app, x, y, board, |s| s.wheel(x, y, dx, dy, false));
    paint(app);
    r
}
/// zgx's `Pinch { x, y, delta }` (bough has no pinch event): hit-tested as a wheel; the board's `on_pinch`.
pub fn pinch(app: &mut App<Sketch>, board: bough_core::Handle, x: f32, y: f32, delta: f32) -> bool {
    let r = crate::harness::route_to(app, x, y, board, |s| s.pinch(x, y, delta));
    paint(app);
    r
}

/// zgx's harness: `SHAPES` shapes in a 25-column grid, 40 points apart, alternately rect and ellipse.
pub fn busy_json(shapes: usize) -> String {
    let list: Vec<String> = (0..shapes)
        .map(|i| {
            let (x, y) = ((i % 25) * 40, (i / 25) * 40);
            format!(
                r##"{{"id":"s{}","kind":"{}","name":"S{i}","x":{x},"y":{y},"w":32,"h":32,"fill":"#4c8dff","stroke":"#202124","stroke_width":1,"radius":4}}"##,
                i + 1,
                if i % 2 == 0 { "rect" } else { "ellipse" }
            )
        })
        .collect();
    format!(r#"{{"version":1,"shapes":[{}]}}"#, list.join(","))
}

/// The board's origin on the window at the starting view, plus (x, y) (zgx's `at`).
pub fn at(x: f32, y: f32) -> (f32, f32) {
    (BX + 80.0 + x, BY + 60.0 + y)
}

fn frames(app: &mut App<Sketch>, n: usize) {
    for _ in 0..n {
        app.frame();
    }
}

/// zgx's `apps.rs` `sketch`: the drawing opened, a shape dragged, the view panned, zoomed.
pub fn bench(render: bool, reps: usize) {
    use crate::harness::{finish, measure};
    let shapes = crate::env_usize("SHAPES", 500);
    let dir = std::env::temp_dir().join(format!("zgx-apps-bench-{:010}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("busy.json");
    std::fs::write(&path, busy_json(shapes)).unwrap();
    let mut app = mount(render, 0, shapes + 64);
    frames(&mut app, 3);
    app.turn(|s| s.open(&path));
    paint(&mut app);
    assert_eq!(*app.owner.title, "busy.json");
    frames(&mut app, 2);
    let board = board_handle(&app);
    let (x, y) = at(16.0, 16.0);
    app.pointer_down(x, y, Button::Primary);
    paint(&mut app);
    app.frame();
    measure(
        &mut app,
        "sketch: drag a shape",
        reps,
        |app, k| {
            let k = k as f32;
            app.pointer_move(x + k * 2.0, y + k);
            paint(app);
            app.frame();
        },
        finish,
    );
    app.pointer_up(x, y, Button::Primary);
    paint(&mut app);
    app.key_down(Key::Escape, Mods::default());
    paint(&mut app);
    app.frame();
    let (x, y) = at(400.0, 300.0);
    measure(
        &mut app,
        "sketch: pan",
        reps,
        |app, k| {
            let step = if (k / 10) % 2 == 0 { 12.0 } else { -12.0 };
            assert!(wheel(app, board, x, y, step, step / 2.0), "the board is under the wheel");
            app.frame();
        },
        finish,
    );
    measure(
        &mut app,
        "sketch: zoom",
        reps,
        |app, k| {
            let delta = if k % 2 == 0 { 0.05 } else { -0.05 };
            assert!(pinch(app, board, x, y, delta), "the board is under the pinch");
            app.frame();
        },
        finish,
    );
    let _ = std::fs::remove_dir_all(&dir);
}

impl Sketch {
    /// A new Sketch holding this one's state (drawing, selection, tool, view, file, history depth): the fresh
    /// mount the pixel gate compares a retained one with.
    pub fn fresh_like(&self) -> Sketch {
        let mut s = Sketch::new(self.cap);
        s.shapes = self.shapes.clone();
        s.names = self.names.clone();
        s.selected = self.selected.clone();
        s.set_tool(self.tool);
        s.view = self.view;
        s.snapping = self.snapping;
        s.snap_on.set(self.snapping);
        s.file = self.file.clone();
        s.next_id = self.next_id;
        s.history.past = self.history.past.clone();
        s.history.future = self.history.future.clone();
        s.publish_history();
        s.status.set((*self.status).clone());
        s.set_dirty(self.dirty);
        s.view_changed();
        s
    }
}
