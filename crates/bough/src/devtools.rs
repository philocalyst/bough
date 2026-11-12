//! The devtools server: an agent (or a person with `nc -U`) drives a running app over a unix domain socket,
//! one JSON request per line, one JSON reply per line (`docs/lanes/composition/DESIGN.md` §4.5).
//!
//! Off by default and free when off: nothing is spawned, installed or checked per turn unless a server is
//! started, by [`serve`] (a headless app: the server owns the loop) or, in a windowed app, by setting
//! `BOUGH_DEVTOOLS=/path/to.sock` before `Application::run` ([`serve_windowed`]: requests reach the main
//! thread through the main dispatch queue, so an idle window still wakes for them and nothing polls).
//!
//! Requests (`"op"` and its fields; every reply has `"ok"`, and `"error"` when it is false):
//!
//! | op | fields | does |
//! |---|---|---|
//! | `tree` | `full`? | the reflected tree as text (`Tree::dump`), and with `full` every node as JSON |
//! | `find` / `get` | `sel` | nodes matching a selector (`find.rs` syntax) / the one node |
//! | `owners`, `fields` | `owner`? | owners in the tree; an owner's fields (path, type, value, editable, reactive) |
//! | `read` | `path`, `owner`? | one field's value |
//! | `set` | `path`, `value`, `owner`? | write a field by path, in a turn (its wires, effects, re-description) |
//! | `click`, `hover` | `sel` | the action through the pointer entry points, at the element's centre |
//! | `type` | `sel`, `text` | focus by click, then text input |
//! | `key` | `key`, `mods`? | a key to the focused element (`"Enter"`, `"a"`, mods `["shift", "cmd"]`) |
//! | `scroll` | `sel`, `dy` | the wheel at the element's centre, `dy` points |
//! | `tick` | `dt` | the frame clock (animations, glides, blink) by `dt` seconds |
//! | `turns` | `turns` | apply raw turns (the session format below) |
//! | `frame` | | draw a frame (damage, tile list, GPU repair) |
//! | `record` | `on` | start (clearing) or stop recording the turns every request takes |
//! | `session` | | the recorded turns |
//! | `replay` | `turns`?, `png`? | a fresh app (the server's factory) given the turns (default: the recording); its tree and pixels are compared with this app's |
//! | `screenshot` | `path` | the retained frame (the surface on screen, no window needed) as a PNG |
//! | `quit` | | stop serving |
//!
//! A session is a list of [`Turn`]s, each one call of an entry point the platform host also calls, in JSON:
//! `{"t":"down","x":12.5,"y":40,"b":"primary"}`, `{"t":"tick","dt":0.016}`, `{"t":"set","owner":0,
//! "path":"count","value":"3"}`, `{"t":"frame"}`. Turns carry no clock and no randomness: the frame clock
//! advances only by `tick`, so a replay of the same turns on the same initial state draws the same pixels.
use crate::app::{App, View};
use crate::find::{Sel, Turn};
use crate::input::{Button, Key, Mods};
use crate::tree::{Node, States, Tree};

// =============================================================================== a small JSON

/// A JSON value (the protocol needs no more than this; no dependency).
#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn get(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(v) => v.iter().find(|e| e.0 == k).map(|e| &e.1),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            J::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn bool(&self) -> Option<bool> {
        match self {
            J::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn arr(&self) -> Option<&[J]> {
        match self {
            J::Arr(v) => Some(v),
            _ => None,
        }
    }
    fn s(v: impl Into<String>) -> J {
        J::Str(v.into())
    }
    fn obj(v: Vec<(&str, J)>) -> J {
        J::Obj(v.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    pub fn parse(s: &str) -> Result<J, String> {
        let mut p = Parser { b: s.as_bytes(), i: 0 };
        let v = p.value()?;
        p.ws();
        if p.i != p.b.len() {
            return Err(format!("trailing input at {}", p.i));
        }
        Ok(v)
    }

    pub fn write(&self, out: &mut String) {
        use std::fmt::Write;
        match self {
            J::Null => out.push_str("null"),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Num(n) if n.is_finite() => {
                let _ = write!(out, "{n}");
            }
            J::Num(_) => out.push_str("null"),
            J::Str(s) => {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => {
                            let _ = write!(out, "\\u{:04x}", c as u32);
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            J::Arr(v) => {
                out.push('[');
                for (k, x) in v.iter().enumerate() {
                    if k > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push(']');
            }
            J::Obj(v) => {
                out.push('{');
                for (k, (key, x)) in v.iter().enumerate() {
                    if k > 0 {
                        out.push(',');
                    }
                    J::Str(key.clone()).write(out);
                    out.push(':');
                    x.write(out);
                }
                out.push('}');
            }
        }
    }
}
impl std::fmt::Display for J {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();
        self.write(&mut s);
        f.write_str(&s)
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}
impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected {:?} at {}", c as char, self.i))
        }
    }
    fn lit(&mut self, w: &str, v: J) -> Result<J, String> {
        if self.b[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            Err(format!("bad literal at {}", self.i))
        }
    }
    fn value(&mut self) -> Result<J, String> {
        self.ws();
        match self.b.get(self.i).copied() {
            Some(b'{') => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(J::Obj(v));
                }
                loop {
                    self.ws();
                    let k = match self.value()? {
                        J::Str(s) => s,
                        _ => return Err(format!("object key at {}", self.i)),
                    };
                    self.eat(b':')?;
                    let x = self.value()?;
                    v.push((k, x));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(J::Obj(v));
                        }
                        _ => return Err(format!("expected ',' or '}}' at {}", self.i)),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(J::Arr(v));
                }
                loop {
                    v.push(self.value()?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(J::Arr(v));
                        }
                        _ => return Err(format!("expected ',' or ']' at {}", self.i)),
                    }
                }
            }
            Some(b'"') => {
                self.i += 1;
                let mut s = String::new();
                loop {
                    match self.b.get(self.i).copied() {
                        None => return Err("unterminated string".into()),
                        Some(b'"') => {
                            self.i += 1;
                            return Ok(J::Str(s));
                        }
                        Some(b'\\') => {
                            let e = self.b.get(self.i + 1).copied().ok_or("bad escape")?;
                            self.i += 2;
                            match e {
                                b'n' => s.push('\n'),
                                b't' => s.push('\t'),
                                b'r' => s.push('\r'),
                                b'b' => s.push('\u{8}'),
                                b'f' => s.push('\u{c}'),
                                b'u' => {
                                    let h = std::str::from_utf8(self.b.get(self.i..self.i + 4).ok_or("bad \\u")?).map_err(|_| "bad \\u")?;
                                    let c = u32::from_str_radix(h, 16).map_err(|_| "bad \\u")?;
                                    s.push(char::from_u32(c).unwrap_or('\u{fffd}'));
                                    self.i += 4;
                                }
                                c => s.push(c as char),
                            }
                        }
                        Some(_) => {
                            let rest = std::str::from_utf8(&self.b[self.i..]).map_err(|_| "invalid utf-8")?;
                            let c = rest.chars().next().unwrap();
                            s.push(c);
                            self.i += c.len_utf8();
                        }
                    }
                }
            }
            Some(b't') => self.lit("true", J::Bool(true)),
            Some(b'f') => self.lit("false", J::Bool(false)),
            Some(b'n') => self.lit("null", J::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => {
                let st = self.i;
                self.i += 1;
                while self.i < self.b.len() && matches!(self.b[self.i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') {
                    self.i += 1;
                }
                std::str::from_utf8(&self.b[st..self.i]).unwrap().parse().map(J::Num).map_err(|_| format!("bad number at {st}"))
            }
            _ => Err(format!("unexpected input at {}", self.i)),
        }
    }
}

// =============================================================================== the session format

fn button_name(b: Button) -> &'static str {
    match b {
        Button::Primary => "primary",
        Button::Secondary => "secondary",
        Button::Middle => "middle",
    }
}
fn button_of(s: &str) -> Option<Button> {
    Some(match s {
        "primary" => Button::Primary,
        "secondary" => Button::Secondary,
        "middle" => Button::Middle,
        _ => return None,
    })
}
const KEYS: [(Key, &str); 13] = [
    (Key::Tab, "Tab"),
    (Key::Backspace, "Backspace"),
    (Key::Delete, "Delete"),
    (Key::Enter, "Enter"),
    (Key::Escape, "Escape"),
    (Key::Left, "Left"),
    (Key::Right, "Right"),
    (Key::Up, "Up"),
    (Key::Down, "Down"),
    (Key::Home, "Home"),
    (Key::End, "End"),
    (Key::PageUp, "PageUp"),
    (Key::PageDown, "PageDown"),
];
fn key_name(k: Key) -> String {
    match k {
        Key::Char(c) => c.to_string(),
        k => KEYS.iter().find(|e| e.0 == k).map_or("?", |e| e.1).to_string(),
    }
}
fn key_of(s: &str) -> Option<Key> {
    let mut cs = s.chars();
    match (cs.next(), cs.next()) {
        (Some(c), None) => Some(Key::Char(c)),
        _ => KEYS.iter().find(|e| e.1 == s).map(|e| e.0),
    }
}
fn mods_json(m: Mods) -> J {
    J::Arr([(m.shift, "shift"), (m.ctrl, "ctrl"), (m.alt, "alt"), (m.cmd, "cmd")].into_iter().filter(|x| x.0).map(|x| J::s(x.1)).collect())
}
fn mods_of(j: Option<&J>) -> Mods {
    let mut m = Mods::default();
    for x in j.and_then(J::arr).unwrap_or(&[]) {
        match x.str() {
            Some("shift") => m.shift = true,
            Some("ctrl") => m.ctrl = true,
            Some("alt") => m.alt = true,
            Some("cmd") => m.cmd = true,
            _ => {}
        }
    }
    m
}

/// A turn as JSON (the session format).
pub fn turn_json(t: &Turn) -> J {
    let n = |v: f32| J::Num(v as f64);
    match t {
        Turn::Move { x, y } => J::obj(vec![("t", J::s("move")), ("x", n(*x)), ("y", n(*y))]),
        Turn::Down { x, y, button } => J::obj(vec![("t", J::s("down")), ("x", n(*x)), ("y", n(*y)), ("b", J::s(button_name(*button)))]),
        Turn::Up { x, y, button } => J::obj(vec![("t", J::s("up")), ("x", n(*x)), ("y", n(*y)), ("b", J::s(button_name(*button)))]),
        Turn::Leave => J::obj(vec![("t", J::s("leave"))]),
        Turn::Wheel { x, y, dy } => J::obj(vec![("t", J::s("wheel")), ("x", n(*x)), ("y", n(*y)), ("dy", n(*dy))]),
        Turn::Key { key, mods } => J::obj(vec![("t", J::s("key")), ("key", J::s(key_name(*key))), ("mods", mods_json(*mods))]),
        Turn::Text(s) => J::obj(vec![("t", J::s("text")), ("s", J::s(s.clone()))]),
        Turn::Tick(dt) => J::obj(vec![("t", J::s("tick")), ("dt", n(*dt))]),
        Turn::Set { owner, path, value } => J::obj(vec![("t", J::s("set")), ("owner", J::Num(*owner as f64)), ("path", J::s(path.clone())), ("value", J::s(value.clone()))]),
        Turn::Frame => J::obj(vec![("t", J::s("frame"))]),
    }
}

/// A turn from JSON. Positions round-trip exactly: an `f32` printed as `f64` parses back to the same `f32`.
pub fn turn_of(j: &J) -> Result<Turn, String> {
    let f = |k: &str| j.get(k).and_then(J::num).map(|v| v as f32).ok_or_else(|| format!("turn field {k}"));
    let b = || j.get("b").and_then(J::str).and_then(button_of).ok_or("turn field b");
    Ok(match j.get("t").and_then(J::str).ok_or("turn field t")? {
        "move" => Turn::Move { x: f("x")?, y: f("y")? },
        "down" => Turn::Down { x: f("x")?, y: f("y")?, button: b()? },
        "up" => Turn::Up { x: f("x")?, y: f("y")?, button: b()? },
        "leave" => Turn::Leave,
        "wheel" => Turn::Wheel { x: f("x")?, y: f("y")?, dy: f("dy")? },
        "key" => Turn::Key { key: j.get("key").and_then(J::str).and_then(key_of).ok_or("turn field key")?, mods: mods_of(j.get("mods")) },
        "text" => Turn::Text(j.get("s").and_then(J::str).ok_or("turn field s")?.to_string()),
        "tick" => Turn::Tick(f("dt")?),
        "set" => Turn::Set {
            owner: j.get("owner").and_then(J::num).unwrap_or(0.0) as usize,
            path: j.get("path").and_then(J::str).ok_or("turn field path")?.to_string(),
            value: j.get("value").and_then(J::str).ok_or("turn field value")?.to_string(),
        },
        "frame" => Turn::Frame,
        t => return Err(format!("unknown turn {t:?}")),
    })
}

/// A node as JSON.
pub fn node_json(t: &Tree, n: &Node) -> J {
    let mut v: Vec<(&str, J)> = vec![("pos", J::Num(n.pos as f64)), ("role", J::s(format!("{:?}", n.role))), ("name", J::s(n.name.clone())), ("elem", J::s(n.elem))];
    let f4 = |a: [f32; 4]| J::Arr(a.iter().map(|&x| J::Num(x as f64)).collect());
    if let Some(x) = &n.text {
        v.push(("text", J::s(x.clone())));
    }
    if let Some(x) = &n.value {
        v.push(("value", J::s(x.clone())));
    }
    if let Some(b) = n.bounds {
        v.push(("bounds", f4(b)));
    }
    if let Some(b) = n.visible {
        v.push(("visible", J::Arr(b.iter().map(|&x| J::Num(x as f64)).collect())));
    }
    v.push(("states", J::Arr(States::NAMES.iter().filter(|s| n.states.has(s.0)).map(|s| J::s(s.1)).collect())));
    if let Some((p, i)) = n.part {
        v.push(("part", J::Arr(vec![J::s(p), J::Num(i as f64)])));
    }
    if let Some(i) = n.id {
        v.push(("id", J::s(i)));
    }
    if let Some(c) = n.fill {
        v.push(("fill", J::s(format!("#{:08x}", c.0))));
    }
    if let Some(s) = n.source {
        v.push(("source", J::s(s.to_string())));
    }
    if let Some(e) = n.element {
        v.push(("element", J::s(e)));
    }
    if let Some(id) = n.foreign {
        v.push(("foreign", J::Num(id as f64)));
    }
    v.push(("owner", J::s(t.owner_path(n.pos))));
    if !n.routes.is_empty() {
        v.push(("routes", J::Arr(n.routes.iter().map(|r| J::obj(vec![("verb", J::s(format!("{:?}", r.0))), ("fn", r.1.map_or(J::Null, J::s))])).collect())));
    }
    if !n.binds.is_empty() {
        v.push(("binds", J::Arr(n.binds.iter().map(|b| J::obj(vec![("prop", J::s(b.prop)), ("path", b.path.clone().map_or(J::Null, J::Str)), ("ty", J::s(b.ty)), ("value", b.value.clone().map_or(J::Null, J::Str))])).collect())));
    }
    J::obj(v)
}

// =============================================================================== pixels

impl<V: View> App<V> {
    /// The retained frame (the surface the last frame showed) as straight-alpha RGBA, `(width, height,
    /// bytes)`; `None` without a GPU presenter. No window is needed: surfaces are IOSurfaces.
    pub fn screenshot(&mut self) -> Option<(u32, u32, Vec<u8>)> {
        #[cfg(target_os = "macos")]
        {
            self.gpu.as_ref()?;
            let (w, h) = (self.rt.win_w as u32, self.rt.win_h as u32);
            let mut px = self.presented_pixels();
            for p in px.chunks_exact_mut(4) {
                // BGRA premultiplied -> RGBA straight
                let (b, g, r, a) = (p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32);
                let un = |c: u32| if a == 0 { 0 } else { ((c * 255 + a / 2) / a).min(255) as u8 };
                p.copy_from_slice(&[un(r), un(g), un(b), a as u8]);
            }
            Some((w, h, px))
        }
        #[cfg(not(target_os = "macos"))]
        None
    }
}

/// Encode straight-alpha RGBA as PNG.
pub fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = ::png::Encoder::new(&mut out, w, h);
        e.set_color(::png::ColorType::Rgba);
        e.set_depth(::png::BitDepth::Eight);
        let mut wr = e.write_header().expect("png header");
        wr.write_image_data(rgba).expect("png data");
    }
    out
}

// =============================================================================== the server's state

/// One app's devtools state: the recording, and (for `replay`) how to make a fresh app.
pub struct Devtools<V: View> {
    pub recording: bool,
    pub turns: Vec<Turn>,
    make: Option<Box<dyn Fn() -> App<V>>>,
    /// Requests that change the app draw a frame afterwards (a server driving a window; a test turns it off
    /// to measure turns alone).
    pub frames: bool,
    pub quit: bool,
}

impl<V: View> Default for Devtools<V> {
    fn default() -> Self {
        Devtools { recording: false, turns: Vec::new(), make: None, frames: true, quit: false }
    }
}

fn err(e: impl std::fmt::Display) -> J {
    J::obj(vec![("ok", J::Bool(false)), ("error", J::s(e.to_string()))])
}
fn ok(mut v: Vec<(&str, J)>) -> J {
    v.insert(0, ("ok", J::Bool(true)));
    J::obj(v)
}

impl<V: View> Devtools<V> {
    pub fn new() -> Self {
        Self::default()
    }
    /// `replay` builds a fresh app with `make` (the same view and configuration the served app started from).
    pub fn with_factory(make: impl Fn() -> App<V> + 'static) -> Self {
        Devtools { make: Some(Box::new(make)), ..Self::default() }
    }

    /// Turns taken: recorded if recording, followed by a frame if this server draws.
    fn took(&mut self, app: &mut App<V>, mut turns: Vec<Turn>) -> J {
        if self.frames {
            app.frame();
            turns.push(Turn::Frame);
        }
        let n = turns.len();
        if self.recording {
            self.turns.extend(turns);
        }
        ok(vec![("turns", J::Num(n as f64))])
    }

    /// Handle one request line; the reply is one line of JSON.
    pub fn handle(&mut self, app: &mut App<V>, line: &str) -> String {
        let r = match J::parse(line) {
            Ok(j) => self.request(app, &j),
            Err(e) => err(format!("bad request: {e}")),
        };
        r.to_string()
    }

    fn request(&mut self, app: &mut App<V>, j: &J) -> J {
        let Some(op) = j.get("op").and_then(J::str) else { return err("no \"op\"") };
        let sel = || -> Result<Sel, J> { Sel::parse(j.get("sel").and_then(J::str).ok_or_else(|| err("no \"sel\""))?).map_err(err) };
        let owner = j.get("owner").and_then(J::num).unwrap_or(0.0) as usize;
        macro_rules! act {
            ($e:expr) => {
                match $e {
                    Ok(t) => self.took(app, t),
                    Err(e) => err(e),
                }
            };
        }
        match op {
            "ping" => ok(vec![("pong", J::Bool(true))]),
            "tree" => {
                let t = app.tree();
                let mut v = vec![("dump", J::s(t.dump())), ("lost", J::Num(t.lost as f64)), ("count", J::Num(t.nodes.len() as f64))];
                if j.get("full").and_then(J::bool) == Some(true) {
                    v.push(("nodes", J::Arr(t.nodes.iter().map(|n| node_json(&t, n)).collect())));
                }
                ok(v)
            }
            "find" => match sel() {
                Ok(s) => {
                    let t = app.tree();
                    let ns = t.find(s).into_iter().map(|n| node_json(&t, n)).collect();
                    ok(vec![("nodes", J::Arr(ns))])
                }
                Err(e) => e,
            },
            "get" => match sel() {
                Ok(s) => {
                    let t = app.tree();
                    match t.try_get(s) {
                        Ok(n) => ok(vec![("node", node_json(&t, n))]),
                        Err(e) => err(e),
                    }
                }
                Err(e) => e,
            },
            "owners" => {
                let t = app.tree();
                ok(vec![("owners", J::Arr(t.owners.iter().map(|o| J::obj(vec![("ty", J::s(o.ty)), ("kind", J::s(format!("{:?}", o.kind))), ("field", o.field.clone().map_or(J::Null, J::Str))])).collect()))])
            }
            "fields" => {
                let f = app.reflect_owner_fields(owner);
                ok(vec![(
                    "fields",
                    J::Arr(
                        f.iter()
                            .map(|f| J::obj(vec![("path", J::s(f.path.clone())), ("ty", J::s(f.type_name)), ("value", f.value.clone().map_or(J::Null, J::Str)), ("editable", J::Bool(f.editable)), ("reactive", J::Bool(f.reactive))]))
                            .collect(),
                    ),
                )])
            }
            "read" => {
                let Some(p) = j.get("path").and_then(J::str) else { return err("no \"path\"") };
                match app.reflect_owner_fields(owner).into_iter().find(|f| f.path == p) {
                    Some(f) => ok(vec![("value", f.value.map_or(J::Null, J::Str)), ("ty", J::s(f.type_name))]),
                    None => err(format!("no field {p:?}")),
                }
            }
            "set" => {
                let (Some(p), Some(v)) = (j.get("path").and_then(J::str), j.get("value").and_then(J::str)) else { return err("set needs \"path\" and \"value\"") };
                if !app.reflect_owner_fields(owner).iter().any(|f| f.path == p && f.editable) {
                    return err(format!("no editable field {p:?}"));
                }
                let t = Turn::Set { owner, path: p.to_string(), value: v.to_string() };
                app.apply(&t);
                self.took(app, vec![t])
            }
            "click" => match sel() {
                Ok(s) => act!(app.try_click(s)),
                Err(e) => e,
            },
            "hover" => match sel() {
                Ok(s) => act!(app.try_hover(s)),
                Err(e) => e,
            },
            "type" => match sel() {
                Ok(s) => act!(app.try_type(s, j.get("text").and_then(J::str).unwrap_or(""))),
                Err(e) => e,
            },
            "scroll" => match sel() {
                Ok(s) => act!(app.try_scroll(s, j.get("dy").and_then(J::num).unwrap_or(0.0) as f32)),
                Err(e) => e,
            },
            "key" => match j.get("key").and_then(J::str).and_then(key_of) {
                Some(k) => {
                    let t = app.press_key(k, mods_of(j.get("mods")));
                    self.took(app, t)
                }
                None => err("no or unknown \"key\""),
            },
            "tick" => {
                let t = Turn::Tick(j.get("dt").and_then(J::num).unwrap_or(1.0 / 60.0) as f32);
                app.apply(&t);
                self.took(app, vec![t])
            }
            "turns" => match j.get("turns").and_then(J::arr).map(|v| v.iter().map(turn_of).collect::<Result<Vec<_>, _>>()) {
                Some(Ok(ts)) => {
                    for t in &ts {
                        app.apply(t);
                    }
                    if self.recording {
                        self.turns.extend(ts.iter().cloned());
                    }
                    ok(vec![("turns", J::Num(ts.len() as f64))])
                }
                Some(Err(e)) => err(e),
                None => err("no \"turns\""),
            },
            "frame" => {
                let drew = app.frame();
                if self.recording {
                    self.turns.push(Turn::Frame);
                }
                ok(vec![("drew", J::Bool(drew))])
            }
            "record" => {
                let on = j.get("on").and_then(J::bool).unwrap_or(true);
                if on {
                    self.turns.clear();
                }
                self.recording = on;
                ok(vec![("recording", J::Bool(on))])
            }
            "session" => ok(vec![("turns", J::Arr(self.turns.iter().map(turn_json).collect()))]),
            "replay" => {
                let turns = match j.get("turns").and_then(J::arr) {
                    Some(v) => match v.iter().map(turn_of).collect::<Result<Vec<_>, _>>() {
                        Ok(t) => t,
                        Err(e) => return err(e),
                    },
                    None => self.turns.clone(),
                };
                let Some(make) = &self.make else { return err("this server has no factory to replay with") };
                let mut b = make();
                for t in &turns {
                    b.apply(t);
                }
                b.frame();
                app.frame();
                let same_tree = b.tree().dump() == app.tree().dump();
                let mut v = vec![("replayed", J::Num(turns.len() as f64)), ("tree_equal", J::Bool(same_tree))];
                if let (Some((w, h, pa)), Some((_, _, pb))) = (app.screenshot(), b.screenshot()) {
                    let diff = pa.chunks_exact(4).zip(pb.chunks_exact(4)).filter(|(x, y)| x != y).count();
                    v.push(("diff_pixels", J::Num(diff as f64)));
                    if let Some(path) = j.get("png").and_then(J::str) {
                        if let Err(e) = std::fs::write(path, png(w, h, &pb)) {
                            return err(e);
                        }
                    }
                }
                ok(v)
            }
            "screenshot" => {
                let Some(path) = j.get("path").and_then(J::str) else { return err("no \"path\"") };
                app.frame();
                match app.screenshot() {
                    Some((w, h, px)) => match std::fs::write(path, png(w, h, &px)) {
                        Ok(()) => ok(vec![("w", J::Num(w as f64)), ("h", J::Num(h as f64)), ("path", J::s(path))]),
                        Err(e) => err(e),
                    },
                    None => err("no GPU presenter: this app was made without one"),
                }
            }
            "quit" => {
                self.quit = true;
                ok(vec![])
            }
            op => err(format!("unknown op {op:?}")),
        }
    }
}

// =============================================================================== transports

/// Serve `app` on the unix socket at `path` until a client sends `quit` (one client at a time; the socket
/// file is replaced if it exists and removed at the end). The calling thread owns the app: a headless app
/// an agent drives.
pub fn serve<V: View>(app: &mut App<V>, dev: &mut Devtools<V>, path: &std::path::Path) -> std::io::Result<()> {
    use std::io::{BufRead, BufReader, Write};
    let _ = std::fs::remove_file(path);
    let listener = std::os::unix::net::UnixListener::bind(path)?;
    'clients: for conn in listener.incoming() {
        let conn = conn?;
        let mut out = conn.try_clone()?;
        for line in BufReader::new(conn).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let mut reply = dev.handle(app, &line);
            reply.push('\n');
            out.write_all(reply.as_bytes())?;
            if dev.quit {
                break 'clients;
            }
        }
    }
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[cfg(target_os = "macos")]
mod windowed {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Mutex;
    use std::sync::mpsc::{SyncSender, sync_channel};

    type Job = (String, SyncSender<String>);
    static PENDING: Mutex<Vec<Job>> = Mutex::new(Vec::new());
    thread_local! {
        static HANDLER: RefCell<Option<Box<dyn FnMut(&str) -> String>>> = const { RefCell::new(None) };
    }

    /// Run the queued requests on the main thread (scheduled on the main dispatch queue by the listener).
    fn drain() {
        let jobs = std::mem::take(&mut *PENDING.lock().unwrap());
        HANDLER.with(|h| {
            let mut h = h.borrow_mut();
            for (line, reply) in jobs {
                let r = match h.as_mut() {
                    Some(f) => f(&line),
                    None => err("no app").to_string(),
                };
                let _ = reply.send(r);
            }
        });
    }

    /// Serve a windowed app (called on the main thread by `Application::run` when `BOUGH_DEVTOOLS` is set).
    /// The listener thread hands each request to the main thread through the main dispatch queue and waits
    /// for its reply: nothing runs in the app's turns or frames unless a request arrives.
    pub fn serve_windowed<V: View>(app: Rc<RefCell<App<V>>>, path: std::path::PathBuf) -> std::io::Result<()> {
        use std::io::{BufRead, BufReader, Write};
        let mut dev = Devtools::<V>::new();
        HANDLER.with(|h| {
            *h.borrow_mut() = Some(Box::new(move |line: &str| match app.try_borrow_mut() {
                Ok(mut a) => dev.handle(&mut a, line),
                Err(_) => err("the app is busy").to_string(),
            }))
        });
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path)?;
        std::thread::Builder::new().name("bough-devtools".into()).spawn(move || {
            for conn in listener.incoming().flatten() {
                let Ok(mut out) = conn.try_clone() else { continue };
                for line in BufReader::new(conn).lines().map_while(Result::ok) {
                    let (tx, rx) = sync_channel(1);
                    PENDING.lock().unwrap().push((line, tx));
                    dispatch2::DispatchQueue::main().exec_async(drain);
                    let Ok(mut r) = rx.recv() else { return };
                    r.push('\n');
                    if out.write_all(r.as_bytes()).is_err() {
                        break;
                    }
                }
            }
        })?;
        Ok(())
    }
}
#[cfg(target_os = "macos")]
pub use windowed::serve_windowed;
