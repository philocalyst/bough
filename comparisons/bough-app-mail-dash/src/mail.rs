//! zgx's `examples/mail` (app.luau, state.luau, actions.luau, components/, lib/sample.luau) on bough.
//!
//! The window is zgx's at 1180 x 760 points, the "wide" layout (every pane): the mailboxes (210), a divider,
//! the message list (360), a divider, the open message (the rest). zgx's narrow layouts (drawer, one pane at a
//! time), the compose sheet and the settings window are never shown at this size or by the measured
//! interactions and are not built.
//!
//! **Data**: the same 4,000 messages, made by the same seeded Park–Miller generator in the same order of
//! draws (`messages`). A message stores its sender and subject as indices into the generator's tables (they
//! are interned strings there too) and its body as sentence indices; the body text is made when a message is
//! opened.
//!
//! **The list** is virtualized as zgx's `ui.list` is (rows of 72 points, two rows of overscan): a `List<MailRow>`
//! holds the window's rows only, keyed by message id as zgx's are (`key = shown:at(i).id`), and a change of
//! the shown messages is a keyed splice of the window (`refill`).
//!
//! **Search** (`ui.sorted` with a script filter in zgx: one Luau call per message) is a representation
//! change: a message's haystack `lower(subject .. " " .. from)` is a function of its (subject, sender) pair,
//! 80 pairs in all, so a query is tested against the 80 pairs once (a 128-bit mask) and each message costs one
//! bit test.
//!
//! **Layout islands**: the open message's texts and the search field's text are each the one row of a
//! `List` (`Pane`, `FieldText`), so a new message or a keystroke lays out that row alone, not the window.
use crate::theme::*;
use crate::{button, caret_lane, field_box, lay_rect, text_width, truncate, Caret, FieldText};
use bough::layout::prelude::*;
use bough::prelude::*;
use std::sync::Arc;

// =============================================================================== lib/sample.luau

pub const PEOPLE: [(&str, &str); 8] = [
    ("Ada Lovelace", "ada@analytical.engine"),
    ("Grace Hopper", "grace@navy.mil"),
    ("Alan Turing", "alan@bletchley.uk"),
    ("Katherine Johnson", "katherine@nasa.gov"),
    ("Edsger Dijkstra", "ewd@utexas.edu"),
    ("Barbara Liskov", "liskov@mit.edu"),
    ("Donald Knuth", "knuth@stanford.edu"),
    ("Margaret Hamilton", "margaret@apollo.org"),
];
pub const TOPICS: [&str; 10] = [
    "Quarterly review notes",
    "Lunch on Thursday?",
    "Draft of the compiler paper",
    "Re: flaky test in CI",
    "Trip itinerary",
    "Your invoice is ready",
    "Design review: the sidebar",
    "Weekend hike photos",
    "Benchmarks from last night",
    "Question about the API",
];
pub const SENTENCES: [&str; 8] = [
    "I went through the numbers again and they hold up.",
    "Let me know if Thursday works for you, or suggest another day.",
    "The second section still reads a little dense; maybe split it?",
    "We should talk about the release before the end of the week.",
    "Attached are the photos from the trail, the view was worth it.",
    "The flaky test turned out to be a race in the setup code.",
    "Thanks for the quick turnaround on the review.",
    "I think the sidebar should collapse below 800 points.",
];
/// The sidebar's boxes (components/sidebar.luau); every one but Starred is a mailbox a message is in.
pub const BOXES: [&str; 6] = ["Inbox", "Starred", "Sent", "Drafts", "Archive", "Trash"];
pub const INBOX: u8 = 0;
pub const STARRED: u8 = 1;
pub const SENT: u8 = 2;
pub const DRAFTS: u8 = 3;
pub const ARCHIVE: u8 = 4;
pub const TRASH: u8 = 5;
/// `mailboxes` of sample.luau (weights: four Inbox, one Sent, two Archive, one Drafts, one Trash).
const MAILBOXES: [u8; 9] = [INBOX, INBOX, INBOX, INBOX, SENT, ARCHIVE, ARCHIVE, DRAFTS, TRASH];

/// One message. Sender and subject are indices into `PEOPLE` / `TOPICS`, the body sentence indices
/// (`paras[p]` sentences in paragraph `p`, 0 for none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Msg {
    pub mailbox: u8,
    pub person: u8,
    pub topic: u8,
    pub day: u8,
    pub unread: bool,
    pub starred: bool,
    /// `topic * 8 + person`: the search haystack's key.
    pub pair: u8,
    pub paras: [u8; 4],
    pub sent: [u8; 16],
}

/// Park and Miller's generator, seeded 42: exact in integers (the Luau doubles never round: products < 2^53).
struct Pick(u64);
impl Pick {
    /// `pick(n)`: 1..=n.
    fn pick(&mut self, n: u64) -> usize {
        self.0 = self.0 * 48271 % 2147483647;
        (self.0 % n + 1) as usize
    }
}

/// `sample.messages(count)`: the draws in the script's order (sender; the paragraphs, each its sentence count
/// then its sentences; then the table constructor's fields in order: mailbox, subject, unread, starred).
pub fn messages(count: usize) -> Vec<Msg> {
    let mut g = Pick(42);
    (1..=count)
        .map(|i| {
            let person = (g.pick(8) - 1) as u8;
            let mut paras = [0u8; 4];
            let mut sent = [0u8; 16];
            let mut k = 0;
            let np = g.pick(3) + 1;
            for p in paras.iter_mut().take(np) {
                let nl = g.pick(3) + 1;
                *p = nl as u8;
                for _ in 0..nl {
                    sent[k] = (g.pick(8) - 1) as u8;
                    k += 1;
                }
            }
            let mailbox = MAILBOXES[g.pick(9) - 1];
            let topic = (g.pick(10) - 1) as u8;
            let unread = g.pick(4) == 1;
            let starred = g.pick(9) == 1;
            let day = (28 - (i / 40) % 28) as u8;
            Msg { mailbox, person, topic, day, unread, starred, pair: topic * 8 + person, paras, sent }
        })
        .collect()
}

impl Msg {
    /// Paragraph `p` (`""` past the last): its sentences joined by a space.
    pub fn paragraph(&self, p: usize) -> String {
        let start: usize = self.paras[..p].iter().map(|&n| n as usize).sum();
        let n = self.paras[p] as usize;
        let mut s = String::with_capacity(n * 64);
        for (j, &k) in self.sent[start..start + n].iter().enumerate() {
            if j > 0 {
                s.push(' ');
            }
            s.push_str(SENTENCES[k as usize]);
        }
        s
    }
    /// The body as zgx holds it (paragraphs joined by a blank line).
    pub fn body(&self) -> String {
        (0..4).map(|p| self.paragraph(p)).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n")
    }
    /// `string.sub(paragraphs[1], 1, 90)`.
    pub fn preview(&self) -> String {
        let mut p = self.paragraph(0);
        p.truncate(90);
        p
    }
    pub fn from(&self) -> &'static str {
        PEOPLE[self.person as usize].0
    }
    pub fn email(&self) -> &'static str {
        PEOPLE[self.person as usize].1
    }
    pub fn subject(&self) -> &'static str {
        TOPICS[self.topic as usize]
    }
}

// =============================================================================== geometry (points)

pub const W: f32 = 1180.0;
pub const H: f32 = 760.0;
pub const LIST_X: f32 = 211.0;
pub const LIST_W: f32 = 360.0;
/// The list's top: its header row (pt 40, the 36-point field, pb 8).
pub const LIST_TOP: f32 = 84.0;
/// The list's viewport: the column less its header and the status line (py 6 + 18).
pub const LIST_H: f32 = H - LIST_TOP - 30.0;
pub const ROW_H: f32 = 72.0;
/// Rows mounted: those the viewport meets (9) and zgx's two rows of overscan.
pub const WINDOW: usize = 11;
pub const DETAIL: [f32; 4] = [572.0, 0.0, 608.0, H];
/// The preview's width: the row's content (360 less its padding, 6 and 14) less the dot's gutter (14).
const PREVIEW_W: f32 = LIST_W - 6.0 - 14.0 - 14.0;
/// Layout paths (child indices from the window's root group) of what the harness clicks.
pub const SEARCH_PATH: [usize; 3] = [1, 0, 0];
pub const ARCHIVE_PATH: [usize; 4] = [3, 0, 0, 2];

/// zgx's scrollbar thumb length (`max(24, L^2 / total)`, at most `L`) for `n` rows; 0 when nothing scrolls.
fn thumb_len(n: usize) -> f32 {
    let total = n as f32 * ROW_H;
    if total <= LIST_H { 0.0 } else { (LIST_H * LIST_H / total).max(24.0).min(LIST_H) }
}

// =============================================================================== state

/// A row of the message list: one message (its key), the texts it shows, and what changes while it is
/// mounted: whether it is the open one, whether it is unread.
pub struct MailRow {
    pub id: u32,
    from: &'static str,
    subject: &'static str,
    date: String,
    preview: String,
    pub sel: Signal<bool>,
    pub unread: Signal<bool>,
}

fn dot(u: &bool) -> String {
    if *u { "\u{25cf}".to_owned() } else { String::new() }
}
fn sel_bg(s: &bool) -> Color {
    if *s { SELECTION } else { CLEAR }
}
fn count_text(n: &u32) -> String {
    if *n > 0 { n.to_string() } else { String::new() }
}

impl View for MailRow {
    const FIXED: bool = true;
    /// components/list.luau `row`: the unread dot in a gutter of its own, then sender and date, subject,
    /// preview (comfortable density).
    fn view(&self) -> impl Element<Self> + '_ {
        row((
            column(label(sig!(self.unread).map(dot)).font(8.0, 12.0).color(ACCENT)).w(14.0).shrink(0.0).pad(0.0, 5.0, 0.0, 0.0).items(Align::Center),
            column((
                row((
                    label(self.from).font(MD.0, MD.1).color(TEXT).grow(1.0).min_w(0.0),
                    label(self.date.clone()).font(SM.0, SM.1).color(TEXT3).shrink(0.0),
                ))
                .gap(6.0)
                .items(Align::Center),
                label(self.subject).font(MD.0, MD.1).color(TEXT).min_w(0.0),
                label(self.preview.clone()).font(SM.0, SM.1).color(TEXT2).min_w(0.0),
            ))
            .grow(1.0)
            .min_w(0.0)
            .gap(2.0),
        ))
        .w_pct(1.0)
        .h(ROW_H)
        .pad(6.0, 8.0, 14.0, 8.0)
        .bg(sig!(self.sel).map(sel_bg))
        .hover_bg(HOVER)
    }
}

/// The open message's texts (detail.luau: subject, sender line, body) as a layout island: the one row of a
/// `List`, so a new message lays out this row alone. The toolbar, whose buttons carry routes, stays in the
/// window's group: a route must not be mounted in a list row (routes are dispatched with the root owner).
#[derive(Default)]
pub struct Pane {
    pub subject: Signal<String>,
    pub from: Signal<String>,
    pub email: Signal<String>,
    pub date: Signal<String>,
    pub b0: Signal<String>,
    pub b1: Signal<String>,
    pub b2: Signal<String>,
    pub b3: Signal<String>,
}

impl View for Pane {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            para(sig!(self.subject)).font(XL.0, XL.1).color(TEXT),
            row((
                label(sig!(self.from)).font(MD.0, MD.1).color(TEXT),
                label(sig!(self.email)).font(MD.0, MD.1).color(TEXT2),
                label(sig!(self.date)).font(MD.0, MD.1).color(TEXT3),
            )),
            block(0.0, 1.0).w_pct(1.0).bg(BORDER_SUBTLE),
            // the body: its paragraphs a blank line (22 points) apart, as zgx's text holding "\n\n" shows them
            column((
                para(sig!(self.b0)).font(MD.0, MD.1).color(TEXT),
                para(sig!(self.b1)).font(MD.0, MD.1).color(TEXT),
                para(sig!(self.b2)).font(MD.0, MD.1).color(TEXT),
                para(sig!(self.b3)).font(MD.0, MD.1).color(TEXT),
            ))
            .gap(22.0)
            .w_pct(1.0),
        ))
        .gap(12.0)
        .w_pct(1.0)
    }
}

/// The app: the model (plain data, as state.luau's) and the signals the view shows.
pub struct Mail {
    pub msgs: Vec<Msg>,
    /// The messages the list shows, in order (actions.luau `shown`), as message indices (id - 1).
    pub shown: Vec<u32>,
    /// The (subject, sender) pairs the query lets through (bit `topic * 8 + person`).
    pub pairs: u128,
    /// Each pair's haystack, `lower(subject .. " " .. from)`.
    hay: Vec<String>,
    pub selected: Option<u32>,
    /// Unread messages by mailbox (state.luau `unread_counts`).
    pub counts: [u32; 6],
    /// The window's first row (the list's offset, in rows).
    pub first: usize,
    pressed: Option<u32>,
    shaper: Option<Arc<bough_text::Shaper>>,
    run: bough_text::GlyphRun,
    /// The search field's text origin (points), from the layout.
    field: (f32, f32),
    // ---- what the view shows
    pub rows: List<MailRow>,
    pub mailbox: Signal<u8>,
    pub c0: Signal<u32>,
    pub c1: Signal<u32>,
    pub c2: Signal<u32>,
    pub c3: Signal<u32>,
    pub c4: Signal<u32>,
    pub c5: Signal<u32>,
    /// The search field's text (its island).
    pub search: List<FieldText>,
    pub caret: Signal<Caret>,
    pub status: Signal<String>,
    pub thumb: Signal<f32>,
    /// A message is open (detail.luau's presence; the empty state otherwise).
    pub open: Signal<bool>,
    pub starred: Signal<bool>,
    /// The open message's texts (one row).
    pub pane: List<Pane>,
}

impl Default for Mail {
    fn default() -> Self {
        Mail::new()
    }
}

impl Mail {
    pub fn new() -> Mail {
        Mail::with_messages(messages(4000))
    }
    pub fn with_messages(msgs: Vec<Msg>) -> Mail {
        let mut counts = [0u32; 6];
        for m in &msgs {
            if m.unread {
                counts[m.mailbox as usize] += 1;
            }
        }
        let hay = (0..80).map(|p| format!("{} {}", TOPICS[p / 8], PEOPLE[p % 8].0).to_lowercase()).collect();
        Mail {
            shown: Vec::with_capacity(msgs.len()),
            msgs,
            pairs: u128::MAX,
            hay,
            selected: None,
            counts,
            first: 0,
            pressed: None,
            shaper: None,
            run: Default::default(),
            field: (0.0, 0.0),
            rows: List::with_capacity(WINDOW + 1),
            mailbox: Signal::new(INBOX),
            c0: Signal::new(counts[0]),
            c1: Signal::new(counts[1]),
            c2: Signal::new(counts[2]),
            c3: Signal::new(counts[3]),
            c4: Signal::new(counts[4]),
            c5: Signal::new(counts[5]),
            search: FieldText::list("Search", String::new()),
            caret: Signal::new(Caret::default()),
            status: Signal::new(String::new()),
            thumb: Signal::new(0.0),
            open: Signal::new(false),
            starred: Signal::new(false),
            pane: List::from_iter_in([Pane::default()]),
        }
    }

    /// A fresh model at the state `o` reached (its messages, mailbox, query, selection, focus, offset): for
    /// the gate that a retained app equals a fresh mount.
    pub fn same_state(o: &Mail) -> Mail {
        let mut m = Mail::with_messages(o.msgs.clone());
        for b in 0..6 {
            m.set_count(b as u8, o.counts[b]);
        }
        m.mailbox = Signal::new(*o.mailbox);
        m.search = FieldText::list("Search", o.query().to_owned());
        m.caret = Signal::new(*o.caret);
        m.status = Signal::new((*o.status).clone());
        m.selected = o.selected;
        m.first = o.first;
        m.field = o.field;
        m
    }

    // ------------------------------------------------------------------ the shown list

    /// The search field's text.
    pub fn query(&self) -> &str {
        &self.search.get(0).unwrap().q
    }

    /// Every message judged again (a mailbox or a query changed): `shown`, the thumb.
    pub fn refilter(&mut self) {
        let needle = self.query().to_lowercase();
        self.pairs = if needle.is_empty() {
            u128::MAX
        } else {
            self.hay.iter().enumerate().filter(|(_, h)| h.contains(&needle)).fold(0, |a, (i, _)| a | 1u128 << i)
        };
        let (mb, pairs) = (*self.mailbox, self.pairs);
        self.shown.clear();
        for (i, m) in self.msgs.iter().enumerate() {
            let here = if mb == STARRED { m.starred } else { m.mailbox == mb };
            if here && pairs >> m.pair & 1 != 0 {
                self.shown.push(i as u32);
            }
        }
        self.thumb.set(thumb_len(self.shown.len()));
    }
    fn judge(&self, i: u32) -> bool {
        let m = &self.msgs[i as usize];
        let here = if *self.mailbox == STARRED { m.starred } else { m.mailbox == *self.mailbox };
        here && self.pairs >> m.pair & 1 != 0
    }
    /// Message `i` was written: put it in or out of `shown` (actions.luau's `ui.sorted` re-judges the item a
    /// write touched, not 4,000).
    fn rejudge(&mut self, i: u32) {
        let at = self.shown.binary_search(&i);
        match (at, self.judge(i)) {
            (Ok(a), false) => {
                self.shown.remove(a);
            }
            (Err(a), true) => self.shown.insert(a, i),
            _ => {}
        }
        self.thumb.set(thumb_len(self.shown.len()));
    }

    fn make_row(&mut self, i: u32) -> MailRow {
        let m = self.msgs[i as usize];
        let preview = m.preview();
        let preview = match &self.shaper {
            Some(s) => truncate(s, &preview, SM.0, PREVIEW_W, &mut self.run),
            None => preview,
        };
        MailRow { id: i, from: m.from(), subject: m.subject(), date: format!("Oct {}", m.day), preview, sel: Signal::new(self.selected == Some(i)), unread: Signal::new(m.unread) }
    }

    /// The window's rows follow `shown[first..first + WINDOW]`, keyed by message: a row whose message left is
    /// removed, a message that entered gets a row where it belongs. Both windows are increasing in message
    /// order, so the rows that stay are a subsequence of the new window and no row moves but by the splices.
    pub fn refill(&mut self) {
        let lo = self.first.min(self.shown.len());
        let hi = (lo + WINDOW).min(self.shown.len());
        let mut want = [0u32; WINDOW];
        let n = hi - lo;
        want[..n].copy_from_slice(&self.shown[lo..hi]);
        let want = &want[..n];
        let mut k = self.rows.len();
        while k > 0 {
            k -= 1;
            if !want.contains(&self.rows.get(k).unwrap().id) {
                drop(self.rows.remove(k));
            }
        }
        for (p, &i) in want.iter().enumerate() {
            if self.rows.get(p).map(|r| r.id) != Some(i) {
                let r = self.make_row(i);
                self.rows.insert(p, r);
            }
        }
    }

    fn row_of(&self, i: u32) -> Option<usize> {
        (0..self.rows.len()).find(|&k| self.rows.get(k).unwrap().id == i)
    }
    fn set_row_sel(&mut self, i: u32, v: bool) {
        if let Some(k) = self.row_of(i) {
            self.rows.edit(k).sel.set(v);
        }
    }
    fn set_count(&mut self, b: u8, n: u32) {
        self.counts[b as usize] = n;
        match b {
            0 => self.c0.set(n),
            1 => self.c1.set(n),
            2 => self.c2.set(n),
            3 => self.c3.set(n),
            4 => self.c4.set(n),
            _ => self.c5.set(n),
        };
    }

    // ------------------------------------------------------------------ actions.luau

    /// `actions.open(id)`: selected, and read (one key and its mailbox's count, not the list).
    pub fn open(&mut self, i: u32) {
        if self.selected != Some(i) {
            if let Some(o) = self.selected {
                self.set_row_sel(o, false);
            }
            self.selected = Some(i);
            self.set_row_sel(i, true);
        }
        let m = self.msgs[i as usize];
        if m.unread {
            self.msgs[i as usize].unread = false;
            self.set_count(m.mailbox, self.counts[m.mailbox as usize].saturating_sub(1));
            if let Some(k) = self.row_of(i) {
                self.rows.edit(k).unread.set(false);
            }
        }
        self.show(i);
    }
    /// detail.luau: the open message's fields.
    fn show(&mut self, i: u32) {
        let m = self.msgs[i as usize];
        self.open.set(true);
        self.starred.set(m.starred);
        let p = self.pane.edit(0);
        p.subject.set(m.subject().to_owned());
        p.from.set(m.from().to_owned());
        p.email.set(format!("  <{}>", m.email()));
        p.date.set(format!("  \u{b7}  Oct {}", m.day));
        p.b0.set(m.paragraph(0));
        p.b1.set(m.paragraph(1));
        p.b2.set(m.paragraph(2));
        p.b3.set(m.paragraph(3));
    }
    fn deselect(&mut self) {
        if let Some(o) = self.selected.take() {
            self.set_row_sel(o, false);
        }
        self.open.set(false);
    }

    /// `actions.move(id, mailbox)`: its unread count moves with it, it leaves the list, and the message
    /// after it (now in its place) opens, as a mail app moves on.
    pub fn move_to(&mut self, i: u32, mb: u8) {
        let at = self.shown.binary_search(&i).ok();
        let m = self.msgs[i as usize];
        if m.unread {
            self.set_count(m.mailbox, self.counts[m.mailbox as usize].saturating_sub(1));
            self.set_count(mb, self.counts[mb as usize] + 1);
        }
        self.msgs[i as usize].mailbox = mb;
        self.rejudge(i);
        if self.selected == Some(i) {
            let n = self.shown.len();
            match at {
                Some(a) if n > 0 => {
                    let next = self.shown[a.min(n - 1)];
                    self.open(next);
                }
                _ => self.deselect(),
            }
        }
        self.status.set(format!("Moved to {}", BOXES[mb as usize]));
        self.refill();
    }
    pub fn move_open(&mut self, mb: u8) {
        if let Some(i) = self.selected {
            self.move_to(i, mb);
        }
    }
    /// `actions.toggle_star(id)`.
    pub fn toggle_star_open(&mut self) {
        if let Some(i) = self.selected {
            let s = !self.msgs[i as usize].starred;
            self.msgs[i as usize].starred = s;
            self.starred.set(s);
            if *self.mailbox == STARRED {
                self.rejudge(i);
                self.refill();
            }
        }
    }
    /// A sidebar box chosen (components/sidebar.luau: the mailbox, nothing selected).
    pub fn choose_mailbox(&mut self, b: u8) {
        self.mailbox.set(b);
        self.deselect();
        self.first = 0;
        self.refilter();
        self.refill();
    }

    // ------------------------------------------------------------------ the list's pointer

    /// A press on the list: the row under it (rows stack from the list's top, `ROW_H` each).
    pub fn press_list(&mut self, y: f32) {
        self.pressed = None;
        if y >= LIST_TOP {
            let k = ((y - LIST_TOP) / ROW_H) as usize;
            self.pressed = self.rows.get(k).map(|r| r.id);
        }
    }
    /// The release over the pressed row: it opens (list.luau `on_click`).
    pub fn click_list(&mut self) {
        if let Some(i) = self.pressed.take() {
            self.open(i);
        }
    }

    // ------------------------------------------------------------------ the search field

    pub fn focus_search(&mut self) {
        self.place_caret();
    }
    fn place_caret(&mut self) {
        let x = match &self.shaper {
            Some(s) => text_width(s, &self.search.get(0).unwrap().q, MD.0, &mut self.run),
            None => 0.0,
        };
        self.caret.set(Caret { x: self.field.0 + x, y: self.field.1, on: true });
    }
    /// A key in the search field: the query edited where it is written, the list judged again, the window
    /// refilled, the caret moved.
    pub fn search_key(&mut self, e: &Ev) {
        match e.key {
            Some(Key::Char(c)) => self.search.edit(0).q.push(c),
            Some(Key::Backspace) if !self.query().is_empty() => {
                self.search.edit(0).q.pop();
            }
            _ => return,
        }
        self.refilter();
        self.first = 0;
        self.refill();
        self.place_caret();
    }

    /// After mounting: the labels' shaper (for truncation and the caret), the field's text origin, the list
    /// filled, the open message.
    pub fn attach(&mut self, shaper: Arc<bough_text::Shaper>, field: (f32, f32)) {
        self.shaper = Some(shaper);
        self.field = field;
        self.refilter();
        self.refill();
        if let Some(i) = self.selected {
            self.show(i);
        }
        if self.caret.on {
            self.place_caret();
        }
    }
}

/// Mount `v` and fill it (the labels' shaper is the app's, made by the mount).
pub fn mount(v: Mail, cfg: Config, gpu: bool) -> App<Mail> {
    let mut app = App::new(v, cfg, gpu);
    let shaper = app.rt.text.shaper.clone();
    // the field's text: inside its 1-point border and 8-point padding; the 18-point caret centred in 36
    let f = lay_rect(&app.rt, &SEARCH_PATH);
    app.turn(|m| m.attach(shaper, (f[0] + 9.0, f[1] + 9.0)));
    app
}

/// The rect (points) of the `k`th mounted row of the list (zgx `click_row`: rows by where they are; a list
/// region stacks its rows from its top by a prefix sum of their heights, here all `ROW_H`).
pub fn row_rect(app: &App<Mail>, k: usize) -> [f32; 4] {
    assert!(k < app.owner.rows.len(), "`messages` has {} rows mounted, not {}", app.owner.rows.len(), k + 1);
    [LIST_X, LIST_TOP + k as f32 * ROW_H, LIST_W, ROW_H]
}

fn vline<V: 'static>() -> impl Element<V> {
    block(1.0, 0.0).h_pct(1.0).shrink(0.0).bg(BORDER_SUBTLE)
}

impl Mail {
    /// components/sidebar.luau.
    fn sidebar(&self) -> impl Element<Mail> + '_ {
        column((
            row(button("New Message").grow(1.0).hover_bg(BUTTON_HOVER).on_click(|_: &mut Mail| {})).pad(0.0, 0.0, 0.0, 10.0),
            column(each((0..6u8).map(|k| self.mailbox_row(k)))).gap(2.0),
        ))
        .w(210.0)
        .h_pct(1.0)
        .pad(10.0, 44.0, 10.0, 0.0)
        .gap(2.0)
        .bg(SUNKEN)
    }
    fn mailbox_row(&self, k: u8) -> impl Element<Mail> + '_ {
        let count = match k {
            0 => sig!(self.c0),
            1 => sig!(self.c1),
            2 => sig!(self.c2),
            3 => sig!(self.c3),
            4 => sig!(self.c4),
            _ => sig!(self.c5),
        };
        row((label(BOXES[k as usize]).font(MD.0, MD.1).color(TEXT).grow(1.0), label(count.map(count_text)).font(SM.0, SM.1).color(TEXT2)))
            .h(30.0)
            .px(10.0)
            .rounded(6.0)
            .items(Align::Center)
            .gap(8.0)
            .bg(sig!(self.mailbox).select(k, SELECTION, CLEAR))
            .hover_bg(HOVER)
            .on_click(move |m: &mut Mail| m.choose_mailbox(k))
    }
    /// components/list.luau: the search field, the list, the status line.
    fn list_pane(&self) -> impl Element<Mail> + '_ {
        let field = field_box(sig!(self.search)).grow(1.0).on_key(|m: &mut Mail, e: &Ev| m.search_key(e)).on_click(|m: &mut Mail| m.focus_search());
        column((
            row(field).w_pct(1.0).pad(12.0, 40.0, 12.0, 8.0).gap(8.0).items(Align::Center),
            clip(
                LIST_X,
                LIST_TOP,
                LIST_W,
                LIST_H,
                list(sig!(self.rows)).w_pct(1.0).h(LIST_H).on_press(|m: &mut Mail, e: &Ev| m.press_list(e.y)).on_click(|m: &mut Mail| m.click_list()),
            ),
            row(label(sig!(self.status)).font(SM.0, SM.1).color(TEXT2)).px(14.0).py(6.0),
        ))
        .w(LIST_W)
        .shrink(0.0)
        .h_pct(1.0)
        .bg(BASE)
    }
    /// components/detail.luau: the open message, or "No message selected" (each shown through a clip that
    /// the selection switches: the pane is mounted once and its texts change in place, as zgx's presence
    /// keyed by whether a message is open, not which).
    fn detail(&self) -> impl Element<Mail> + '_ {
        let open = column((
            row((
                button("Reply").hover_bg(BUTTON_HOVER).on_click(|_: &mut Mail| {}),
                button(sig!(self.starred).map(|s: &bool| if *s { "Unstar" } else { "Star" }.to_owned())).hover_bg(BUTTON_HOVER).on_click(|m: &mut Mail| m.toggle_star_open()),
                button("Archive").hover_bg(BUTTON_HOVER).on_click(|m: &mut Mail| m.move_open(ARCHIVE)),
                button("Delete").hover_bg(BUTTON_HOVER).on_click(|m: &mut Mail| m.move_open(TRASH)),
            ))
            .gap(8.0),
            list(sig!(self.pane)).w_pct(1.0).h(0.0).grow(1.0),
        ))
        .grow(1.0)
        .pad(28.0, 40.0, 28.0, 0.0)
        .gap(12.0);
        let empty = column(label("No message selected").font(MD.0, MD.1).color(TEXT3)).abs(0.0, 0.0).size(DETAIL[2], DETAIL[3]).items(Align::Center).justify(Justify::Center);
        column((
            clip_by(sig!(self.open), |o: &bool| if *o { DETAIL } else { [0.0; 4] }, open),
            clip_by(sig!(self.open), |o: &bool| if *o { [0.0; 4] } else { DETAIL }, empty),
        ))
        // the rest of the window whatever the message holds (a basis of 0, as `flex: 1` is)
        .w(0.0)
        .grow(1.0)
        .h_pct(1.0)
        .bg(RAISED)
        .relative()
    }
}

impl View for Mail {
    const FIXED: bool = true;
    /// app.luau (the wide layout).
    fn view(&self) -> impl Element<Self> + '_ {
        (
            row((row((self.sidebar(), vline())).h_pct(1.0).shrink(0.0), self.list_pane(), vline(), self.detail())).w(W).h(H),
            // the list's scrollbar (an overlay, always shown: zgx's default) and the field's caret, over the panes
            quad(LIST_X + LIST_W - 8.0, 0.0, 8.0, 0.0).bg(TRACK).span(sig!(self.thumb).map(|t: &f32| (LIST_TOP, if *t > 0.0 { LIST_H } else { 0.0 }))),
            quad(LIST_X + LIST_W - 7.0, 0.0, 6.0, 0.0).bg(ACCENT).rounded(3.0).span(sig!(self.thumb).map(|t: &f32| (LIST_TOP + 2.0, (*t - 4.0).max(0.0)))),
            lanes(1, sig!(self.caret), caret_lane),
        )
    }
}
