//! The probe: a description walked again, on demand, with nothing mounted and nothing written
//! (`docs/lanes/composition/DESIGN.md` §4).
//!
//! Reflection in bough stores nothing per instance and runs nothing per turn. What a record *is* (its element
//! type, the source line that built it, the part key a component gave it, the role or label its author said,
//! the owner it belongs to, the signals bound into it, the names of its routes) is learnt only when somebody
//! asks: the owner's `view` runs again and the new description is shown to a [`Probe`] instead of a
//! `Mounter` ([`Element::probe`](crate::elem::Element::probe)). The probe joins it to the retained records by a
//! preorder cursor and the depth column: an element that emits a record owns the record under the cursor and
//! its subtree. Everything static comes from tables the compiler builds (`type_name`, the owners' `type_info`
//! schemas, closure types, part keys); everything retained comes from the runtime's own columns (`tree.rs`).
//!
//! **Why a walk and not a column.** A column (an element-type name per handle, a part table, an owner
//! registry: what round 4 and the first composition commits kept) is written at every mount whether or not
//! anybody reads it, and must be kept in step by hand through every splice and reuse. A walk costs nothing
//! until it runs, and it cannot drift: it reads the same description the next re-description would.
//!
//! **Out of step.** The probe checks each attribution against the record (its kind family and its depth). A
//! description that no longer matches what is retained (a `Config::once()` mount whose plain fields changed,
//! an element type that does not say how it mounts) is not misattributed: the rest of that container is left
//! unattributed and [`Walk::lost`] counts it. The record scan still covers those records.
use crate::reflect::Schema;
use crate::rt::*;
use bough_core::page::Record;
use std::marker::PhantomData;
use std::panic::Location;

/// The record an element emits, as the probe checks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// A layout box: painted (`REC_QUAD`) or not (`REC_BOX`).
    Box,
    Text,
    Quad,
    Foreign,
    /// Any record kind.
    Any,
}
impl Family {
    fn admits(self, kind: u8) -> bool {
        match self {
            Family::Box => kind == REC_BOX || kind == REC_QUAD,
            Family::Text => kind == REC_TEXT,
            Family::Quad => kind == REC_QUAD,
            Family::Foreign => kind == REC_FOREIGN,
            Family::Any => true,
        }
    }
}

/// What an author said a record is, where the builders' own meaning is not enough (`Describe`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// A container with no meaning of its own.
    Generic,
    /// A named region: a part, or a container the author labelled.
    Group,
    Text,
    Button,
    TextInput,
    Image,
    ScrollArea,
    List,
    ListItem,
    /// Content the host composites (`foreign.rs`): its own accessibility tree hangs here.
    Foreign,
    Checkbox,
    Link,
    Heading,
    Tooltip,
}

/// A route verb (what input reaches a record's act).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verb {
    Click,
    Press,
    Hover,
    Key,
    Drag,
    Scroll,
    Edit,
}

/// One element as the walk saw it.
#[derive(Clone, Debug)]
pub struct Seen {
    /// The element's type (`type_name`: static).
    pub ty: &'static str,
    /// The builder's short name (`"box"`, `"row"`, `"text"`, `"button"`, ...).
    pub name: &'static str,
    /// The record it was attributed (`None`: it emits none, or the walk was out of step there).
    pub pos: Option<usize>,
    /// Index of its owner in [`Walk::owners`].
    pub owner: u32,
    /// Where it was built (builders that take `#[track_caller]`: parts, `Describe`, foreign elements).
    pub at: Option<&'static Location<'static>>,
    pub part: Option<(&'static str, u32)>,
    pub role: Option<Role>,
    pub label: Option<&'static str>,
    pub id: Option<&'static str>,
    /// Route verbs and the type names of their closures (`Counter::inc` reads as its path).
    pub routes: Vec<(Verb, &'static str)>,
    /// Signals bound into its operands by `sig!`.
    pub binds: Vec<Bind>,
    /// Seen whole (`Probe::leaf`): what lies under its record is its own implementation.
    pub leaf: bool,
}

/// A wired operand: which property, which signal (its path in the owner), its type and current value.
#[derive(Clone, Debug, PartialEq)]
pub struct Bind {
    pub prop: &'static str,
    /// The signal's field path from the owner (`"count"`, `"depart.month"`); `None` if the owner's schema
    /// does not reach it.
    pub path: Option<String>,
    /// Byte offset of the signal in its owner.
    pub off: u32,
    pub ty: &'static str,
    /// `Debug` of the current value (`None` without `Debug`).
    pub value: Option<String>,
}

/// An operand's binding as it describes itself (`IntoProp::bound`, `TextSrc::bound`).
#[derive(Clone, Copy)]
pub struct Bound {
    /// Offset of the signal in its owner (`offset_of!`-proven).
    pub off: u32,
    pub schema: &'static Schema,
    /// The signal's value.
    pub value: *const u8,
}

/// What kind of owner a part of the tree belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnerKind {
    /// The app's view.
    Root,
    /// A view held in a field of its parent owner (`embed!`).
    Embedded,
    /// A row of a list: the item in its slab.
    Row { slot: u32 },
    /// An entity shown by `entity_view` (its index in its `Entities`).
    Entity { idx: u32 },
}

/// An owner the walk met.
#[derive(Clone)]
pub struct OwnerSeen {
    pub ty: &'static str,
    pub kind: OwnerKind,
    pub parent: Option<u32>,
    /// Offset inside the root owner (embedded views; 0 for the root and for rows).
    pub base: u32,
    /// The field path from the parent owner (`embed!(self.depart)` is `"depart"`).
    pub field: Option<String>,
    pub(crate) ptr: *const u8,
    pub(crate) schema: &'static Schema,
    pub(crate) fields: unsafe fn(*const u8) -> Vec<crate::reflect::FieldView>,
    pub(crate) set: unsafe fn(*mut u8, &str, &str) -> bool,
}

/// The walk's state, shared by the probes of every owner it enters.
pub struct Walk {
    pub(crate) recs: Vec<Record>,
    /// The next record not yet attributed.
    pos: usize,
    pub seen: Vec<Seen>,
    /// Per record: the index in `seen` of the element attributed it, or `u32::MAX`.
    pub(crate) at_rec: Vec<u32>,
    /// Open containers: (record, its depth).
    stack: Vec<(usize, u16)>,
    pub owners: Vec<OwnerSeen>,
    owner: u32,
    /// Attributions refused (the description was out of step with the records there).
    pub lost: u32,
    lost_here: bool,
}

impl Walk {
    pub(crate) fn new(rt: &Rt) -> Walk {
        let recs = rt.bough.records();
        let n = recs.len();
        Walk { recs, pos: 0, seen: Vec::new(), at_rec: vec![u32::MAX; n], stack: Vec::new(), owners: Vec::new(), owner: 0, lost: 0, lost_here: false }
    }
    /// Was record `p` attributed to an element the walk sees whole (`Probe::leaf`)?
    pub(crate) fn leaf_at(&self, p: usize) -> bool {
        self.at_rec.get(p).is_some_and(|&i| i != u32::MAX && self.seen[i as usize].leaf)
    }
    /// First record after the subtree of `p` (the children have moved the cursor past their own records).
    fn subtree_end(&self, p: usize) -> usize {
        let d = self.recs[p].depth;
        let mut j = self.pos.max(p + 1);
        while j < self.recs.len() && self.recs[j].depth > d {
            j += 1;
        }
        j
    }
    /// The record under the cursor if it is what an element of family `f` emits here.
    fn attribute(&mut self, f: Family) -> Option<usize> {
        if self.lost_here {
            return None;
        }
        let p = self.pos;
        let want = self.stack.last().map_or(0, |t| t.1 + 1);
        match self.recs.get(p) {
            Some(r) if r.depth == want && f.admits(r.kind & 0x1F) => Some(p),
            _ => {
                self.lost += 1;
                self.lost_here = true;
                None
            }
        }
    }
    fn push(&mut self, ty: &'static str, name: &'static str, pos: Option<usize>, at: Option<&'static Location<'static>>) -> usize {
        let i = self.seen.len();
        if let Some(p) = pos {
            self.at_rec[p] = i as u32;
        }
        self.seen.push(Seen { ty, name, pos, owner: self.owner, at, part: None, role: None, label: None, id: None, routes: Vec::new(), binds: Vec::new(), leaf: false });
        i
    }
}

/// An element the probe has opened (its children are being walked).
#[must_use]
pub struct Open {
    seen: usize,
    pos: Option<usize>,
    lost: bool,
}
impl Open {
    /// The opened element's index (for `Probe::bind`, `Probe::seen`).
    pub fn seen(&self) -> usize {
        self.seen
    }
    /// The record it was attributed.
    pub fn pos(&self) -> Option<usize> {
        self.pos
    }
}

/// The visitor [`Element::probe`](crate::elem::Element::probe) is shown to: the walk, typed by the owner `V`
/// whose description is being walked.
pub struct Probe<'a, V> {
    pub(crate) rt: &'a Rt,
    pub(crate) owner: *const V,
    pub(crate) w: &'a mut Walk,
    _v: PhantomData<fn(&V)>,
}

impl<'a, V> Probe<'a, V> {
    pub(crate) fn new(rt: &'a Rt, owner: *const V, w: &'a mut Walk) -> Probe<'a, V> {
        Probe { rt, owner, w, _v: PhantomData }
    }
    /// An element whose record (and everything under it) is the one under the cursor.
    pub fn leaf(&mut self, f: Family, ty: &'static str, name: &'static str) -> usize {
        let pos = self.w.attribute(f);
        let i = self.w.push(ty, name, pos, None);
        self.w.seen[i].leaf = true;
        if let Some(p) = pos {
            self.w.pos = p + 1;
            // the gate's mutant: a leaf that steps one record, not its subtree
            #[cfg(not(feature = "mutant_probe"))]
            {
                self.w.pos = self.w.subtree_end(p);
            }
        }
        i
    }
    /// An element whose children follow: walk them, then [`close`](Self::close).
    pub fn open(&mut self, f: Family, ty: &'static str, name: &'static str) -> Open {
        let pos = self.w.attribute(f);
        let seen = self.w.push(ty, name, pos, None);
        let lost = self.w.lost_here;
        if let Some(p) = pos {
            self.w.stack.push((p, self.w.recs[p].depth));
            self.w.pos = p + 1;
        }
        Open { seen, pos, lost }
    }
    pub fn close(&mut self, o: Open) {
        if let Some(p) = o.pos {
            self.w.stack.pop();
            self.w.pos = self.w.subtree_end(p);
        }
        self.w.lost_here = o.lost;
    }
    /// An element that does not say how it mounts: the rest of its container is left unattributed.
    pub fn unknown(&mut self, ty: &'static str) {
        self.w.push(ty, "?", None, None);
        if !self.w.lost_here {
            self.w.lost += 1;
            self.w.lost_here = true;
        }
    }
    /// Index of the next element the walk will see (wrappers annotate the first one their inner element
    /// produces).
    pub fn mark(&self) -> usize {
        self.w.seen.len()
    }
    /// The element `i` (from `leaf`, `open` or a `mark` the inner element filled).
    pub fn seen(&mut self, i: usize) -> Option<&mut Seen> {
        self.w.seen.get_mut(i)
    }
    /// Annotate the first element produced since `mark` with its source location.
    pub fn at(&mut self, mark: usize, at: &'static Location<'static>) {
        if let Some(s) = self.w.seen.get_mut(mark) {
            s.at.get_or_insert(at);
        }
    }
    /// The operand `b` of the element `i` is bound to a signal of this probe's owner.
    pub fn bind(&mut self, i: usize, prop: &'static str, b: Option<Bound>) {
        let Some(b) = b else { return };
        let o = &self.w.owners[self.w.owner as usize];
        let path = crate::reflect::signal_path(o.schema, b.off as usize);
        let (ty, value) = (b.schema.nodes[0].type_name(), crate::reflect::debug_value(b.schema, b.value));
        if let Some(s) = self.w.seen.get_mut(i) {
            s.binds.push(Bind { prop, path, off: b.off, ty, value });
        }
    }
    /// The records of an owner `C` held at `off` inside this one (`embed!`): its description is walked with
    /// a probe of its own type.
    pub fn embed<C: 'static>(&mut self, c: &C, off: u32, f: impl FnOnce(&mut Probe<'_, C>)) {
        let parent = self.w.owner;
        let (pbase, pschema) = (self.w.owners[parent as usize].base, self.w.owners[parent as usize].schema);
        let field = crate::reflect::field_path(pschema, off as usize, std::any::type_name::<C>());
        self.enter(c, OwnerKind::Embedded, pbase + off, field, f)
    }
    /// A list row: the item `c` is an owner of its own.
    pub(crate) fn row<C: 'static>(&mut self, c: &C, slot: u32, f: impl FnOnce(&mut Probe<'_, C>)) {
        self.enter(c, OwnerKind::Row { slot }, 0, None, f)
    }
    /// An entity view: the entity `c` is an owner of its own.
    pub(crate) fn entity<C: 'static>(&mut self, c: &C, idx: u32, f: impl FnOnce(&mut Probe<'_, C>)) {
        self.enter(c, OwnerKind::Entity { idx }, 0, None, f)
    }
    fn enter<C: 'static>(&mut self, c: &C, kind: OwnerKind, base: u32, field: Option<String>, f: impl FnOnce(&mut Probe<'_, C>)) {
        let parent = self.w.owner;
        let o = OwnerSeen {
            ty: std::any::type_name::<C>(),
            kind,
            parent: Some(parent),
            base,
            field,
            ptr: c as *const C as *const u8,
            schema: crate::reflect::schema::<C>(),
            fields: crate::reflect::owner_fields::<C>,
            set: crate::reflect::owner_set::<C>,
        };
        self.w.owners.push(o);
        self.w.owner = (self.w.owners.len() - 1) as u32;
        {
            let mut p = Probe { rt: self.rt, owner: c as *const C, w: &mut *self.w, _v: PhantomData };
            f(&mut p);
        }
        self.w.owner = parent;
    }
    /// The owner this probe walks the description of.
    pub fn owner(&self) -> &V {
        // SAFETY: the probe was made from a live owner borrowed for the walk (`App::tree`, `embed`, `row`).
        unsafe { &*self.owner }
    }
    /// The runtime (read only).
    pub fn rt(&self) -> &Rt {
        self.rt
    }
    /// The record under the cursor, if the walk is in step.
    pub fn here(&self) -> Option<usize> {
        (!self.w.lost_here && self.w.pos < self.w.recs.len()).then_some(self.w.pos)
    }
}

/// Walk the root owner `v`'s description against the records of `rt`.
pub(crate) fn walk_root<V: crate::app::View>(rt: &Rt, v: &V) -> Walk {
    let mut w = Walk::new(rt);
    w.owners.push(OwnerSeen {
        ty: std::any::type_name::<V>(),
        kind: OwnerKind::Root,
        parent: None,
        base: 0,
        field: None,
        ptr: v as *const V as *const u8,
        schema: crate::reflect::schema::<V>(),
        fields: crate::reflect::owner_fields::<V>,
        set: crate::reflect::owner_set::<V>,
    });
    {
        let mut p = Probe::new(rt, v as *const V, &mut w);
        crate::elem::Element::probe(v.view(), &mut p);
    }
    w
}

// =============================================================================== what an author says

/// The builders: the element types the author's statements ([`Describe`]) are methods of. A marker, so
/// `.role` / `.label` / `.id` are not methods of every type the prelude reaches.
pub trait Builder {}

/// An element with something said about what it is. Mounting, re-description and seals pass straight
/// through (the statement costs nothing in a turn); only the walk reads it.
pub struct Said<E> {
    e: E,
    role: Option<Role>,
    label: Option<&'static str>,
    id: Option<&'static str>,
    at: &'static Location<'static>,
}

/// Saying what an element is, where the builders' own meaning is not enough: an icon button's name, a
/// `div` that acts as a checkbox, an address for an agent or a plugin. Tests do not need these: they find
/// elements by role, text, part and type (`find.rs`).
pub trait Describe: Builder + Sized {
    /// This element's role.
    #[track_caller]
    fn role(self, role: Role) -> Said<Self> {
        Said { e: self, role: Some(role), label: None, id: None, at: Location::caller() }
    }
    /// Its accessible name (an icon button has no text of its own).
    #[track_caller]
    fn label(self, label: &'static str) -> Said<Self> {
        Said { e: self, role: None, label: Some(label), id: None, at: Location::caller() }
    }
    /// A stable id (`#send` in a selector).
    #[track_caller]
    fn id(self, id: &'static str) -> Said<Self> {
        Said { e: self, role: None, label: None, id: Some(id), at: Location::caller() }
    }
}
impl<E: Builder> Describe for E {}

impl<E> Said<E> {
    /// Several statements about one element fold into one.
    pub fn role(mut self, role: Role) -> Self {
        self.role = Some(role);
        self
    }
    pub fn label(mut self, label: &'static str) -> Self {
        self.label = Some(label);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }
}
impl<E> Builder for Said<E> {}
impl<E: crate::layout::Styled> crate::layout::Styled for Said<E> {
    fn spec_mut(&mut self) -> &mut crate::layout::Spec {
        self.e.spec_mut()
    }
}
impl<E: crate::part::Restyle> crate::part::Restyle for Said<E> {
    type Style = E::Style;
    type Out = Said<E::Out>;
    fn restyled(self, index: u32, f: &dyn Fn(E::Style) -> E::Style) -> Self::Out {
        Said { e: self.e.restyled(index, f), role: self.role, label: self.label, id: self.id, at: self.at }
    }
}

impl<V, E: crate::elem::Element<V>> crate::elem::Element<V> for Said<E> {
    #[inline(always)]
    fn mount(self, m: &mut crate::elem::Mounter<'_, V>) {
        self.e.mount(m)
    }
    #[inline(always)]
    fn update(self, m: &mut crate::elem::Mounter<'_, V>) {
        self.e.update(m)
    }
    #[inline(always)]
    fn dyn_key(&self) -> u64 {
        self.e.dyn_key()
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        self.e.seal(s)
    }
    fn probe(self, p: &mut Probe<'_, V>) {
        let mark = p.mark();
        self.e.probe(p);
        if let Some(s) = p.seen(mark) {
            s.role = self.role.or(s.role);
            s.label = self.label.or(s.label);
            s.id = self.id.or(s.id);
        }
        p.at(mark, self.at);
    }
}

macro_rules! builders {
    ($($t:ty => [$($g:tt)*]),* $(,)?) => {$(impl<$($g)*> Builder for $t {})*};
}
builders!(
    crate::layout::LBox<Bg, Ch, X> => [Bg, Ch, X],
    crate::layout::Para<S, C> => [S, C],
    crate::elem::Text<S, G> => [S, G],
    crate::elem::Quad<Bg, Wd, Sp, Ch> => [Bg, Wd, Sp, Ch],
    crate::input::On<E, O> => [E, O],
    crate::input::NoHit<E> => [E],
    crate::input::Tooltip<T> => [T],
    crate::input::TextField<'a, V> => ['a, V],
    crate::part::Parted<E, S> => [E, S],
    crate::part::With<E, O> => [E, O],
    crate::foreign::Foreign => [],
    crate::ui::ImageBox => [],
    crate::ui::TextInput<'a, V> => ['a, V],
    crate::listview::LList<'a, S, V> => ['a, S, V],
    crate::template::ScrollBox<'a, V, Bg, Ch> => ['a, V, Bg, Ch],
    crate::template::ScrollY<'a, V, C> => ['a, V, C],
    crate::template::Anchor<C> => [C],
);
