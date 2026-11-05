//! Reactions — the delta is a boundary instrument (04 §1.2, L7, L10).
//!
//! The sharpest question about this model is "why must I derive `Delta` everywhere, and why would I
//! ever watch my own fields?" The answer, made executable here: **you don't, on both counts.**
//!
//!   - A transition YOU caused — a keypress edits a field, a click flips a flag — is handled in the
//!     handler that caused it, inline, with the value in hand. It never round-trips through the
//!     delta. Watching your own scalar in `changed` is the footgun the model removes by not
//!     offering the round trip as the easy path (04 §1.2).
//!   - `changed` carries only transitions you did NOT clock: a child's `Handoff`, a shared
//!     `Entity`'s change, an async `Reader` landing. Those are the only fields whose timing you
//!     don't own — which is the only reason a diff exists at all.
//!   - So `#[derive(Delta)]` is PULLED by the observer: a type needs it exactly when someone writes
//!     `d.child.field`, and the missing-derive error is located at that read (01 §8).
//!
//! One screen — a `Thread` panel — exercises all three boundary shapes and none of the footgun.
//!
//! Syntax sketch against final/01, 04, 05; not expected to compile against current GPUI.

use gpui::prelude::*;

// ════════════════════════════════════════════════════════════════════════════════════════════════
// A CHILD. It reports upward the only way anything does: by being a field the parent observes.
// It derives `Delta` ONLY because `Thread` (below) reads `d.composer.sent`/`.stage` across the
// boundary — pull, not push. Remove those reads and this derive is unjustified and comes off.
// ════════════════════════════════════════════════════════════════════════════════════════════════

#[derive(Clone, PartialEq, Serialize, Deserialize)]
enum Stage { Editing, Discarded }        // a RESTING occurrence: an enum in a Signal (L10)

#[derive(Delta, Serialize, Deserialize)]
struct Composer {
    body: Signal<String>,
    stage: Signal<Stage>,
    sent: Handoff<Message>,              // a CONSUME-ONCE occurrence: linear give/take (01 §6)
}

impl Composer {
    // A self-caused transition, handled where it happens. There is no `d.body` read anywhere; the
    // edit routes straight to this method and the value is the parameter. (Compare typeahead.rs.)
    fn edit(&mut self, text: String) { *self.body = text; }

    fn send(&mut self) {
        self.sent.give(Message { text: self.body.to_string() });   // give once; parent takes once
        *self.body = String::new();
    }
    fn discard(&mut self) { *self.stage = Stage::Discarded; }       // resting state; parent observes
}

impl View for Composer {
    fn view(&self) -> El<'_> {
        column((
            text_input(&self.body).on_change(Composer::edit),       // self-caused → a route
            row((
                button("Send").on_click(Composer::send),
                button("Discard").on_click(Composer::discard),
            )).gap(8),
        )).gap(8)
    }
    // No `changed` at all: a leaf that observes no boundary reacts to nothing.
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// A SHARED ENTITY. Several panels observe one `Draft` store; its changes are transitions the panel
// did not clock, so they arrive through the delta (04 §1, 01 §5) — no "entity event" mechanism.
// ════════════════════════════════════════════════════════════════════════════════════════════════

#[derive(Delta, Serialize, Deserialize)]
struct Draft { unsent: Signal<u32> }

impl Draft {
    fn stash(&mut self, _m: Message) { *self.unsent += 1; }
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// THE PARENT. Its `changed` reads exactly three things — a child handoff, a child resting state, an
// entity delta, and an async reader — and writes NONE of its own scalars in reaction to itself.
// ════════════════════════════════════════════════════════════════════════════════════════════════

#[derive(Delta, Serialize, Deserialize)]
struct Thread {
    composer: Composer,                  // a plain field child (rung 2) — not an Entity
    drafts: Entity<Draft>,               // shared, independently scheduled (rung 3)
    // A question: the sync status is a latency-bearing derived read of the draft count.
    sync: Reader<SyncState>,
    composing: Signal<bool>,
    sound_on: bool,                      // plain owner state — immediate floor
}

impl Thread {
    fn new(drafts: Entity<Draft>) -> Self {
        let unsent = drafts.reader(|d| *d.unsent);         // a Reader off the entity's field
        Self {
            composer: Composer {
                body: Signal::new(String::new()),
                stage: Signal::new(Stage::Editing),
                sent: Handoff::new(),
            },
            sync: unsent.map_async(sync::push),
            drafts,
            composing: Signal::new(true),
            sound_on: true,
        }
    }
}

impl View for Thread {
    fn view(&self) -> El<'_> {
        column((
            self.composing.then(|| dialog().child(&self.composer)),
            boundary()
                .loading(|| text("Syncing…").text_muted())
                .child(self.sync.project(|s| text(s.summary()).text_xs())),
        )).gap(12)
    }

    /// Every branch here reads a BOUNDARY field. Not one reads `d.composing` or `d.sound_on` —
    /// those are the parent's own scalars, and reacting to them would be the circuit-lint footgun
    /// (04 §5). They are written by the handlers that own them, never observed here.
    fn changed(&mut self, d: Changed<'_, Self>) {
        // (a) child Handoff — taking IS the acknowledgement (L10, 01 §6). Draining FIFO.
        while let Some(msg) = d.composer.sent.take() {
            self.drafts.send(Draft::stash.with(msg.clone()));   // sibling hop: take, republish
            if self.sound_on { self.chime(); }                  // a real effect — belongs in changed
        }

        // (b) child RESTING state — an enum Change, answered by changing MY own state (unmount).
        if let Some(c) = &d.composer.stage {
            if c.new == Stage::Discarded { *self.composing = false; }
        }

        // (c) shared ENTITY delta — reaches through `Entity<Draft>` into Draft's own delta, exactly
        //     as if `drafts` were contained. `d.drafts.unsent` is field syntax, no method call.
        if let Some(c) = &d.drafts.unsent {
            if c.new == 0 { self.announce("All drafts sent"); }
        }

        // (d) async READER landing — an async completion is an ordinary delta arrival (05 §1), and
        //     the stamp discipline already dropped any stale one before it reached here (L5).
        if let Some(state) = d.sync.landed() {
            if state.is_error() { self.announce("Sync failed"); }
        }
    }
}

impl Thread {
    fn chime(&mut self) { /* haptics/sound — an external effect, never undone (01 §10.4) */ }
    fn announce(&mut self, _s: &str) {}
}

// ── The located error that makes the derive "pulled, not pushed" ────────────────────────────────
//
// Delete `#[derive(Delta)]` from `Composer` and the read in (a)/(b) is the diagnostic:
//
// ```rust,compile_fail
// // error: observing `Composer` across this boundary requires `#[derive(Delta)]` on `Composer`.
// //        (or read `d.composer.any()` for a coarse changed-marker — 01 §8 degrade tier.)
// while let Some(msg) = d.composer.sent.take() { .. }
// ```
//
// A type earns the derive at the exact edge that observes it, and nowhere else. A `Thread` with no
// parent derives `Delta` here only because IT is embeddable in a workspace that reads it.

// ── Routes are method paths; that is the idiom (04 §3.1) ─────────────────────────────────────────
//
// Name the method — it reads well, the receiver is inferred, nothing is annotated. A guard is
// ordinary control flow INSIDE the method (return `event.propagate()` to fall through).

fn shortcuts() -> El<'static> {
    editor_surface()
        .on_key(Key::Escape, Thread::stop_composing)          // ← the idiom
        .on_key(Key::Enter, Thread::submit)                   // ← still a method, guard is inside
    // A non-capturing closure is ALSO a legal route, but it is the uglier fallback — coercion to
    // `fn` forces the receiver annotation (`|t: &mut Thread, e| ..`), so prefer a method for
    // anything past a throwaway one-liner. A *capturing* closure is not a route at all:
    //   .on_key(Key::Tab, |t| t.indent(width))   // ✗ captures `width`
    //   fix-it: .on_key(Key::Tab, Thread::indent.with(width))  — the capture, made owned state.
}

impl Thread {
    fn stop_composing(&mut self) { *self.composing = false; }
    fn submit(&mut self, e: KeyEvent) -> KeyEvent {
        if self.at_prompt() { return e.propagate(); }         // guard the function, not the chord
        self.composer.send();
        e.consume()
    }
    fn at_prompt(&self) -> bool { true }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_only_carries_boundary_transitions() {
        let drafts = Entity::new(Draft { unsent: Signal::new(0) });
        let win = Win::demo(Thread::new(drafts));

        // A self-caused edit routes to the child method; it produces NO parent `changed`
        // invocation attributable to a self-scalar read (there is no such read).
        win.type_text(win.query::<TextInput>(), "hi");
        win.click(win.button("Send"));

        // The child Handoff crossed the boundary → the parent took it and stashed to the entity.
        assert_eq!(win.root().drafts.inspect(|d| *d.unsent), 1);

        // Discard is a resting-state Change; the parent answered by unmounting the composer.
        win.type_text(win.query::<TextInput>(), "draft");
        win.click(win.button("Discard"));
        assert_eq!(*win.root().composing, false);
    }

    #[test]
    fn a_handoff_survives_a_journal_round_trip_before_being_taken() {
        let drafts = Entity::new(Draft { unsent: Signal::new(0) });
        let mut win = Win::demo(Thread::new(drafts));
        win.type_text(win.query::<TextInput>(), "unsaved");
        win.click(win.button("Send"));
        // give → checkpoint → restore → the value is still there to take (01 §6 point 1).
        let bytes = win.journal_checkpoint();
        let restored = win.restore_from(bytes);
        assert_eq!(restored.root().drafts.inspect(|d| *d.unsent), 1);
    }
}

fn main() {
    let drafts = Entity::new(Draft { unsent: Signal::new(0) });
    Application::run(Window::document(Thread::new(drafts)).title("Thread"));
}
