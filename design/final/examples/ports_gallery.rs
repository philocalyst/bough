//! Ports gallery — the whole `IntoProp`/`Source` algebra through one property method (03).
//!
//! A *port* is any place a value enters the tree. The claim of 03 is that there is exactly ONE
//! closed family of things you may plug into a port (`IntoProp<T>`), all instances of one sealed
//! algebra (`Source`/`Sink`/`Cell`), so that the questions that look like they need per-feature
//! design are actually type facts. This file is that claim, executable: the SAME `.opacity(..)` and
//! `.fg(..)` methods accept a constant, a `&Signal`, a `Reader`, a theme token, a `Motion`, and a
//! mixed tuple — one method, six plug forms, one cost each (03 §2 law 3).
//!
//! Read it as the port catalog it is; each section is one row of `IntoProp<T>`'s grammar (03 §2).
//!
//! Syntax sketch against final/03; not expected to compile against current GPUI.

use gpui::prelude::*;

// A palette recipe is `const` — the static tier of "reactive" is Rust's own `const` (00 §2.2,
// 02 §3). A token is resolved through the cascade at mount, not a graph node.
const CARD: Flex = Flex::column().gap(8).align(Align::Stretch);

struct Gallery {
    // Rung 1 sources — independently publishable. Each drives a DIFFERENT plug form below.
    fade: Signal<f32>,          // a Cell: read AND write (03 §1)
    label: Signal<String>,      // a Cell feeding a two-way control and a one-way display
    reveal: Signal<bool>,       // a Cell animated as a Motion target
    // A question — a latency-bearing derived read (05 §1). `Reader` is Source-only, never a Sink.
    price: Reader<Money>,
    // Plain owner state — the immediate floor. Mixed with a signal in one region below.
    currency: Currency,
}

impl Gallery {
    fn new() -> Self {
        let label = Signal::new(String::new());
        Self {
            fade: Signal::new(1.0),
            reveal: Signal::new(false),
            // The reader is declared once, owner-scoped, from a source. Restoring the source
            // re-arms it (L11) — there is no second "fetch" call anywhere.
            price: label.map_async(pricing::quote),
            label,
            currency: Currency::Usd,
        }
    }
}

impl View for Gallery {
    fn view(&self) -> El<'_> {
        // A Motion is a Source too, so it plugs into the same sinks a constant does (03 §4).
        let reveal = animate(&self.reveal).spring(Spring::snappy());

        div().layout(CARD).p(16).child(column((
            // ── (1) CONSTANT ────────────────────────────────────────────────────────────────
            // `T`/`&T`: promoted only when the diff finds it changed (L2). Zero graph, zero cost
            // until it differs.
            text("Ports").text_lg().opacity(1.0),

            // ── (2) &Signal — the direct two-way port ───────────────────────────────────────
            // A property reference is the zero-ceremony fast path: publication patches the slab,
            // no owner rerun (01 §3). `text_input` demands `impl Cell` (03 §1), and a Signal is
            // the canonical Cell, so this compiles; the same `&self.label` also feeds a one-way
            // `text` below, because a Cell is also a Source.
            text_input(&self.label),
            text(&self.label).text_muted(),

            // ── (3) Reader — the latency-aware port ─────────────────────────────────────────
            // A `Reader` has no `Deref`; you do not read its field, you `project` it, and the
            // boundary routes its verdict (Pending/Fault/Ready) to the enclosing `boundary()`.
            // This is the shape to use when the user MUST see loading/error.
            boundary()
                .loading(|| text("Pricing…").text_muted())
                .child(self.price.project(|money: &Money| text(money.to_string()))),

            // (3b) The Deref-able shape (05 §3.1): `.or(fallback)` folds Pending+Fault into a
            // default, yielding an always-ready `Settled<Money>` — so field access works with no
            // `project` and no boundary. The trade is explicit: you have opted OUT of ever showing
            // the loading/fault verdicts, so use this only where a default reads correctly.
            text(self.price.or(Money::zero()).formatted),   // Settled<Money>: Deref → field access

            // ── (4) impl Source composition — is_pending / map, total functions ──────────────
            // These are NOT features; they are total functions on `impl Source` (03 §1.2). No
            // `if_(..)` combinator exists — the answer is always `map` over a source.
            text("computing")
                .opacity(is_pending(&self.price).map(|p| if p { 0.5 } else { 1.0 })),

            // ── (5) Motion — a sampled property value ───────────────────────────────────────
            // The spring feeds `.opacity` and, scaled/reversed, `.translate_y` — one program,
            // several sinks, sampled on a lane with no per-frame closure (03 §4).
            row((text("Details"),))
                .opacity(reveal)
                .translate_y(reveal.reverse().scale(8.0)),

            // ── (6) semantic token — cascade-resolved feed ──────────────────────────────────
            // `.fg()` is the ONE inking port: text ink, icon ink, mask-sprite ink all take it,
            // and it accepts the same `IntoProp<Color>` family (constant, signal, token). There
            // is no `colorize` (03 §5). `theme.accent.on` is the contrast-checked ink of the pair.
            row((
                icon(Star).fg(theme.accent.on),          // an icon is ink → `.fg()`, same as text
                text("Featured").fg(theme.accent.on),
            )).gap(6).bg(theme.accent),                  // `.accent` in a fill port is the fill

            // ── (7) mixed tuple source — readiness is the meet of the parts ─────────────────
            // A tuple of sources is itself a Source by product (03 §1). Here a signal and plain
            // owner state combine; the signal is tracked, the plain part is frozen by value into
            // the region and re-installed when the OWNER reruns. This is the honest mixed read
            // (see reactive_compare.rs (c) for the same shape hand-lowered).
            text(self.fade.project({
                let currency = self.currency;            // owner-grained, captured by value
                move |f| format!("{currency:?} · {:.0}% shown", f * 100.0)
            })),
        )))
        .on_key(Key::Space, Gallery::toggle)
    }
}

impl Gallery {
    fn toggle(&mut self) {
        *self.reveal = !*self.reveal;    // publish; the Motion retargets from the presented value
    }
}

// ── The direction check is where the widget is typed, not an annotation you write ────────────────
//
// A two-way control demands `impl Cell`; a read-only derivation is `Source`-only, so it simply does
// not satisfy the bound, and the `#[diagnostic::on_unimplemented]` text IS the fix-it:
//
// ```rust,compile_fail
// fn broken(g: &Gallery) -> El<'_> {
//     text_input(&g.price)   // ERROR: `Reader<Money>` is not a `Sink`.
//     //                        a two-way port needs a place to write; bind a `Signal<Money>`,
//     //                        or use `text(..)` to display it.
// }
// ```
//
// You never annotate direction — the port's bound states it (03 §1). This is the whole reason a
// gallery of plug forms is safe: the type of the sink decides which forms are legal at it.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_method_many_plug_forms_one_cost_each() {
        let win = Win::demo(Gallery::new());

        // (2) editing the signal patches its slab and reruns ZERO owners (gate A1: one cost per
        // plug form, measured as descriptor identity, 03 §2 law 3 / L9).
        win.type_text(win.root().label, "widget");
        assert_eq!(win.counters().owner_reruns, 0, "a &Signal port never reruns its owner");

        // (5) space animates opacity+translate on the motion lane — no signal write per frame,
        // no owner turn (03 §4).
        win.key(Key::Space);
        assert_eq!(win.counters().owner_reruns, 0, "a Motion samples a lane, not the owner");
        assert!(win.counters().motion_lanes >= 1);

        // (3)/(4) the reader drives the boundary and the is_pending-mapped opacity from ONE
        // verdict; a stale completion can never touch either (L5, admission stamps).
        win.settle_async();
        assert_eq!(win.semantic_text_of(win.query::<Money>()).is_some(), true);
    }
}

fn main() {
    Application::run(Window::document(Gallery::new()).title("Ports").size(360, 420));
}
