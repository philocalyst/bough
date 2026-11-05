//! Progressive enhancement — one component, four rungs, one `view` (00 §2.2, §3, L13/L14).
//!
//! There is no "static tier" and no "dynamic tier." There is immediate (untracked) authoring and
//! retained (tracked) authoring, and they are the SAME language — the difference is only whether a
//! value is tracked. This file promotes one `Counter` up the storage ladder without ever rewriting
//! its `view`:
//!
//!   rung 0  `count: u32`           immediate — a click reruns the whole `view`; correct and cheap
//!   rung 1  `count: Signal<u32>`   retained — the label patches its slab; the owner does not rerun
//!   rung 2  `rows:  List<Row>`     a hot row updates by slot delta, not by realigning the list
//!   rung 3  `shared: Entity<Tally>` an independently scheduled owner several views observe
//!
//! Promotion is a TYPE change at a field, monomorphized; the ports accept every rung (03), so the
//! body is identical text. Gate 3 (L13) is the proof: the pixels never move — only WHO reruns does.
//!
//! Syntax sketch against final/00–03; not expected to compile against current GPUI.

use gpui::prelude::*;

// `const` is the real "static" tier — Rust's own, not a bespoke compiler (00 §2.2). This recipe is
// evaluated at compile time and can seed a static tape template (08 §7); nothing here diffs it.
const BAR: Flex = Flex::row().align(Align::Center).gap(8);

// ── RUNG 0 · immediate ───────────────────────────────────────────────────────────────────────────
// The honest floor: plain `u32`, mutated behind `&mut self`, whole-`view` rerun. No signal, no
// macro, no constraints. The retained-tape diff still turns the rerun into the actual delta of work
// (02 §1), so this is not "slow mode" — it is "undeclared dependencies mode," and fine for a mockup.
mod rung0 {
    use super::*;
    pub struct Counter { pub count: u32 }
    impl Counter {
        fn inc(&mut self) { self.count += 1; }        // reruns view; the diff finds one text delta
    }
    impl View for Counter {
        fn view(&self) -> El<'_> {
            div().layout(BAR).child(row((
                text(format!("Count: {}", self.count)),
                button("+").on_click(Counter::inc),
            )))
        }
    }
}

// ── RUNG 1 · retained ──────────────────────────────────────────────────────────────────────────
// Promote the one field that earned it. The body is BYTE-IDENTICAL except `self.count` is now a
// `Signal<u32>` read through `*`. `text(..)` takes `impl IntoProp`, so `format!` over `*self.count`
// still type-checks; but now the label is a direct port and a click patches its slab with zero
// owner rerun (01 §3, L2). This is the promotion the whole design is organized around.
mod rung1 {
    use super::*;
    pub struct Counter { pub count: Signal<u32> }
    impl Counter {
        fn inc(&mut self) { *self.count += 1; }        // publish; the label patches, owner idle
    }
    impl View for Counter {
        fn view(&self) -> El<'_> {
            div().layout(BAR).child(row((
                text(format!("Count: {}", *self.count)),   // only `*` changed vs rung 0
                button("+").on_click(Counter::inc),
            )))
        }
    }
}

// ── RUNG 2 · a hot sequence ────────────────────────────────────────────────────────────────────
// When rows need durable identity across reorders/edits, `Vec<Row>` promotes to `List<Row>`: same
// `column(&self.rows)` children spelling, but now an edit to one row is a SLOT delta, not a realign
// of the whole sequence (01 §4, L8). Promotion changes delta grain, never meaning.
mod rung2 {
    use super::*;
    pub struct Row { pub count: Signal<u32> }
    impl View for Row {
        fn view(&self) -> El<'_> {
            row((text(format!("{}", *self.count)), button("+").on_click(Row::inc))).gap(6)
        }
    }
    impl Row { fn inc(&mut self) { *self.count += 1; } }

    pub struct Board { pub rows: List<Row> }             // was `Vec<Row>` at the immediate floor
    impl View for Board {
        fn view(&self) -> El<'_> {
            column((column(&self.rows), button("Add row").on_click(Board::add))).gap(8)
        }
    }
    impl Board {
        fn add(&mut self) { self.rows.push(Row { count: Signal::new(0) }); }
    }
}

// ── RUNG 3 · shared, independently scheduled ───────────────────────────────────────────────────
// The top rung: a `Tally` several views present at once, owning its own turn (01 §5, L6). It is a
// scope-owned generational capability (09 §1) — no strong/weak axis. Views observe it through the
// recursive delta; they never `Deref` it.
mod rung3 {
    use super::*;
    #[derive(Delta, Serialize, Deserialize)]
    pub struct Tally { pub count: Signal<u32> }
    impl Tally { pub fn inc(&mut self) { *self.count += 1; } }

    pub struct Header { pub shared: Entity<Tally> }
    impl View for Header {
        fn view(&self) -> El<'_> {
            // A view over an entity reads it as a source; the button sends a queued verb.
            div().layout(BAR).child(row((
                text(self.shared.reader(|t| *t.count).project(|n| format!("Total: {n}"))),
                button("+").on_click(Header::bump),
            )))
        }
    }
    impl Header {
        fn bump(&mut self) { self.shared.send(Tally::inc); }   // queued, non-reentrant (01 §5)
    }
}

// ── Gate 3: promotion moves counters, never pixels (L13) ────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_pixels_up_the_ladder() {
        let r0 = Win::demo(rung0::Counter { count: 0 });
        let r1 = Win::demo(rung1::Counter { count: Signal::new(0) });

        for w in [&r0, &r1] { w.click(w.button("+")); }

        // Identical pixels after the same interaction (L13 — adding tracking changes WHEN work
        // runs, never what it means).
        assert_eq!(r0.scene_digest(), r1.scene_digest());

        // Only the work counters differ: rung 0 reran its owner, rung 1 patched a slab.
        assert_eq!(r0.counters().owner_reruns, 1);
        assert_eq!(r1.counters().owner_reruns, 0, "the promoted label is a direct port");
    }
}

fn main() {
    // Ship any rung; they are the same program at different grains. Start immediate, promote later.
    Application::run(Window::document(rung1::Counter { count: Signal::new(0) }).title("Counter"));
}
