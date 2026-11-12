//! What a window reports to its host loop, decoded and in arrival order.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

/// A key as the keymap names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sym {
    /// A key that types a character (under the current shift level).
    Char(char),
    Backspace,
    Tab,
    Enter,
    Escape,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    /// F1 is `F(1)`.
    F(u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The pointer is at (x, y) buffer pixels (also on enter).
    Motion { x: f64, y: f64 },
    Leave,
    /// `button`: 0 primary, 1 secondary, 2 middle.
    Button { x: f64, y: f64, button: u8, pressed: bool },
    /// One pointer frame's scroll in buffer pixels; positive `dy` moves the content up.
    Scroll { x: f64, y: f64, dx: f64, dy: f64 },
    Key { sym: Sym, pressed: bool, mods: Mods },
    /// The window gained or lost the keyboard.
    Focus(bool),
    Close,
}

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;
