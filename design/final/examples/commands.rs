//! Commands — the function is the identity (04 §3.1–§3.2).
//!
//! The right instinct is "why can't the function itself be the identity?" — and it IS. `Pane::split`
//! is a distinct zero-sized type with a unique path; a key, a click, or a Rust-built menu routes to
//! it with NO name, NO registration, NO `Command` type. That is the common case and it stays bare.
//!
//! The one thing a `fn` cannot do is survive serialization: a keymap in a JSON file names an
//! operation as DATA, and Rust gives a function item no stable runtime representation and no reverse
//! lookup (`"pane.split"` → `Pane::split` needs reflection Rust does not have). So the boundary
//! needs exactly one thing: a `&'static str → dispatch-thunk` table, populated at startup. That is
//! not a bespoke framework type — it is the ecosystem's ordinary link-time registration
//! (`inventory`/`linkme`), the same tool used to register plugins and test cases. No `Command<A>`.
//!
//! Syntax sketch against final/04; not expected to compile against current GPUI.

use gpui::prelude::*;

#[derive(Clone, Copy, Serialize, Deserialize)]
enum Axis { Horizontal, Vertical }

struct Pane {
    ratio: Signal<f32>,
    splits: List<Pane>,
}

impl Pane {
    // ── In-process: the fn IS the identity. No attribute, no name, nothing registered. ──────────
    fn balance(&mut self) { *self.ratio = 0.5; }

    // ── Remappable: one attribute gives it a durable string, because a keymap FILE must name it.
    // The attribute is pure sugar over an `inventory::submit!` you could write by hand (below); it
    // does not introduce a `Command<A>` type. The payload's type safety is the fn's OWN signature,
    // checked where the thunk deserializes the bytes. Renaming the method never breaks the keymap
    // (the id is explicit); deleting the attribute makes it a plain un-named route again.
    #[command("pane.split")]
    fn split(&mut self, axis: Axis) {
        self.splits.push(Pane { ratio: Signal::new(0.5), splits: List::new() });
        let _ = axis;
    }
}

// What `#[command("pane.split")]` desugars to — nothing framework-owned, just the ecosystem slice:
//
// ```rust,ignore
// inventory::submit!(CommandEntry {
//     id: "pane.split",
//     // deserialize the payload as the fn's own parameter type, then dispatch to the route:
//     dispatch: |bytes, target| Pane::split.dispatch(target, from_json::<Axis>(bytes)?),
// });
// ```

impl View for Pane {
    fn view(&self) -> El<'_> {
        column((
            row((text("split"),))
                // A bare key routes to a fn-item with an owned binding — no command name involved.
                .on_key(Key::Backslash, Pane::split.with(Axis::Vertical))
                .on_key(Key::Minus, Pane::split.with(Axis::Horizontal))
                .on_key(Key::Equal, Pane::balance),   // a method path — the idiom (04 §3.1)
            column(&self.splits).gap(4),
        )).gap(8)
    }
}

// ── A keymap file references the STRING; the ecosystem slice resolves it to the typed thunk ──────
//
// The user's `keymap.json` is data, and it is the only place a name is needed:
//
//   { "ctrl-shift-\\": { "command": "pane.split", "args": "Vertical" } }
//
// At load, the dispatcher looks the id up in the `inventory`-populated table and gets a thunk that
// deserializes `"Vertical"` into `Axis` and calls `Pane::split`. The bytes-fit-the-payload check is
// this deserialize — a runtime check either way, because bytes from a file are runtime data.

fn install_keymap(json: &str) -> Keymap {
    // `Keymap::parse` walks entries, resolving each `command` string against the registered slice;
    // an unknown id or a payload that fails to deserialize is a located keymap-load error, never a
    // silent no-op and never a panic.
    Keymap::parse(json).expect("keymap")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> Pane { Pane { ratio: Signal::new(0.5), splits: List::new() } }

    #[test]
    fn bare_key_needs_no_name() {
        let win = Win::demo(root());
        win.key(Key::Backslash);                      // routes to `Pane::split.with(Vertical)`
        assert_eq!(win.root().splits.len(), 1);
        // The closure route balances — also nameless.
        win.key(Key::Equal);
        assert_eq!(*win.root().splits.get(win.root().splits.first()).unwrap().ratio, 0.5);
    }

    #[test]
    fn keymap_string_round_trips_to_the_same_fn() {
        let win = Win::demo(root()).with_keymap(install_keymap(
            r#"{ "ctrl-shift-\\": { "command": "pane.split", "args": "Vertical" } }"#,
        ));
        win.chord("ctrl-shift-\\");                    // resolved via the id → thunk → Pane::split
        assert_eq!(win.root().splits.len(), 1, "the string id dispatched the same route a key does");
    }

    #[test]
    fn unknown_command_is_a_located_load_error_not_a_panic() {
        let err = Keymap::parse(r#"{ "ctrl-x": { "command": "pane.nope" } }"#).unwrap_err();
        assert!(err.to_string().contains("pane.nope"));   // names the id and the source span
    }
}

fn main() {
    Application::run(Window::document(Pane { ratio: Signal::new(0.5), splits: List::new() }));
}
