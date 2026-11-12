# Composition: foreign content, overridable components, reflection

Lane `composition`, round 6. Card: `cards/2026-11-11-foreign-parts-reflection.md`. Results: `REPORT.md`.
Code: `crates/bough/src/{foreign,part}.rs`, `crates/bough/src/backend/layers.rs`, `Bough::first_over`
(`crates/bough-core/src/bough/hit.rs`), kernel kinds 7 and 8 (`crates/bough-metal/src/shader.metal`).
Tests: `crates/bough/tests/{foreign,parts}.rs`, the doctests of `part.rs`, `crates/bough-core/tests/hit.rs`,
`crates/bough/examples/foreign_layers.rs`.

Everything marked **built** is in the tree and tested. Everything marked **design** is not.

## 0. The one mechanism

The three problems are the same problem seen from three sides:

> **A record has an address, and a description can be patched where it lowers.**

* A **foreign element** is a record whose payload is an instruction to the compositor instead of pixels. Its
  address (handle → paint position, box, clip, transform) is exactly what the compositor needs.
* A **part** is an address the component's author gives one of its records (a typed key and an index). A
  consumer's **override** is a patch on the description of whatever carries that address: a style rewrite
  before the builder lowers, guards on its routes after.
* The **inspector** reads the same addresses (every record already has a handle, an element type, and now
  provenance and part names), and its edits are the consumer's patches made at run time.

Nothing here is a component model. There is no `#[component]`, no hook, no context object. The pieces are:

| piece | what it is | Rust it needs |
|---|---|---|
| `foreign(id)` | a builder; one record of kind `REC_FOREIGN`; a slot in `Rt::foreign` | stable |
| `ForeignHost` | a trait object at the compositor boundary | stable |
| `Part<S>` | a `const` key: a name and a phantom style type | stable |
| `.part(KEY)` / `.part_at(KEY, i)` | a wrapper element at the author's site | stable (`#[track_caller]`) |
| `.restyle` / `.intercept` / `.after` | a wrapper element whose mount pushes a stack frame | stable |
| `embed!(self.field)` | a `View` held as a field, mounted in place | stable (`offset_of!`) |
| `Lens<T, V>` | a proven field offset as a `Copy` value | stable |
| `reflect_parts`, `reflect_foreign`, `reflect_owners` | side tables filled at mount | stable, except the *field walk* of an owner (§3.2) |

---

## 1. Foreign content

### 1.1 The record

A foreign element is a layout leaf and **one record** in the paint preorder (**built**):

```text
RecordInit { depth, ink, kind: REC_FOREIGN (4) | KF_NOHIT?, payload: <one instance>, handle, clip }

instance (48 B, the existing layout)          by mode
  kind 7  KIND_PUNCH   pos, size, radius, color.a = strength     Under
  kind 8  KIND_NONE    pos, size, radius                          Over
  kind 4  KIND_IMAGE   pos, size, uv = atlas rect                 Sampled

Rt::nodes[node]   the quad node layout places (LInfo::kind 1, rtn = node): Rt::quad_geom, edge-band damage
Rt::foreign.slots[i] = Slot {
    id: u64,                      // the host's name for the content
    handle: Handle, node: u32,
    mode: Mode, hit: Hit, radius: f32,
    signal: Arc<AtomicU64>,       // content serial, bumped from any thread
    seen: u64,
    uv: [u16; 4],                 // Sampled: the atlas rect
    last: Option<Placement>,      // what the host was last told
    at: &'static Location,        // provenance
}
```

Because it is an ordinary record with an ordinary layout node, every existing mechanism applies without a
special case: flex sizing, absolute placement, clips, scroll views (`scroll_run`), list regions (rows mounted on
exposure, `scroll_run_copy`), transforms, hit testing, `reflect_records`, re-description (it is opaque to it, as
an image is). The only new code on those paths is none.

### 1.2 What the compositor sees: three modes

```text
window root layer                                         (CALayer, bottom to top)
├─ foreign layers, plane Below, in paint order            Mode::Under
├─ bough's surface layer                                  not opaque once a Below layer exists
└─ clip wrappers, plane Above, in paint order             Mode::Over (wrapper = the element's clip,
                                                                      content layer inside, corner radius)
```

**Under: a hole.** The record's instance is a *destination-out* quad. In `fs_tile` the accumulator is
premultiplied RGBA starting at zero; kind 7 shades to `(0, 0, 0, coverage)` and is applied as
`acc = acc * (1 - coverage)` (the rounded-box SDF of the existing kinds, the record's clip clamp, a transformed
space's inverse: all reused). So:

* what was painted **before** the record is removed inside the hole and the surface is transparent there;
* what is painted **after** the record lies over the hole with its own alpha;
* the hole is cut to the record's clip and to its corner radius, anti-aliased.

Core Animation then composes `surface over foreign` with premultiplied source-over, which is the formula bough
itself would have used had it painted the content: a bough pixel `(c, a)` over a foreign pixel `f` is
`c + (1 - a) f`; at an anti-aliased hole edge with coverage `k` over a bough background `b`, the surface
holds `(b (1 - k), 1 - k)` and the result is `b (1 - k) + k f`. Consequences:

* **z-interleaving with one surface**, for any number of foreign elements;
* **the foreign layer needs only its frame.** Scroll-port clipping, rounded corners, a translucent badge, a
  tooltip, a focus ring, a drop shadow cast over the content: all are already in the surface's alpha. The
  host applies no mask.

It is right for **opaque** content. Content with its own transparency would show the window's backdrop through
itself, not the bough content that was painted beneath it (that content was removed).

**Over: nothing.** The instance is kind 8, which `f_stage_rec` rejects, so the record has geometry (ink, hit
target, damage) and no pixels. The host puts the layer above the surface inside a wrapper layer whose frame is
the element's clip, and rounds it itself. Right when nothing is painted over the element. bough tells the host
whether something is: `Placement::covered` is `Bough::first_over(pos, visible rect)`, the page-summary descent
`hit_cell` already uses, asked only when core damage or the element's own move could have changed the answer.

**Sampled: bough paints it.** The instance is the image kernel's, over an atlas rect the host fills
(`ForeignHost::pixels`, premultiplied RGBA at device size). Fully general: any order, any blending, and
snapshots contain it. It costs an upload and a repaint of the element's tiles per content change.

| | Under | Over | Sampled |
|---|---|---|---|
| bough content painted after it | yes | **no** (`covered` reports it) | yes |
| bough content painted before it shows through translucent content | no | yes | yes |
| clip, rounded corners | free (surface alpha) | host masks per element | free |
| bough work per content frame | none | none | upload + its visible tiles |
| bough work per move | edge bands of the hole | edge bands (avoidable, §1.7) | the image's box |
| in a snapshot / `presented_pixels` | a hole | absent | present |
| needs | a non-opaque surface layer | nothing | pixels (IOSurface binding: §1.7) |

`Mode::Auto` (**design**): `Over` while `!covered` and the clip is a rectangle; else `Under` if the content
declared itself opaque; else `Sampled`. A mode switch is a write of the instance's kind (a paint write) and a
new `Placement::plane`.

What each kind of content wants:

| content | host object | mode |
|---|---|---|
| web view (`WKWebView`) | an `NSView` sibling of bough's view, below it | Under; `Hit::Capture` |
| video (`AVPlayerLayer`, `AVSampleBufferDisplayLayer`) | a layer | Under: zero bough work per video frame |
| another renderer's Metal/wgpu surface | a layer with an `IOSurface` (or `CAMetalLayer`) | Under; Sampled when translucent |
| another process's surface | `CALayerHost` (a remote context id), or a looked-up `IOSurface` | Under; Sampled |
| a native control with its own focus ring | an `NSView` above | Over |
| software-rendered plugin content, off-screen web rendering | CPU pixels | Sampled |

A layer host (`backend/layers.rs`, **built**) covers the layer cases. Views need an `NSView`-level host with the
same `ForeignHost` contract, where bough's layer-hosting view sits between the foreign views (**design**; AppKit
composes sibling layer-backed views in subview order).

### 1.3 The host contract (built)

```rust
pub enum Mode  { Under, Over, Sampled }
pub enum Hit   { Bough, Pass, Capture }
pub enum Plane { Below, Above, Inline }

pub struct Placement {            // device px, window coordinates, y down
    pub id: u64,
    pub rect: [f32; 4],           // the laid-out box, unclipped
    pub clip: [i32; 4],           // the record's clip ∩ window
    pub radius: f32,
    pub scale: f32,
    pub plane: Plane,
    pub order: u32,               // rank among foreign elements in paint order
    pub pos: u32,                 // paint position
    pub covered: bool,            // a bough record painted after it meets its visible rect
    pub xform: Option<([f32; 4], [f32; 2])>,
    pub serial: u64,              // content changes signalled so far
}

pub trait ForeignHost {
    fn place(&mut self, p: &Placement);                       // whenever it differs from the last one
    fn remove(&mut self, id: u64);                            // unmounted
    fn pixels(&mut self, id: u64, w: u32, h: u32, out: &mut [u8]) -> bool { false }   // Sampled
    fn pointer(&mut self, id: u64, what: Pointer, ev: &Ev) {}                         // Hit::Capture
}

pub fn foreign(id: u64) -> Foreign;          // .mode(..) .hit(..) .rounded(r) + every Styled method
impl App<V> {
    pub fn set_foreign_host(&mut self, host: Box<dyn ForeignHost>);
    pub fn foreign_signal(&self, id: u64) -> Option<ForeignSignal>;   // Clone + Send + Sync
    pub fn foreign_at(&self, x: f32, y: f32) -> Option<(u64, Hit)>;
    pub fn reflect_foreign(&self) -> Vec<ForeignView>;
}
```

`App::frame` begins with `Rt::foreign_sync` when a foreign element is live. For each one it reads the record's
position, the node's box in its space, the clip, the transform and the content serial; recomputes `covered`
only if this epoch has core damage or the element moved; and calls `place` iff the placement differs from the
last one handed over. An element whose handle no longer resolves was unmounted: `remove`.

**General mechanism this is a case of.** `foreign_sync` is a *geometry observer*: "tell me when record `h`'s
visible rectangle changes". Accessibility (frame-changed notifications), the inspector's highlight, anchored
popovers (`.at(handle)`, design 02 §7 "geometry is a source") and the IME caret rect want the same thing. The
general form is a sparse watch list `(handle, last, sink)` swept after layout and scroll; the foreign slots are
that list with one sink. It is not generalised here because no second sink exists in the tree yet.

### 1.4 Damage, scroll, and the foreign clock

* **Layout and scroll.** The box moves through `Rt::quad_geom` (the hole's edge bands, as any rounded quad) or
  with its space (`scroll_run`, `scroll_run_copy`). Under scroll by copy the hole is surface pixels like any
  other: the copy moves it, alpha included, and nothing is repainted. A *static* foreign record over a
  scrolling region is repainted whole per scroll (`scroll_band` answers -1 for kinds it does not know); giving
  kind 7 a rounded quad's band is a one-line follow-up.
* **Content changes.** `ForeignSignal::content_changed()` is one atomic add from any thread. At the next frame,
  in `Sampled` mode it is a **paint write** on the record (`Rt::paint_write(handle, old serial, new serial)`:
  its visible ink goes to the paint ring, the write to the epoch's paint log, so content addressing still
  works). In the layer modes bough's pixels do not depend on the content and nothing is damaged: a video frame
  costs bough nothing.
* **Waking.** In `Sampled` mode something must run a frame. The signal should carry the host's waker
  (**design**: `CFRunLoopSourceSignal` on macOS). The prototype's tests call `frame()` themselves.
* **Transaction.** The hole and its layer must move in the same Core Animation transaction. The publisher
  thread already commits a resize's layer frame with the new contents (`pending_frame`); foreign placements
  should ride the same publication job. **Not built:** the layer host commits its own transaction on the
  calling thread, so a moving hole and its layer can differ for one refresh (a sliver of backdrop at an edge).

### 1.5 Input (built: policies; design: the view-level routing)

* `Hit::Bough`: the record is an ordinary hit target; routes on it run (`foreign(7).on_click(..)`).
* `Hit::Pass`: `KF_NOHIT`; what bough paints beneath answers.
* `Hit::Capture`: when the element is the topmost hit, `pointer_down/up/move` go to `ForeignHost::pointer` and
  to no route. Anything bough paints over the element still answers first, because hit testing is paint order.

For a real `NSView` beneath bough's view, the host's view must return `nil` from `hitTest:` where
`foreign_at` answers `Capture`, so AppKit delivers the event to the view below. Focus: a capturing element is
a tab stop whose focus claim makes the foreign view first responder. Both **design**.

Accessibility (**design**): the record's semantic entry has role `group` and the foreign view as its only
accessibility child, so VoiceOver descends into the web view at the right place in the order.

### 1.6 Cost, invariant, tests

* **Unused:** `Rt::foreign.live != 0` is tested once per `frame` and once per pointer event. The kernel has one
  more compare where a record is staged and one select per staged record per tile. Slope cells: `REPORT.md`.
* **Invariant:** after `frame()`, for every live foreign element, `slot.last` equals the placement computed
  from the record, and the host has been given every distinct placement in order. For an `Under` element the
  surface's alpha is zero on `(box rounded by radius) ∩ clip` minus the ink of later records.
* **Tests** (`crates/bough/tests/foreign.rs`, GPU, pixel-exact):
  * `under_the_hole_is_transparent_rounded_and_overlaid`: hole `(0,0,0,0)`; the rounded-off corner keeps the
    panel beneath; a half-alpha badge painted after the hole is premultiplied over nothing; placement = layout
    box, scroll port clip, order, radius, `covered`.
  * `the_hole_gate_fails_without_the_punch`: negative control (the same scene as `Over` has bough's pixels
    where the hole must be) and the `Over` contract (`Plane::Above`, `covered`).
  * `placement_follows_scroll_and_frames_stay_exact_at_every_ring_age`: 12 scroll steps in a `ScrollBox`,
    presented == full redraw at each, placement == scrolled box; then cut by the port, then scrolled away.
  * `rows_of_a_virtual_list_mount_place_scroll_by_copy_and_remove`: web views in rows of a `list(..)`; 59
    scroll-by-copy frames exact; mount on exposure places, leaving the window removes.
  * `sampled_content_is_painted_and_repainted_on_its_signal_only`: the host's pixels texel for pixel; a signal
    from another thread is damage of exactly the element's tiles; idle otherwise.
  * `an_idle_frame_places_nothing_and_draws_nothing`, `hit_policies`,
    `order_is_paint_order_and_unmount_removes`.
  * Feature `sabotage_foreign` (placements ignore the element's space): the two placement gates fail.
  * `first_over_equals_the_oracle` (bough-core): 21,600 queries against a scan, across random trees and edits.
  * `examples/foreign_layers`: a real window, two real layers; reads the layer tree back.

### 1.7 Special cases, and what they are cases of

| built as | general mechanism | why not built |
|---|---|---|
| kind 7, destination-out | a blend operator per record (over, destination-out, plus, multiply) in two bits of the instance | only one operator has a user; the select would be per record either way |
| kind 8, staged and rejected | "ink without payload": a payload sentinel the **bin** pass skips, so the record costs no bin entry and moving it need not damage | touches `fs_bin` and its wgsl twin; kind 8 is one compare |
| CPU pixels into the image atlas | an `IOSurface` bound as a texture in the bind table (`FBind` is append-only: `f[n]` foreign textures, the instance's `flags` picks one) | needs residency-set changes (drain and re-arm) and a kernel case |
| `foreign_sync` | the geometry watch list (§1.3) | one sink |
| the layer host's own transaction | placements in the publication job (§1.4) | crosses the publisher thread's protocol |

Not ported: `bough-gpu` (the wgsl kernels, feature `wgpu`) knows neither kind. A foreign record there would
shade as the default case. The wgsl port is part of the next card.

### 1.8 Example: a web view between two panels in a scroll view

`crates/bough/examples/foreign_layers.rs`, abridged (it runs; see `REPORT.md`):

```rust
use bough::prelude::*;

struct Page { y: Signal<f32> }

impl View for Page {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0x202830)).p(20.0).gap(10.0)
            .child(
                div().w(440.0).h(240.0).bg(rgb(0xffffff)).scroll(sig!(self.y))
                    .child(div().w(420.0).h(80.0).bg(rgb(0xcc3333)).child(text("a bough panel above the web view")))
                    .child(
                        div().relative().w(420.0).h(120.0)
                            // the web view: laid out, clipped by the scroll port, rounded, under bough's surface
                            .child(foreign(1).w(420.0).h(120.0).abs(0.0, 0.0).rounded(16.0))
                            // bough content painted after it: lies over the web view, translucent
                            .child(div().abs(16.0, 16.0).px(10.0).py(4.0).bg(Color::rgba(0, 0, 255, 128)).rounded(8.0)
                                .child(text("bough, over the web view"))),
                    )
                    .child(div().w(420.0).h(260.0).bg(rgb(0x33aa33)).child(text("a bough panel below the web view"))),
            )
            // a native control: above the surface
            .child(foreign(2).mode(Mode::Over).w(120.0).h(40.0).rounded(8.0))
    }
}

// the host side, once the window exists:
app.set_foreign_host(Box::new(LayerHost::new(&surface_layer, |id| layer_for(id))));
```

---

## 2. External components that compose

### 2.1 The component does not name its consumer (AUDIT D4, D8)

Elements are typed by their owner (`Element<V>`), because a wire and a route are typed code that reads
`owner + offset`. That typing is what makes them free of dispatch, and it is what made a shared helper
impossible to write without `impl Element<V>` in every signature. Two forms remove the consumer's type from
the component. Both are stable Rust, and neither erases anything.

**(a) A component with state is a `View`, held as a field (`embed!`, built).**

```rust
// crate date_picker: nothing here mentions who uses it
pub struct MonthPicker { pub month: Signal<u32>, pub selected: Signal<u32> }
impl MonthPicker {
    fn next(&mut self) { *self.month = *self.month % 12 + 1; *self.selected = 0; }
    fn day(&self, d: u32) -> impl Element<Self> + '_ { /* .. .on_click(move |p: &mut MonthPicker| *p.selected = d) */ }
}
impl View for MonthPicker {
    fn view(&self) -> impl Element<Self> + '_ { /* text(sig!(self.month).map(..)), button(">").on_click(MonthPicker::next), .. */ }
}

// the consumer
struct Trip { depart: MonthPicker, back: MonthPicker, nights: Signal<i32> }
impl View for Trip {
    fn view(&self) -> impl Element<Self> + '_ {
        div().child(embed!(self.depart)).child(embed!(self.back)).child(text(sig!(self.nights).map(..)))
    }
}
```

```rust
pub struct Embed<'a, C, V> { c: &'a C, off: u32, _v: PhantomData<fn(&V)> }
impl<'a, C: View, V: 'static> Element<V> for Embed<'a, C, V>;
// embed!(self.a.b) = embed_proven(&self.a.b, offset_of!(Self, a.b), |o: &Self| &o.a.b)   (as sig!)
```

`Embed::mount` builds a `Mounter<C>` that shares everything with the consumer's `Mounter<V>` (runtime, record
buffer, scope, tape, layout cursor, override chain) except the owner pointer and one new field,
`Mounter::base: u32`, the sum of the proven offsets on the way down. `Mounter::wire` installs a wire at
`offset + base`, so the embedded view's signals are wired cells **of the consumer's class**: one settle pass,
no second owner, no registry walked per turn. `Route` carries `base`, and dispatch calls the trampoline with
`owner + base`. That is the design's "routes resolve their receiver by the owner chain" (L4), resolved at mount
to one add.

This is the contained-child rung of the storage ladder (D8). The only signatures a component author writes are
`fn view(&self) -> impl Element<Self> + '_` and `&mut self` methods.

Limits of the prototype: inside an embedded view, lists, readers, entities, virtual lists, scroll boxes and
text fields are refused at mount (their patches are handed the class's owner and read it as the mounting
owner). The fix is to give each embedded view its own owner class with a `class_base`, so `settle_cls` hands
`owner + base`: one add per class per turn. `View::changed` of an embedded view is not called for the same
reason.

**(b) A helper without state is generic over its consumer, and writes through a lens (built).**

```rust
pub struct Lens<T, V> { off: u32, .. }                 // Copy, 4 bytes: fits a route's 16
impl<T, V> Lens<T, V> { pub fn of<'a>(&self, v: &'a mut V) -> &'a mut Signal<T>; }
impl<'a, T, V> Wired<'a, T, V> { pub fn lens(&self) -> Lens<T, V>; }

pub fn date_picker<V: 'static>(selected: Wired<'_, u32, V>) -> impl Element<V> + '_;
```

A `Lens` is the `offset_of!` proof of `sig!` kept as a value, so a route in another crate can write the
consumer's signal safely. This form keeps `impl Element<V>` in the signature; it is for leaf helpers.

What neither form fixes: the return type itself (`impl Element<Self> + '_`, D3: arms of `if`/`match` do not
unify). That is `El<'f>`, another lane's.

### 2.2 Parts: addresses

```rust
pub struct Part<S> { name: &'static str, _s: PhantomData<fn(S) -> S> }
impl<S> Part<S> { pub const fn new(name: &'static str) -> Part<S>; }

pub const DAY: Part<BoxStyle> = Part::new("date-picker.day");            // the author's whole opt-in...
row((text(format!("{d}")).part(DAY_LABEL),)).on_click(..).part_at(DAY, d)   // ...and one call per site

pub trait Restyle: Sized {                       // implemented by LBox and Para; On and Parted pass through
    type Style: Copy + 'static;                  // BoxStyle, TextStyle
    type Out;                                    // the same builder with its style replaced
    fn restyled(self, index: u32, f: &dyn Fn(Self::Style) -> Self::Style) -> Self::Out;
}
pub struct BoxStyle  { spec: Spec, bg: Option<Color>, radius: f32, border: Option<(f32, Color)>, draw: bool, index: u32 }
pub struct TextStyle { spec: Spec, color: Option<Color>, size: f32, lh: f32, index: u32 }
// both implement Styled, so the override closure speaks the builders' own vocabulary: s.bg(..).rounded(..).w(..)
```

The author exposes **addresses, not props**. A key says "this element can be reached" and what kind of style it
has; it does not enumerate what may be changed. The key's type parameter is the only coupling: `.part(DAY)` on
a text does not compile (`expected Part<TextStyle>, found Part<BoxStyle>`), and a `restyle` closure can only
call what the style type has.

Addresses, from most to least stable:

1. **`Part<S>` keys** (built): typed, named, chosen by the author. For code.
2. **The semantic column** (design; BRIEF §5.3 item 1): role, id, class, accessible name, filled by the
   builders themselves (`button` is `role:button`, its label is its name). Addressed by the selector language,
   at run time, by the inspector, tests and plugins. Parts are a column of the same table (`hook:` in zgx's
   selectors is a part name).
3. **Provenance** (built for parts and foreign elements; design for every builder): the source location of the
   builder call. The inspector's address for an element with no part.

### 2.3 Overrides: patches on the description

```rust
pub trait Compose: Sized {                                    // blanket: every element
    fn part<S>(self, key: Part<S>) -> Parted<Self, S> where Self: Restyle<Style = S>;
    fn part_at<S>(self, key: Part<S>, index: u32) -> Parted<Self, S> where Self: Restyle<Style = S>;
    fn restyle<S: Copy + 'static, F: Fn(S) -> S>(self, key: Part<S>, f: F) -> With<Self, StyleOv<S, F>>;
    fn intercept<V, S, F: Fn(&mut V, &PartEv) -> Flow + Copy + 'static>(self, key: Part<S>, f: F) -> With<Self, GuardOv<S, F>>;
    fn after<V, S, F: Fn(&mut V, &PartEv) + Copy + 'static>(self, key: Part<S>, f: F) -> With<Self, AfterOv<S, F>>;
}
pub struct PartEv { pub ev: Ev, pub index: u32 }
pub enum Flow { Continue, Stop }

pub struct OvFrame { parent: *const OvFrame, name: &'static str, ty: TypeId, kind: Style{..} | Guard{..} }
pub struct Mounter<'a, V> { .., pub(crate) ov: *const OvFrame, pub(crate) base: u32 }
```

**Resolution.** `With<E, O>::mount` (and `::update`) makes one `OvFrame` on its own stack frame, links it in
front of `m.ov`, mounts `E`, and unlinks it. A part site walks the chain from the innermost frame outwards and
applies every style frame with its name and style `TypeId`, so the **outermost wrapper (the application) has the
last word**, and an inner wrapper's other properties stay. The chain is a stack discipline threaded through an
argument: nothing is allocated, nothing is global, and it cannot outlive the mount.

**Why it rewrites the description and does not patch records.** A view re-describes after every handler
(`describe.rs`). A patch applied to the retained records after the author's style would be undone by the next
re-description and re-applied after it: two layout invalidations per handler, forever. Rewriting the builder
before it lowers means mount and re-description both see the patched description, and the lockstep walk finds
nothing to change (`a_redescription_under_an_override_is_quiet`).

**Typing.** A background override replaces a possibly-wired operand with a constant, which changes the
builder's type. `Restyle::Out` is that type: `LBox<Bg, ..>` restyles to `LBox<Ov<Bg>, ..>`, where
`Ov<P>(Option<Color>, P)` lowers to the constant if there is one and to `P` otherwise. `Parted` always mounts
`E::Out` (with an identity rewrite when no wrapper is in force), so a part site instantiates its subtree's
mount code once, not twice.

**Behaviour.** A route is data in the act table (`fn` pointer plus 16 bytes). A guard is the same kind of data
in `Input::guards`, attached to the part's act when the part mounts under a guard frame. At dispatch:

```text
click on the part (or a descendant: dispatch bubbles to the part's act)
  before-guards, outermost wrapper first     Flow::Stop  -> the component's route does not run   (intercept / replace)
  the component's own route
  after-guards, outermost first                                                                   (extend)
```

The guard runs on the consumer (`&mut V`, at the consumer's own `base`), the route on the component. They run
one after the other, so the consumer never holds `&mut` to itself while the component's handler runs: no
aliasing, no re-entrancy, no callback handed into the component. With `embed!`, the consumer's guard reads and
writes the component's state directly (`me.depart.selected`), because the component is its field.

**General mechanism this is a case of.** A guard is a capture-phase handler scoped to a part key: the DOM's
capture/bubble. Bubbling already exists (the dispatch chain of flagged ancestors); guards are capture added
where a wrapper asked for it.

### 2.4 Cost, invariant, tests

* **Unused:** an element that is not a part site: nothing, at mount, re-description or dispatch (one
  `guards.is_empty()` per dispatched click). A part site with no wrapper: the builder is moved through
  `restyled` with an identity rewrite, one null test, one hash insert for reflection. `Mounter` grew by 12
  bytes (`ov`, `base`); `Route` by 4.
* **Invariant:** for a part mounted under wrappers `w1 ⊃ w2 ⊃ .. ⊃ wk`, the retained style is
  `f1(f2(..fk(author's style)))` after mount and after every re-description, and a re-description without a
  state change leaves no damage.
* **Tests** (`crates/bough/tests/parts.rs`, GPU): the plain component; restyle (paint, layout box, text colour
  of 35 cells by index); intercept and after (state and order); quiet re-description; nested wrappers;
  retained == full redraw == fresh mount; the embedded component (own state, routes, wires, guard on one
  instance only); reflection of parts, foreign elements and embedded owners with edits. Compile-fail doctests
  in `part.rs`: a text property on a box part; a box key on a text; a guard capturing 32 bytes; `embed!`
  through a `Box`.

### 2.5 Limits and what closes them

| limit | closing mechanism |
|---|---|
| rows a list mounts later (on exposure) see no wrapper: the frames are gone | the region keeps the chain as records (style closures `Copy`, at most 16 bytes, like routes) in its owner class and rebuilds the frames for each row mount |
| guards cover click only | the same list keyed by `(act, verb)`; named commands (`#[command]`, D10) are guarded by command id |
| the guard's payload is a `u32` index | `Part<S, P = ()>` with `part_with(KEY, payload)`, `P: Copy` of at most 8 bytes in the guard record |
| hover colour is not in `BoxStyle` | `On<E, HoverBg>` restyles its op too |
| `On::update` does not refresh a route's closure (generalization lane); guards inherit that | fix `On::update`; `Parted::update` then refreshes its guards |
| a remounted dynamic item leaves its old guards in `Input::guards` (dead act indices, never matched) | free with the act |
| `.part`, `.restyle`, `.after`, `.intercept` are methods on every type in scope of the prelude, as `.on_click` is; they shadow an inherent `&mut self` method of the same name (it happened: `Reflect::part`) | a marker trait implemented by the builders (one line each), `impl<E: Builder> Compose for E` |
| framework widgets have no parts yet | `button::BOX`, `button::LABEL`, `text_input::BOX`, list rows |

### 2.6 Example: a third-party DatePicker, restyled and intercepted

`crates/bough/tests/parts.rs` (it runs). The component, as another crate writes it:

```rust
pub const DAY: Part<BoxStyle> = Part::new("date-picker.day");
pub const DAY_LABEL: Part<TextStyle> = Part::new("date-picker.day-label");

fn day<V: 'static>(d: u32, selected: Wired<'_, u32, V>) -> impl Element<V> + '_ {
    let at = selected.lens();
    row((text(format!("{d}")).color(INK).part(DAY_LABEL),))
        .center().w(28.0).h(28.0)
        .bg(selected.map(move |s| if *s == d { SELECTED } else { CELL }))
        .rounded(4.0)
        .on_click(move |v: &mut V| { at.of(v).set(d); })       // the "select" route
        .part_at(DAY, d)
}
pub fn date_picker<V: 'static>(selected: Wired<'_, u32, V>) -> impl Element<V> + '_ {
    column(each((0..5u32).map(move |w| row(each((0..7u32).map(move |c| day(w * 7 + c + 1, selected)))).gap(2.0)))).gap(2.0).p(4.0).bg(WHITE)
}
```

The consumer. The author exposed no style prop and no callback:

```rust
struct Booking { date: Signal<u32>, refused: Signal<u32>, log: Vec<u32> }
impl View for Booking {
    fn view(&self) -> impl Element<Self> + '_ {
        div().p(10.0).child(
            date_picker(sig!(self.date))
                // style: Sundays tinted, every day a 32 pt pill, labels navy
                .restyle(DAY, |s| (if s.index() % 7 == 0 { s.bg(SUNDAY) } else { s }).rounded(14.0).w(32.0))
                .restyle(DAY_LABEL, |t| t.color(NAVY))
                // behaviour: Sundays are closed (the select route is stopped), every selection is logged
                .intercept(DAY, |me: &mut Booking, e: &PartEv| {
                    if e.index % 7 == 0 { *me.refused += 1; Flow::Stop } else { Flow::Continue }
                })
                .after(DAY, |me: &mut Booking, e: &PartEv| me.log.push(e.index)),
        )
    }
}
```

---

## 3. Reflection for the inspector

### 3.1 What the inspector can read and edit today

| what | call | since |
|---|---|---|
| every record: kind, depth, ink, clip, element type name, layout box, resolved style, colour, text | `reflect_records`, `record_view` | round 4 |
| root owner state, typed, editable by path | `reflect_fields`, `reflect_set` | round 4 (nightly `type_info`) |
| layout style and paint of a record, editable | `reflect_style`, `reflect_set_style`, `reflect_set_color` | round 4 |
| **parts**: name, index, record handle, paint position, source line of the `.part(..)` call | `reflect_parts` | this lane |
| **foreign elements**: id, mode, hit policy, source line, the placement the host holds | `reflect_foreign` | this lane |
| **every embedded owner**: type name, offset, fields, edit by path as an ordinary write in a turn | `reflect_owners`, `reflect_owner_fields`, `reflect_set_owner` | this lane |

The inspector listing and editing both a component's parts and a foreign element
(`the_inspector_lists_and_edits_parts_and_foreign_elements`, `reflection_reaches_every_embedded_owner`; they
run):

```rust
let parts = app.reflect_parts();          // 70: 35 "date-picker.day" with index 1..=35, 35 "date-picker.day-label"
let foreign = app.reflect_foreign();      // [ForeignView { id: 42, mode: Under, source: "tests/parts.rs:..", placement: Some(..) }]
let owners = app.reflect_owners();        // [Trip @0, MonthPicker @offset_of!(Trip, depart), MonthPicker @offset_of!(Trip, back)]

app.reflect_set_color(day_3.handle, rgb(0xff0000));     // a paint write on the part's record
app.reflect_set_owner(2, "month", "9");                 // a write in a turn: the embedded view's wire runs
app.reflect_set("date", "12");                          // the root owner
```

### 3.2 Stable Rust: what each tool gives, and what needs nightly or a derive

Today `crates/bough` enables `type_info, ptr_metadata, rustc_attrs, const_trait_impl, const_cmp,
arbitrary_self_types_pointers`, all for `reflect.rs`. Everything this lane added is stable Rust.

| need | stable mechanism | what it cannot do |
|---|---|---|
| element type of a record | `type_name::<Self>()` in each builder's `mount` (exists) | nothing |
| the source line that made a record | `#[track_caller]` on the builder constructors and `Location::caller()`; the `&'static Location` rides in the builder and goes to the sparse column at mount | costs 8 bytes per builder value: make the field a ZST without the `devtools` feature |
| part and foreign addresses | `const` keys, `TypeId` of the style type | nothing |
| a field's fixed offset (wires, lenses, embedded owners) | `offset_of!`, with a never-called accessor closure pinning the field's type | cannot follow a `Box` or name an enum variant's field: by design |
| a registry of live owners | a monomorphic thunk captured where the type is concrete: `embed!`, a list's row class, an entity view, `App::new` | nothing |
| the routes of a record, by name | `type_name::<F>()` of the route's fn item or closure at `.on_click` (`Counter::inc` reads as its path), plus its `Location` | closures have no better name than their location |
| framework style types (`Spec`, `BoxStyle`, cephal's `Style`) | a hand-written field table per framework type: `fn visit(&mut self, f: &mut dyn FnMut(&'static str, &mut dyn Edit))` | it is written by hand, once per framework type |
| **named wired state, read and write** | `sig!` already has the field path as tokens: `stringify!` it, and pick `Debug` / `Edit` thunks for the signal's type by **autoref specialisation** (the macro expands where the type is concrete). One `static SigMeta { path, type_name, fmt, edit }` per `sig!` site, its address in `Wired` | covers every field that drives the UI through a wire; not plain fields |
| the state of an owner, read-only | autoref specialisation at the registration macro: if `C: Debug`, keep a `fmt` thunk and show the pretty-printed tree | needs `#[derive(Debug)]`; no editing |
| the state of an owner, read and write, with enums | if `C: Serialize + Deserialize` (the design's journaling, L11/D20, requires them anyway): reflect through a `Serializer` that records the field tree and a `Deserializer` that replays a patched one | needs the serde derives; an edit replaces the owner's value and re-describes |
| **the fields of an arbitrary type with no derive at all** | **none.** Stable Rust has no field enumeration. | this is what `type_info` buys |
| which trait a *nested* field type implements (is `self.a.b.c: Debug`?) | none (autoref specialisation sees only the type the macro is handed) | `type_info`'s `trait_info_of` |
| the active variant of an enum | parse the leading identifier of its `Debug` output and show the variant's fields from `Debug` too (never by offset: a hand-written `Debug` could name another variant) | needs `Debug`; read-only. With serde: exact |

So the recommendation is a **ladder of probes chosen at the registration site by autoref specialisation**, the
nightly one being the top rung and optional:

```text
feature "reflect-nightly" (type_info)   structural walk, no derive, edit any Edit leaf      <- today's reflect.rs
Serialize + Deserialize                 field tree with enums, edit by replace              <- stable
Debug                                   read-only tree                                      <- stable
nothing                                 type name, size, and its wired signals (SigMeta)    <- stable
```

The inspector's protocol is the same on every rung (`FieldView`); rungs differ in coverage. `reflect.rs` moves
behind the feature and the crate builds on stable without it.

What stays impossible on stable, stated plainly:

1. A no-derive field walk of user types (the first rung).
2. `&self.count` as a wire without `sig!` (D2). Proving that an address is a struct-field path of the owner
   needs the compile-time schema; `offset_of!` needs the path as tokens, which is what `sig!` is.
3. Compile-time detection of trait impls below the top level.

### 3.3 The gap list

| # | the inspector needs | mechanism | stable Rust | state |
|---|---|---|---|---|
| 1 | every live owner's state, not only the root | owner registry filled at mount with per-type thunks; embedded views here, then list row classes and entity views (they already have a class: add the thunks to it), readers as cells | yes (the field walk itself: §3.2 ladder) | **built** for `embed!`; rows and entities: design |
| 2 | provenance of every record | `#[track_caller]` on builder constructors; `&'static Location` in the sparse column keyed by handle | yes | **built** for `.part` and `foreign`; all builders: design (8 bytes per builder, ZST without `devtools`) |
| 3 | a semantic column (role, id, class, name, value, hook) | sparse table keyed by handle, filled by the builders; parts are its `hook` | yes | parts **built** (`Reflect::parts`); role/name: design |
| 4 | foreign elements in the tree, with their compositor state | `reflect_foreign`; the record kind in `reflect_records` | yes | **built** |
| 5 | style edits that survive re-description | a **pin**: an override frame the runtime owns, as data (`StylePatch { set: bitmask, .. }`), keyed by part or by `Location`, applied at the tail of `apply_chain`. For elements with no part, builders consult the pin table when it is non-empty (one predictable branch; compiled out without `devtools`) | yes | design. Today `reflect_set_style` and `reflect_set_color` are overwritten by the next re-description |
| 6 | routes of a record, and who handles an event | `type_name` and `Location` of each route at install; a dispatch trace (chain, guards, verdicts) in the frame log | yes | design |
| 7 | style editing by builder method ("Rust styles" in the gpui-ce inspector) | a hand-written method table for `Styled` (name, doc, apply), shared with the pin's `StylePatch` | yes | design |
| 8 | picking that swallows the app's input, depth stepping | `pick_record` and `record_ancestors` exist; an inspect flag at dispatch | yes | flag: design |
| 9 | a highlight over the picked element | an overlay record at the end of the paint order, driven by the geometry watch (§1.3); the focus ring is the same thing | yes | design |
| 10 | the inspector next to the app | either a second window (multi-window host, missing), or docked: a wrapper view that embeds both, `row((embed!(self.app), embed!(self.inspector)))` | yes | `embed!` built; the wrapper: design |
| 11 | enums opened in a value walk | §3.2: serde exact; `Debug`-parsed read-only; no rustc extension needed | yes | design (answers BRIEF §5.3 item 4's "decide": (c) extended by parsing, or serde) |
| 12 | zero cost when nothing inspects | the side columns are sparse and written at mount only; with a `devtools` cargo feature off: `Location` fields are ZSTs, `Reflect::elem`/`add_part` are empty, the pin branch is gone | yes | design (today the tables are always written: one store per handle, one hash insert per part) |
| 13 | transitions addressable by property (INSPECTOR #7) | the transition's key is `(handle, Apply type)`; `type_name::<K>()` names the property | yes | design |
| 14 | the fields of an arbitrary owner without any derive | `type_info` | **no** | built (nightly), to become the optional top rung |

### 3.4 Porting the gpui-ce inspector

The fork's inspector (`gpui-ce-0.3/crates/gpui/src/inspector.rs`, panel in
`gpui-component/crates/component/src/inspector.rs`) tracks one selected element and edits one `StyleRefinement`.
What its UI asks of the framework, and what answers it here:

| the panel needs | gpui-ce | bough |
|---|---|---|
| a stable element id | `InspectorElementId { path: (GlobalElementId, Location), instance_id }`, rebuilt each frame | the record handle (retained); part name + index; `Location` |
| source location | `Interactivity::new` is `#[track_caller]` (div, img, svg only) | gap 2: every builder |
| bounds, content size | recorded in `prepaint` for the selected element | `record_view(pos).layout`, ink, clip: every record, always |
| read and replace the style, applied before the next layout | `with_inspector_state::<DivInspectorState>`; `base_style` overwritten in `request_layout` | gap 5: pins (the same shape: an override consulted where the description lowers) |
| picking, hover preview, depth by wheel, input swallowed | hitboxes for every `Interactivity` element while picking | `pick_record`, `record_ancestors`; gap 8 |
| highlight quad | painted by the window | gap 9 |
| "Rust styles" with completion; JSON styles | `#[derive_inspector_reflection]` on `Styled`; serde of `StyleRefinement` | gap 7 (a table, no proc macro); `StylePatch` is plain data and can be serde |
| per-element-type panels | `register_inspector_element::<T>` | `Restyle::Style` already names the style type per element kind |
| element tree, owner state | **absent** | `reflect_records`, `reflect_owners` |
| web views in the tree | absent (wry child view placed on top, not interleaved) | `reflect_foreign` |

The panel is therefore portable once gaps 2, 5, 8 and 9 are built, and it gains a tree and a state view it does
not have upstream. The fork's shader work (`origin/shader-paint`) maps onto the paint floor (D21), not onto this
lane. No translation work was found in the fork beyond gpui-component's upstream `rust_i18n`.
