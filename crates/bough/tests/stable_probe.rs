//! The stable-Rust rungs of the reflection ladder (`docs/lanes/composition/DESIGN.md` §3.2), demonstrated
//! without any feature gate: this file uses no nightly feature and no derive of its own beyond `Debug`.
//!
//! A registration macro expands where the owner's type is concrete, so **autoref specialisation** can pick,
//! per type and at compile time, the richest thunk the type supports (`Debug` here; `Serialize` is the same
//! shape), and a `sig!`-like macro has the field path as tokens, so it can name the state it wires and take
//! read and write thunks for the field's type. No trait bound reaches the component author.
use std::fmt::Debug;

/// What a probe yields for one type: how to show a value of it, if it can be shown.
#[derive(Clone, Copy)]
struct Meta {
    type_name: &'static str,
    fmt: Option<unsafe fn(*const u8) -> String>,
    /// Which rung answered.
    rung: &'static str,
}

struct Probe<T>(std::marker::PhantomData<T>);

/// The rung for types with `Debug`: found first, because method resolution tries `&Probe<T>` before
/// `&&Probe<T>`... the receiver below is `&&Probe<T>`, and this impl is on `&Probe<T>` (one autoref less).
trait ViaDebug {
    fn meta(&self) -> Meta;
}
impl<T: Debug> ViaDebug for &Probe<T> {
    fn meta(&self) -> Meta {
        unsafe fn fmt<T: Debug>(p: *const u8) -> String {
            // SAFETY: the caller hands a pointer to a live `T` (the registry stores it with this thunk).
            format!("{:#?}", unsafe { &*(p as *const T) })
        }
        Meta { type_name: std::any::type_name::<T>(), fmt: Some(fmt::<T>), rung: "debug" }
    }
}
/// The floor: any type at all.
trait ViaNothing {
    fn meta(&self) -> Meta;
}
impl<T> ViaNothing for Probe<T> {
    fn meta(&self) -> Meta {
        Meta { type_name: std::any::type_name::<T>(), fmt: None, rung: "opaque" }
    }
}

/// What `embed!` / `Application::run` would expand to for the owner's type.
macro_rules! probe {
    ($t:ty) => {{
        #[allow(unused_imports)]
        use {ViaDebug as _, ViaNothing as _};
        (&&Probe::<$t>(std::marker::PhantomData)).meta()
    }};
}

/// A wired field's name and thunks, as a `sig!`-like macro can take them: the path from the macro's own tokens,
/// the offset from `offset_of!`, the field's type from a never-called accessor.
struct SigMeta {
    path: &'static str,
    off: usize,
    meta: Meta,
}
fn field_meta<V, T>(_accessor: fn(&V) -> &T, path: &'static str, off: usize, meta: Meta) -> SigMeta {
    SigMeta { path, off, meta }
}
macro_rules! sig_meta {
    ($owner:ty, $ft:ty => $($f:ident).+) => {
        field_meta::<$owner, $ft>(|o: &$owner| &o.$($f).+, concat!(stringify!($owner), ".", stringify!($($f).+)), ::core::mem::offset_of!($owner, $($f).+), probe!($ft))
    };
}

#[derive(Debug)]
enum Filter {
    All,
    Unread { since: u32 },
}
#[derive(Debug)]
struct Inbox {
    selected: u32,
    filter: Filter,
    stats: Stats,
}
#[derive(Debug)]
struct Stats {
    p50: f32,
}
/// A component whose author derived nothing.
struct Bare {
    _n: u64,
}

#[test]
fn the_probe_picks_the_richest_rung_per_type_at_compile_time() {
    let (a, b) = (probe!(Inbox), probe!(Bare));
    assert_eq!((a.rung, b.rung), ("debug", "opaque"));
    assert!(a.type_name.ends_with("Inbox") && b.type_name.ends_with("Bare"));
    assert!(b.fmt.is_none(), "no bound was required of Bare: it is listed by name only");
    let v = Inbox { selected: 3, filter: Filter::Unread { since: 7 }, stats: Stats { p50: 1.5 } };
    // SAFETY: a pointer to a live `Inbox` with `Inbox`'s thunk.
    let shown = unsafe { (a.fmt.unwrap())(&v as *const Inbox as *const u8) };
    // the tree the inspector shows, enum variant and its fields included (read through Debug, never by offset)
    assert!(shown.contains("selected: 3") && shown.contains("Unread {") && shown.contains("since: 7") && shown.contains("p50: 1.5"), "{shown}");
    let all = Inbox { selected: 0, filter: Filter::All, stats: Stats { p50: 0.0 } };
    let shown = unsafe { (a.fmt.unwrap())(&all as *const Inbox as *const u8) };
    assert!(shown.contains("filter: All"));
}

#[test]
fn a_wire_macro_names_the_state_it_wires() {
    let m = sig_meta!(Inbox, f32 => stats.p50);
    assert_eq!(m.path, "Inbox.stats.p50");
    assert_eq!(m.off, std::mem::offset_of!(Inbox, stats) + std::mem::offset_of!(Stats, p50));
    assert_eq!(m.meta.rung, "debug");
    let v = Inbox { selected: 3, filter: Filter::All, stats: Stats { p50: 2.25 } };
    // SAFETY: `off` is the proven offset of an `f32` field of `v`.
    let shown = unsafe { (m.meta.fmt.unwrap())((&v as *const Inbox as *const u8).add(m.off)) };
    assert_eq!(shown, "2.25");
    let _ = (v.selected, &v.filter);
}
