//! The spreadsheet: a toolbar, a header row, a body of `rows` rows of ten cells in a `scrollable` and a footer of
//! column totals. The model holds the cells and the totals; the hot column, the theme and the edit are state that the
//! styles read (`style(|theme, status| ...)` closures over the state), so a hover, `t` or `e` is a message, an
//! `update` and a new `view` that iced diffs against the old tree.

use iced::alignment::Vertical;
use iced::widget::{Column, column, container, mouse_area, row, scrollable, text};
use iced::{Background, Border, Element, Length, Subscription, Theme, keyboard, theme};

use super::{key_of, rgb};
use crate::rows::scrollbar;
use crate::rows::wheel::Wheel;
use crate::scenes::sheet::{self as spec, COLS};
use crate::scenes::THEME_KEY;

/// One cell: its value, and its text (formatted when the value changes, not on every `view`).
struct Cell {
    value: f64,
    text: String,
}

/// The state of the sheet.
struct Sheet {
    cells: Vec<Cell>,
    totals: [f64; COLS],
    /// 0 light, 1 dark.
    theme: usize,
    hot: Option<usize>,
    edited: bool,
}

#[derive(Debug, Clone)]
enum Message {
    /// The pointer entered header cell `c`.
    Entered(usize),
    /// The pointer left header cell `c`.
    Left(usize),
    /// A key press that no widget took.
    Key(String),
}

/// Opens the window and runs the event loop until the window closes.
pub fn run(rows: usize, title: &'static str) -> iced::Result {
    iced::application(move || Sheet::new(rows), Sheet::update, Sheet::view)
        .subscription(Sheet::subscription)
        .title(title)
        .theme(Sheet::theme)
        .style(Sheet::style)
        .window(super::window())
        .run()
}

impl Sheet {
    fn new(rows: usize) -> Self {
        let mut totals = [0.0; COLS];
        let cells = (0..rows * COLS)
            .map(|i| {
                let value = spec::value(i / COLS, i % COLS);
                totals[i % COLS] += value;
                Cell {
                    value,
                    text: spec::text(value),
                }
            })
            .collect();
        Self {
            cells,
            totals,
            theme: 0,
            hot: None,
            edited: false,
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Entered(c) => self.hot = Some(c),
            // entering the next cell may be heard before leaving the last one
            Message::Left(c) => {
                if self.hot == Some(c) {
                    self.hot = None;
                }
            }
            Message::Key(key) if key == THEME_KEY => self.theme = 1 - self.theme,
            Message::Key(key) if key == spec::EDIT_KEY => {
                let (row, col) = spec::EDIT;
                let delta = if self.edited {
                    -spec::EDIT_DELTA
                } else {
                    spec::EDIT_DELTA
                };
                self.edited = !self.edited;
                let cell = &mut self.cells[row * COLS + col];
                cell.value += delta;
                cell.text = spec::text(cell.value);
                self.totals[col] += delta;
            }
            Message::Key(_) => {}
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().filter_map(|event| key_of(event).map(Message::Key))
    }

    fn theme(&self) -> Theme {
        if self.theme == 0 {
            Theme::Light
        } else {
            Theme::Dark
        }
    }

    /// The page: the color of the body right of the last cell.
    fn style(&self, _: &Theme) -> theme::Style {
        theme::Style {
            background_color: rgb(spec::PAGE[self.theme]),
            text_color: rgb(spec::INK[self.theme]),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        column![
            self.toolbar(),
            self.header(),
            self.body(),
            self.footer(),
        ]
        .into()
    }

    fn toolbar(&self) -> Element<'_, Message> {
        let t = self.theme;
        container(
            row![
                text("Sheet 1").size(14).color(rgb(spec::INK[t])).width(88),
                text("\u{3a3} totals").size(12).color(rgb(spec::GUTTER_INK)),
            ]
            .align_y(Vertical::Center)
            .padding([0.0, 12.0]),
        )
        .width(Length::Fill)
        .height(spec::TOOLBAR_H)
        .align_y(Vertical::Center)
        .style(chrome_style(rgb(spec::CHROME[t])))
        .into()
    }

    /// The header: the gutter cell and one cell per column; the pointer over a cell makes its column hot.
    fn header(&self) -> Element<'_, Message> {
        let t = self.theme;
        let cells = (0..COLS).map(|c| {
            let background = if self.hot == Some(c) {
                rgb(spec::HEADER_HOT)
            } else {
                rgb(spec::CHROME[t])
            };
            mouse_area(cell(
                spec::COLUMN_NAMES[c],
                12.0,
                rgb(spec::INK[t]),
                spec::CELL_W,
                spec::HEADER_H,
                background,
                None,
            ))
            .on_enter(Message::Entered(c))
            .on_exit(Message::Left(c))
            .into()
        });
        // right of the last header cell the strip stays CHROME (the spec leaves it open)
        container(row![gutter(None, spec::HEADER_H, t), row(cells)])
            .width(Length::Fill)
            .height(spec::HEADER_H)
            .style(chrome_style(rgb(spec::CHROME[t])))
            .into()
    }

    /// The body: every row, in a vertical scroll container.
    fn body(&self) -> Element<'_, Message> {
        let rows = Column::with_children(
            self.cells
                .chunks(COLS)
                .enumerate()
                .map(|(r, cells)| self.row(r, cells)),
        )
        .width(Length::Fill);
        Wheel::new(
            scrollable(rows)
                .direction(scrollable::Direction::Vertical(scrollbar()))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .into()
    }

    fn row<'a>(&self, r: usize, cells: &'a [Cell]) -> Element<'a, Message> {
        let t = self.theme;
        let ink = rgb(spec::INK[t]);
        let cells = cells.iter().enumerate().map(|(c, cell_)| {
            let fill = rgb(spec::fill(t, self.hot == Some(c), r));
            let selected = (r, c) == (0, 0);
            cell(
                &cell_.text,
                12.0,
                ink,
                spec::CELL_W,
                spec::CELL_H,
                fill,
                selected.then(|| rgb(spec::SELECT)),
            )
        });
        row![gutter(Some(r + 1), spec::CELL_H, t), row(cells)]
            .height(spec::CELL_H)
            .into()
    }

    /// The footer: the gutter's sigma and each column's total.
    fn footer(&self) -> Element<'_, Message> {
        let t = self.theme;
        let chrome = rgb(spec::CHROME[t]);
        let totals = self.totals.iter().map(|total| {
            cell(
                spec::text(*total),
                12.0,
                rgb(spec::INK[t]),
                spec::CELL_W,
                spec::FOOTER_H,
                chrome,
                None,
            )
        });
        container(
            row![
                container(text("\u{3a3}").size(11).color(rgb(spec::GUTTER_INK)))
                    .width(spec::GUTTER_W)
                    .height(spec::FOOTER_H)
                    .padding([0.0, spec::TEXT_PAD])
                    .align_y(Vertical::Center),
                row(totals),
            ],
        )
        .width(Length::Fill)
        .height(spec::FOOTER_H)
        .style(chrome_style(chrome))
        .into()
    }
}

/// A container style: one solid fill.
fn chrome_style(color: iced::Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::background(color)
}

/// The gutter cell of a row: its number in `GUTTER_INK`, on `CHROME`.
fn gutter<'a>(number: Option<usize>, height: f32, t: usize) -> Element<'a, Message> {
    let label = match number {
        Some(n) => text(n),
        None => text(""),
    };
    container(label.size(11).color(rgb(spec::GUTTER_INK)))
        .width(spec::GUTTER_W)
        .height(height)
        .padding([0.0, spec::TEXT_PAD])
        .align_y(Vertical::Center)
        .style(chrome_style(rgb(spec::CHROME[t])))
        .into()
}

/// A cell: its text at `TEXT_PAD` from the left, vertically centred, on `fill`, with a `SELECT` border when selected.
fn cell<'a, Message: 'a>(
    label: impl iced::widget::text::IntoFragment<'a>,
    size: f32,
    ink: iced::Color,
    width: f32,
    height: f32,
    fill: iced::Color,
    select: Option<iced::Color>,
) -> Element<'a, Message> {
    container(text(label).size(size).color(ink))
        .width(width)
        .height(height)
        .padding([0.0, spec::TEXT_PAD])
        .align_y(Vertical::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(fill)),
            border: match select {
                Some(color) => Border {
                    color,
                    width: spec::SELECT_BORDER,
                    radius: 0.0.into(),
                },
                None => Border::default(),
            },
            ..container::Style::default()
        })
        .into()
}
