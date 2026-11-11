//! Sketch's model: `lib/geometry.luau`, `lib/snap.luau`, `history.luau`, and the drawing's JSON.
//!
//! A shape is plain old data (its name is an index into a table of names), so the drawing is one column of
//! 48-byte records: an undo snapshot is a copy of the column, a comparison is a compare of it.
use bough::color::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rect,
    Ellipse,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// `s{id}`.
    pub id: u32,
    pub kind: Kind,
    /// Index into the drawing's names.
    pub name: u32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub fill: Color,
    pub stroke: Color,
    pub stroke_width: f32,
    pub radius: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where the world's origin is on the board, and the zoom (`state.view`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
}
pub const HANDLES: [Handle; 8] = [Handle::Nw, Handle::N, Handle::Ne, Handle::E, Handle::Se, Handle::S, Handle::Sw, Handle::W];

impl Viewport {
    #[inline]
    pub fn to_world(&self, sx: f32, sy: f32) -> (f32, f32) {
        ((sx - self.x) / self.zoom, (sy - self.y) / self.zoom)
    }
    #[inline]
    pub fn to_screen(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.zoom + self.x, y * self.zoom + self.y)
    }
}

/// The rectangle two corners span, whichever way they were dragged.
pub fn span(x1: f32, y1: f32, x2: f32, y2: f32) -> Rect {
    Rect { x: x1.min(x2), y: y1.min(y2), w: (x2 - x1).abs(), h: (y2 - y1).abs() }
}

pub fn bounds<'a>(shapes: impl Iterator<Item = &'a Shape>) -> Option<Rect> {
    let (mut x1, mut y1, mut x2, mut y2) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut any = false;
    for s in shapes {
        any = true;
        x1 = x1.min(s.x);
        y1 = y1.min(s.y);
        x2 = x2.max(s.x + s.w);
        y2 = y2.max(s.y + s.h);
    }
    any.then(|| Rect { x: x1, y: y1, w: x2 - x1, h: y2 - y1 })
}

pub fn contains(s: &Shape, x: f32, y: f32) -> bool {
    if s.kind == Kind::Ellipse {
        let (rx, ry) = (s.w / 2.0, s.h / 2.0);
        if rx <= 0.0 || ry <= 0.0 {
            return false;
        }
        let (dx, dy) = ((x - s.x - rx) / rx, (y - s.y - ry) / ry);
        return dx * dx + dy * dy <= 1.0;
    }
    x >= s.x && x <= s.x + s.w && y >= s.y && y <= s.y + s.h
}

pub fn overlaps(a: &Rect, b: &Shape) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// Where handle `h` of a rect is.
pub fn handle_at(x: f32, y: f32, w: f32, h: f32, hd: Handle) -> (f32, f32) {
    use Handle::*;
    let hx = match hd {
        Nw | W | Sw => x,
        N | S => x + w / 2.0,
        _ => x + w,
    };
    let hy = match hd {
        Nw | N | Ne => y,
        W | E => y + h / 2.0,
        _ => y + h,
    };
    (hx, hy)
}

/// `r` with handle `h` moved to (x, y), never inside out.
pub fn resize(r: Rect, hd: Handle, x: f32, y: f32) -> Rect {
    use Handle::*;
    let (mut x1, mut y1, mut x2, mut y2) = (r.x, r.y, r.x + r.w, r.y + r.h);
    match hd {
        Nw | W | Sw => x1 = x.min(x2 - 1.0),
        Ne | E | Se => x2 = x.max(x1 + 1.0),
        _ => {}
    }
    match hd {
        Nw | N | Ne => y1 = y.min(y2 - 1.0),
        Sw | S | Se => y2 = y.max(y1 + 1.0),
        _ => {}
    }
    Rect { x: x1, y: y1, w: x2 - x1, h: y2 - y1 }
}

// ============================================================================ snapping (`lib/snap.luau`)

pub const GRID: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Guide {
    X(f32),
    Y(f32),
}

/// The lines along one axis `others` offer: each one's start, middle and end, in order.
pub fn lines<'a>(others: impl Iterator<Item = &'a Shape>, x_axis: bool) -> Vec<f32> {
    let mut out = Vec::new();
    for r in others {
        let (start, size) = if x_axis { (r.x, r.w) } else { (r.y, r.h) };
        out.extend_from_slice(&[start, start + size / 2.0, start + size]);
    }
    out.sort_by(f32::total_cmp);
    out
}

/// How far to shift `edges` so the closest one lands on one of the ordered `lines` within `within`, and the
/// line; else onto the grid when `grid` (the first edge), and no line. Each edge looks at its two nearest
/// lines alone.
pub fn shift(edges: &[f32], lines: &[f32], within: f32, grid: bool) -> (f32, Option<f32>) {
    let (mut best, mut line) = (f32::INFINITY, None);
    for &e in edges {
        let i = lines.partition_point(|&l| l < e);
        for j in [i.wrapping_sub(1), i] {
            if let Some(&l) = lines.get(j) {
                let d = l - e;
                if d.abs() < best.abs() && d.abs() <= within {
                    best = d;
                    line = Some(l);
                }
            }
        }
    }
    if let Some(l) = line {
        return (best, Some(l));
    }
    if grid && !edges.is_empty() {
        let e = edges[0];
        return ((e / GRID + 0.5).floor() * GRID - e, None);
    }
    (0.0, None)
}

// ============================================================================ history (`history.luau`)

pub const HISTORY_LIMIT: usize = 200;

#[derive(Default)]
pub struct History {
    pub past: Vec<Vec<Shape>>,
    pub future: Vec<Vec<Shape>>,
    pub pending: Option<Vec<Shape>>,
}

impl History {
    pub fn record(&mut self, now: &[Shape]) {
        self.past.push(now.to_vec());
        if self.past.len() > HISTORY_LIMIT {
            self.past.remove(0);
        }
        self.future.clear();
    }
    pub fn begin(&mut self, now: &[Shape]) {
        self.pending = Some(now.to_vec());
    }
    /// Keep the shapes noted by `begin` if they changed; whether they did.
    pub fn finish(&mut self, now: &[Shape]) -> bool {
        match self.pending.take() {
            Some(before) if before != now => {
                self.past.push(before);
                if self.past.len() > HISTORY_LIMIT {
                    self.past.remove(0);
                }
                self.future.clear();
                true
            }
            _ => false,
        }
    }
    pub fn reset(&mut self) {
        self.past.clear();
        self.future.clear();
        self.pending = None;
    }
}

// ============================================================================ the drawing as JSON

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, k: &str) -> Option<&Json> {
        match self {
            Json::Obj(v) => v.iter().find(|(key, _)| key == k).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn num(&self) -> Option<f64> {
        if let Json::Num(n) = self { Some(*n) } else { None }
    }
    pub fn str(&self) -> Option<&str> {
        if let Json::Str(s) = self { Some(s) } else { None }
    }
}

/// A JSON document (`ui.json.decode`).
pub fn parse_json(text: &str) -> Option<Json> {
    let b = text.as_bytes();
    let mut i = 0;
    let v = value(b, &mut i)?;
    ws(b, &mut i);
    (i == b.len()).then_some(v)
}

fn ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && b[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

fn value(b: &[u8], i: &mut usize) -> Option<Json> {
    ws(b, i);
    match *b.get(*i)? {
        b'{' => {
            *i += 1;
            let mut out = Vec::new();
            ws(b, i);
            if b.get(*i) == Some(&b'}') {
                *i += 1;
                return Some(Json::Obj(out));
            }
            loop {
                ws(b, i);
                let Json::Str(k) = string(b, i)? else { return None };
                ws(b, i);
                (b.get(*i) == Some(&b':')).then_some(())?;
                *i += 1;
                out.push((k, value(b, i)?));
                ws(b, i);
                match b.get(*i)? {
                    b',' => *i += 1,
                    b'}' => {
                        *i += 1;
                        return Some(Json::Obj(out));
                    }
                    _ => return None,
                }
            }
        }
        b'[' => {
            *i += 1;
            let mut out = Vec::new();
            ws(b, i);
            if b.get(*i) == Some(&b']') {
                *i += 1;
                return Some(Json::Arr(out));
            }
            loop {
                out.push(value(b, i)?);
                ws(b, i);
                match b.get(*i)? {
                    b',' => *i += 1,
                    b']' => {
                        *i += 1;
                        return Some(Json::Arr(out));
                    }
                    _ => return None,
                }
            }
        }
        b'"' => string(b, i),
        b't' if b[*i..].starts_with(b"true") => {
            *i += 4;
            Some(Json::Bool(true))
        }
        b'f' if b[*i..].starts_with(b"false") => {
            *i += 5;
            Some(Json::Bool(false))
        }
        b'n' if b[*i..].starts_with(b"null") => {
            *i += 4;
            Some(Json::Null)
        }
        _ => {
            let s = *i;
            while *i < b.len() && matches!(b[*i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                *i += 1;
            }
            std::str::from_utf8(&b[s..*i]).ok()?.parse().ok().map(Json::Num)
        }
    }
}

fn string(b: &[u8], i: &mut usize) -> Option<Json> {
    (b.get(*i) == Some(&b'"')).then_some(())?;
    *i += 1;
    let mut out = Vec::new();
    while *i < b.len() {
        match b[*i] {
            b'"' => {
                *i += 1;
                return String::from_utf8(out).ok().map(Json::Str);
            }
            b'\\' => {
                *i += 1;
                let c = *b.get(*i)?;
                match c {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'u' => {
                        let h = std::str::from_utf8(b.get(*i + 1..*i + 5)?).ok()?;
                        let ch = char::from_u32(u32::from_str_radix(h, 16).ok()?)?;
                        out.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
                        *i += 4;
                    }
                    c => out.push(c),
                }
                *i += 1;
            }
            c => {
                out.push(c);
                *i += 1;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_shift_is_zgx_s() {
        let lines = [0.0, 16.0, 32.0, 40.0, 56.0, 72.0];
        assert_eq!(shift(&[2.0, 18.0, 34.0], &lines, 6.0, true), (-2.0, Some(0.0)));
        assert_eq!(shift(&[37.0], &lines, 6.0, true), (3.0, Some(40.0)));
        assert_eq!(shift(&[100.0], &lines, 6.0, true), (4.0, None));
        assert_eq!(shift(&[100.0], &lines, 6.0, false), (0.0, None));
    }

    #[test]
    fn json_round() {
        let j = parse_json(r##"{"version":1,"shapes":[{"id":"s1","x":-1.5e1,"fill":"#4c8dff","ok":true,"n":null}]}"##).unwrap();
        let s = &j.get("shapes").unwrap();
        let Json::Arr(v) = s else { panic!() };
        assert_eq!(v[0].get("x").unwrap().num(), Some(-15.0));
        assert_eq!(v[0].get("fill").unwrap().str(), Some("#4c8dff"));
    }
}
