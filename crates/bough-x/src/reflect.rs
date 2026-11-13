//! Reflection in the core: the one typed walk that tests, theming, accessibility, events and an inspector read.
//!
//! The view already is a typed description and the stores already are archetype tables, so reflection adds no
//! table of its own to them. It is a sink, [`Learn`], driven once over the same `view` code: at every record it
//! is handed the record's type, so it records, per column, what only the type knows (the paint kind, the
//! operands with their names, code, stage and the words they read, and monomorphised readers of their values),
//! and per site, where in the view it was described (`#[track_caller]` on the sink's methods). Everything else
//! is read from the stores as they are: which records are live, their rects, their words.
//!
//! * [`Reflect::each`]: every live record, in paint order, as an [`Item`]: its entity address ([`Ent`]: the
//!   store, the row, the position in the row's shape), its place (column, record), its rect, its operands and
//!   their values now, its source location.
//! * [`Reflect::stack`]: the records under a point, topmost first (any record, handler or not).
//! * [`Reflect::rows`]: rows of a list that are not materialised, described on demand from the view's own row
//!   function (it is pure), with nothing stored and nothing scrolled.
//!
//! Cost when unused: nothing. `Learn` and its readers are generic code instantiated only by a program that calls
//! them; [`Arch::OPS`] and [`Arch::val`] are a constant and a method that nothing else names; the sink methods'
//! `#[track_caller]` location is a constant argument the other sinks never read (measured in the surface lane's
//! journal).
use crate::expr::{StageId, U};
use crate::kinds::Txt;
use crate::sink::{Leaf, List, Row, ScopeOf, Sink, View};
use crate::store::{Arch, Hov, Live, Op, Pass, Path, Rect, Space, Store};
use core::any::TypeId;
use core::ops::Range;
use core::panic::Location;

/// An operand's value, as reflection reads it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Val {
    None,
    /// 0xAARRGGBB.
    Color(u32),
    /// An integer scaled by `10^d`, and `d`.
    Num(i32, u8),
    Text(Txt),
    Pair(i32, i32),
}
impl core::fmt::Debug for Txt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (b, n) = self.bytes();
        write!(f, "{:?}", core::str::from_utf8(&b[..n]).unwrap_or("?"))
    }
}
impl Val {
    /// What a person reads: a label's text, a number with its decimals, `#rrggbb`.
    pub fn text(&self) -> String {
        match *self {
            Val::None => String::new(),
            Val::Color(c) => format!("#{:06x}", c & 0xff_ffff),
            Val::Num(v, d) => {
                let mut b = [0u8; 24];
                let n = crate::soft::format_num(v, d, &mut b);
                String::from_utf8_lossy(&b[..n]).into_owned()
            }
            Val::Text(t) => {
                let (b, n) = t.bytes();
                String::from_utf8_lossy(&b[..n]).into_owned()
            }
            Val::Pair(x, y) => format!("({x}, {y})"),
        }
    }
    /// A number or a text: what a record says, as opposed to how it looks.
    pub fn is_content(&self) -> bool {
        matches!(self, Val::Num(..) | Val::Text(_))
    }
}

/// What a column's archetype is, as only its type knows. Learned once, when [`Learn`] meets the type.
pub struct ColInfo {
    /// The record type, as the column's own `arch` must be for the readers below to apply (checked when the
    /// column is learned and whenever the reflection is used).
    pub arch: TypeId,
    /// The paint kind (`"fill"`, `"num"`, ...).
    pub kind: &'static str,
    /// The record type, written out by the compiler.
    pub ty: &'static str,
    pub stage: StageId,
    pub reads: u64,
    pub moves: bool,
    pub ops: &'static [Op],
    /// The column has a pointer handler.
    pub hears: bool,
    /// Its handlers: the kinds they hear, their type as the compiler writes it.
    pub on: Option<Hov>,
    /// The store's sites that share the column, in the order of their record bases.
    pub sites: Vec<u32>,
    val: unsafe fn(*const u8, usize, &U) -> Val,
    changed: unsafe fn(*const u8, &U, &U) -> bool,
    offset: unsafe fn(*const u8, &U) -> (i32, i32),
}
/// SAFETY (all three): `p` points at a record of type `A`.
unsafe fn val_of<A: Arch>(p: *const u8, op: usize, u: &U) -> Val {
    unsafe { (*(p as *const A)).val(op, u) }
}
unsafe fn changed_of<A: Arch>(p: *const u8, old: &U, new: &U) -> bool {
    unsafe { (*(p as *const A)).changed(old, new) }
}
unsafe fn offset_of<A: Arch>(p: *const u8, u: &U) -> (i32, i32) {
    unsafe { (*(p as *const A)).offset(u) }
}
impl ColInfo {
    fn of<A: Arch>(on: Option<Hov>) -> ColInfo {
        ColInfo {
            arch: TypeId::of::<A>(),
            kind: A::NAME,
            ty: core::any::type_name::<A>(),
            stage: A::STAGE,
            reads: A::READS,
            moves: A::MOVES,
            ops: A::OPS,
            hears: on.is_some_and(|o| o.kinds & crate::event::kind::POINTER != 0),
            on,
            sites: Vec::new(),
            val: val_of::<A>,
            changed: changed_of::<A>,
            offset: offset_of::<A>,
        }
    }
}

/// A position of a store's shape.
#[derive(Clone, Copy, Debug)]
pub struct SiteInfo {
    pub col: u32,
    /// Which row call of a fixed shape (0 in a list).
    pub row: u32,
    /// The position in that row.
    pub pos: u32,
    /// Where the row (or the list) was described.
    pub loc: &'static Location<'static>,
}

#[derive(Clone, Debug)]
pub struct ListInfo {
    pub n: u32,
    pub row_h: i32,
    pub x: i32,
    pub y: i32,
    pub shape: Vec<Rect>,
}

#[derive(Clone, Debug)]
pub enum Kind {
    Root,
    Scope,
    List(ListInfo),
}

/// What a store's view described, in order: a row of its fixed shape (by row call), or a child store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Row(u32),
    Sub(u32),
}

/// A store as reflection knows it.
pub struct StoreInfo {
    pub kind: Kind,
    /// Rows and child stores in the order the view described them.
    pub seq: Vec<Part>,
    /// Where the scope or list was described.
    pub loc: Option<&'static Location<'static>>,
    pub sites: Vec<Option<SiteInfo>>,
    pub cols: Vec<Option<ColInfo>>,
    pub subs: Vec<StoreInfo>,
    /// Records met whose column was not made for their type (see `LearnLeaf`): not reflected.
    pub refused: u32,
}
impl StoreInfo {
    fn new(kind: Kind, loc: Option<&'static Location<'static>>) -> StoreInfo {
        StoreInfo { kind, seq: Vec::new(), loc, sites: Vec::new(), cols: Vec::new(), subs: Vec::new(), refused: 0 }
    }
    fn child(&mut self, q: usize, st: &Store, kind: Kind, loc: &'static Location<'static>) -> &mut StoreInfo {
        while self.subs.len() <= q {
            self.subs.push(StoreInfo::new(Kind::Scope, None));
        }
        let c = &mut self.subs[q];
        c.kind = kind;
        c.loc = Some(loc);
        c.seq.clear();
        c.refused = 0;
        c.sites.resize_with(st.sites.len(), || None);
        c.cols.resize_with(st.cols.len(), || None);
        c
    }
    fn finish(&mut self) {
        for c in self.cols.iter_mut().flatten() {
            c.sites.clear();
        }
        for (i, s) in self.sites.iter().enumerate() {
            if let Some(s) = s
                && let Some(Some(c)) = self.cols.get_mut(s.col as usize)
            {
                c.sites.push(i as u32);
            }
        }
        for s in &mut self.subs {
            s.finish();
        }
    }
    /// The store at `path` under this one.
    pub fn at(&self, path: &Path) -> Option<&StoreInfo> {
        let mut s = self;
        for &q in &path.at[..path.n as usize] {
            s = s.subs.get(q as usize)?;
        }
        Some(s)
    }
}

// ---- learning: the view described once with a sink that keeps what the types say ----------------------------

/// The reflecting sink. It writes nothing and allocates nothing in the world; it walks the stores in step with
/// the view (the same counters the writing sink keeps) and, at every record, keeps what the record's type says.
pub struct Learn<'a> {
    st: &'a Store,
    out: &'a mut StoreInfo,
    rowc: u32,
    sub: u32,
}
struct LearnLeaf<'a> {
    st: &'a Store,
    out: &'a mut StoreInfo,
    site: u32,
    row: u32,
    pos: u32,
    loc: &'static Location<'static>,
}
impl<V> Leaf<V> for LearnLeaf<'_> {
    fn leaf<A: Arch>(&mut self, _: &A, _: TypeId, hov: Option<Hov>) {
        let k = self.site as usize;
        self.site += 1;
        let pos = self.pos;
        self.pos += 1;
        let Some(s) = self.st.sites.get(k) else { return };
        self.out.sites[k] = Some(SiteInfo { col: s.col, row: self.row, pos, loc: self.loc });
        // The readers are installed only for a column made for this very record type. A record whose type holds
        // a closure is another type in each sink's instantiation of `view` (the closure's type carries the sink),
        // so its column is not readable through this sink: it is skipped, never read as another type.
        if self.st.cols[s.col as usize].arch != TypeId::of::<A>() {
            self.out.refused += 1;
            return;
        }
        let c = &mut self.out.cols[s.col as usize];
        if c.is_none() {
            *c = Some(ColInfo::of::<A>(hov));
        }
    }
}
impl<'r, 'a, V> ScopeOf<'r, V> for Learn<'a> {
    type Scope = Learn<'r>;
}
impl<'a, V> Sink<V> for Learn<'a> {
    #[track_caller]
    fn row<R: Row<V>, L: AsRef<[Rect]>, F: FnOnce() -> (L, R)>(&mut self, f: F) {
        let loc = Location::caller();
        let k = self.rowc;
        self.rowc += 1;
        self.out.seq.push(Part::Row(k));
        let Some(&(_, p0)) = self.st.rows.get(k as usize) else { return };
        let (_, row) = f();
        row.visit(&mut LearnLeaf { st: self.st, out: self.out, site: p0, row: k, pos: 0, loc });
    }
    #[track_caller]
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(&mut self, _: Space, f: F) {
        let loc = Location::caller();
        let q = self.sub as usize;
        self.sub += 1;
        self.out.seq.push(Part::Sub(q as u32));
        let Some(st) = self.st.subs.get(q) else { return };
        let out = self.out.child(q, st, Kind::Scope, loc);
        f(&mut Learn { st, out, rowc: 0, sub: 0 });
    }
    #[track_caller]
    fn list<R: Row<V>, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, mut f: F) {
        let loc = Location::caller();
        let q = self.sub as usize;
        self.sub += 1;
        self.out.seq.push(Part::Sub(q as u32));
        let Some(st) = self.st.subs.get(q) else { return };
        let info = ListInfo { n: l.n, row_h: l.row_h, x: l.x, y: l.y, shape: l.shape.to_vec() };
        let out = self.out.child(q, st, Kind::List(info), loc);
        if !st.sites.is_empty() && l.n > 0 {
            // any row will do: every row of a list has the type the columns were made for
            let r = if st.hi > st.lo { st.lo } else { 0 };
            f(r as usize).visit(&mut LearnLeaf { st, out, site: 0, row: 0, pos: 0, loc });
        }
    }
}

// ---- the walk ------------------------------------------------------------------------------------------------

/// An entity: the store (child indices from the root), the row (the row call of a fixed shape, or the row of a
/// list), and the position in that row's shape. It names the same thing across scrolling and re-description.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ent {
    pub path: Path,
    pub row: u32,
    pub pos: u32,
}

/// One operand of one record, now.
#[derive(Clone, Copy, Debug)]
pub struct Operand<'a> {
    pub op: &'static Op,
    pub val: Val,
    /// The record words that hold it.
    pub words: &'a [u32],
}
impl Operand<'_> {
    pub fn name(&self) -> &'static str {
        self.op.name
    }
    pub fn stage(&self) -> StageId {
        self.op.stage
    }
    /// The expression it was built from, as the compiler writes its type.
    pub fn ty(&self) -> &'static str {
        (self.op.ty)()
    }
}

/// A live record, as the walk hands it out.
pub struct Item<'w> {
    pub ent: Ent,
    /// Its place in the stores: column and record index (what a pick id names).
    pub col: u32,
    pub rec: u32,
    /// The layout's answer, in the store's space.
    pub rect: Rect,
    /// On screen: shifted by the space's origin and the paint's own offset, not cut by the clip.
    pub screen: Rect,
    pub clip: Rect,
    pub info: &'w ColInfo,
    pub site: &'w SiteInfo,
    ptr: *const u8,
    u: &'w U,
}
impl<'w> Item<'w> {
    pub fn kind(&self) -> &'static str {
        self.info.kind
    }
    pub fn location(&self) -> &'static Location<'static> {
        self.site.loc
    }
    /// The record's words as stored.
    pub fn words(&self) -> &'w [u32] {
        // SAFETY: `ptr` is a live record of the column, whose size is a whole number of words (`Words`).
        unsafe { core::slice::from_raw_parts(self.ptr as *const u32, self.rec_words()) }
    }
    fn rec_words(&self) -> usize {
        self.info.ops.iter().map(|o| (o.word + o.words) as usize).max().unwrap_or(0)
    }
    pub fn ops(&self) -> impl Iterator<Item = Operand<'w>> + '_ {
        let words = self.words();
        self.info.ops.iter().enumerate().map(move |(i, op)| Operand {
            op,
            // SAFETY: `ptr` is a record of the archetype `val` was instantiated for.
            val: unsafe { (self.info.val)(self.ptr, i, self.u) },
            words: &words[op.word as usize..(op.word + op.words) as usize],
        })
    }
    pub fn get(&self, name: &str) -> Option<Operand<'w>> {
        self.ops().find(|o| o.op.name == name)
    }
    /// The first operand that is content (a text or a number), as text.
    pub fn text(&self) -> Option<String> {
        self.ops().find(|o| o.val.is_content()).map(|o| o.val.text())
    }
    /// The record shows something else under `new` than under `old` (the core's own derivative).
    pub fn changed(&self, old: &U, new: &U) -> bool {
        // SAFETY: as `ops`.
        unsafe { (self.info.changed)(self.ptr, old, new) }
    }
    pub fn visible(&self) -> bool {
        !self.screen.and(self.clip).empty()
    }
    /// An owned description of the record.
    pub fn snap(&self) -> Snap {
        // SAFETY: `ptr` is a record of the column's archetype.
        let off = if self.info.moves { unsafe { (self.info.offset)(self.ptr, self.u) } } else { (0, 0) };
        Snap { ent: self.ent, kind: self.info.kind, on: self.info.on, words: self.words().to_vec(), loc: self.site.loc, rect: self.rect, off, ops: self.info.ops, vals: self.ops().map(|o| o.val).collect() }
    }
}

/// A record described, owned: from the stores ([`Item::snap`]) or from the view alone ([`Reflect::rows`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    pub ent: Ent,
    pub kind: &'static str,
    /// Its handlers, if any.
    pub on: Option<Hov>,
    /// The record's words.
    pub words: Vec<u32>,
    pub loc: &'static Location<'static>,
    /// In the store's space.
    pub rect: Rect,
    /// Where its paint moves it, under the block it was described with (zero for a record that does not move).
    pub off: (i32, i32),
    pub ops: &'static [Op],
    pub vals: Vec<Val>,
}
impl PartialEq for Op {
    fn eq(&self, o: &Op) -> bool {
        core::ptr::eq(self, o)
    }
}
impl Snap {
    pub fn get(&self, name: &str) -> Option<Val> {
        self.ops.iter().position(|o| o.name == name).map(|i| self.vals[i])
    }
    pub fn text(&self) -> Option<String> {
        self.vals.iter().find(|v| v.is_content()).map(|v| v.text())
    }
}

/// Reflection of one world: what the view's types said, kept beside the stores (not in them). It is tied to the
/// store tree it was learned from: every use checks it is that tree, with the columns it learned, of the record
/// types it learned (admission, per column; never per record), and refuses anything else.
pub struct Reflect {
    pub root: StoreInfo,
    store: *const Store,
}

impl Reflect {
    /// Describe the view once with the reflecting sink. Run it again after the store tree grows (a list that
    /// was empty got rows); [`Reflect::current`] says whether it has.
    pub fn learn<V: View>(root: &Store, v: &V) -> Reflect {
        let mut out = StoreInfo::new(Kind::Root, None);
        out.sites.resize_with(root.sites.len(), || None);
        out.cols.resize_with(root.cols.len(), || None);
        v.view(&mut Learn { st: root, out: &mut out, rowc: 0, sub: 0 });
        out.finish();
        Reflect { root: out, store: root }
    }

    /// Whether this reflection applies to `root`: the tree it was learned from, every column and site known,
    /// and every learned column still of the record type it was learned for.
    pub fn current(&self, root: &Store) -> bool {
        fn same(i: &StoreInfo, s: &Store) -> bool {
            i.cols.len() == s.cols.len()
                && i.sites.len() == s.sites.len()
                && i.subs.len() == s.subs.len()
                && i.cols.iter().zip(&s.cols).all(|(a, c)| a.as_ref().is_none_or(|a| a.arch == c.arch))
                && i.subs.iter().zip(&s.subs).all(|(a, b)| same(a, b))
        }
        core::ptr::eq(self.store, root) && same(&self.root, root)
    }
    /// Records the view described whose columns were not made for their type (closures inside `view`).
    pub fn refused(&self) -> u32 {
        fn go(i: &StoreInfo) -> u32 {
            i.refused + i.subs.iter().map(go).sum::<u32>()
        }
        go(&self.root)
    }

    /// Every list of the view: its store path, what it was described with, where.
    pub fn lists(&self) -> Vec<(Path, &ListInfo, &'static Location<'static>)> {
        fn go<'a>(s: &'a StoreInfo, p: &mut Path, out: &mut Vec<(Path, &'a ListInfo, &'static Location<'static>)>) {
            if let (Kind::List(l), Some(loc)) = (&s.kind, s.loc) {
                out.push((*p, l, loc));
            }
            for (q, c) in s.subs.iter().enumerate() {
                if (p.n as usize) < p.at.len() {
                    p.at[p.n as usize] = q as u16;
                    p.n += 1;
                    go(c, p, out);
                    p.n -= 1;
                }
            }
        }
        let mut out = Vec::new();
        go(&self.root, &mut Path::default(), &mut out);
        out
    }

    /// Every live record in paint order (the order the stores paint: a column as a whole, child stores where
    /// the view described them).
    pub fn each<'w>(&'w self, root: &'w Store, u: &'w U, mut f: impl FnMut(&Item<'w>)) {
        assert!(self.current(root), "reflection used on a store tree it was not learned from (or that changed shape): learn again");
        walk(&self.root, root, u, &mut Path::default(), &mut f);
    }

    /// Every live record, collected.
    ///
    /// Panics if `root` is not the tree this reflection was learned from, or has changed shape since: its typed
    /// readers would read other records as their types.
    pub fn items<'w>(&'w self, root: &'w Store, u: &'w U) -> Vec<Item<'w>> {
        assert!(self.current(root), "reflection used on a store tree it was not learned from (or that changed shape): learn again");
        let mut v = Vec::new();
        walk_owned(&self.root, root, u, &mut Path::default(), &mut v);
        v
    }

    /// The records under device point (x, y), topmost first: any record, with a handler or not.
    pub fn stack<'w>(&'w self, root: &'w Store, u: &'w U, x: i32, y: i32) -> Vec<Item<'w>> {
        let mut v: Vec<Item<'w>> = self.items(root, u).into_iter().filter(|it| it.screen.and(it.clip).hits(Rect { x0: x, y0: y, x1: x + 1, y1: y + 1 })).collect();
        v.reverse();
        v
    }

    /// The live record of an entity, if it is materialised.
    pub fn find<'w>(&'w self, root: &'w Store, u: &'w U, e: Ent) -> Option<Item<'w>> {
        self.items(root, u).into_iter().find(|it| it.ent == e)
    }

    /// Rows `rows` of the list at `path`, described from the view's row function: whether or not they are
    /// materialised, with nothing written and nothing scrolled. The rects are in the list's store space.
    pub fn rows<V: View>(v: &V, u: &U, path: &Path, rows: Range<u32>) -> Vec<Snap> {
        let mut out = Vec::new();
        let mut d = Detach { at: Path::default(), want: path, rows, u, out: &mut out, sub: 0 };
        v.view(&mut d);
        out
    }
}

fn walk<'w>(info: &'w StoreInfo, st: &'w Store, u: &'w U, path: &mut Path, f: &mut impl FnMut(&Item<'w>)) {
    let mut v = Vec::new();
    walk_owned(info, st, u, path, &mut v);
    for it in &v {
        f(it);
    }
}

fn walk_owned<'w>(info: &'w StoreInfo, st: &'w Store, u: &'w U, path: &mut Path, out: &mut Vec<Item<'w>>) {
    let org = st.space.origin(u);
    for p in &st.order {
        match *p {
            Pass::Col(j) => {
                let (c, Some(Some(ci))) = (&st.cols[j as usize], info.cols.get(j as usize)) else { continue };
                let live = Live { cap: st.cap, rows: &st.row_live, nrec: c.nrec };
                for pg in 0..c.npages {
                    let mut m = live.page(pg as usize);
                    while m != 0 {
                        let i = m.trailing_zeros();
                        m &= m - 1;
                        let rec = pg * 64 + i;
                        let (k, slot) = (rec / st.cap, rec % st.cap);
                        let Some(&si) = ci.sites.get(k as usize) else { continue };
                        let Some(Some(site)) = info.sites.get(si as usize) else { continue };
                        let row = if st.cap == 1 {
                            site.row
                        } else if cfg!(feature = "mutant_reflect") {
                            slot
                        } else {
                            // the row of the window that lives at this slot
                            st.lo + (slot + st.cap - st.lo % st.cap) % st.cap
                        };
                        let ptr = c.rec_ptr(rec);
                        // SAFETY: a site of this store, `slot` under its capacity, pointers current.
                        let rect = unsafe { st.sites[si as usize].rect(slot) };
                        // SAFETY: `ptr` is a record of the column's archetype.
                        let off = if ci.moves { unsafe { (ci.offset)(ptr, u) } } else { (0, 0) };
                        let screen = rect.shift(org).shift(off);
                        out.push(Item { ent: Ent { path: *path, row, pos: site.pos }, col: j, rec, rect, screen, clip: st.space.clip, info: ci, site, ptr, u });
                    }
                }
            }
            Pass::Sub(q) => {
                let (Some(si), Some(ss)) = (info.subs.get(q as usize), st.subs.get(q as usize)) else { continue };
                assert!((path.n as usize) < path.at.len(), "stores nest at most six deep");
                path.at[path.n as usize] = q as u16;
                path.n += 1;
                walk_owned(si, ss, u, path, out);
                path.n -= 1;
            }
        }
    }
}

// ---- rows on demand: the view described for rows nobody materialised ------------------------------------------

struct Detach<'a> {
    at: Path,
    want: &'a Path,
    rows: Range<u32>,
    u: &'a U,
    out: &'a mut Vec<Snap>,
    sub: u32,
}
impl Detach<'_> {
    /// The path of the next child, and whether the wanted store is it or under it.
    fn next(&mut self) -> (Path, bool) {
        let q = self.sub;
        self.sub += 1;
        let mut p = self.at;
        if (p.n as usize) < p.at.len() {
            p.at[p.n as usize] = q as u16;
            p.n += 1;
        }
        let under = p.n <= self.want.n && p.at[..p.n as usize] == self.want.at[..p.n as usize];
        (p, under)
    }
}
struct DLeaf<'a> {
    ent: Ent,
    loc: &'static Location<'static>,
    shape: &'a [Rect],
    at: (i32, i32),
    u: &'a U,
    out: &'a mut Vec<Snap>,
}
impl<V> Leaf<V> for DLeaf<'_> {
    fn leaf<A: Arch>(&mut self, a: &A, _: TypeId, hov: Option<Hov>) {
        let k = self.ent.pos as usize;
        let vals = (0..A::OPS.len()).map(|i| a.val(i, self.u)).collect();
        let off = if A::MOVES { a.offset(self.u) } else { (0, 0) };
        self.out.push(Snap { ent: self.ent, kind: A::NAME, on: hov, words: a.words().to_vec(), loc: self.loc, rect: self.shape.get(k).map_or(Rect::default(), |r| r.shift(self.at)), off, ops: A::OPS, vals });
        self.ent.pos += 1;
    }
}
impl<'r, 'a, V> ScopeOf<'r, V> for Detach<'a> {
    type Scope = Detach<'r>;
}
impl<'a, V> Sink<V> for Detach<'a> {
    #[track_caller]
    fn row<R: Row<V>, L: AsRef<[Rect]>, F: FnOnce() -> (L, R)>(&mut self, _: F) {}
    #[track_caller]
    fn scope<F: for<'r> FnOnce(&mut <Self as ScopeOf<'r, V>>::Scope)>(&mut self, _: Space, f: F) {
        let (p, under) = self.next();
        if under {
            f(&mut Detach { at: p, want: self.want, rows: self.rows.clone(), u: self.u, out: self.out, sub: 0 });
        }
    }
    #[track_caller]
    fn list<R: Row<V>, F: FnMut(usize) -> R>(&mut self, l: &List<'_>, mut f: F) {
        let loc = Location::caller();
        let (p, _) = self.next();
        if p != *self.want {
            return;
        }
        for r in self.rows.start..self.rows.end.min(l.n) {
            let at = (l.x, l.y + r as i32 * l.row_h);
            f(r as usize).visit(&mut DLeaf { ent: Ent { path: p, row: r, pos: 0 }, loc, shape: l.shape, at, u: self.u, out: self.out });
        }
    }
}
