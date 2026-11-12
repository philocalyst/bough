//! Reflection with no opt-in: what the compiler and the runtime already know, typed.
//!
//! * **Values.** Any value, recursively, from rustc's compile-time type reflection (`#![feature(type_info)]`):
//!   every struct, tuple, enum (its variants and their fields), array and primitive, with the compiler's
//!   names and offsets, and per node the vtables of `Debug` (read), bough's [`Edit`] (write from text), a
//!   blanket `Meta` (the type's name) and bough's `Items` (a container's children: `Vec`, `Option`, `Box`,
//!   `Cell`, `RefCell`, `Rc`, `Arc`, `List`, `Reader`), each found by `TypeId` at compile time. No derive, no
//!   registration, nothing in component crates. A type without `Debug` or `Edit` is still walked by its
//!   structure; std's containers are opaque (shown by `Debug`, opened through `Items`).
//!   A reactive cell (a struct whose first field is a [`WireHead`](crate::signal::WireHead): `Signal`,
//!   `List`, `Reader`, `Entities`) carries its head, so an edit through reflection queues the cell's wires and
//!   runs as an ordinary write in a turn (its damage, re-description and effects included).
//! * **Elements.** Every element records its builder's type name on its record's handle at mount.
//! * **Records.** The Bough's records (preorder, depth, kind, ink, clip) with, where they exist, the layout
//!   box, its resolved style (cephal's `Style`, itself reflected and editable), the quad's paint and the text.
//!
//! The schema is built at compile time (`schema::<T>()`, a `const` per type); a `TypeId` from it never leaves
//! this module (no downcasting: `type_info`'s ids are not lifetime-checked).
//!
//! **Enums.** The schema lists every variant and its fields, but a value walk does not open an enum: which
//! variant is active is not a variant index the compiler hands out (`discriminant_value` is the declared
//! discriminant), so reading a variant's fields could read the wrong variant. Enums show by `Debug`;
//! `Option` opens through `Items` (its own code knows the variant).
use std::fmt::Debug;
use std::mem::type_info::TypeKind;
use std::ptr::DynMetadata;

/// Editing a value from text: bough's own trait (numbers, `bool`, `char`, `String`, colours), found for any
/// field by its type at compile time. Implement it for a type of your own to make it editable (optional).
pub trait Edit {
    fn edit_str(&mut self, s: &str) -> bool;
}
macro_rules! edit_parse {
    ($($t:ty),*) => {$(
        impl Edit for $t {
            fn edit_str(&mut self, s: &str) -> bool {
                match s.trim().parse::<$t>() {
                    Ok(v) => {
                        *self = v;
                        true
                    }
                    Err(_) => false,
                }
            }
        }
    )*};
}
edit_parse!(f32, f64, i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, bool, char);
impl Edit for String {
    fn edit_str(&mut self, s: &str) -> bool {
        self.clear();
        self.push_str(s);
        true
    }
}
impl Edit for crate::color::Color {
    /// `#rrggbb` or `#aarrggbb`.
    fn edit_str(&mut self, s: &str) -> bool {
        let h = s.trim().trim_start_matches('#');
        match u32::from_str_radix(h, 16) {
            Ok(v) if h.len() == 6 => {
                *self = crate::color::rgb(v);
                true
            }
            Ok(v) if h.len() == 8 => {
                *self = crate::color::Color(v);
                true
            }
            _ => false,
        }
    }
}

/// What every type answers (a blanket impl: reachable for any field type by its `TypeId`). Raw-pointer
/// receivers: it never reads `self`, so the schema can name a type it has no value of.
trait Meta {
    fn type_name(self: *const Self) -> &'static str;
}
impl<T> Meta for T {
    fn type_name(self: *const Self) -> &'static str {
        std::any::type_name::<T>()
    }
}

/// Types the value walk does not open structurally: std's containers and bough's cells (their fields are
/// implementation). Shown by `Debug`; their children, if any, come from `Items`.
pub(crate) trait Opaque {}
impl Opaque for String {}
impl Opaque for std::ffi::OsString {}
impl Opaque for std::path::PathBuf {}
impl Opaque for std::time::Duration {}
impl Opaque for std::time::Instant {}
impl<T> Opaque for Vec<T> {}
impl<T> Opaque for std::collections::VecDeque<T> {}
impl<K, V, S> Opaque for std::collections::HashMap<K, V, S> {}
impl<K, S> Opaque for std::collections::HashSet<K, S> {}
impl<K, V> Opaque for std::collections::BTreeMap<K, V> {}
impl<K> Opaque for std::collections::BTreeSet<K> {}
impl<T> Opaque for Option<T> {}
impl<T: ?Sized> Opaque for Box<T> {}
impl<T: ?Sized> Opaque for std::rc::Rc<T> {}
impl<T: ?Sized> Opaque for std::sync::Arc<T> {}
impl<T: ?Sized> Opaque for std::cell::Cell<T> {}
impl<T: ?Sized> Opaque for std::cell::RefCell<T> {}
impl<T: ?Sized> Opaque for std::cell::UnsafeCell<T> {}
impl<T: ?Sized> Opaque for std::marker::PhantomData<T> {}
impl<T: ?Sized> Opaque for std::sync::Mutex<T> {}
impl<T: ?Sized> Opaque for std::sync::RwLock<T> {}
impl<T> Opaque for crate::list::List<T> {}
impl<T> Opaque for crate::reader::Reader<T> {}
impl<T> Opaque for crate::entity::Entities<T> {}
macro_rules! opaque_atomic {
    ($($t:ident),*) => {$(impl Opaque for std::sync::atomic::$t {})*};
}
opaque_atomic!(AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, AtomicI8, AtomicI16, AtomicI32, AtomicI64, AtomicIsize);

/// A container's children, each a value of the type `schema` describes.
trait Items {
    /// The schema of the children.
    fn item_schema(self: *const Self) -> &'static Schema;
    /// Number of children.
    ///
    /// # Safety
    /// `self` points at a live value.
    unsafe fn len(self: *const Self) -> usize;
    /// Child `i < len`.
    ///
    /// # Safety
    /// `self` points at a live value and `i < len`.
    unsafe fn item(self: *const Self, i: usize) -> *const u8;
    /// Can a child be written in place (not behind shared ownership)?
    fn writable(self: *const Self) -> bool {
        true
    }
    /// Child `i` was written through reflection (a `List` logs the edit so its row re-patches).
    ///
    /// # Safety
    /// `self` points at a live value nothing else borrows, derived from a unique borrow, and `i < len`.
    unsafe fn touched(self: *const Self, _i: usize) {}
}
macro_rules! items_common {
    () => {
        fn item_schema(self: *const Self) -> &'static Schema {
            schema::<T>()
        }
    };
}
impl<T> Items for Vec<T> {
    items_common!();
    unsafe fn len(self: *const Self) -> usize {
        // SAFETY: a live `Vec` (the contract).
        unsafe { (*self).len() }
    }
    unsafe fn item(self: *const Self, i: usize) -> *const u8 {
        // SAFETY: `i < len` (the contract); the element pointer keeps the buffer's provenance.
        unsafe { (*self).as_ptr().add(i) as *const u8 }
    }
}
impl<T> Items for Option<T> {
    items_common!();
    unsafe fn len(self: *const Self) -> usize {
        // SAFETY: a live `Option`.
        unsafe { (*self).is_some() as usize }
    }
    unsafe fn item(self: *const Self, _i: usize) -> *const u8 {
        // SAFETY: `len` was 1, so it is `Some`; `as_ref` is `Option`'s own variant test.
        unsafe { (*self).as_ref().unwrap_unchecked() as *const T as *const u8 }
    }
}
impl<T> Items for Box<T> {
    items_common!();
    unsafe fn len(self: *const Self) -> usize {
        1
    }
    unsafe fn item(self: *const Self, _i: usize) -> *const u8 {
        // SAFETY: a live `Box` owns its value.
        unsafe { &**self as *const T as *const u8 }
    }
}
impl<T> Items for std::rc::Rc<T> {
    items_common!();
    unsafe fn len(self: *const Self) -> usize {
        1
    }
    unsafe fn item(self: *const Self, _i: usize) -> *const u8 {
        // SAFETY: a live `Rc`.
        unsafe { &**self as *const T as *const u8 }
    }
    fn writable(self: *const Self) -> bool {
        false
    }
}
impl<T> Items for std::sync::Arc<T> {
    items_common!();
    unsafe fn len(self: *const Self) -> usize {
        1
    }
    unsafe fn item(self: *const Self, _i: usize) -> *const u8 {
        // SAFETY: a live `Arc`.
        unsafe { &**self as *const T as *const u8 }
    }
    fn writable(self: *const Self) -> bool {
        false
    }
}
impl<T> Items for std::cell::Cell<T> {
    items_common!();
    unsafe fn len(self: *const Self) -> usize {
        1
    }
    unsafe fn item(self: *const Self, _i: usize) -> *const u8 {
        // SAFETY: a live `Cell`; reflection runs outside every borrow of the owner (`&mut App`).
        unsafe { (*self).as_ptr() as *const u8 }
    }
}
impl<T> Items for std::cell::RefCell<T> {
    items_common!();
    unsafe fn len(self: *const Self) -> usize {
        // SAFETY: a live `RefCell`; a value borrowed mutably right now (a leaked guard) is not opened.
        unsafe { (*self).try_borrow_mut().is_ok() as usize }
    }
    unsafe fn item(self: *const Self, _i: usize) -> *const u8 {
        // SAFETY: a live `RefCell` with no borrow (`len`).
        unsafe { (*self).as_ptr() as *const u8 }
    }
}
impl<T> Items for crate::list::List<T> {
    items_common!();
    unsafe fn len(self: *const Self) -> usize {
        // SAFETY: a live `List`.
        unsafe { (*self).len() }
    }
    unsafe fn item(self: *const Self, i: usize) -> *const u8 {
        // SAFETY: a live `List`, `i < len`.
        unsafe { (*self).item_ptr(i) as *const u8 }
    }
    unsafe fn touched(self: *const Self, i: usize) {
        // SAFETY: the contract: a unique, writable pointer to a live `List`.
        unsafe { (*(self as *mut Self)).mark_edit_pos(i) }
    }
}
impl<T> Items for crate::reader::Reader<T> {
    items_common!();
    unsafe fn len(self: *const Self) -> usize {
        // SAFETY: a live `Reader`.
        unsafe { (*self).has_value() as usize }
    }
    unsafe fn item(self: *const Self, _i: usize) -> *const u8 {
        // SAFETY: a live `Reader` with a value (`len`).
        unsafe { (*self).value_ptr() as *const u8 }
    }
    fn writable(self: *const Self) -> bool {
        false
    }
}

/// What a schema node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Struct,
    Tuple,
    Enum,
    /// One variant of its parent enum; its children are the variant's fields.
    Variant,
    Array,
    Union,
    Bool,
    Char,
    Int,
    Float,
    Str,
    Slice,
    Ref,
    Ptr,
    FnPtr,
    Dyn,
    Other,
}

#[derive(Clone, Copy)]
struct Vt {
    meta: Option<DynMetadata<dyn Meta>>,
    debug: Option<DynMetadata<dyn Debug>>,
    edit: Option<DynMetadata<dyn Edit>>,
    items: Option<DynMetadata<dyn Items>>,
}

/// One node of a type's schema (preorder; a node's subtree is `index + 1 .. end`).
#[derive(Clone, Copy)]
pub struct Node {
    pub kind: Kind,
    /// Field name (`"0"`, `"1"` for tuple fields), variant name, `"[]"` for an array's element, `""` for the root.
    pub name: &'static str,
    pub depth: u8,
    /// One past this node's subtree.
    pub end: u16,
    /// Byte offset from the parent's start (an array's element: element 0, `size` apart; a variant's field:
    /// from the enum's start, meaningful only while that variant is active).
    pub offset: usize,
    /// Fields of a struct, tuple, union or variant; variants of an enum; length of an array.
    pub count: usize,
    /// Size of the node's type in bytes.
    pub size: usize,
    /// A reactive cell: its wire head's offset from this node's start (a `Signal`'s node is its value, so
    /// the head sits before it).
    pub head: Option<isize>,
    /// Not opened by a value walk (see `Opaque`).
    pub opaque: bool,
    vt: Vt,
}
impl Node {
    /// The node's type name.
    pub fn type_name(&self) -> &'static str {
        match self.vt.meta {
            // SAFETY: `Meta` never reads `self` (raw-pointer receiver, blanket impl), so a null data pointer
            // with the type's own vtable is fine.
            Some(vt) => {
                let m: *const dyn Meta = std::ptr::from_raw_parts(std::ptr::null::<()>(), vt);
                m.type_name()
            }
            None => "?",
        }
    }
    pub fn has_debug(&self) -> bool {
        self.vt.debug.is_some()
    }
    pub fn has_edit(&self) -> bool {
        self.vt.edit.is_some()
    }
    pub fn has_items(&self) -> bool {
        self.vt.items.is_some()
    }
}

/// Nodes a schema holds at most (a deeper or wider type is cut, `truncated`).
pub const SCHEMA_CAP: usize = 256;
const MAX_DEPTH: u8 = 12;

/// A type's schema, built at compile time from `type_info`.
#[derive(Clone, Copy)]
pub struct Schema {
    pub n: usize,
    pub truncated: bool,
    pub nodes: [Node; SCHEMA_CAP],
}
impl Schema {
    pub fn nodes(&self) -> &[Node] {
        &self.nodes[..self.n]
    }
    /// One line per node: indent, name, kind, offset, size, type, then `debug`/`edit`/`items`/`opaque`/
    /// `head@k` flags (the schema gate compares this against hand-written expectations).
    pub fn dump(&self) -> String {
        let mut out = String::new();
        for nd in self.nodes() {
            use std::fmt::Write;
            let _ = write!(out, "{}{}: {:?} @{} [{}] {}", "  ".repeat(nd.depth as usize), nd.name, nd.kind, nd.offset, nd.size, nd.type_name());
            if !nd.opaque && matches!(nd.kind, Kind::Struct | Kind::Tuple | Kind::Enum | Kind::Variant | Kind::Array | Kind::Union) {
                let _ = write!(out, " n={}", nd.count);
            }
            for (on, f) in [(nd.has_debug(), " debug"), (nd.has_edit(), " edit"), (nd.has_items(), " items"), (nd.opaque, " opaque")] {
                if on {
                    out.push_str(f);
                }
            }
            if let Some(h) = nd.head {
                let _ = write!(out, " head@{h}");
            }
            out.push('\n');
        }
        out
    }
}

/// The schema of `T`, built once at compile time.
pub fn schema<T>() -> &'static Schema {
    const { &build(std::mem::type_info::of::<T>()) }
}

#[rustc_comptime]
fn build(t: std::any::TypeId) -> Schema {
    let blank = Node { kind: Kind::Other, name: "", depth: 0, end: 0, offset: 0, count: 0, size: 0, head: None, opaque: false, vt: Vt { meta: None, debug: None, edit: None, items: None } };
    let mut s = Schema { n: 0, truncated: false, nodes: [blank; SCHEMA_CAP] };
    emit(&mut s, t, "", 0, 0, None);
    s
}

/// Append the node for a value of type `t` named `name` at `off` in its parent, and its subtree.
#[rustc_comptime]
fn emit(s: &mut Schema, t: std::any::TypeId, name: &'static str, off: usize, depth: u8, head: Option<isize>) {
    if s.n >= SCHEMA_CAP {
        s.truncated = true;
        return;
    }
    let opaque = t.trait_info_of::<dyn Opaque>().is_some();
    let kind = kind_of(t);
    // a reactive cell: a struct whose first field is a wire head
    if head.is_none() && matches!(kind, Kind::Struct) && t.fields(0) >= 1 && t.field(0, 0).type_id() == std::mem::type_info::of::<crate::signal::WireHead>() {
        let h = t.field(0, 0).offset() as isize;
        if !opaque {
            // a `Signal`: the node is its `value`
            let nf = t.fields(0);
            let mut k = 1;
            while k < nf {
                let f = t.field(0, k);
                if str_eq(f.name(), "value") {
                    emit(s, f.type_id(), name, off + f.offset(), depth, Some(h - f.offset() as isize));
                    return;
                }
                k += 1;
            }
        }
        emit_node(s, t, kind, name, off, depth, Some(h), opaque);
        return;
    }
    emit_node(s, t, kind, name, off, depth, head, opaque);
}

#[rustc_comptime]
fn emit_node(s: &mut Schema, t: std::any::TypeId, kind: Kind, name: &'static str, off: usize, depth: u8, head: Option<isize>, opaque: bool) {
    let i = s.n;
    s.n += 1;
    let size = match t.size() {
        Some(z) => z,
        None => 0,
    };
    let vt = Vt { meta: vt_meta(t), debug: vt_debug(t), edit: vt_edit(t), items: vt_items(t) };
    let mut count = 0;
    let open = !opaque && depth < MAX_DEPTH;
    match kind {
        Kind::Struct | Kind::Tuple | Kind::Union => {
            count = t.fields(0);
            if open {
                let mut k = 0;
                while k < count {
                    let f = t.field(0, k);
                    emit(s, f.type_id(), f.name(), f.offset(), depth + 1, None);
                    k += 1;
                }
            }
        }
        Kind::Enum => {
            count = t.variants();
            if open {
                let mut v = 0;
                while v < count && s.n < SCHEMA_CAP {
                    let j = s.n;
                    s.n += 1;
                    let nf = t.fields(v);
                    let mut k = 0;
                    while k < nf {
                        let f = t.field(v, k);
                        emit(s, f.type_id(), f.name(), f.offset(), depth + 2, None);
                        k += 1;
                    }
                    let blank = Vt { meta: None, debug: None, edit: None, items: None };
                    s.nodes[j] = Node { kind: Kind::Variant, name: t.variant(v).name(), depth: depth + 1, end: s.n as u16, offset: 0, count: nf, size, head: None, opaque: false, vt: blank };
                    v += 1;
                }
                if v < count {
                    s.truncated = true;
                }
            }
        }
        Kind::Array => {
            count = t.array_len();
            if open {
                if let Some(e) = t.element_ty() {
                    emit(s, e, "[]", 0, depth + 1, None);
                }
            }
        }
        _ => {}
    }
    s.nodes[i] = Node { kind, name, depth, end: s.n as u16, offset: off, count, size, head, opaque, vt };
}

/// Tuples, recognised by a trait: this toolchain's `TypeId::info` ICEs on a tuple type (rustc_abi index
/// out of bounds), so `kind_of` asks this first and never calls `info` on one.
pub(crate) trait IsTuple {}
macro_rules! tuples {
    ($(($($t:ident),*))*) => {$(impl<$($t),*> IsTuple for ($($t,)*) {})*};
}
tuples!(() (A) (A, B) (A, B, C) (A, B, C, D) (A, B, C, D, E) (A, B, C, D, E, F) (A, B, C, D, E, F, G) (A, B, C, D, E, F, G, H) (A, B, C, D, E, F, G, H, I) (A, B, C, D, E, F, G, H, I, J) (A, B, C, D, E, F, G, H, I, J, K) (A, B, C, D, E, F, G, H, I, J, K, L));

#[rustc_comptime]
fn kind_of(t: std::any::TypeId) -> Kind {
    if t.trait_info_of::<dyn IsTuple>().is_some() {
        return Kind::Tuple;
    }
    match t.info().kind {
        TypeKind::Struct => Kind::Struct,
        TypeKind::Tuple => Kind::Tuple,
        TypeKind::Enum => Kind::Enum,
        TypeKind::Array => Kind::Array,
        TypeKind::Union => Kind::Union,
        TypeKind::Bool => Kind::Bool,
        TypeKind::Char => Kind::Char,
        TypeKind::Int => Kind::Int,
        TypeKind::Float => Kind::Float,
        TypeKind::Str(_) => Kind::Str,
        TypeKind::Slice => Kind::Slice,
        TypeKind::Reference => Kind::Ref,
        TypeKind::Pointer => Kind::Ptr,
        TypeKind::FnPtr => Kind::FnPtr,
        TypeKind::DynTrait(_) => Kind::Dyn,
        _ => Kind::Other,
    }
}
#[rustc_comptime]
fn vt_meta(t: std::any::TypeId) -> Option<DynMetadata<dyn Meta>> {
    match t.trait_info_of::<dyn Meta>() {
        Some(v) => Some(v.get_vtable()),
        None => None,
    }
}
#[rustc_comptime]
fn vt_debug(t: std::any::TypeId) -> Option<DynMetadata<dyn Debug>> {
    match t.trait_info_of::<dyn Debug>() {
        Some(v) => Some(v.get_vtable()),
        None => None,
    }
}
#[rustc_comptime]
fn vt_edit(t: std::any::TypeId) -> Option<DynMetadata<dyn Edit>> {
    match t.trait_info_of::<dyn Edit>() {
        Some(v) => Some(v.get_vtable()),
        None => None,
    }
}
#[rustc_comptime]
fn vt_items(t: std::any::TypeId) -> Option<DynMetadata<dyn Items>> {
    match t.trait_info_of::<dyn Items>() {
        Some(v) => Some(v.get_vtable()),
        None => None,
    }
}
#[rustc_comptime]
fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ------------------------------------------------------------------ walking and editing values

/// Array elements and container children a walk shows at most per node.
pub const WALK_ITEMS: usize = 64;

/// One node of a value, as reflection reads it.
#[derive(Clone, Debug)]
pub struct FieldView {
    /// Dotted path from the root (`stats.p50`, `rows[3].title`); the root is `""`.
    pub path: String,
    pub depth: u16,
    pub kind: Kind,
    pub type_name: &'static str,
    /// `Debug` of a leaf, an enum or an opaque value; `None` for an opened struct, or a type without `Debug`
    /// (its structure is still walked).
    pub value: Option<String>,
    /// Has `Edit` and is reachable for writing (not behind shared ownership).
    pub editable: bool,
    /// A reactive cell (an edit runs its wires).
    pub reactive: bool,
    /// Children a container has beyond those shown (`WALK_ITEMS`).
    pub more: usize,
}

/// Every node of `v`, preorder.
pub fn fields_of<T>(v: &T) -> Vec<FieldView> {
    let mut out = Vec::new();
    // SAFETY: `v` is a live `T`, the schema `T`'s; the walk only reads.
    unsafe { visit(schema::<T>(), 0, v as *const T as *const u8, String::new(), 0, true, &mut out) };
    out
}

/// Edit the node of `v` at `path` from text. Returns whether it applied (the path exists, its type has
/// `Edit`, it is writable, and the text parsed). Enclosing reactive cells are queued; an enclosing `List`
/// logs the edit of that row.
pub fn set_in<T>(v: &mut T, path: &str, text: &str) -> bool {
    // SAFETY: `v` is a live `T` uniquely borrowed; the pointer comes from that borrow.
    unsafe { set_raw(schema::<T>(), v as *mut T as *mut u8, path, text) }
}

/// # Safety
/// `p` points at a live value of the type node `i` of `s` describes.
unsafe fn visit(s: &'static Schema, i: usize, p: *const u8, path: String, depth: u16, writable: bool, out: &mut Vec<FieldView>) {
    let nd = &s.nodes[i];
    let shown = nd.opaque || !matches!(nd.kind, Kind::Struct | Kind::Tuple | Kind::Array);
    // SAFETY: `p` and the vtable are the same type's (the schema was built for it).
    let value = if shown { nd.vt.debug.map(|vt| format!("{:?}", unsafe { &*std::ptr::from_raw_parts::<dyn Debug>(p as *const (), vt) })) } else { None };
    let at = out.len();
    out.push(FieldView { path: path.clone(), depth, kind: nd.kind, type_name: nd.type_name(), value, editable: writable && nd.vt.edit.is_some(), reactive: nd.head.is_some(), more: 0 });
    let join = |name: &str| if path.is_empty() { name.to_string() } else { format!("{path}.{name}") };
    if nd.opaque {
        if let Some(vt) = nd.vt.items {
            let c: *const dyn Items = std::ptr::from_raw_parts(p as *const (), vt);
            // SAFETY: `p` is a live value of the container's type (the vtable's).
            let n = unsafe { c.len() };
            let w = writable && c.writable();
            let is = c.item_schema();
            for k in 0..n.min(WALK_ITEMS) {
                // SAFETY: `k < len`; the child is a live value of the item schema's type.
                unsafe { visit(is, 0, c.item(k), format!("{path}[{k}]"), depth + 1, w, out) };
            }
            out[at].more = n.saturating_sub(WALK_ITEMS);
        }
        return;
    }
    match nd.kind {
        Kind::Struct | Kind::Tuple => {
            let mut c = i + 1;
            while c < nd.end as usize {
                let ch = &s.nodes[c];
                // SAFETY: a field at its compiler offset inside the live value.
                unsafe { visit(s, c, p.add(ch.offset), join(ch.name), depth + 1, writable, out) };
                c = ch.end as usize;
            }
        }
        Kind::Array if nd.end as usize > i + 1 => {
            let el = &s.nodes[i + 1];
            for k in 0..nd.count.min(WALK_ITEMS) {
                // SAFETY: element `k < len`, `size` apart.
                unsafe { visit(s, i + 1, p.add(k * el.size), format!("{path}[{k}]"), depth + 1, writable, out) };
            }
            out[at].more = nd.count.saturating_sub(WALK_ITEMS);
        }
        _ => {}
    }
}

/// One step of a path: a field name or an index.
enum Step<'a> {
    Name(&'a str),
    Index(usize),
}
fn steps(path: &str) -> Option<Vec<Step<'_>>> {
    let mut out = Vec::new();
    for part in path.split('.').filter(|p| !p.is_empty()) {
        let (name, mut rest) = match part.find('[') {
            Some(b) => (&part[..b], &part[b..]),
            None => (part, ""),
        };
        if !name.is_empty() {
            out.push(Step::Name(name));
        }
        while let Some(r) = rest.strip_prefix('[') {
            let e = r.find(']')?;
            out.push(Step::Index(r[..e].trim().parse().ok()?));
            rest = &r[e + 1..];
        }
        if !rest.is_empty() {
            return None;
        }
    }
    Some(out)
}

/// # Safety
/// `root` points at a live value of `s`'s type, derived from a unique borrow that nothing else uses
/// while this runs.
pub(crate) unsafe fn set_raw(s: &'static Schema, root: *mut u8, path: &str, text: &str) -> bool {
    let Some(steps) = steps(path) else { return false };
    let (mut sc, mut i, mut p) = (s, 0usize, root);
    // reactive heads to queue and containers to tell, outermost first
    let mut heads: Vec<*const std::cell::Cell<u32>> = Vec::new();
    let mut touched: Vec<(*const dyn Items, usize)> = Vec::new();
    let note = |nd: &Node, p: *mut u8, heads: &mut Vec<*const std::cell::Cell<u32>>| {
        if let Some(h) = nd.head {
            heads.push(p.wrapping_offset(h) as *const std::cell::Cell<u32>);
        }
    };
    note(&sc.nodes[0], p, &mut heads);
    for st in &steps {
        let nd = &sc.nodes[i];
        match *st {
            Step::Name(name) => {
                if nd.opaque || !matches!(nd.kind, Kind::Struct | Kind::Tuple) {
                    return false;
                }
                let mut c = i + 1;
                loop {
                    if c >= nd.end as usize {
                        return false;
                    }
                    if sc.nodes[c].name == name {
                        break;
                    }
                    c = sc.nodes[c].end as usize;
                }
                // SAFETY: a field at its compiler offset inside the live value.
                p = unsafe { p.add(sc.nodes[c].offset) };
                i = c;
            }
            Step::Index(k) => {
                if nd.opaque {
                    let Some(vt) = nd.vt.items else { return false };
                    let c: *const dyn Items = std::ptr::from_raw_parts(p as *const (), vt);
                    // SAFETY: `p` is a live container of the vtable's type.
                    if !c.writable() || k >= unsafe { c.len() } {
                        return false;
                    }
                    touched.push((c, k));
                    // SAFETY: `k < len`; the child keeps the unique borrow's provenance (raw pointers all the way).
                    p = unsafe { c.item(k) } as *mut u8;
                    sc = c.item_schema();
                    i = 0;
                } else if nd.kind == Kind::Array && nd.end as usize > i + 1 && k < nd.count {
                    // SAFETY: element `k < len`.
                    p = unsafe { p.add(k * sc.nodes[i + 1].size) };
                    i += 1;
                } else {
                    return false;
                }
            }
        }
        note(&sc.nodes[i], p, &mut heads);
    }
    let Some(vt) = sc.nodes[i].vt.edit else { return false };
    // SAFETY: the node's type and its `Edit` vtable agree; the unique borrow (the contract) makes this the
    // only access.
    let e: &mut dyn Edit = unsafe { &mut *std::ptr::from_raw_parts_mut::<dyn Edit>(p as *mut (), vt) };
    if !e.edit_str(text) {
        return false;
    }
    for h in heads {
        // SAFETY: the wire head of a reactive cell on the path (its node's `head` offset).
        let h = unsafe { &*h };
        crate::signal::queue(h);
    }
    for (c, k) in touched {
        // SAFETY: a live container on the path, reached from the unique borrow.
        unsafe { c.touched(k) };
    }
    true
}

/// Element type names by record handle (recorded at mount; reflection of the element tree).
#[derive(Default)]
pub struct Reflect {
    pub(crate) types: Vec<&'static str>,
}
impl Reflect {
    #[inline]
    pub(crate) fn elem(&mut self, handle: u32, ty: &'static str) {
        let i = handle as usize;
        if self.types.len() <= i {
            self.types.resize(i + 1, "");
        }
        self.types[i] = ty;
    }
}

// ------------------------------------------------------------------ the view the inspector reads

/// One record, as reflection sees it.
#[derive(Clone, Debug)]
pub struct RecordView {
    pub pos: usize,
    pub depth: u16,
    pub kind: u8,
    /// Absolute device-px ink (x0, y0, x1, y1); empty records report `None`.
    pub ink: Option<[i32; 4]>,
    pub clip: u16,
    pub handle: u32,
    /// The element type that mounted it (a builder type name), if the record has a handle.
    pub element: Option<&'static str>,
    /// The layout box (group-local device px: x, y, w, h), if the record is a layout node.
    pub layout: Option<[f32; 4]>,
    /// The resolved layout style (cephal's), if any.
    pub style: Option<String>,
    /// The quad's colour word or the text's, and the text.
    pub color: Option<u32>,
    pub text: Option<String>,
}

impl<V: crate::app::View> crate::app::App<V> {
    /// Every record in paint preorder with what the runtime knows about it.
    pub fn reflect_records(&self) -> Vec<RecordView> {
        let rt = &self.rt;
        // node by handle (quads and texts carry their record's handle)
        let mut by_handle: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        for (i, n) in rt.nodes.iter().enumerate() {
            // a released node waits for reclamation still `live`, and its handle's index may already name a newer
            // record: only a node whose handle (with its generation) is current speaks for the record
            if n.live && n.handle.idx != 0 && rt.bough.pos_of(n.handle).is_some() {
                by_handle.insert(n.handle.idx, i as u32);
            }
        }
        rt.bough
            .records()
            .iter()
            .enumerate()
            .map(|(pos, r)| {
                let h = r.handle;
                let lay = (h != 0 && rt.lay.info.get(h as usize).is_some_and(|l| l.live)).then(|| {
                    let (x, y, w, hh) = rt.lay.eng.rect(h);
                    [x, y, w, hh]
                });
                let style = lay.map(|_| format!("{:?}", rt.lay.eng.style(h)));
                let node = by_handle.get(&h).copied();
                let (color, text) = match node {
                    Some(n) => {
                        let nd = &rt.nodes[n as usize];
                        let t = (nd.text != crate::rt::NONE).then(|| String::from_utf8_lossy(rt.texts[nd.text as usize].bytes()).into_owned());
                        (Some(nd.color), t)
                    }
                    None => (None, None),
                };
                RecordView {
                    pos,
                    depth: r.depth,
                    kind: r.kind,
                    ink: (!r.ink.is_empty()).then(|| r.ink.to_xyxy()),
                    clip: r.clip,
                    handle: h,
                    element: rt.reflect.types.get(h as usize).copied().filter(|t| !t.is_empty() && h != 0),
                    layout: lay,
                    style,
                    color,
                    text,
                }
            })
            .collect()
    }

    /// Every field of the app's root owner, preorder (its type's compile-time schema).
    pub fn reflect_fields(&self) -> Vec<FieldView> {
        fields_of::<V>(&self.owner)
    }

    /// Edit a field of the root owner from text, as a handler's write in a turn: the cell's wires run, then
    /// effects and re-description, like any write.
    pub fn reflect_set(&mut self, path: &str, value: &str) -> bool {
        let mut ok = false;
        self.turn(|v| ok = set_in(v, path, value));
        ok
    }

    /// The resolved layout style of record `handle` (cephal's `Style`, reflected), if it is a layout node.
    pub fn reflect_style(&self, handle: u32) -> Option<Vec<FieldView>> {
        let rt = &self.rt;
        rt.lay.info.get(handle as usize).is_some_and(|l| l.live).then(|| fields_of(rt.lay.eng.style(handle)))
    }

    /// Edit the layout style of record `handle` (a path into cephal's `Style`): the node is restyled and
    /// its group laid out again at the next flush (a re-description restyles it back from its `Spec`).
    pub fn reflect_set_style(&mut self, handle: u32, path: &str, value: &str) -> bool {
        let rt = &mut self.rt;
        if !rt.lay.info.get(handle as usize).is_some_and(|l| l.live) {
            return false;
        }
        let mut st = rt.lay.eng.style(handle).clone();
        if !set_in(&mut st, path, value) {
            return false;
        }
        crate::layout::restyle_to(rt, handle, st);
        rt.lay_flush();
        true
    }

    /// The unmaterialized runs of rows (typed placeholders: region, rows, extent).
    pub fn reflect_stubs(&self) -> Vec<crate::listview::Stub> {
        self.rt.stubs()
    }

    /// Set the paint colour of the quad drawn by record `handle` (a paint-only write).
    pub fn reflect_set_color(&mut self, handle: u32, c: crate::color::Color) -> bool {
        let Some(n) = self.rt.nodes.iter().position(|n| n.live && n.handle.idx == handle && n.run != crate::rt::NONE && n.text == crate::rt::NONE) else { return false };
        <crate::elem::ApplyBg as crate::elem::Apply>::apply(&mut self.rt, n as u32, &c);
        true
    }
}
