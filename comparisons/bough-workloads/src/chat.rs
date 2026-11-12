//! A chat client with media: a sidebar of 200 conversations, a conversation of 2,000 messages from 12
//! participants (200 images, quotes, reactions), a typing indicator, a composer and a header with participant
//! avatars and a search box. Window 1280 x 800 points.
//!
//! # What is on screen (counts at `SCENE_SCALE` 1)
//! * sidebar (300 pt, a scroll box): 200 rows, each an avatar block with initials, the name, a one-line preview
//!   (a `Signal<String>` per conversation), a time (a signal) and an unread badge (a `Signal<u32>` per
//!   conversation, `.num(..)` text); a translucent selection layer that moves with a `translate` transition;
//! * conversation (a scroll box): 2,000 messages. Each: avatar, author name, time, a bubble with a wrapped
//!   `para` (1-6 lines), sometimes a quoted message (a bordered box, 14 %), every 10th an image (200 in all), one
//!   in five a row of reaction pills (a letter and a count). Messages that arrive while the scene runs are rows
//!   of a `List<Arrival>` after the 2,000;
//! * header: the conversation name, 12 participant avatars (the hover targets), a search `text_input`;
//! * typing row: three dots whose opacity pulses; composer: a `text_input` bound to the draft and a send button.
//!
//! # The decision: slots or `List<Message>` (the brief asks)
//! The history is **2,000 mounted slots** (`each` over the message array), not a `List<Message>`; arrivals are a
//! `List<Arrival>` after them. Why:
//! * the fan-out. Every message element that depends on "this participant is hovered" or on the theme reads
//!   `sig!(self.alook).at(author)`, one signal per participant, so hovering an avatar writes **one signal** whose
//!   wires (~167 messages x 4-8 properties = `fanout()`) repaint that participant's messages. A `List<Message>`
//!   row is an owner of its own: it reads only its own fields, not the root's `alook` (`sig!(m: Message => ..)`
//!   binds to the row), and a route in a row is refused (`root_owner_only`). The same highlight would need a copy
//!   of the look in every `Message` and `messages.edit(i).look.set(..)` for each of the participant's ~167
//!   messages: 167 list edits (each logged in the delta log, mounted or not) instead of one write, and a theme
//!   switch 2,000 edits instead of 12 writes. With `.scroll(..)` the list is virtual and only ~15 rows hold
//!   wires, so a hover would repaint far less, but the scene would then measure ~15 messages (mount, resize and
//!   damage of a window, not of a conversation): a different workload. The slots keep the whole conversation
//!   resident, the standing workload for "a long document that is all there";
//! * a message that arrives is a new row at the end: that is what a list is for (`push`, measured natural
//!   height, wrapped text, an optional image). A pool of hidden slots would need a way to collapse an unused
//!   slot (there is none: see below). The arrival rows keep a copy of the author's look in a signal of their
//!   own; `relook` writes the copies of the (few) rows by hand. (Non-virtual lists inside a scroll box window
//!   themselves by exposure: rows scrolled out of view unmount.)
//!
//! # Phases here
//! * `steady`: one character typed into the composer per step (`App::text_input` into the focused field; the
//!   first step scrolls to the newest message and presses on the field to focus it; the draft is cleared every
//!   36 characters); every 6th step a message from another participant arrives (a list push, the sidebar's
//!   preview and time for the open conversation and the preview, time and unread count of one background
//!   conversation change, the conversation glides to the end: `App::wheel` aims the smooth scroll, `tick`
//!   advances it); every 12th step the user presses the send button (an own message: the same arrival) and
//!   presses the field again to refocus; every 10th step a wheel detent in the conversation. Each step ends
//!   with one `tick(1/60)`;
//! * `hover`: the pointer onto participant 3's header avatar (even k) and off it (odd k): one signal write, the
//!   fan-out is that participant's messages (`fanout()` counts their look wires: 830 at full size);
//! * `theme`: dark/light for everything (the 12 look signals, the chrome signal, the arrivals' copies);
//! * `animate`: a message arrives and the conversation glides to it (the slide-in is the scroll glide), and in
//!   one turn three transitions are retargeted: the typing dots' opacity (a pulse), the veil over the newest
//!   history image (the image fades in and out), and the sidebar's selection layer (`translate`: it jumps to
//!   another conversation). These keep running while the window is resized and the bubbles' text re-wraps.
//!   Arrivals carry no transitions of their own: see the bug below.
//!
//! # Images
//! 200 `image(&'static [u8])` calls (every 10th message) over `PICS` = 8 distinct PNGs generated at build time
//! (gradient + rings + light noise; four sizes: 480x320, 320x200, 200x200, 360x240, shown at 41-62 % of their
//! pixel size, i.e. resampled to 1:1 device pixels at device scale 2). Each `image()` hashes its PNG bytes at
//! mount (200 hashes of ~50-150 KB each) and decodes + resamples only on a cache miss (8 decodes). The image
//! atlas is one 1024 x 1024 RGBA texture shared by everything that is an image. `CHAT_PICS=n` (environment)
//! changes the number of distinct images: 8 is the most that fits at these sizes (9 panics with `image atlas
//! full`, as does 200).
//!
//! # Wanted and not expressible today (results, not workarounds)
//! * **200 distinct images do not fit.** The image atlas is a single 1024 x 1024 texture (4 MiB) with a shelf
//!   packer that never evicts (`deco.rs` `Images::alloc`, `IMG_W`/`IMG_H` in bough-metal); 200 images of
//!   ~200x130 pt at device scale 2 need ~85 MB of texels. `CHAT_PICS=9` (and 200) panic at mount with `image
//!   atlas full` (`crates/bough/src/deco.rs:215`). The scene reuses 8 distinct images across the 200 slots (the
//!   cache key is content hash + device size). Media needs a larger or paged atlas, per-image textures, or
//!   decode and upload only for what is on screen;
//! * **lazy image decode**: `image()` decodes at mount, so mounting the conversation decodes (and uploads) every
//!   image in it, scrolled into view or not (a virtual list would defer it; the slots cannot);
//! * **image scaling modes**: an image is resampled (bilinear, on the CPU, at mount) to its box's device size;
//!   there is no fit/cover/contain, no crop, no aspect lock, no resize after mount (a `w_pct` box would draw the
//!   atlas texels 1:1), so images have fixed sizes and do not follow the window; **no rounded corners or clip on
//!   an image** (the bubble is rounded, its image is square); **no opacity on an image** (`ImageBox` is not an
//!   `LBox`: the fade is a bubble-coloured veil box whose opacity transitions);
//! * **translate of a subtree**: `.translate(..)` moves only the box's own paint, not its children, and text is
//!   not a box, so a message cannot slide with its text by translate; a layout transition (`h_op` of a spacer)
//!   can, but see the next item;
//! * **BUG: a transition inside a list row outlives the row.** `Rt::cut_scope` kills a released scope's wires
//!   but not its transitions (`Transitions.items` keep stepping). Giving each `Arrival` an `h_op(sig!(self.slide)
//!   .transition(..))` spacer (and an opacity veil) and then resizing the window in the same step panicked with
//!   `layout node is in the Bough` (`crates/bough/src/lay_deep.rs:507`, from `Rt::lay_compute_pending` in
//!   `App::resize`): the resize changed the content height, the row left the visible window (non-virtual lists in
//!   a scroll box unmount by exposure), the running transition kept restyling its dead layout node. (Cause by
//!   reading the code; the repro was arrival + scroll-to-end + the row transition + `app.resize` before a
//!   frame.) Paint-only transitions would write to freed (and reusable) nodes. So arrivals carry none;
//! * **collapsing / hiding a subtree**: there is no `display: none`, no conditional element, and
//!   `clip_overflow` is a layout rule, not a paint clip, so a pool of "not yet arrived" slots cannot be made
//!   invisible; hence the list for arrivals;
//! * **scroll offsets are not re-clamped when the content changes height**: a resize that re-wraps 2,000
//!   paragraphs leaves the offset where it was, so after the conversation scrolled to the end, widening the
//!   window shows blank space beyond the (now shorter) content until the next wheel (a chat wants an offset
//!   anchored at the bottom, or a clamp). The resize phases of this scene therefore show text for about half of
//!   the sweep (the narrow half) and the background for the rest; the layout work is the same either way;
//! * **a colour from two signals**: the theme and the hovered participant are packed into one `u32`
//!   (`DARK | HOT`) per participant by hand (as in the sheet); chrome reads a second signal (`theme`);
//! * **reactive border colour, reactive text-field colours**: `.border(w, c)` and `text_input` take constants,
//!   so the quote box edge is one mid grey in both themes and the composer and search fields stay light in the
//!   dark theme; `text_input` has a fixed size (no `grow`), so the composer does not follow the window width;
//!   no placeholder text, no multi-line field, no Enter-to-send without an `on_key` on a focusable ancestor;
//! * **text selection, copy, links, ellipsis**: not available; a one-line label never clips or ellipsises, so
//!   previews are truncated by hand to a length that fits;
//! * **shared state for list rows**: a `List<T>` row cannot read the root's signals or mount a route (see the
//!   decision above), so the arrival rows carry copies of the look;
//! * **max-width bubbles**: no `max_w`, so a history bubble is a fixed fraction of its column (`w_pct`), not
//!   shrink-to-fit (an arrival row, whose width is not definite yet when it mounts, comes out content-sized).
use crate::{scaled, Rng, Scene};
use bough::prelude::*;
use bough::raw::{no_hit, App, Button, Caps, Ev, NumFmt, Wired};

pub const PARTS: usize = 12;
pub const CONVS: usize = 200;
pub const MESSAGES: usize = 2_000;

const W: f32 = 1280.0;
const H: f32 = 800.0;
const SIDE_W: f32 = 300.0;
const ROW_H: f32 = 64.0;
const SIDE_HEAD: f32 = 52.0;
const HEADER_H: f32 = 60.0;
const TYPING_H: f32 = 24.0;
const COMPOSER_H: f32 = 64.0;
const TITLE_W: f32 = 200.0;
const AVATAR: f32 = 28.0;
const AVATAR_GAP: f32 = 4.0;
const FIELD_W: f32 = 360.0;
const SEND_W: f32 = 72.0;
const BUBBLE_PCT: f32 = 0.74;
/// The participant whose header avatar the hover phase targets.
const HOT_PERSON: usize = 3;
const ACCENT: u32 = 0x2f6bff;
const QUOTE_EDGE: Color = rgb(0x8a93a6);

const DARK: u32 = 1;
const HOT: u32 = 2;

const COUNT: NumFmt = NumFmt::new("", None, "");

// ------------------------------------------------------------------------------------------------ palette

fn pick(l: u32, light: u32, dark: u32) -> Color {
    rgb(if l & DARK != 0 { dark } else { light })
}
fn page_bg(l: u32) -> Color {
    pick(l, 0xeef0f4, 0x101216)
}
fn side_bg(l: u32) -> Color {
    pick(l, 0xffffff, 0x171a20)
}
fn bar_bg(l: u32) -> Color {
    pick(l, 0xffffff, 0x1b1e25)
}
fn ink(l: u32) -> Color {
    pick(l, 0x1d2129, 0xe6e9ef)
}
fn dim(l: u32) -> Color {
    pick(l, 0x7a808c, 0x8b93a3)
}
fn bubble_bg(l: u32) -> Color {
    if l & HOT != 0 { pick(l, 0xc9dcff, 0x2c4b80) } else { pick(l, 0xffffff, 0x232831) }
}
fn pill_bg(l: u32) -> Color {
    if l & HOT != 0 { pick(l, 0xa9c5ff, 0x3a5fa0) } else { pick(l, 0xe6e9ef, 0x323843) }
}
fn name_ink(l: u32, a: usize) -> Color {
    let c = PEOPLE[a].color;
    if l & DARK != 0 {
        let f = |s: u32| ((c >> s & 255) + 255) / 2;
        rgb(f(16) << 16 | f(8) << 8 | f(0))
    } else {
        rgb(c)
    }
}
const CLEAR: Color = Color::rgba(0, 0, 0, 0);

// ------------------------------------------------------------------------------------------------ the model

struct Person {
    name: &'static str,
    initials: &'static str,
    color: u32,
}
const PEOPLE: [Person; PARTS] = [
    Person { name: "Ada Okafor", initials: "AO", color: 0x2f6bff },
    Person { name: "Ben Lindqvist", initials: "BL", color: 0xd9485a },
    Person { name: "Chloe Park", initials: "CP", color: 0x1f9d6b },
    Person { name: "Dmitri Volkov", initials: "DV", color: 0x8b5cf6 },
    Person { name: "Elena Rossi", initials: "ER", color: 0xe08a1e },
    Person { name: "Farid Haddad", initials: "FH", color: 0x0f9db5 },
    Person { name: "Grace Tanaka", initials: "GT", color: 0xc2410c },
    Person { name: "Hugo Martin", initials: "HM", color: 0x4d7c0f },
    Person { name: "Ines Alvarez", initials: "IA", color: 0xbe185d },
    Person { name: "Jonas Weber", initials: "JW", color: 0x475569 },
    Person { name: "Keiko Mori", initials: "KM", color: 0x7c3aed },
    Person { name: "Liam O'Neil", initials: "LO", color: 0x0369a1 },
];

/// One generated PNG and the size it is shown at (points).
#[derive(Clone, Copy)]
struct Pic {
    png: &'static [u8],
    w: f32,
    h: f32,
}

struct Msg {
    author: usize,
    time: String,
    text: String,
    quote: Option<(usize, String)>,
    pic: Option<usize>,
    reacts: Vec<String>,
}

struct Conv {
    name: String,
    initials: String,
    color: u32,
}

/// A message that arrived while the scene runs: a row of `Chat::arrivals`. A row is its own owner, so it keeps
/// a copy of its author's look. (No transitions live in a row: see "Wanted".)
pub struct Arrival {
    author: usize,
    time: String,
    text: String,
    pic: Option<Pic>,
    look: Signal<u32>,
}

pub struct Chat {
    msgs: Vec<Msg>,
    convs: Vec<Conv>,
    pics: Vec<Pic>,
    nconv: usize,
    /// Chrome's theme bit (`DARK`).
    theme: Signal<u32>,
    /// Per participant: `DARK` | `HOT`; what every one of their messages' elements read.
    alook: [Signal<u32>; PARTS],
    hot: Option<usize>,
    title: Signal<String>,
    preview: [Signal<String>; CONVS],
    when: [Signal<String>; CONVS],
    unread: [Signal<u32>; CONVS],
    sel: Signal<u32>,
    draft: Signal<String>,
    search: Signal<String>,
    scroll: Signal<f32>,
    side_scroll: Signal<f32>,
    pulse: Signal<f32>,
    /// Opacity of the veil over the newest history image (1: image hidden, 0: shown).
    fade: Signal<f32>,
    last_img: Option<usize>,
    arrivals: List<Arrival>,
    arrived: usize,
    rng: Rng,
    /// Wires on one participant's look (the fan-out), counted at build.
    hot_wires: usize,
}

// ------------------------------------------------------------------------------------------------ synthetic data

const SYL: [&str; 28] = [
    "ka", "lo", "mi", "ra", "te", "su", "no", "vi", "da", "pe", "ru", "ha", "bo", "ni", "fe", "za", "qui", "tra", "len", "mar", "dor", "sel", "an", "or", "ex", "il", "pon", "ver",
];

fn word(rng: &mut Rng, out: &mut String) {
    for _ in 0..1 + rng.below(3) {
        out.push_str(SYL[rng.below(SYL.len())]);
    }
}

/// `n` synthetic words, the first capitalised, a full stop at the end.
fn sentence(rng: &mut Rng, n: usize) -> String {
    let mut s = String::with_capacity(n * 7);
    for i in 0..n {
        if i > 0 {
            s.push(' ');
        }
        let at = s.len();
        word(rng, &mut s);
        if i == 0 {
            s[at..at + 1].make_ascii_uppercase();
        }
    }
    s.push('.');
    s
}

fn clock(minute: usize) -> String {
    let m = 8 * 60 + minute;
    format!("{:02}:{:02}", (m / 60) % 24, m % 60)
}

fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// A PNG of `w` x `h` (a diagonal gradient with rings and light noise), leaked for `image(&'static [u8])`.
fn make_png(w: u32, h: u32, seed: u64) -> &'static [u8] {
    let mut rng = Rng::new(seed);
    let col = |r: &mut Rng| [60.0 + r.below(170) as f32, 60.0 + r.below(170) as f32, 60.0 + r.below(170) as f32];
    let (c0, c1) = (col(&mut rng), col(&mut rng));
    let (cx, cy, freq) = (0.2 + 0.6 * rng.unit(), 0.2 + 0.6 * rng.unit(), 8.0 + 14.0 * rng.unit());
    let mut raw = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
            let t = 0.6 * u + 0.4 * v;
            let d = ((u - cx) * (u - cx) + (v - cy) * (v - cy)).sqrt();
            let band = 0.82 + 0.18 * (d * freq).sin();
            for c in 0..3 {
                let n = rng.below(5) as f32 - 2.0;
                raw.push(((c0[c] + (c1[c] - c0[c]) * t) * band + n).clamp(0.0, 255.0) as u8);
            }
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut wr = enc.write_header().expect("png header");
        wr.write_image_data(&raw).expect("png data");
    }
    Box::leak(out.into_boxed_slice())
}

/// (pixel size, size shown at in points)
const PIC_KINDS: [((u32, u32), (f32, f32)); 4] = [((480, 320), (200.0, 133.0)), ((320, 200), (176.0, 110.0)), ((200, 200), (120.0, 120.0)), ((360, 240), (168.0, 112.0))];

fn n_pics() -> usize {
    std::env::var("CHAT_PICS").ok().and_then(|v| v.parse().ok()).unwrap_or(8)
}

fn make_pics(n: usize) -> Vec<Pic> {
    (0..n)
        .map(|i| {
            let ((pw, ph), (w, h)) = PIC_KINDS[i % PIC_KINDS.len()];
            Pic { png: make_png(pw, ph, 0x1000 + i as u64), w, h }
        })
        .collect()
}

fn wires_of(m: &Msg) -> usize {
    // name, time, bubble fill, text; the quote's name and text; a pill's text and fill
    4 + if m.quote.is_some() { 2 } else { 0 } + 2 * m.reacts.len()
}

// ------------------------------------------------------------------------------------------------ elements

fn avatar<'a, V: 'static>(color: u32, initials: &'a str, size: f32, font: f32) -> impl Element<V> + 'a {
    row((text(initials).font(font, font + 4.0).color(rgb(0xffffff)),)).center().w(size).h(size).shrink(0.0).rounded(size * 0.34).bg(rgb(color))
}

/// What one message shows (borrowed from the message or the arrival).
struct Parts<'a> {
    author: usize,
    time: &'a str,
    text: &'a str,
    quote: Option<(usize, &'a str)>,
    reacts: &'a [String],
}

/// A message: avatar, name and time, a bubble (quote, text, `media`, reactions). `look` is the author's
/// look signal (the root's `alook[author]`, or an arrival's own copy).
fn message<'a, V: 'static, I: Element<V> + 'a>(look: Wired<'a, u32, V>, p: Parts<'a>, media: I) -> impl Element<V> + 'a {
    let a = p.author;
    let person = &PEOPLE[a];
    let quote = p.quote.into_iter().map(move |(qa, qt)| {
        column((
            text(PEOPLE[qa].name).font(11.0, 14.0).color(look.map(move |l: &u32| name_ink(*l, qa))),
            para(qt).font(12.0, 16.0).color(look.map(|l: &u32| dim(*l))),
        ))
        .gap(1.0)
        .px(8.0)
        .py(4.0)
        .rounded(6.0)
        .border(1.0, QUOTE_EDGE)
    });
    let reacts = (!p.reacts.is_empty())
        .then(|| {
            row(each(p.reacts.iter().map(move |r| {
                row((text(r.as_str()).font(11.0, 14.0).color(look.map(|l: &u32| ink(*l))),)).px(7.0).py(2.0).rounded(9.0).bg(look.map(|l: &u32| pill_bg(*l)))
            })))
            .gap(4.0)
        })
        .into_iter();
    row((
        avatar(person.color, person.initials, 36.0, 12.0),
        column((
            row((
                text(person.name).font(12.0, 16.0).color(look.map(move |l: &u32| name_ink(*l, a))),
                text(p.time).font(11.0, 16.0).color(look.map(|l: &u32| dim(*l))),
            ))
            .gap(8.0),
            column((each(quote), para(p.text).font(13.0, 18.0).color(look.map(|l: &u32| ink(*l))), media, each(reacts)))
                .gap(6.0)
                .p(10.0)
                .rounded(12.0)
                .bg(look.map(|l: &u32| bubble_bg(*l)))
                .w_pct(BUBBLE_PCT),
        ))
        .gap(3.0)
        .grow(1.0),
    ))
    .gap(10.0)
    .px(16.0)
    .py(6.0)
}

impl View for Arrival {
    fn view(&self) -> impl Element<Self> + '_ {
        let media = self.pic.into_iter().map(|im| image(im.png).w(im.w).h(im.h));
        message(sig!(self.look), Parts { author: self.author, time: &self.time, text: &self.text, quote: None, reacts: &[] }, each(media))
    }
}

impl Chat {
    fn history_row(&self, i: usize) -> impl Element<Chat> + '_ {
        let m = &self.msgs[i];
        let look = sig!(self.alook).at(m.author);
        // the newest image of the history carries a veil (a box of the bubble's colour over it) whose opacity
        // transitions: an image has no opacity of its own
        let veiled = self.last_img == Some(i);
        let media = m.pic.map(|p| self.pics[p]).into_iter().map(move |p| {
            let veil = veiled.then(|| block(p.w, p.h).abs(0.0, 0.0).bg(look.map(|l: &u32| bubble_bg(*l))).opacity(sig!(self.fade).transition(Curve::out(0.7)))).into_iter();
            column((image(p.png).w(p.w).h(p.h), each(veil))).relative()
        });
        message(
            look,
            Parts { author: m.author, time: &m.time, text: &m.text, quote: m.quote.as_ref().map(|(a, t)| (*a, t.as_str())), reacts: &m.reacts },
            each(media),
        )
    }

    fn conv_row(&self, i: usize) -> impl Element<Chat> + '_ {
        let c = &self.convs[i];
        let t = sig!(self.theme);
        let unread = sig!(self.unread).at(i);
        let badge = row((text(unread.num(|v: &u32| *v as f32, COUNT)).font(11.0, 14.0).color(unread.map(|v: &u32| if *v > 0 { rgb(0xffffff) } else { CLEAR })),))
            .center()
            .w(24.0)
            .h(18.0)
            .rounded(9.0)
            .bg(unread.map(|v: &u32| if *v > 0 { rgb(ACCENT) } else { CLEAR }));
        row((
            avatar(c.color, &c.initials, 40.0, 13.0),
            column((
                text(c.name.as_str()).font(14.0, 20.0).color(t.map(|t: &u32| ink(*t))),
                text(sig!(self.preview).at(i)).font(12.0, 16.0).color(t.map(|t: &u32| dim(*t))),
            ))
            .gap(2.0)
            .grow(1.0),
            column((text(sig!(self.when).at(i)).font(11.0, 14.0).color(t.map(|t: &u32| dim(*t))), badge)).gap(4.0).items(Align::End).w(44.0).shrink(0.0),
        ))
        .gap(10.0)
        .px(12.0)
        .h(ROW_H)
        .items(Align::Center)
        .bg(t.map(|t: &u32| side_bg(*t)))
        .on_click(move |s: &mut Chat| s.select(i))
    }

    fn sidebar(&self) -> impl Element<Chat> + '_ {
        let t = sig!(self.theme);
        let head = row((text("Chats").font(18.0, 24.0).color(t.map(|t: &u32| ink(*t))),)).px(16.0).h(SIDE_HEAD).items(Align::Center).shrink(0.0).bg(t.map(|t: &u32| bar_bg(*t)));
        let selection = no_hit(
            block(SIDE_W, ROW_H)
                .bg(Color::rgba(0x2f, 0x6b, 0xff, 0x2e))
                .abs(0.0, 0.0)
                .translate(sig!(self.sel).map(|i: &u32| (0.0, *i as f32 * ROW_H)).transition(Curve::out(0.2))),
        );
        let rows = column(()).grow(1.0).relative().bg(t.map(|t: &u32| side_bg(*t))).scroll(sig!(self.side_scroll)).child(column(each((0..self.nconv).map(move |i| self.conv_row(i))))).child(selection);
        column((head, rows)).w(SIDE_W).h(H).shrink(0.0)
    }

    fn header(&self) -> impl Element<Chat> + '_ {
        let t = sig!(self.theme);
        let people = row(each((0..PARTS).map(|a| {
            avatar::<Chat>(PEOPLE[a].color, PEOPLE[a].initials, AVATAR, 10.0).on_hover(move |s: &mut Chat, e: &Ev| s.hover_participant(a, e.on))
        })))
        .gap(AVATAR_GAP);
        row((
            div().w(TITLE_W).shrink(0.0).child(text(sig!(self.title)).font(16.0, 22.0).color(t.map(|t: &u32| ink(*t)))),
            people,
            div().grow(1.0),
            text("Search").font(12.0, 16.0).color(t.map(|t: &u32| dim(*t))),
            text_input(sig!(self.search)).size(180.0, 30.0),
        ))
        .gap(12.0)
        .px(16.0)
        .h(HEADER_H)
        .items(Align::Center)
        .shrink(0.0)
        .bg(t.map(|t: &u32| bar_bg(*t)))
    }

    fn typing(&self) -> impl Element<Chat> + '_ {
        let t = sig!(self.theme);
        let dot = move |j: usize| {
            block(7.0, 7.0).rounded(3.5).bg(rgb(ACCENT)).opacity(sig!(self.pulse).map(move |p: &f32| if j % 2 == 0 { 0.25 + 0.75 * *p } else { 1.0 - 0.75 * *p }).transition(Curve::in_out(0.45)))
        };
        row((dot(0), dot(1), dot(2), text("Ines is typing").font(11.0, 14.0).color(t.map(|t: &u32| dim(*t)))))
            .gap(4.0)
            .px(16.0)
            .h(TYPING_H)
            .items(Align::Center)
            .shrink(0.0)
            .bg(t.map(|t: &u32| page_bg(*t)))
    }

    fn composer(&self) -> impl Element<Chat> + '_ {
        let t = sig!(self.theme);
        row((text_input(sig!(self.draft)).size(FIELD_W, 36.0), button("Send").w(SEND_W).on_click(|s: &mut Chat| s.send())))
            .gap(10.0)
            .px(16.0)
            .h(COMPOSER_H)
            .items(Align::Center)
            .shrink(0.0)
            .bg(t.map(|t: &u32| bar_bg(*t)))
    }

    fn conversation(&self) -> impl Element<Chat> + '_ {
        let t = sig!(self.theme);
        column(())
            .grow(1.0)
            .py(8.0)
            .bg(t.map(|t: &u32| page_bg(*t)))
            .scroll(sig!(self.scroll))
            .child(column(each((0..self.msgs.len()).map(move |i| self.history_row(i)))))
            .child(list(sig!(self.arrivals)).w_pct(1.0))
    }
}

impl View for Chat {
    fn view(&self) -> impl Element<Self> + '_ {
        let main = column((self.header(), self.conversation(), self.typing(), self.composer())).grow(1.0).h(H);
        row((self.sidebar(), main)).w(W).h(H).bg(sig!(self.theme).map(|t: &u32| page_bg(*t)))
    }
}

// ------------------------------------------------------------------------------------------------ handlers

impl Chat {
    fn look_of(&self, a: usize) -> u32 {
        (*self.theme & DARK) | if self.hot == Some(a) { HOT } else { 0 }
    }

    /// Write participant `a`'s look (and the copies in the arrival rows).
    fn relook(&mut self, a: usize) {
        let l = self.look_of(a);
        self.alook[a].set(l);
        for i in 0..self.arrivals.len() {
            let r = self.arrivals.get(i).unwrap();
            if r.author == a && *r.look != l {
                self.arrivals.edit(i).look.set(l);
            }
        }
    }

    fn set_hot(&mut self, h: Option<usize>) {
        let old = std::mem::replace(&mut self.hot, h);
        for a in [old, h].into_iter().flatten() {
            self.relook(a);
        }
    }

    fn hover_participant(&mut self, a: usize, on: bool) {
        if on {
            self.set_hot(Some(a));
        } else if self.hot == Some(a) {
            self.set_hot(None);
        }
    }

    fn set_dark(&mut self, dark: bool) {
        self.theme.set(dark as u32);
        for a in 0..PARTS {
            self.relook(a);
        }
    }

    fn select(&mut self, i: usize) {
        let i = i.min(self.nconv - 1);
        self.sel.set(i as u32);
        self.unread[i].set(0);
        let n = self.convs[i].name.clone();
        self.title.set(n);
    }

    /// A message arrives (appended after the history); the sidebar follows: the open conversation's preview
    /// and time, and one background conversation's preview, time and unread count.
    fn arrive(&mut self, author: usize, text: String) {
        let n = self.arrived;
        self.arrived += 1;
        let time = clock(self.msgs.len() + n);
        let pic = (n % 3 == 2 && !self.pics.is_empty()).then(|| self.pics[(n / 3) % self.pics.len()]);
        let look = self.look_of(author);
        let prev = trunc(&text, 26);
        self.arrivals.push(Arrival { author, time: time.clone(), text, pic, look: Signal::new(look) });
        let open = *self.sel as usize;
        self.preview[open].set(prev.clone());
        self.when[open].set(time.clone());
        let mut bg = (n * 7 + 3) % self.nconv;
        if bg == open {
            bg = (bg + 1) % self.nconv;
        }
        self.preview[bg].set(prev);
        self.when[bg].set(time);
        let u = *self.unread[bg] + 1;
        self.unread[bg].set(u);
    }

    fn send(&mut self) {
        let t = if self.draft.is_empty() { "Sounds good.".to_string() } else { (*self.draft).clone() };
        self.draft.set(String::new());
        self.arrive(0, t);
    }
}

// ------------------------------------------------------------------------------------------------ geometry of the input the harness feeds

fn conv_pt() -> (f32, f32) {
    (SIDE_W + 400.0, H / 2.0)
}
fn avatar_pt(a: usize) -> (f32, f32) {
    (SIDE_W + 16.0 + TITLE_W + 12.0 + a as f32 * (AVATAR + AVATAR_GAP) + AVATAR / 2.0, HEADER_H / 2.0)
}
fn field_pt() -> (f32, f32) {
    (SIDE_W + 16.0 + FIELD_W / 2.0, H - COMPOSER_H / 2.0)
}
fn send_pt() -> (f32, f32) {
    (SIDE_W + 16.0 + FIELD_W + 10.0 + SEND_W / 2.0, H - COMPOSER_H / 2.0)
}
fn press(app: &mut App<Chat>, (x, y): (f32, f32)) {
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
}

/// A message arrives: the list grows by a row and the conversation glides to the end (the wheel's smooth
/// scroll, advanced by `tick`).
fn arrival(app: &mut App<Chat>, mine: bool) {
    if mine {
        app.turn(|s| s.send());
    } else {
        app.turn(|s| {
            let a = 1 + s.rng.below(PARTS - 1);
            let words = 4 + s.rng.below(26);
            let t = sentence(&mut s.rng, words);
            s.arrive(a, t);
        });
    }
    let (x, y) = conv_pt();
    app.wheel(x, y, 0.0, 1.0e6);
}

impl Scene for Chat {
    const NAME: &'static str = "chat";

    fn build(scale: f32) -> Chat {
        let n = scaled(MESSAGES, scale, 40);
        let nconv = scaled(CONVS, scale, 12).min(CONVS);
        let mut rng = Rng::new(0xc4a7);
        let pics = make_pics(n_pics().min(n / 10).max(1));
        let mut msgs: Vec<Msg> = Vec::with_capacity(n);
        for i in 0..n {
            let author = rng.below(PARTS);
            let words = 12 * (1 + rng.below(6)) - rng.below(8).min(9);
            let text = sentence(&mut rng, words.max(3));
            let quote = (i > 3 && rng.below(100) < 14).then(|| {
                let j = i - 1 - rng.below(i.min(8));
                (msgs[j].author, trunc(&msgs[j].text, 70))
            });
            let pic = (i % 10 == 9).then(|| (i / 10) % pics.len());
            let reacts = if rng.below(100) < 20 {
                (0..1 + rng.below(3)).map(|_| format!("{} {}", ["K", "L", "M", "N", "P", "R", "S", "T"][rng.below(8)], 1 + rng.below(9))).collect()
            } else {
                Vec::new()
            };
            msgs.push(Msg { author, time: clock(i / 3), text, quote, pic, reacts });
        }
        const FIRST: [&str; 16] = ["Mina", "Noah", "Priya", "Oscar", "Lena", "Tariq", "Sofia", "Kofi", "Hana", "Iván", "Zoe", "Mateo", "Amara", "Jin", "Rosa", "Theo"];
        const LAST: [&str; 16] = ["Brandt", "Cole", "Dasgupta", "Eriksen", "Farah", "Gomez", "Hale", "Ito", "Jovanovic", "Khan", "Lopez", "Moreau", "Nair", "Ortega", "Pak", "Quist"];
        let convs: Vec<Conv> = (0..CONVS)
            .map(|i| {
                let (f, l) = (FIRST[rng.below(16)], LAST[rng.below(16)]);
                let name = if i % 5 == 4 { format!("Team {}{}", SYL[rng.below(SYL.len())], SYL[rng.below(SYL.len())]) } else { format!("{f} {l}") };
                let initials = name.split(' ').filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase();
                Conv { name, initials, color: PEOPLE[rng.below(PARTS)].color }
            })
            .collect();
        let sel = 2;
        let preview: [Signal<String>; CONVS] = std::array::from_fn(|_| {
            let w = 3 + rng.below(4);
            Signal::new(trunc(&sentence(&mut rng, w), 26))
        });
        let when: [Signal<String>; CONVS] = std::array::from_fn(|i| Signal::new(clock(i * 5)));
        let unread: [Signal<u32>; CONVS] = std::array::from_fn(|_| Signal::new(if rng.below(100) < 30 { 1 + rng.below(40) as u32 } else { 0 }));
        let last_img = msgs.iter().rposition(|m| m.pic.is_some());
        let hot_wires = msgs.iter().filter(|m| m.author == HOT_PERSON).map(wires_of).sum();
        let title = convs[sel].name.clone();
        Chat {
            msgs,
            convs,
            pics,
            nconv,
            theme: Signal::new(0),
            alook: std::array::from_fn(|_| Signal::new(0)),
            hot: None,
            title: Signal::new(title),
            preview,
            when,
            unread,
            sel: Signal::new(sel as u32),
            draft: Signal::new(String::new()),
            search: Signal::new(String::new()),
            scroll: Signal::new(0.0),
            side_scroll: Signal::new(0.0),
            pulse: Signal::new(0.0),
            fade: Signal::new(0.0),
            last_img,
            arrivals: List::new(),
            arrived: 0,
            rng,
            hot_wires,
        }
    }

    fn caps(&self) -> Caps {
        let n = self.msgs.len();
        let chars: usize = self.msgs.iter().map(|m| m.text.len() + m.quote.as_ref().map_or(0, |q| q.1.len()) + 24 + m.reacts.len() * 4).sum::<usize>() + self.nconv * 80;
        // a message: avatar fill, bubble fill, quote border, image, pills; a conversation: avatar, badge, row fill
        let instances = n * 10 + self.nconv * 8 + 16_384;
        let records = n * 18 + self.nconv * 14 + 8_192;
        Caps {
            instances: instances.next_power_of_two().min(Caps::RESERVE.instances),
            pages: (records / 32 + 1024).next_power_of_two().min(Caps::RESERVE.pages),
            glyphs: (chars * 5 / 4 + 65_536).next_power_of_two().min(Caps::RESERVE.glyphs),
            runs: (n * 20 + self.nconv * 8 + 16_384).next_power_of_two().min(Caps::RESERVE.runs),
            spaces: 4096,
            clips: 1024,
            ..Caps::DEFAULT
        }
    }

    fn describe(&self) -> String {
        let imgs = self.msgs.iter().filter(|m| m.pic.is_some()).count();
        let quotes = self.msgs.iter().filter(|m| m.quote.is_some()).count();
        let pills: usize = self.msgs.iter().map(|m| m.reacts.len()).sum();
        let chars: usize = self.msgs.iter().map(|m| m.text.len()).sum();
        format!(
            "{} messages from {PARTS} participants ({imgs} images over {} distinct PNGs, {quotes} quotes, {pills} reaction pills, {chars} chars), {} conversations, arrivals in a List",
            self.msgs.len(),
            self.pics.len(),
            self.nconv
        )
    }

    fn fanout(&self) -> usize {
        self.hot_wires
    }

    fn steady(app: &mut App<Self>, k: usize) {
        if k == 0 {
            // a chat opens at its newest message; press on the composer to focus it
            let (x, y) = conv_pt();
            app.wheel_points(x, y, 1.0e7);
            press(app, field_pt());
        }
        // one character of typing
        const TEXT: &[u8] = b"the quick brown fox jumps over the lazy dog ";
        let c = TEXT[k % TEXT.len()] as char;
        app.text_input(c.encode_utf8(&mut [0u8; 4]));
        if k % 36 == 35 {
            app.turn(|s| {
                s.draft.set(String::new());
            });
        }
        if k % 6 == 5 {
            arrival(app, false);
        }
        if k % 12 == 2 {
            // send: press the button (focus leaves the field), then press the field again
            press(app, send_pt());
            let (x, y) = conv_pt();
            app.wheel(x, y, 0.0, 1.0e6);
            press(app, field_pt());
        }
        if k % 10 == 9 {
            let dy = if (k / 10) % 2 == 0 { -3.0 } else { 3.0 };
            let (x, y) = conv_pt();
            app.wheel(x, y, 0.0, dy);
        }
        app.tick(1.0 / 60.0);
    }

    fn hover(app: &mut App<Self>, k: usize) {
        // onto participant 3's avatar in the header, then left onto the conversation's title
        if k % 2 == 0 {
            let (x, y) = avatar_pt(HOT_PERSON);
            app.pointer_move(x, y);
        } else {
            app.pointer_move(SIDE_W + 16.0 + TITLE_W / 2.0, HEADER_H / 2.0);
        }
    }

    fn theme(app: &mut App<Self>, k: usize) {
        app.turn(|s| s.set_dark(k % 2 == 0));
    }

    fn animate(app: &mut App<Self>, k: usize) {
        // a message arrives and the conversation glides to it
        arrival(app, k % 4 == 3);
        // the typing dots pulse, the newest history image fades, the selection layer moves: three transitions
        // retargeted in one turn
        app.turn(|s| {
            s.pulse.set(if k % 2 == 0 { 1.0 } else { 0.0 });
            s.fade.set(if k % 2 == 0 { 1.0 } else { 0.0 });
            let to = (k * 7 + 3) % s.nconv;
            s.select(to);
        });
    }
}
