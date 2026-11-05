//! Open item sets and scope-owned entities — no catalog, no weak handles (09 §1–§3, L5, L11).
//!
//! Two "leaky abstraction" smells resolved by reaching for the ecosystem instead of inventing:
//!
//!   1. An "item catalog" is genuinely uncommon, and the open-set problem is THREE cases, only the
//!      last of which needs anything special — none of it framework-owned:
//!        - closed set  → a plain enum (the linker resolves it);
//!        - open, one binary → `typetag` (polymorphic serde by string tag — the ecosystem's answer);
//!        - dynamic `.so`/`.dll` → `abi_stable` (the stable-ABI problem; rare; we don't roll our own).
//!      `ItemInstanceId` is framework-minted like `Slot<T>` and NEVER named in item code.
//!
//!   2. `WeakEntity` is deleted. `Entity<T>` is a scope-owned generational CAPABILITY, not a
//!      refcounted owning pointer, so a handle neither owns nor keeps-alive its target — there are
//!      no ownership cycles to break, hence no strong/weak axis. Cyclic construction uses ONE type.
//!
//! Syntax sketch against final/09; not expected to compile against current GPUI.

use gpui::prelude::*;

// ════════════════════════════════════════════════════════════════════════════════════════════════
// (1a) CLOSED SET — a plain enum. This is the 95% case; no registry, no tags, no dynamic dispatch.
// ════════════════════════════════════════════════════════════════════════════════════════════════

enum Tab { Editor(EditorItem), Terminal(TerminalItem) }

impl View for Tab {
    fn view(&self) -> El<'_> {
        match self {                          // exhaustive; arms unify as `El<'_>`, no `el(..)` join
            Tab::Editor(e) => e.view(),
            Tab::Terminal(t) => t.view(),
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// (1b) OPEN, ONE BINARY — `typetag`. "Restore a `Box<dyn Item>` from a string tag" is exactly what
// the serde ecosystem already solves. An item is `View + Serialize + Deserialize` with a tag, plus
// a `KIND` version for migration. There is NO `catalog.register(..)` and NO `ItemSpec` bag.
// ════════════════════════════════════════════════════════════════════════════════════════════════

#[typetag::serde(tag = "kind")]           // the string ↔ type registry IS typetag, populated at link
trait Item: View {
    const KIND_VERSION: u32 = 1;          // bump for a breaking journal change; migrate via serde
    fn locator(&self) -> Locator;         // how to re-acquire the domain object (a path, a session)
    fn follow(&self) -> Option<Follow> { None }   // presence hook; defaulted off (09 §4)
}

#[derive(Serialize, Deserialize)]         // deserialize IS construct — restore = re-derivation (L11)
struct EditorItem {
    // Only these bytes are item-owned. `kind` (the tag) is typetag's; the per-tab instance id is
    // the FRAMEWORK's, minted like a Slot and never spelled here (09 §3).
    locator: EditorLocator,
    view_state: EditorViewState,
    // The domain object is re-acquired from `locator` by an async producer declared at construction;
    // the SAME producer runs on first open and on restore (L11). No separate "load" path exists.
    #[serde(skip)]
    document: Option<Reader<Document>>,
}

#[typetag::serde(name = "zed.editor")]
impl Item for EditorItem {
    const KIND_VERSION: u32 = 3;
    fn locator(&self) -> Locator { Locator::Editor(self.locator.clone()) }
}

impl View for EditorItem {
    fn view(&self) -> El<'_> {
        boundary()
            .loading(|| text("Opening…").text_muted())
            .child(match &self.document {
                Some(doc) => doc.project(|d| editor_surface(d)),
                None => text("no document").into(),
            })
    }
}

// A terminal item is a different tag in the same tag-space; two crates can each add items with no
// central list — that is the whole point of a distributed, link-time registry.
#[derive(Serialize, Deserialize)]
struct TerminalItem { session: SessionDescriptor }
#[typetag::serde(name = "zed.terminal")]
impl Item for TerminalItem {
    fn locator(&self) -> Locator { Locator::Terminal(self.session.clone()) }
}
impl View for TerminalItem {
    fn view(&self) -> El<'_> { terminal_surface(&self.session) }
}

// (1c) DYNAMIC PLUGIN — not shown as code because it is a DIFFERENT problem: a separately compiled
// `.so`/`.dll` crossing a C-stable ABI. That is `abi_stable`/`stabby`'s job (a versioned manifest +
// `AnyEntity` proving the item protocol), confined to that one seam. It is rare, and the framework
// contributes no bespoke ABI layer for it (09 §2, §5).

// ════════════════════════════════════════════════════════════════════════════════════════════════
// (2) SCOPE-OWNED ENTITIES — one type, cyclic construction, no weak handles.
// ════════════════════════════════════════════════════════════════════════════════════════════════

struct Editor {
    document: Entity<Document>,
    me: Entity<Editor>,       // a handle to MYSELF. Held as a field, it does NOT leak: a capability
                              // neither owns nor keeps-alive its target (09 §1), so a self-held or
                              // mutually-held `Entity` is not a cycle to break — the SCOPE frees it.
}

impl Editor {
    // `new_cyclic` mints the generational key before `T` exists — the same move `List` makes when a
    // `Slot` is minted ahead of its value (01 §4). `me` is a live capability whose slot is empty
    // until this returns; a send/inspect through it DURING construction generation-fails cleanly
    // (`Delivery::Closed`), so a partial `T` is never observed. It is a plain `Entity<Editor>` —
    // there is no `WeakEntity`, no `downgrade`, no `upgrade`.
    fn build(document: Entity<Document>) -> Entity<Editor> {
        Entity::new_cyclic(|me: Entity<Editor>| Editor { document, me })
    }

    fn refresh(&mut self) { /* .. */ }
}

fn use_editor(editor: &Entity<Editor>) {
    editor.inspect(|e| { let _ = &e.document; });   // generation-checked read
    editor.send(Editor::refresh);                    // queued; `Delivery::Closed` after scope frees
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typetag_restores_the_right_item_by_tag_no_catalog() {
        let item: Box<dyn Item> = Box::new(EditorItem {
            locator: EditorLocator::path("/src/main.rs"),
            view_state: EditorViewState::default(),
            document: None,
        });
        // Polymorphic round trip via typetag — restore = deserialize; the tab knows its own type
        // again with no framework registry involved.
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains(r#""kind":"zed.editor""#));
        let back: Box<dyn Item> = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.locator(), Locator::Editor(_)));
    }

    #[test]
    fn a_self_held_entity_is_not_a_leak_and_needs_no_weakening() {
        let doc = Entity::new(Document::empty());
        let editor = Editor::build(doc);
        use_editor(&editor);
        // Dropping the last external handle does not free the editor — its owning scope does; and
        // the internal `me` handle never kept it alive. After scope teardown, any surviving
        // capability sends `Delivery::Closed`, never a use-after-free (09 §1, L5).
        let ghost = editor.clone();
        drop(editor);
        assert_eq!(ghost.send(Editor::refresh), Delivery::Closed);
    }
}

fn main() {
    let doc = Entity::new(Document::empty());
    Application::run(Window::document(Tab::Editor(EditorItem {
        locator: EditorLocator::path("/src/main.rs"),
        view_state: EditorViewState::default(),
        document: Some(doc.reader(|d| d.clone()).map_async(|d| async move { d })),
    })));
}
