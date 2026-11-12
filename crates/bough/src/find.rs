//! Finding elements and acting on them: the testing API, and what the devtools server speaks
//! (`docs/lanes/composition/DESIGN.md` §4).
//!
//! A test needs no ids and no wrappers in the app: it finds what a person would look for (a role, a text,
//! a part of a component, the view that owns it) and acts through the entry points real input takes
//! (`pointer_move`/`pointer_down`/`pointer_up`, `key_down`, `text_input`, `wheel_points`), at the place a
//! pointer would be: the centre of what shows of the element. An element something else covers there is
//! refused, as a person could not click it either.
//!
//! ```ignore
//! use bough::find::*;
//! app.click("button \"+\"");                       // a selector string
//! app.click(role(Role::Button).name("+"));         // the same, typed
//! assert_eq!(app.get(part(DAY).at(3)).fill, Some(SUNDAY));
//! assert_eq!(app.get("text ~\"Count\"").bind("text").unwrap().value.as_deref(), Some("1"));
//! ```
//!
//! Selector strings: steps separated by ` ` (a descendant) or ` > ` (a child). A step begins with a word
//! (a role, a builder's short name, or an owner's type: `button`, `row`, `MonthPicker`) or `*`, or with
//! any of the other atoms, which join the step they follow: `#id`, `@part.name`, `[index]` (the part's
//! index), `"name"` (exact accessible name), `~"text"` (name contains), `:state` (`:focused`, `:hovered`,
//! `:hidden`, ...), `:nth(k)` (the k-th match of the whole selector, from 0). So `button "+"` is a button
//! named "+", and `list * "beta"` is anything named "beta" inside a list.
use crate::app::{App, View};
use crate::input::{Button, Key, Mods};
use crate::part::Part;
use crate::probe::Role;
use crate::tree::{Node, States, Tree, short};

#[derive(Clone, Debug, PartialEq)]
enum Pred {
    /// Any element (`*`).
    Any,
    /// A role, a builder name or an owner type (`button`, `row`, `MonthPicker`).
    Word(String),
    /// Bound to the signal at this offset of an owner of this type (`bound(sig!(..))`).
    Bound(&'static str, u32),
    Role(Role),
    Name(String),
    NameHas(String),
    Id(String),
    Part(String),
    Index(u32),
    State(States),
}

#[derive(Clone, Debug, PartialEq)]
struct Step {
    preds: Vec<Pred>,
    /// A child of the previous step's match (`>`), not any descendant.
    child: bool,
}

/// A selector: what to find in a [`Tree`].
#[derive(Clone, Debug, PartialEq)]
pub struct Sel {
    steps: Vec<Step>,
    nth: Option<usize>,
}

impl Sel {
    fn one(p: Pred) -> Sel {
        Sel { steps: vec![Step { preds: vec![p], child: false }], nth: None }
    }
    fn also(mut self, p: Pred) -> Sel {
        self.steps.last_mut().expect("a selector has a step").preds.push(p);
        self
    }
    /// ... whose accessible name is exactly `s`.
    pub fn name(self, s: &str) -> Sel {
        self.also(Pred::Name(s.into()))
    }
    /// ... whose accessible name contains `s`.
    pub fn has(self, s: &str) -> Sel {
        self.also(Pred::NameHas(s.into()))
    }
    /// ... of role `r`.
    pub fn role(self, r: Role) -> Sel {
        self.also(Pred::Role(r))
    }
    /// ... the part with this index (`part_at`'s).
    pub fn at(self, index: u32) -> Sel {
        self.also(Pred::Index(index))
    }
    /// ... in state `s`.
    pub fn state(self, s: States) -> Sel {
        self.also(Pred::State(s))
    }
    /// ... inside a match of `outer`.
    pub fn within(self, outer: impl Into<Sel>) -> Sel {
        let mut o = outer.into();
        o.steps.extend(self.steps);
        o.nth = self.nth;
        o
    }
    /// The `k`-th match (from 0).
    pub fn nth(mut self, k: usize) -> Sel {
        self.nth = Some(k);
        self
    }
}

/// Elements of role `r`.
pub fn role(r: Role) -> Sel {
    Sel::one(Pred::Role(r))
}
/// Elements whose accessible name is exactly `s`.
pub fn name(s: &str) -> Sel {
    Sel::one(Pred::Name(s.into()))
}
/// Elements whose accessible name contains `s`.
pub fn has(s: &str) -> Sel {
    Sel::one(Pred::NameHas(s.into()))
}
/// The element its author gave `id` (`.id("send")`).
pub fn id(s: &str) -> Sel {
    Sel::one(Pred::Id(s.into()))
}
/// Every part `key` (typed: a key that does not exist does not compile).
pub fn part<S>(key: Part<S>) -> Sel {
    Sel::one(Pred::Part(key.name().into()))
}
/// Elements a builder or an owner type names (`"row"`, `"MonthPicker"`), or of the role of that name.
pub fn word(w: &str) -> Sel {
    Sel::one(Pred::Word(w.into()))
}
/// Every element bound to the signal `w` names (`bound(sig!(page: Page => count))`): no string, and a
/// renamed field is a compile error, not a test that finds nothing. A signal of the root owner reaches into
/// embedded views (`sig!(page: Page => depart.month)`); a list row's is named by its item type.
pub fn bound<T, V>(w: crate::signal::Wired<'_, T, V>) -> Sel {
    Sel::one(Pred::Bound(std::any::type_name::<V>(), w.off))
}
/// Every element inside an owner of type `T` (a view embedded, a list row, an entity).
pub fn owner<T>() -> Sel {
    Sel::one(Pred::Word(short(std::any::type_name::<T>()).into()))
}

impl From<Role> for Sel {
    fn from(r: Role) -> Sel {
        role(r)
    }
}
impl<S> From<Part<S>> for Sel {
    fn from(k: Part<S>) -> Sel {
        part(k)
    }
}
impl From<&str> for Sel {
    /// Parse a selector string (module docs); panics on a malformed one (a test's typo).
    #[track_caller]
    fn from(s: &str) -> Sel {
        Sel::parse(s).unwrap_or_else(|e| panic!("selector {s:?}: {e}"))
    }
}
impl From<&Sel> for Sel {
    fn from(s: &Sel) -> Sel {
        s.clone()
    }
}

fn role_named(w: &str) -> Option<Role> {
    use Role::*;
    let all = [Generic, Group, Text, Button, TextInput, Image, ScrollArea, List, ListItem, Foreign, Checkbox, Link, Heading, Tooltip];
    let w = w.to_ascii_lowercase();
    if w == "input" {
        return Some(TextInput);
    }
    all.into_iter().find(|r| format!("{r:?}").to_ascii_lowercase() == w)
}

impl Sel {
    /// Parse a selector string (module docs).
    pub fn parse(s: &str) -> Result<Sel, String> {
        let b = s.as_bytes();
        let mut steps: Vec<Step> = Vec::new();
        let mut cur: Vec<Pred> = Vec::new();
        let mut child = false;
        let mut nth = None;
        let mut i = 0;
        let ident = |i: &mut usize| {
            let st = *i;
            while *i < b.len() && (b[*i].is_ascii_alphanumeric() || matches!(b[*i], b'_' | b'-' | b'.')) {
                *i += 1;
            }
            s[st..*i].to_string()
        };
        let quoted = |i: &mut usize| -> Result<String, String> {
            // at the opening quote
            *i += 1;
            let mut out = String::new();
            while *i < b.len() && b[*i] != b'"' {
                if b[*i] == b'\\' && *i + 1 < b.len() {
                    *i += 1;
                }
                let c = s[*i..].chars().next().unwrap();
                out.push(c);
                *i += c.len_utf8();
            }
            if *i >= b.len() {
                return Err("unterminated quote".into());
            }
            *i += 1;
            Ok(out)
        };
        while i < b.len() {
            match b[i] {
                b' ' | b'\t' => {
                    i += 1;
                    // a new step begins only at a word (or `*`); other atoms join the step before them
                    let next = b[i..].iter().find(|c| !matches!(c, b' ' | b'\t'));
                    if !cur.is_empty() && next.is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'*')) {
                        steps.push(Step { preds: std::mem::take(&mut cur), child });
                        child = false;
                    }
                }
                b'*' => {
                    cur.push(Pred::Any);
                    i += 1;
                }
                b'>' => {
                    if !cur.is_empty() {
                        steps.push(Step { preds: std::mem::take(&mut cur), child });
                    }
                    if steps.is_empty() {
                        return Err("'>' with nothing before it".into());
                    }
                    child = true;
                    i += 1;
                }
                b'#' => {
                    i += 1;
                    cur.push(Pred::Id(ident(&mut i)));
                }
                b'@' => {
                    i += 1;
                    cur.push(Pred::Part(ident(&mut i)));
                }
                b'[' => {
                    let e = s[i..].find(']').ok_or("unterminated '['")? + i;
                    cur.push(Pred::Index(s[i + 1..e].trim().parse().map_err(|_| "a part index is a number")?));
                    i = e + 1;
                }
                b'"' => cur.push(Pred::Name(quoted(&mut i)?)),
                b'~' if b.get(i + 1) == Some(&b'"') => {
                    i += 1;
                    cur.push(Pred::NameHas(quoted(&mut i)?));
                }
                b':' => {
                    i += 1;
                    let w = ident(&mut i);
                    if w == "nth" {
                        let e = s[i..].find(')').ok_or("unterminated ':nth('")? + i;
                        nth = Some(s[i + 1..e].trim().parse().map_err(|_| ":nth takes a number")?);
                        i = e + 1;
                    } else {
                        cur.push(Pred::State(States::named(&w).ok_or_else(|| format!("unknown state :{w}"))?));
                    }
                }
                c if c.is_ascii_alphanumeric() || c == b'_' => cur.push(Pred::Word(ident(&mut i))),
                c => return Err(format!("unexpected {:?} at {i}", c as char)),
            }
        }
        if !cur.is_empty() {
            steps.push(Step { preds: cur, child });
        } else if child {
            return Err("'>' with nothing after it".into());
        }
        if steps.is_empty() {
            return Err("empty selector".into());
        }
        Ok(Sel { steps, nth })
    }
}

impl std::fmt::Display for Sel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (k, st) in self.steps.iter().enumerate() {
            if k > 0 {
                f.write_str(if st.child { " > " } else { " " })?;
            }
            for p in &st.preds {
                match p {
                    Pred::Any => f.write_str("*")?,
                    Pred::Bound(t, o) => write!(f, "<bound {}+{o}>", short(t))?,
                    Pred::Word(w) => write!(f, "{w}")?,
                    Pred::Role(r) => write!(f, "{}", format!("{r:?}").to_ascii_lowercase())?,
                    Pred::Name(n) => write!(f, "{n:?}")?,
                    Pred::NameHas(n) => write!(f, "~{n:?}")?,
                    Pred::Id(i) => write!(f, "#{i}")?,
                    Pred::Part(p) => write!(f, "@{p}")?,
                    Pred::Index(i) => write!(f, "[{i}]")?,
                    Pred::State(s) => write!(f, ":{}", States::NAMES.iter().find(|n| n.0 == *s).map_or("?", |n| n.1))?,
                }
            }
        }
        if let Some(k) = self.nth {
            write!(f, ":nth({k})")?;
        }
        Ok(())
    }
}

impl Tree {
    fn owner_is(&self, mut o: Option<u32>, w: &str) -> bool {
        while let Some(k) = o {
            if short(self.owners[k as usize].ty) == w {
                return true;
            }
            o = self.owners[k as usize].parent;
        }
        false
    }
    fn pred(&self, n: &Node, p: &Pred) -> bool {
        match p {
            Pred::Any => true,
            Pred::Bound(ty, off) => {
                let o = &self.owners[n.owner as usize];
                let root = o.kind == crate::probe::OwnerKind::Embedded && self.owners[0].ty == *ty;
                n.binds.iter().any(|b| (o.ty == *ty && b.off == *off) || (root && o.base + b.off == *off))
            }
            Pred::Word(w) => role_named(w) == Some(n.role) || n.elem.eq_ignore_ascii_case(w) || self.owner_is(Some(n.owner), w),
            Pred::Role(r) => n.role == *r,
            Pred::Name(s) => n.name == *s,
            Pred::NameHas(s) => n.name.contains(s.as_str()),
            Pred::Id(s) => n.id == Some(s.as_str()),
            Pred::Part(s) => n.part.is_some_and(|p| p.0 == s),
            Pred::Index(i) => n.part.is_some_and(|p| p.1 == *i),
            Pred::State(s) => n.states.has(*s),
        }
    }
    fn step(&self, n: &Node, s: &Step) -> bool {
        s.preds.iter().all(|p| self.pred(n, p))
    }
    /// Does node `i` match steps `..=k` (step `k` at `i` itself)?
    fn chain(&self, i: usize, steps: &[Step], k: usize) -> bool {
        if !self.step(&self.nodes[i], &steps[k]) {
            return false;
        }
        if k == 0 {
            return true;
        }
        let mut p = self.nodes[i].parent;
        if steps[k].child {
            return p.is_some_and(|a| self.chain(a, steps, k - 1));
        }
        while let Some(a) = p {
            if self.chain(a, steps, k - 1) {
                return true;
            }
            p = self.nodes[a].parent;
        }
        false
    }
    /// Every node matching `sel`, in paint order.
    pub fn find(&self, sel: impl Into<Sel>) -> Vec<&Node> {
        let sel = sel.into();
        let last = sel.steps.len() - 1;
        let all: Vec<&Node> = (0..self.nodes.len()).filter(|&i| self.chain(i, &sel.steps, last)).map(|i| &self.nodes[i]).collect();
        match sel.nth {
            Some(k) => all.into_iter().nth(k).into_iter().collect(),
            None => all,
        }
    }
    /// The one node matching `sel`.
    pub fn try_get(&self, sel: impl Into<Sel>) -> Result<&Node, ActError> {
        let sel = sel.into();
        let v = self.find(&sel);
        match v.len() {
            1 => Ok(v[0]),
            0 => Err(ActError::NotFound(sel.to_string())),
            k => Err(ActError::Many(sel.to_string(), k, v.iter().map(|n| describe(n)).collect())),
        }
    }
    /// The one node matching `sel`; none or several is a panic that says what was asked and what was there.
    #[track_caller]
    pub fn get(&self, sel: impl Into<Sel>) -> &Node {
        match self.try_get(sel) {
            Ok(n) => n,
            Err(e) => panic!("{e}\n{}", self.dump()),
        }
    }
}

fn describe(n: &Node) -> String {
    format!("{:?} {:?} {} at pos {}", n.role, n.name, n.elem, n.pos)
}

/// Why an action could not be taken.
#[derive(Clone, Debug, PartialEq)]
pub enum ActError {
    NotFound(String),
    Many(String, usize, Vec<String>),
    /// Nothing of it shows.
    Hidden(String),
    /// Something else is under the pointer at its centre.
    Covered {
        sel: String,
        by: String,
    },
    /// It has no route for this action.
    Inert(String),
}
impl std::fmt::Display for ActError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActError::NotFound(s) => write!(f, "nothing matches {s}"),
            ActError::Many(s, k, v) => write!(f, "{k} nodes match {s}: {v:?}"),
            ActError::Hidden(s) => write!(f, "{s} is not visible"),
            ActError::Covered { sel, by } => write!(f, "{sel} is covered at its centre by {by}"),
            ActError::Inert(s) => write!(f, "{s} cannot take that action"),
        }
    }
}
impl std::error::Error for ActError {}

/// One turn of input, at the level the app's entry points take it: what a session records and replays.
#[derive(Clone, Debug, PartialEq)]
pub enum Turn {
    Move {
        x: f32,
        y: f32,
    },
    Down {
        x: f32,
        y: f32,
        button: Button,
    },
    Up {
        x: f32,
        y: f32,
        button: Button,
    },
    Leave,
    Wheel {
        x: f32,
        y: f32,
        dy: f32,
    },
    Key {
        key: Key,
        mods: Mods,
    },
    Text(String),
    /// The frame clock (animations, glides, caret blink) advanced by `dt` seconds.
    Tick(f32),
    /// A field of owner `owner` (`reflect_owners` numbering) written by path, in a turn.
    Set {
        owner: usize,
        path: String,
        value: String,
    },
    /// A frame drawn (damage, tile list, GPU repair).
    Frame,
}

impl<V: View> App<V> {
    /// Apply one recorded turn through the entry point real input (or the driver) takes.
    pub fn apply(&mut self, t: &Turn) {
        match t {
            Turn::Move { x, y } => self.pointer_move(*x, *y),
            Turn::Down { x, y, button } => self.pointer_down(*x, *y, *button),
            Turn::Up { x, y, button } => self.pointer_up(*x, *y, *button),
            Turn::Leave => self.pointer_leave(),
            Turn::Wheel { x, y, dy } => self.wheel_points(*x, *y, *dy),
            Turn::Key { key, mods } => self.key_down(*key, *mods),
            Turn::Text(s) => self.text_input(s),
            Turn::Tick(dt) => {
                self.tick(*dt);
            }
            Turn::Set { owner, path, value } => {
                self.reflect_set_owner(*owner, path, value);
            }
            Turn::Frame => {
                self.frame();
            }
        }
    }

    /// Every node matching `sel`, now.
    pub fn find(&self, sel: impl Into<Sel>) -> Vec<Node> {
        self.tree().find(sel).into_iter().cloned().collect()
    }
    /// The one node matching `sel`, now (panics with the tree's listing otherwise).
    #[track_caller]
    pub fn get(&self, sel: impl Into<Sel>) -> Node {
        self.tree().get(sel).clone()
    }
    /// Does anything match `sel`?
    pub fn exists(&self, sel: impl Into<Sel>) -> bool {
        !self.tree().find(sel).is_empty()
    }

    /// Where a pointer acts on the one match of `sel` (points), checked as a person would find it: it
    /// shows, and nothing unrelated is on top at its centre (a pointer-transparent element answers through
    /// its ancestor, as a real press would).
    pub fn point_of(&self, sel: impl Into<Sel>) -> Result<(f32, f32), ActError> {
        let sel = sel.into();
        let t = self.tree();
        let n = t.try_get(&sel)?;
        let (x, y) = n.center_pt(t.scale).ok_or_else(|| ActError::Hidden(sel.to_string()))?;
        let s = self.rt.scale;
        let top = self.rt.bough.hit_top((x * s).floor() as i32, (y * s).floor() as i32);
        match top {
            Some(h) if t.contains(n.pos, h) || t.contains(h, n.pos) => Ok((x, y)),
            Some(h) => Err(ActError::Covered { sel: sel.to_string(), by: describe(&t.nodes[h]) }),
            None => Err(ActError::Covered { sel: sel.to_string(), by: "nothing hit-testable".into() }),
        }
    }

    /// Click the one match of `sel` (move, press, release at its centre), returning the turns it took.
    pub fn try_click(&mut self, sel: impl Into<Sel>) -> Result<Vec<Turn>, ActError> {
        let (x, y) = self.point_of(sel)?;
        let turns = vec![Turn::Move { x, y }, Turn::Down { x, y, button: Button::Primary }, Turn::Up { x, y, button: Button::Primary }];
        for t in &turns {
            self.apply(t);
        }
        Ok(turns)
    }
    /// Click the one match of `sel`; panics if it cannot be clicked.
    #[track_caller]
    pub fn click(&mut self, sel: impl Into<Sel>) {
        if let Err(e) = self.try_click(sel) {
            panic!("click: {e}\n{}", self.tree().dump());
        }
    }
    /// Move the pointer over the one match of `sel`.
    pub fn try_hover(&mut self, sel: impl Into<Sel>) -> Result<Vec<Turn>, ActError> {
        let (x, y) = self.point_of(sel)?;
        let t = Turn::Move { x, y };
        self.apply(&t);
        Ok(vec![t])
    }
    #[track_caller]
    pub fn hover(&mut self, sel: impl Into<Sel>) {
        if let Err(e) = self.try_hover(sel) {
            panic!("hover: {e}\n{}", self.tree().dump());
        }
    }
    /// Focus the one match of `sel` by clicking it, then type `text` (a text field edits its signal).
    pub fn try_type(&mut self, sel: impl Into<Sel>, text: &str) -> Result<Vec<Turn>, ActError> {
        let sel = sel.into();
        if !self.tree().try_get(&sel)?.routes.iter().any(|r| r.0 == crate::probe::Verb::Edit) {
            return Err(ActError::Inert(sel.to_string()));
        }
        let mut turns = self.try_click(&sel)?;
        let t = Turn::Text(text.to_string());
        self.apply(&t);
        turns.push(t);
        Ok(turns)
    }
    #[track_caller]
    pub fn type_text(&mut self, sel: impl Into<Sel>, text: &str) {
        if let Err(e) = self.try_type(sel, text) {
            panic!("type: {e}\n{}", self.tree().dump());
        }
    }
    /// A key, to whatever has focus.
    pub fn press_key(&mut self, key: Key, mods: Mods) -> Vec<Turn> {
        let t = Turn::Key { key, mods };
        self.apply(&t);
        vec![t]
    }
    /// Scroll the scroll area containing the one match of `sel` by `dy` points (positive: down), at its
    /// centre, through the wheel's own entry point.
    pub fn try_scroll(&mut self, sel: impl Into<Sel>, dy: f32) -> Result<Vec<Turn>, ActError> {
        let (x, y) = self.point_of(sel)?;
        let t = Turn::Wheel { x, y, dy };
        self.apply(&t);
        Ok(vec![t])
    }
    #[track_caller]
    pub fn scroll(&mut self, sel: impl Into<Sel>, dy: f32) {
        if let Err(e) = self.try_scroll(sel, dy) {
            panic!("scroll: {e}\n{}", self.tree().dump());
        }
    }
}
