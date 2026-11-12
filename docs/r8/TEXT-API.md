# The text API (round 8, text lane)

Status: **proposal v0, 2026-11-12.** Programs first; nothing here is built. Section 1 is the rules the programs
follow, section 2 the programs, section 3 what the types infer (staging), section 4 the gap table against
gpui-ce and the web with the order to close it, section 5 the questions that are the user's to decide.

Binding constraints (ROUND8 Amendment 2 and the lead's direction of 2026-11-12):

* The author keeps writing the façade: `text(..)`, `para(..)`, `text_input(..)`, `row((..))`, modifiers such as
  `.font(13.0, 18.0)` and `.color(..)`, `sig!(self.field)` operands, handlers as methods. Text is extended in that
  style. Nothing an author writes names a stage, a slot, a column, a glyph or a cache.
* State is plain fields that handlers mutate. No subscriptions, no `on_change`, no observers. An editor's state
  (text, selection, history, composition) is a plain value the author owns, or that a component owns.
* No special cases: one text element; what differs between a label, a paragraph, a number, an input and a
  document of 300,000 lines is the *type of the source operand and of the modifiers' operands*, and the core
  decides the work from those types.

## 1. The rules

1. **One element, `text(src)`.** `para(src)` stays as `text(src).wrap(Wrap::Words)`; today's `label` is `text`.
2. **The source operand decides the representation.** Anything `TextSrc`:
   * `&'static str`: a constant. Shaped when the program is built (section 3).
   * `&str`, `String`, `sig!(self.title)`, `sig!(self.user).map(|u| u.name.as_str())`: free text.
   * an integer, or `sig!(self.total).num(FMT)`: a number. Its glyphs are a function of the value.
   * a tuple or `each(..)` of spans: styled runs (rule 3).
   * `sig!(self.draft)` where the field is a `TextState`: editable text (rule 6).
3. **A span is text inside text.** `span(src)` takes the same modifiers as `text`; a span overrides what it sets
   and inherits the rest from the text it is in, which inherits from the enclosing element's text style, which
   comes from the theme. Any other element in the content tuple is an inline box.
4. **Every modifier takes an operand, not a value.** `.color(INK)`, `.color(sig!(self.ink))`,
   `.color(sig!(self.theme).map(ink))`, `.weight(sig!(self.hot).transition(Ease::out(0.15)))` are the same
   call. A constant costs nothing at run time; an operand that reads state costs what its *property* needs
   (section 3), and the author never says which.
5. **Ranges are a second way to style, for text that is one buffer.** `.styled(ranges)` takes
   `&[(Range<usize>, Style)]` (a parser's output); `span` nesting is for text written by hand. They produce the
   same thing.
6. **Editing is a value.** `TextState` is a plain struct (`text`, `selection`, `composition`, `history`) with
   plain methods (`insert`, `delete`, `move_to`, `select`, `undo`, `replace_all`, ...), usable in a test with no
   window. `text_input(sig!(self.draft))` routes keys, pointer and IME to those methods. A `String` field works
   as today: then the selection and history are the input component's own state. Nothing is observed: the
   author's handlers read `self.draft.text()` when they run.
7. **Selection is a scope.** `selectable(children)` makes every text under it one selection, in document order,
   with copy. The selection is two points in a plain `Selection` field if the author wants to read or set it, or
   the scope's own state if not.
8. **Geometry questions are asked of the text, in a handler.** A pointer event on text carries the text
   position (`ev.text: TextHit`); a handler that needs more asks `cx.text(sig!(self.draft))` for
   `point_of(index)`, `index_at(point)`, `bounds(range)`, `line_of(index)`. An element that must follow a text
   position says so declaratively (`.anchor(sig!(self.draft).caret())`).

## 2. The programs

Each is followed by what the author did not write.

### P1. Styled runs

```rust
const UI: Family = Family::new("Inter").or("Noto Sans").or(Family::SYSTEM_UI);
const MONO: Family = Family::new("JetBrains Mono").or(Family::MONOSPACE);

fn view(&self) -> impl Element<Self> + '_ {
    column((
        text("Release notes").font(22.0, 28.0).family(UI).weight(700).tracking(-0.2),
        para((
            "Deploys now run in ",
            span(&self.region).weight(600),
            ". The old ",
            span("deploy.yml").family(MONO).bg(sig!(self.theme).map(code_bg)).size(12.0),
            " file is ",
            span("deprecated").strike().color(MUTED),
            " and will be removed in ",
            span(sig!(self.next_version)).underline().italic(),
            ".",
        ))
        .font(14.0, 21.0)
        .color(sig!(self.theme).map(ink)),
        text("H2O and E = mc2").features(Features::new().sub(1..2).sup(13..14)),  // by range, see rule 5
        text("Variable").axis(Axis::WGHT, 430.0).axis(Axis::OPSZ, 14.0).features(Features::new().ss(1).on(*b"cv11")),
    ))
}
```

Not written: font loading or handles (a family is a const chain of names; the fallback chain applies per
cluster); run offsets; a rebuild when `self.region` changes (the span re-shapes, the paragraph re-breaks only if
its width interval ends); anything when `self.theme` changes (ink and code background are paint: no shaping, no
layout, no per-glyph work).

### P2. Inline boxes and images

```rust
para((
    avatar(&self.author).size(18.0).baseline(Baseline::Middle),
    " ", span(&self.author.name).weight(600),
    " attached ",
    image(self.thumb).h(1.2 * EM).rounded(3.0),            // sized in the text's em
    " and mentioned ",
    row((icon(AT), text(&self.mention))).px(4.0).rounded(4.0).bg(CHIP).on_click(Self::open_mention),
    ". ",
    span(&self.body),
))
```

Not written: placeholders, measuring the boxes, positioning them after line breaking, re-breaking when a box
changes size. An inline box is an ordinary element (it can be clicked, focused, animated); it breaks as one
unbreakable unit and aligns on the baseline unless told otherwise.

### P3. Links and interactive ranges

```rust
para((
    "Read the ",
    span("migration guide").link("https://example.org/guide"),         // themed link style, opens by default
    " or ",
    span("ask in #infra").color(ACCENT).hover(|s| s.underline()).on_click(Self::open_channel).tooltip("Open the channel"),
    ".",
))

// ranges found by the application (search hits, mentions, diagnostics) over one buffer:
para(&self.message.body)
    .styled(&self.message.marks)                                  // &[(Range<usize>, Style)]
    .on_click_range(&self.message.mentions, Self::open_user)      // &[(Range<usize>, UserId)] -> fn(&mut Self, &UserId)
```

Not written: hit testing a point to a byte index and then to a range; hover tracking (the hover style is paint
keyed on the pick, no handler runs); the pointer cursor; keyboard access (a link is a tab stop and an
accessibility link by construction); wrapped links (a link broken across lines is one target).

### P4. Wrapping, truncation, clamp, alignment

```rust
text(&self.file.name).overflow(Overflow::EllipsisMiddle)                 // "very-long-na…e-v2.tar.gz"
text(&self.preview).overflow(Overflow::Ellipsis)                         // one line, "…" at the end
para(&self.summary).lines(3).overflow(Overflow::Ellipsis)               // line clamp
para(&self.summary).lines(3).overflow(Overflow::Fade)                   // fade the last line out
para(&self.title).wrap(Wrap::Balance).align(TextAlign::Center)
para(&self.article).align(TextAlign::Justify).hyphens(Hyphens::Auto)
para(&self.url).wrap(Wrap::Anywhere)
text(&self.source).wrap(Wrap::Pre).tab_size(4)                          // newlines and spaces kept, no wrapping
para(&self.log_line).wrap(Wrap::PreWrap)
```

Not written: truncation by hand to a length that fits (the chat scene does this today); a measure pass; a
"was it truncated" flag (ask: `cx.text(..).truncated()`, used to show a tooltip with the full text, which
`.overflow(..).tooltip_if_cut()` does without a handler).

### P5. Scripts, direction, emoji, fallback

```rust
para("The word for peace is سلام in Arabic and שלום in Hebrew.")       // bidi resolved, runs reordered per line
para(&self.comment)                                                    // any script: fallback chain per cluster
text("👩🏽‍🚀 launched 🚀").family(UI)                                    // colour glyphs, ZWJ sequences, skin tones
para(&self.rtl_body).dir(Dir::Auto)                                    // paragraph direction from first strong char
text(&self.name).lang("ja")                                            // han unification picks the JP forms
```

Not written: script itemisation, font fallback, bidi runs, cluster boundaries for the caret, grapheme-aware
deletion. An author who never thinks about scripts gets correct text.

### P6. Numbers

```rust
const MONEY: NumFmt = NumFmt::new("$", Some(2), "").grouping(',');
const COUNT: NumFmt = NumFmt::new("", None, "");

text(sig!(self.total).num(MONEY)).tabular()                 // digits on a fixed advance: never shifts while it changes
text(sig!(self.unread).num(COUNT))                          // proportional digits: its width may change
text(row.id)                                                // an integer is text; no format!()
text(sig!(self.fps).num(NumFmt::new("", Some(1), " fps"))).tabular().align(TextAlign::End)
```

Not written: `format!`, a `String` per row per turn, a re-layout guard. A tabular number in a box of fixed
width is a function of one word: writing it costs the write (section 3).

### P7. Selection across elements, and copy

```rust
fn view(&self) -> impl Element<Self> + '_ {
    selectable(
        column(each(self.messages.iter().map(|m| {
            column((
                row((text(&m.author).weight(600), text(&m.time).color(MUTED).unselectable())),
                para(&m.body),
                each(m.code.iter().map(|c| code_block(c).copy_as(|s| format!("```\n{s}\n```")))),
            ))
        })))
    )
    .selection(sig!(self.sel))                    // optional: only if the app wants to read or set it
    .scroll(sig!(self.scroll))
}

fn quote_reply(&mut self, cx: &mut Cx<Self>) {     // a button's handler
    let quoted = cx.selected_text(sig!(self.sel)); // the text in document order, block boundaries as newlines
    self.draft.insert(&format!("> {quoted}\n"));
}
```

Not written: per-element selection state, mapping a drag across elements to ranges, the highlight rectangles,
double-click word and triple-click paragraph selection, shift-click, auto-scroll while dragging, Cmd-C and the
clipboard, select-all within the scope, what a selection means across text that is scrolled out and not
materialised (the two points are positions in document order, not pointers to mounted things).

### P8. A single-line input

```rust
struct Login { email: TextState, password: TextState, zip: String, error: Option<&'static str> }

fn view(&self) -> impl Element<Self> + '_ {
    column((
        text_input(sig!(self.email)).placeholder("you@example.org").kind(InputKind::Email).on_submit(Self::next),
        text_input(sig!(self.password)).placeholder("Password").secret().on_submit(Self::sign_in),
        text_input(sig!(self.zip)).placeholder("ZIP").on_input(Self::zip_typed).w(80.0),
        each(self.error.map(|e| text(e).color(DANGER))),
        button("Sign in").on_click(Self::sign_in).disabled(self.email.is_empty()),
    ))
}

fn zip_typed(&mut self, edit: TextEdit) {            // the proposed edit; applying it is the handler's choice
    if edit.inserted().bytes().all(|b| b.is_ascii_digit()) && self.zip.len() + edit.inserted().len() <= 5 {
        edit.apply(&mut self.zip);
    }
}
fn sign_in(&mut self) {
    if !self.email.text().contains('@') { self.error = Some("Enter an email address"); self.email.select_all(); return; }
    // ...
}
```

Not written: caret, blink, selection, word and line motion, mouse selection, clipboard, undo, IME composition,
horizontal scroll to keep the caret visible, focus ring, platform keybindings, the accessibility text-field
role with its value and selection. `zip: String` shows the plain form: when the field is a `String` the input
owns selection and history itself.

### P9. A multi-line editor

```rust
struct Composer { draft: TextState, sending: bool }

fn view(&self) -> impl Element<Self> + '_ {
    column((
        text_input(sig!(self.draft))
            .multiline()
            .wrap(Wrap::Words)
            .lines(1..=8)                              // grows with content, scrolls beyond eight lines
            .placeholder("Message #infra")
            .on_key(Self::key)
            .font(14.0, 20.0),
        row((text(self.draft.len()).tabular().color(MUTED), button("Send").on_click(Self::send))),
    ))
}

fn key(&mut self, ev: &Ev) -> bool {                   // true: handled, the default binding does not run
    match (ev.key, ev.mods) {
        (Key::Enter, Mods::NONE) => { self.send(); true }
        (Key::Tab, _) if self.draft.selection().is_caret() => { self.draft.insert("    "); true }
        _ => false,
    }
}
fn send(&mut self) {
    let body = self.draft.take();                      // text out, state reset, one undo group closed
    self.outbox.push(body);
}
```

A test of the same state, with no window:

```rust
let mut t = TextState::from("hello world");
t.move_to(Move::WordEnd, Extend::No); t.select(Move::LineEnd); t.insert("there");
assert_eq!(t.text(), "hello there");
t.undo(); assert_eq!(t.text(), "hello world");
```

Not written: everything in P8, plus up/down with a remembered column, page motion, line and paragraph
selection, the composition underline and candidate-window position, undo grouping (typing coalesces, a pause
or a motion closes a group).

### P10. Measurement and hit testing

```rust
// An autocomplete popup that follows the caret: declarative.
column((
    text_input(sig!(self.query)).on_input(Self::typed),
    each(self.completing.then(||
        popover(completions(&self.matches)).anchor(sig!(self.query).caret(), Side::Below))),
))

// A transcript where clicking a word seeks the audio: the event carries the position.
para(&self.transcript).on_click_text(Self::seek)
fn seek(&mut self, at: TextHit) {                      // at.index, at.line, at.inside
    let w = self.words.partition_point(|w| w.end <= at.index);
    self.player.seek(self.words[w].time);
}

// Custom geometry in a handler (drawing a comment bracket beside a range):
fn annotate(&mut self, cx: &mut Cx<Self>) {
    let t = cx.text(sig!(self.doc));
    self.bracket = t.bounds(self.range.clone());       // one rect per line the range touches
    self.caret_line = t.line_of(self.doc.caret());
    let i = t.index_at(self.pointer);                  // nearest boundary; `at(point)` reports inside/outside
}

// Sizing something to text without mounting it:
let w = cx.measure(text(&self.longest_label).font(13.0, 18.0)).width;
```

Not written: a layout handle threaded from render to handler; a prepaint/paint split; a cache of positions that
the author must invalidate.

### P11. Code, hundreds of thousands of lines

```rust
struct Buffer { doc: TextState, syntax: Syntax, scroll: f32 }

fn view(&self) -> impl Element<Self> + '_ {
    text_input(sig!(self.doc))
        .multiline()
        .family(MONO).font(13.0, 19.0)
        .wrap(Wrap::Pre)
        .highlight(Self::tokens)                // fn(&Self, line: usize) -> &[(Range<usize>, Token)]
        .palette(sig!(self.theme).map(syntax_colors))   // Token -> colour; a theme switch touches no glyph
        .gutter(|line| text(line + 1).tabular().color(MUTED).align(TextAlign::End))
        .scroll(sig!(self.scroll))
}
fn tokens(&self, line: usize) -> &[(Range<usize>, Token)] { self.syntax.line(line) }
```

and read-only, as plain children (no virtual-list API, per ROUND7 Amendment 2):

```rust
selectable(column(each(self.lines.iter().map(|l| text(&l.text).styled(&l.tokens))))).family(MONO).scroll(sig!(self.scroll))
```

Not written: a virtual list; a line-height table; a visible-range computation; a shaping cache; the minimap's
second copy of the text (`minimap(sig!(self.doc))` is another view of the same value at another scale).
`highlight` is asked only for exposed lines, by the framework, when it needs them: a plain method, not a
subscription.

### P12. Live text

```rust
// a counter: a number (P6)
text(sig!(self.requests).num(COUNT)).tabular()

// a log: a list whose rows are text
list(sig!(self.log), |l: &LogLine| row((text(&l.time).color(MUTED).tabular(), text(&l.msg).color(level_ink(l.level)))))
    .stick_to_end()
fn on_line(&mut self, l: LogLine) { self.log.push(l); if self.log.len() > 10_000 { self.log.remove(0); } }

// streamed model output: a String that grows
para(sig!(self.reply)).appear(Fade::new(0.18)).styled(&self.reply_marks)
fn on_token(&mut self, tok: &str) { self.reply.push_str(tok); }
```

Not written: a diff of old and new text (the framework compares; an append is found by the common prefix and
costs the appended glyphs); a ring buffer of visible lines; an animation per token (`appear` is an expression
over time and each glyph's birth time: no turn runs while tokens fade in).

### P13. Animated text properties

```rust
text("Saved").color(sig!(self.saved).map(flash).transition(Ease::out(0.4)))          // colour: paint only
text(&self.tab.title).weight(sig!(self.active).map(|a| if *a { 650.0 } else { 400.0 }).transition(Ease::out(0.12)))
text("9").size(sig!(self.pulse).transition(Curve::spring(0.3)))                      // size animates smoothly
text("Loading").per_glyph(|g| g.offset_y(wave(g.index, TIME)))                       // a wave across glyphs
para(&self.quote).reveal(sig!(self.progress))                                        // typewriter: glyphs up to a fraction
text(&self.title).opacity(sig!(self.shown).transition(Ease::out(0.2))).translate(sig!(self.shown).map(slide))
```

Not written: a timer, a turn per frame, glyph re-rasterisation per size step, a re-layout guard for the weight
animation (the author chose `.weight`, a property that moves advances; the core re-shapes that one text per
frame while it runs, or holds its box at the widest state when told `.hold_box()`).

### P14. Text style as composition

```rust
// a theme is a resource; a text style is a bundle of components an element inherits and may override
const BODY: TextStyle = TextStyle::new().family(UI).font(14.0, 21.0);
const CAPTION: TextStyle = BODY.size(12.0).line_height(16.0).color(MUTED);

column((
    text(&self.title).style(HEADING),
    para(&self.body),                                   // BODY: inherited from the column
    text(&self.footnote).style(CAPTION).italic(),      // a style, then one override
))
.text_style(BODY)
```

Not written: passing the font down by hand; a style struct rebuilt per element per frame (a `const` style is
part of the element's type-level description; an override is one component replaced).

## 3. What the types infer

The author writes operands. The property an operand is given to decides how late it can be evaluated, and the
operand's own type (constant, record value, state read) decides how late it must be. The core takes the later
of the two. In the terms of `SEAM.md`: `Const`, `Mount`, `Turn` on the CPU, `Frame` while shading.

| property | what it feeds | latest stage it can be | what a change costs |
|---|---|---|---|
| source text (free) | shaping, breaking, layout | Turn | shape the changed line off the interaction path, re-break if the width interval ends, a paint write |
| source text (`&'static str`) | nothing at run time | Const | nothing: glyph columns are in the binary |
| source number, tabular, in a box of known width | paint only | Frame | one word; the digits are placed while shading |
| source number, proportional | shaping of digits | Turn | advance lookup per digit, no shaper |
| family, weight, slant, size, features, axes, tracking, `lang`, `dir` | shaping | Turn | re-shape (memoised by font, features and text) |
| line height, wrap, lines, align, justify, hyphens, tab size | breaking and placement | Turn | re-break from the memoised shape, no shaping |
| colour, background, decoration colour, opacity, palette | paint | Frame | one word in the block, damage by finite difference |
| underline, strike (on or off, style) | paint (their geometry is from font metrics, already in the run) | Frame | as colour |
| hover style, selection highlight, caret, composition underline | paint | Frame | words in the block (pick, two selection points, time for the blink) |
| translate, scale, per-glyph offset, reveal, appear | paint geometry | Frame | swept bounds as damage, zero turns |
| size over time (a transition) | paint geometry while it runs, shaping at rest | Frame, then one Turn | glyphs scaled from outlines or a distance field while animating; shaped at the end size once |
| overflow (ellipsis, fade) | breaking | Turn | part of the break |

So in `text(&m.body).color(sig!(self.theme).map(ink))` the glyphs are Mount stage (shaped once when the row is
exposed) and the colour is a paint operand that reads state; in `text(sig!(self.total).num(MONEY)).tabular()`
nothing is CPU stage at all; in `span(sig!(self.next_version))` inside a paragraph one run is Turn stage and
the rest of the paragraph is not re-shaped. None of these distinctions is written.

Two consequences worth stating because they shape the API:

* **A text's colour runs are not separate texts.** Glyph instances carry a small style index; the record holds
  the palette as expressions. A paragraph with nine coloured spans, or a code line with twenty tokens, is one
  record, and a theme switch is one word.
* **Selection is six words.** Two points, each (document-order key, byte index, x). Every text line evaluates
  "am I inside, outside or an edge" from its own constants while shading. A selection across 300,000 lines
  costs the same as across one, and dragging it costs the hit test of the line under the pointer.

## 4. Gap table

Sources: the façade as it is on `r8/text` (`crates/bough/src/{elem,layout,ui,input,text}.rs`, `crates/bough-text`);
gpui-ce 0.3.1 at `776fd1f069` (`crates/gpui/src/text_system`, `elements/text.rs`, `gpui_elements/src/editable_text`,
`gpui_ce_parley`), read in full, nothing run; the web as CSS Text 3/4, CSS Inline, CSS Fonts 4, Selection API,
`contenteditable`/`beforeinput`, CSS Custom Highlight API. "P" is the program above that covers the row.

| capability | bough façade today | gpui-ce | web | proposed | order |
|---|---|---|---|---|---|
| one-line label, wrapped paragraph | `text`, `para`; one face per text, `.font(size, lh)`, `.color(operand)` | `StyledText`, `div().child("..")` | yes | unchanged | 0 |
| weight, slant, width per run | four logical faces per document (`FACE_BOLD`..), raw layer only | per run (`TextRun.font`) | yes | `span(..).weight()/.italic()`, P1 | 1 |
| size per run | no | no in `StyledText`; only nested inline divs | yes | `span(..).size()`, P1 | 1 |
| colour, background per run | colour per text; `glyph_alpha` per glyph | per run, paint-only | yes | `.color()/.bg()` on spans, palette index per glyph, P1 | 1 |
| underline, strike | no | underline (plain, wavy), strike | plus dotted, dashed, double, overline, thickness, offset, skip-ink | `.underline()/.strike()/.overline()` with a `Line` style, P1 | 1 |
| letter and word spacing | no | letter only | both | `.tracking()`, `.word_spacing()` | 1 |
| font features, variable axes | no | features yes (raw tags); axes not author-facing | both | `.features()`, `.axis()`, P1 | 1 |
| families and fallback chain | one font file per window (`BOUGH_FONT`), no fallback | per run, fontique | `font-family` lists, `@font-face`, `unicode-range` | `const Family` chains, fonts registered as bytes, fallback per cluster, P1 | 1 |
| style inheritance, theme styles | no | `TextStyle` refinement cascades down the element tree | cascade | `TextStyle` bundles, `.text_style()`, P14 | 1 |
| wrap modes | words at spaces only | word wrap, nowrap | normal, nowrap, pre, pre-wrap, pre-line, anywhere, break-word, balance, pretty | `Wrap::{Words, None, Anywhere, Pre, PreWrap, Balance}`, UAX 14 breaks, P4 | 2 |
| truncation, ellipsis | no (the chat truncates by hand) | start, middle, end, custom affix | end only (`text-overflow`), clip | `Overflow::{Clip, Ellipsis, EllipsisStart, EllipsisMiddle, Fade}`, P4 | 2 |
| line clamp | no | yes | `line-clamp` | `.lines(n)`, P4 | 2 |
| alignment | no (box alignment only) | start, end, left, centre, right | plus justify, `text-align-last` | `TextAlign` incl. `Justify`, P4 | 2 (justify 8) |
| hyphenation | no | no | `hyphens: auto` | `.hyphens()`, P4 | 8 |
| tab stops | no | no | `tab-size` | `.tab_size()` | 5 |
| bidi, complex scripts | Latin-1 by table; beyond it rustybuzz per chunk, no bidi, no fallback | yes (parley, `UnicodeBidi`) | yes | automatic; `.dir()`, `.lang()`, P5; through the vendored parley for what the table refuses | 7 |
| colour emoji | no | yes | yes | P5; colour glyph atlas page | 7 |
| text transform | no | upper, lower, capitalise | plus full-width, kana | `.transform()` | 8 |
| numerals | `.num(FMT)`, digits patched in place (236 instructions of text; about 6,000 for the whole write) | raw `tnum` feature | `font-variant-numeric` | `.tabular()`; a number write is a word, P6 | 0 |
| inline boxes, images in text | no | yes (`div().inline()`, vertical-align); no truncation with boxes | yes | any element in the content tuple, P2 | 6 |
| links, interactive ranges | no | `InteractiveText` click ranges, hover and tooltip by index; no keyboard, no hover style | yes | `.link()`, `.on_click()` on spans, `.on_click_range()`, hover style as paint, P3 | 6 |
| selection of static text | no | no | yes, across elements | `selectable(..)`, P7 | 3 |
| copy | no | editable only | yes, with `copy` event | default with the scope; `.copy_as()`, P7 | 3 |
| custom highlights by range | no | `with_highlights` | Custom Highlight API | `.styled(ranges)`, P3, P11 | 1 |
| single-line input | `text_input(sig!(..))` over `String`: caret, typing; fixed size; no placeholder, no selection styling operands | `text_input`: full; no masking, no validation | `<input>` with types, validation | P8: `TextState` or `String`, placeholder, `.secret()`, `.on_input`, `.on_submit` | 4 |
| multi-line editor | no | `text_area`: single style, single cursor, contiguous `&str` storage, O(n) per edit | `<textarea>`, `contenteditable` | P9 | 4 |
| IME composition | no | yes (marked range, candidate bounds) | yes | in `TextState` (`composition`), P9 | 4 |
| undo history | `TextModel` has bounded undo; not wired to a view | 1,000 entries, 300 ms forward-typing coalescing only | browser-defined | groups closed by pause, motion or kind change; plain methods, P9 | 4 |
| word, line, page, document motion | no | yes except page, select-to-line-edge | yes | all, platform bindings, P9 | 4 |
| mouse selection | no | click, shift, drag, double, triple; drag after double does not extend | yes | plus drag by word and line, P7, P9 | 3 |
| index to point, point to index | no author API | `TextLayout::{position_for_index, index_for_position}`, `selection_bounds`, caret movement | Range `getClientRects`, `caretPositionFromPoint` | `ev.text`, `cx.text(..)`, `.anchor(caret)`, P10 | 3 |
| measure without mounting | no | `shape_line`, `shape_text` | canvas `measureText` | `cx.measure(..)`, P10 | 3 |
| large documents | `\n` text as a ring of lines; lists window by exposure | none: `uniform_list` of `shape_line` rows by hand; 512-entry paragraph FIFO thrashes | none built in | plain children or `text_input(..).multiline()`, exposure-driven, P11 | 5 |
| syntax highlighting | by hand: one label per token (the ide scene: 14,585 spans) | `with_highlights` on one text | by hand or Custom Highlight | `.highlight(method)`, `.palette(operand)`, P11 | 5 |
| live text | `.num` patch; `LineSet` matches lines; `Para::append` | new string, paragraph cache | DOM mutation | plain `String`/`List` writes; append found by prefix compare, P12 | 5 |
| animated colour, opacity | colour through `transition`; `glyph_alpha` | colour, size, spacing, clamp transitions; weight not | transitions, animations | any operand `.transition(..)`, P13 | 0 for paint, 8 for geometry |
| animated size, weight, per-glyph motion | no | size (re-rasterises per frame), no weight | yes (variable fonts) | P13; scalable archetype (SEAM B5) | 8 |
| vertical writing, ruby, text shadow, initial letter, `text-wrap: pretty` | no | no | yes | not proposed (question 10) | n/a |

Where bough would be ahead of both references when the table is closed: selection across elements with no
per-element state; numbers and theme or hover colour that cost one word; documents that need no virtual-list
API; truncation combined with inline boxes; hover styles with no handler; editing state that is a plain
testable value with non-contiguous storage.

### The order, and why

0. **Text on the new core** (label, paragraph, number, with real glyphs, colour as a paint operand). Without
   it none of the five scenes runs on `bough-x` with real text; everything else builds on its three pieces:
   the shape memo, the break table, the line record.
1. **Runs and fonts** (P1, P14, `.styled`). The run table is the representation every later feature styles
   through (selection, links, highlights, composition underline), so it comes before them.
2. **Breaking features** (P4). All are functions of the break table; they change what `block(w)` answers and
   nothing else.
3. **Selection, hit testing, measurement** (P7, P10). Needed by editing, and useful alone (copying from a
   chat or a log).
4. **Editing** (P8, P9). `TextState`, the input element, IME.
5. **Documents and live text** (P11, P12). Editing over lines as a relation; highlights by exposure.
6. **Inline boxes and links** (P2, P3). Needs the two-way boundary with layout (TEXT-SEAM A, amendment 2).
7. **Scripts and emoji as gates** (P5). Much of it arrives at step 1 through the vendored parley; this step is
   the corpus tests and the colour atlas.
8. **Geometry animation, justification, hyphenation, transforms** (P13 and the rest).

## 5. Questions that are the user's to decide

1. **Who owns editing state by default?** P8 shows both: a `TextState` field (the author owns text, selection,
   history, and can test and script them) and a `String` field (the input component owns selection and
   history). Which should the documentation lead with, and should `String` stay supported at all?
2. **Is static text selectable by default?** The web says yes; native toolkits say no. P7 makes it opt-in by
   scope (`selectable(..)`). Opt-out (`.unselectable()`) at the window root is the other choice.
3. **Spans plus ranges, or also markup?** `span(..)` in a content tuple and `.styled(ranges)` cover every case;
   a macro (`rich!("Deploys run in **{region}**")`) would be shorter for prose and is one more thing to learn.
4. **Fonts.** Round 7 removed system font discovery so text is identical on every platform. A real fallback
   chain for CJK and emoji needs fonts from somewhere: bundle a default UI face (a few hundred KB) and ask the
   system only for fallback, discover everything from the system, or require the application to register all
   fonts?
5. **How text should look.** Linear coverage with no gamma or contrast adjustment renders lighter than macOS
   native text. Options: leave it; a contrast table tuned per platform (still exact on CPU and GPU); and is
   LCD subpixel anti-aliasing out for good? Unhinted outlines everywhere, including low-density Linux
   displays?
6. **Keybindings.** Platform conventions built in, overridden per input by `.on_key` returning "handled" (P9).
   Is an application-wide keymap resource wanted as well?
7. **Multiple cursors and rectangular selection** in `TextState`: in scope for round 8 or later?
   (gpui-ce's editable text has neither.)
8. **`TextState::text()` cannot return `&str` if storage is to be anything but one `String`** (gpui-ce's
   storage trait made that mistake and cannot hold a rope). Proposed: it returns a `TextRef` that derefs to
   `&str` when contiguous and iterates chunks otherwise. Acceptable, or is `&str` convenience worth a
   contiguous buffer with a gap?
9. **Numbers at Frame stage are integers** (SEAM 6.1 item 3). `sig!(self.total).num(MONEY)` with an `i32` or
   `i64` of hundredths costs one word per write; with an `f32` it is formatted on the CPU when it changes
   (still no shaping). Should the façade steer authors to scaled integers for money and counters?
10. **Web features not proposed**: vertical writing modes, ruby, text shadow and stroke, initial letter,
    `text-wrap: pretty`. Which, if any, should be on the list?
