use crate::spec::{self, Ids, Op, Row};
use bough::prelude::*;

macro_rules! contents {
    ($label:expr, $value:expr) => {
        row((
            text($label)
                .font(spec::FONT_SIZE, 20.0)
                .color(rgb(spec::LABEL_COLOR)),
            text($value)
                .font(spec::FONT_SIZE, 20.0)
                .color(rgb(spec::VALUE_COLOR)),
        ))
        .w_pct(1.0)
        .h(spec::ROW_HEIGHT - spec::BORDER_HEIGHT)
        .shrink(0.0)
        .px(spec::PADDING_X)
        .items(Align::Center)
        .justify(Justify::SpaceBetween)
    };
}
fn border<V: 'static>() -> impl Element<V> {
    block(0.0, spec::BORDER_HEIGHT)
        .w_pct(1.0)
        .shrink(0.0)
        .bg(rgb(spec::ROW_BORDER))
}
fn header<V: 'static>(count: usize) -> impl Element<V> {
    column((
        row((text(spec::status(count)).color(rgb(spec::LABEL_COLOR)),))
            .w_pct(1.0)
            .h(spec::HEADER_HEIGHT - spec::BORDER_HEIGHT)
            .shrink(0.0)
            .px(spec::PADDING_X)
            .items(Align::Center),
        block(0.0, spec::BORDER_HEIGHT)
            .w_pct(1.0)
            .shrink(0.0)
            .bg(rgb(spec::HEADER_BORDER)),
    ))
    .w_pct(1.0)
    .h(spec::HEADER_HEIGHT)
    .shrink(0.0)
    .bg(rgb(spec::HEADER_BG))
}

pub struct Rows {
    pub count: usize,
    pub y: Signal<f32>,
}
impl Rows {
    pub fn new(count: usize) -> Self {
        Self {
            count,
            y: Signal::new(0.0),
        }
    }
}
impl View for Rows {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(rgb(spec::BACKGROUND))
            .scroll(sig!(self.y))
            .child(each((0..self.count).map(|i| {
                column((
                    contents!(spec::row_label(i), spec::value(i as u64).to_string()),
                    border(),
                ))
                .w_pct(1.0)
                .h(spec::ROW_HEIGHT)
                .shrink(0.0)
                .bg(rgb(spec::row_background(i)))
                .hover_bg(rgb(spec::ROW_HOVER))
            })))
    }
}

pub struct Coarse {
    pub rows: Vec<Row>,
    pub ids: Ids,
    pub count: usize,
    pub selected: Option<u64>,
    pub y: Signal<f32>,
    pub mutant: bool,
}
impl Coarse {
    pub fn new(count: usize, mutant: bool) -> Self {
        let mut ids = Ids::default();
        Self {
            rows: ids.rows(count),
            ids,
            count,
            selected: None,
            y: Signal::new(0.0),
            mutant,
        }
    }
    fn select(&mut self, id: u64) {
        if self.mutant {
            for r in &self.rows {
                std::hint::black_box(r.id);
            }
        }
        self.selected = if self.selected == Some(id) {
            None
        } else {
            Some(id)
        };
    }
    pub fn snapshot(&self) -> Self {
        Self {
            rows: self.rows.clone(),
            ids: self.ids.clone(),
            count: self.count,
            selected: self.selected,
            y: Signal::new(*self.y),
            mutant: self.mutant,
        }
    }
}
impl View for Coarse {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            header(self.rows.len()),
            div()
                .w_pct(1.0)
                .grow(1.0)
                .min_h(0.0)
                .scroll(sig!(self.y))
                .child(each(self.rows.iter().map(|r| {
                    let id = r.id;
                    column((contents!(r.label.clone(), r.value.to_string()), border()))
                        .w_pct(1.0)
                        .h(spec::ROW_HEIGHT)
                        .shrink(0.0)
                        .bg(rgb(if self.selected == Some(id) {
                            spec::SELECTED
                        } else {
                            spec::ROW_EVEN
                        }))
                        .on_click(move |v: &mut Self| v.select(id))
                }))),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(spec::BACKGROUND))
        .on_key(|v: &mut Self, e| {
            if let Some(op) = key_op(e.key) {
                op.apply(&mut v.rows, &mut v.ids, v.count);
            }
        })
    }
}

pub struct FineRow {
    pub id: u64,
    pub label: Signal<String>,
    pub value: Signal<u32>,
    pub selected: Signal<bool>,
}
impl FineRow {
    fn new(r: Row) -> Self {
        Self {
            id: r.id,
            label: Signal::new(r.label),
            value: Signal::new(r.value),
            selected: Signal::new(false),
        }
    }
    fn row(&self) -> Row {
        Row {
            id: self.id,
            label: (*self.label).clone(),
            value: *self.value,
        }
    }
}
impl View for FineRow {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            contents!(sig!(self.label), sig!(self.value).map(|n| n.to_string())),
            border(),
        ))
        .w_pct(1.0)
        .h(spec::ROW_HEIGHT)
        .shrink(0.0)
        .bg(sig!(self.selected).map(|s| rgb(if *s { spec::SELECTED } else { spec::ROW_EVEN })))
    }
}
pub struct Fine {
    pub rows: List<FineRow>,
    pub ids: Ids,
    pub count: usize,
    pub selected: Option<Slot<FineRow>>,
    pressed: Option<Slot<FineRow>>,
    pub y: Signal<f32>,
    pub mutant: bool,
}
impl Fine {
    pub fn new(count: usize, mutant: bool) -> Self {
        let mut ids = Ids::default();
        Self {
            rows: List::from_iter_in(ids.rows(count).into_iter().map(FineRow::new)),
            ids,
            count,
            selected: None,
            pressed: None,
            y: Signal::new(0.0),
            mutant,
        }
    }
    pub fn data(&self) -> Vec<Row> {
        self.rows.iter().map(FineRow::row).collect()
    }
    pub fn selected_id(&self) -> Option<u64> {
        self.selected
            .and_then(|s| self.rows.get_slot(s))
            .map(|r| r.id)
    }
    pub fn snapshot(&self) -> Self {
        let mut out = Self::new(0, self.mutant);
        out.ids = self.ids.clone();
        out.count = self.count;
        out.y = Signal::new(*self.y);
        for r in self.rows.iter() {
            let item = FineRow::new(r.row());
            let selected = *r.selected;
            let slot = out.rows.push(item);
            if selected {
                out.rows.edit_slot(slot).unwrap().selected.set(true);
                out.selected = Some(slot);
            }
        }
        out
    }
    fn select_pressed(&mut self) {
        if self.mutant {
            for r in self.rows.iter() {
                std::hint::black_box(r.id);
            }
        }
        let Some(slot) = self.pressed.take() else {
            return;
        };
        let same = self.selected == Some(slot);
        if let Some(old) = self.selected.take() {
            if let Some(row) = self.rows.edit_slot(old) {
                row.selected.set(false);
            }
        }
        if !same {
            if let Some(row) = self.rows.edit_slot(slot) {
                row.selected.set(true);
                self.selected = Some(slot);
            }
        }
    }
    fn apply(&mut self, op: Op) {
        match op {
            Op::Update10 => {
                for i in (0..self.rows.len()).step_by(10) {
                    let row = self.rows.edit(i);
                    row.label.set(spec::toggle_suffix(&row.label));
                }
            }
            Op::Swap => {
                if self.rows.len() > 3 {
                    self.rows.swap(1, self.rows.len() - 2);
                }
            }
            Op::Remove => {
                if self.rows.len() > spec::EDIT_AT {
                    self.rows.remove(spec::EDIT_AT);
                }
            }
            Op::Insert => {
                self.rows.insert(
                    spec::EDIT_AT.min(self.rows.len()),
                    FineRow::new(self.ids.row()),
                );
            }
            Op::Replace => {
                self.rows.clear();
                self.rows
                    .extend(self.ids.rows(self.count).into_iter().map(FineRow::new));
            }
            Op::Append => self
                .rows
                .extend(self.ids.rows(spec::APPEND).into_iter().map(FineRow::new)),
            Op::Truncate => self.rows.truncate(self.count),
            Op::Clear => {
                if self.rows.is_empty() {
                    self.rows
                        .extend(self.ids.rows(self.count).into_iter().map(FineRow::new));
                } else {
                    self.rows.clear();
                }
            }
        }
    }
}
impl View for Fine {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            header(self.rows.len()),
            // Deliberately nonvirtual: .scroll belongs to the parent box, not List.
            div()
                .w_pct(1.0)
                .grow(1.0)
                .min_h(0.0)
                .scroll(sig!(self.y))
                .child(list(sig!(self.rows)).w_pct(1.0)),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(spec::BACKGROUND))
        .on_press(|v: &mut Self, e| {
            v.pressed = if e.y >= spec::HEADER_HEIGHT {
                v.rows
                    .slot(((e.y - spec::HEADER_HEIGHT + *v.y) / spec::ROW_HEIGHT).floor() as usize)
            } else {
                None
            };
        })
        .on_click(Self::select_pressed)
        .on_key(|v: &mut Self, e| {
            if let Some(op) = key_op(e.key) {
                v.apply(op);
            }
        })
    }
}

/// AppKit's existing key_of path forwards these function-key Unicode characters (FRICTION G3).
pub fn f_key(n: u8) -> Key {
    assert!((1..=8).contains(&n));
    Key::Char(char::from_u32(0xf703 + u32::from(n)).unwrap())
}
fn key_op(key: Option<Key>) -> Option<Op> {
    match key {
        Some(Key::Char(c)) if (0xf704..=0xf70b).contains(&(c as u32)) => {
            Op::from_f_key((c as u32 - 0xf703) as u8)
        }
        _ => None,
    }
}
