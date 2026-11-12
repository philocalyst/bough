//! The look and the data that every app shares, so all frameworks render the same pixels and run
//! the same operations. The runner mirrors the geometry in `runner/src/layout.rs`.
//!
//! Layout: the window is 1280x800 at scale 1. The `rows` and `virtual` scenarios start their rows
//! at the top of the window. The other scenarios put a header of `HEADER_HEIGHT` above the rows:
//! the status text on the left, and the spinner (spinner scenario) or the text input (search and
//! typing scenarios) in it. Rows below a header all take `ROW_EVEN`, with no stripes and no hover
//! color, so no row's look depends on its position or on the pointer; the selected row of the
//! table takes `SELECTED`.

#![allow(dead_code)]

/// The window size in logical pixels.
pub const WINDOW: (f32, f32) = (1280.0, 800.0);
/// The height of one row, its 1 px bottom border included.
pub const ROW_HEIGHT: f32 = 32.0;
/// The height of the bottom border of a row.
pub const BORDER_HEIGHT: f32 = 1.0;
/// The height of the header above the rows, its 1 px bottom border included.
pub const HEADER_HEIGHT: f32 = 40.0;
/// The horizontal padding of rows and the header.
pub const PADDING_X: f32 = 12.0;
/// The font size of all text.
pub const FONT_SIZE: f32 = 14.0;
/// The font family of all text.
pub const FONT_FAMILY: &str = "sans-serif";

/// The side of the spinner square.
pub const SPINNER_SIZE: f32 = 24.0;
/// The corner radius of the spinner square.
pub const SPINNER_RADIUS: f32 = 3.0;
/// The spinner turns once in this time.
pub const SPINNER_PERIOD_MS: u64 = 1000;
/// The left edge of the spinner square, from the left of the window. The square sits at the
/// right end of the header, `PADDING_X` from the edge, and centered vertically.
pub const SPINNER_X: f32 = WINDOW.0 - PADDING_X - SPINNER_SIZE;
/// The top edge of the spinner square.
pub const SPINNER_Y: f32 = (HEADER_HEIGHT - BORDER_HEIGHT - SPINNER_SIZE) / 2.0;

/// The width of the text input in the header.
pub const INPUT_WIDTH: f32 = 400.0;
/// The height of the text input in the header.
pub const INPUT_HEIGHT: f32 = 28.0;

pub const BACKGROUND: u32 = 0xffffff;
pub const ROW_EVEN: u32 = 0xffffff;
pub const ROW_ODD: u32 = 0xf6f8fa;
pub const ROW_HOVER: u32 = 0xcfe3ff;
pub const ROW_BORDER: u32 = 0xe5e7eb;
pub const LABEL_COLOR: u32 = 0x1f2328;
pub const VALUE_COLOR: u32 = 0x6e7781;
pub const HEADER_BG: u32 = 0xeef1f4;
pub const HEADER_BORDER: u32 = 0xd0d7de;
pub const SELECTED: u32 = 0xffe08a;
pub const SPINNER_COLOR: u32 = 0x2a78d6;

/// Returns the red, green and blue bytes of a color.
pub const fn rgb(color: u32) -> (u8, u8, u8) {
    ((color >> 16) as u8, (color >> 8) as u8, color as u8)
}

/// Returns the background of a row by its position in the list.
pub const fn row_background(position: usize) -> u32 {
    if position % 2 == 1 { ROW_ODD } else { ROW_EVEN }
}

/// The row label of the `rows` and `virtual` scenarios.
pub fn row_label(index: usize) -> String {
    format!("Row {index:05}")
}

/// The value text of a row id.
pub fn value(id: u64) -> u32 {
    (id.wrapping_mul(7919) % 10000) as u32
}

const ADJECTIVES: &[&str] = &[
    "pretty",
    "large",
    "big",
    "small",
    "tall",
    "short",
    "long",
    "handsome",
    "plain",
    "quaint",
    "clean",
    "elegant",
    "easy",
    "angry",
    "curious",
    "helpful",
    "mushy",
    "odd",
    "unsightly",
    "adorable",
    "important",
    "inexpensive",
    "cheap",
    "expensive",
    "fancy",
];
const COLOURS: &[&str] = &[
    "red", "yellow", "blue", "green", "pink", "brown", "purple", "brown", "white", "black",
    "orange",
];
const NOUNS: &[&str] = &[
    "table", "chair", "house", "bbq", "desk", "car", "pony", "cookie", "sandwich", "burger",
    "pasta", "mouse", "keyboard",
];

/// The label of a table row id: an adjective, a colour and a noun. No word holds a `z`, so a
/// search for `z` hides every row.
pub fn label(id: u64) -> String {
    let a = ADJECTIVES[(id.wrapping_mul(7) % ADJECTIVES.len() as u64) as usize];
    let c = COLOURS[(id.wrapping_mul(13) % COLOURS.len() as u64) as usize];
    let n = NOUNS[(id.wrapping_mul(17) % NOUNS.len() as u64) as usize];
    format!("{a} {c} {n}")
}

/// The suffix that the partial update adds to every tenth label, or takes away.
pub const UPDATE_SUFFIX: &str = " !!!";

/// Returns the label with the update suffix added, or taken away when it has one.
pub fn toggle_suffix(label: &str) -> String {
    match label.strip_suffix(UPDATE_SUFFIX) {
        Some(base) => base.to_owned(),
        None => format!("{label}{UPDATE_SUFFIX}"),
    }
}

/// One table row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Row {
    /// A stable id: no two rows of a run share one.
    pub id: u64,
    pub label: String,
    pub value: u32,
}

impl Row {
    /// Returns the row with an id, its label and its value.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            label: label(id),
            value: value(id),
        }
    }
}

/// A row type that the table operations can build and edit.
pub trait TableRow {
    /// Returns the app row for a new row.
    fn from_row(row: Row) -> Self;
    /// Adds the update suffix to the label, or takes it away.
    fn toggle_suffix(&mut self);
}

impl TableRow for Row {
    fn from_row(row: Row) -> Self {
        row
    }

    fn toggle_suffix(&mut self) {
        self.label = toggle_suffix(&self.label);
    }
}

/// Hands out row ids. The first dataset takes ids `0..n`; later rows take fresh ids.
#[derive(Debug, Default, Clone)]
pub struct Ids {
    next: u64,
}

impl Ids {
    /// Returns `n` new rows with fresh ids.
    pub fn rows(&mut self, n: usize) -> Vec<Row> {
        (0..n).map(|_| self.row()).collect()
    }

    /// Returns `n` new rows of an app's row type with fresh ids.
    pub fn rows_of<T: TableRow>(&mut self, n: usize) -> Vec<T> {
        (0..n).map(|_| T::from_row(self.row())).collect()
    }

    /// Returns one new row with a fresh id.
    pub fn row(&mut self) -> Row {
        let row = Row::new(self.next);
        self.next += 1;
        row
    }
}

/// The table operations and their keys. Every operation is undone by another key, so a probe can
/// alternate between two states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// F1: every tenth row, from the first, gets the update suffix or loses it.
    Update10,
    /// F2: rows 1 and `len - 2` trade places.
    Swap,
    /// F3: the row at `EDIT_AT` goes away.
    Remove,
    /// F4: a new row goes in at `EDIT_AT`.
    Insert,
    /// F5: every row is replaced by `count` new rows.
    Replace,
    /// F6: `APPEND` new rows go at the end.
    Append,
    /// F7: the list is cut back to `count` rows.
    Truncate,
    /// F8: the list is cleared, or gets `count` new rows when it is empty.
    Clear,
}

/// The position that remove and insert change.
pub const EDIT_AT: usize = 2;
/// The number of rows that append adds.
pub const APPEND: usize = 1000;

impl Op {
    /// Every operation in key order.
    pub const ALL: [Op; 8] = [
        Op::Update10,
        Op::Swap,
        Op::Remove,
        Op::Insert,
        Op::Replace,
        Op::Append,
        Op::Truncate,
        Op::Clear,
    ];

    /// Returns the operation of a function key number, 1 to 8.
    pub fn from_f_key(n: u8) -> Option<Op> {
        Op::ALL.get(usize::from(n).checked_sub(1)?).copied()
    }

    /// Returns false for the operation that changes labels only and keeps every row in place.
    pub fn changes_structure(self) -> bool {
        self != Op::Update10
    }

    /// Applies the operation to a list. `count` is the row count of the scenario. The rows are
    /// plain `Row`s, or an app's own row type with per-row state.
    pub fn apply<T: TableRow>(self, rows: &mut Vec<T>, ids: &mut Ids, count: usize) {
        match self {
            Op::Update10 => {
                for row in rows.iter_mut().step_by(10) {
                    row.toggle_suffix();
                }
            }
            Op::Swap => {
                if rows.len() > 3 {
                    let last = rows.len() - 2;
                    rows.swap(1, last);
                }
            }
            Op::Remove => {
                if rows.len() > EDIT_AT {
                    rows.remove(EDIT_AT);
                }
            }
            Op::Insert => {
                let at = EDIT_AT.min(rows.len());
                rows.insert(at, T::from_row(ids.row()));
            }
            Op::Replace => *rows = ids.rows_of(count),
            Op::Append => rows.extend(ids.rows_of(APPEND)),
            Op::Truncate => rows.truncate(count),
            Op::Clear => {
                if rows.is_empty() {
                    *rows = ids.rows_of(count);
                } else {
                    rows.clear();
                }
            }
        }
    }
}

/// Returns the status text of the table header.
pub fn status(rows: usize) -> String {
    format!("{rows} rows")
}

/// Returns the status text of the ticker header.
pub fn ticker_status(rows: usize, tick: u64) -> String {
    format!("{rows} rows \u{b7} tick {tick}")
}

/// Returns the positions that a ticker tick updates: `updates` positions spread evenly over a
/// list of `len` rows, from the first.
pub fn ticker_positions(len: usize, updates: usize) -> impl Iterator<Item = usize> {
    let updates = updates.min(len);
    (0..updates).map(move |j| j * len / updates)
}

/// Returns the value of a row at a ticker tick.
pub fn ticker_value(id: u64, tick: u64) -> u32 {
    ((u64::from(value(id)) + tick) % 10000) as u32
}

/// Returns true when a label matches a search query: it holds the query, ignoring ASCII case.
pub fn matches(label: &str, query: &str) -> bool {
    query.is_empty()
        || label
            .to_ascii_lowercase()
            .contains(&query.to_ascii_lowercase())
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    #[test]
    fn replace_changes_the_first_label() {
        let mut ids = Ids::default();
        let a = ids.rows(1000);
        let b = ids.rows(1000);
        assert_ne!(a[0].label, b[0].label);
        assert_ne!(a[0].value, b[0].value);
    }

    #[test]
    fn no_label_holds_a_z() {
        assert!((0..10_000).all(|id| !label(id).contains('z')));
    }

    #[test]
    fn edits_change_visible_rows() {
        let mut ids = Ids::default();
        let mut rows = ids.rows(1000);
        let before = rows.clone();
        Op::Remove.apply(&mut rows, &mut ids, 1000);
        assert_ne!(rows[EDIT_AT].label, before[EDIT_AT].label);
        Op::Insert.apply(&mut rows, &mut ids, 1000);
        assert_eq!(rows.len(), 1000);
        Op::Swap.apply(&mut rows, &mut ids, 1000);
        assert_eq!(rows[998].id, before[1].id);
        Op::Update10.apply(&mut rows, &mut ids, 1000);
        assert!(rows[0].label.ends_with(UPDATE_SUFFIX));
        Op::Update10.apply(&mut rows, &mut ids, 1000);
        assert!(!rows[0].label.ends_with(UPDATE_SUFFIX));
    }

    #[test]
    fn ticker_positions_spread_from_the_first_row() {
        assert_eq!(ticker_positions(1000, 1).collect::<Vec<_>>(), vec![0]);
        let p: Vec<_> = ticker_positions(1000, 100).collect();
        assert_eq!((p[0], p[1], p.len()), (0, 10, 100));
    }

    #[test]
    fn function_keys_map_to_ops() {
        assert_eq!(Op::from_f_key(1), Some(Op::Update10));
        assert_eq!(Op::from_f_key(8), Some(Op::Clear));
        assert_eq!(Op::from_f_key(0), None);
        assert_eq!(Op::from_f_key(9), None);
    }
}
