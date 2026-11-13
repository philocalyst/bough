//! The part of an xkb text keymap (`XKB_KEYMAP_FORMAT_TEXT_V1`, what `wl_keyboard.keymap` carries) a host needs
//! to turn key codes into characters: the key names of `xkb_keycodes`, and for each key of `xkb_symbols` the
//! symbols of its first group at levels one and two.
//!
//! Level two is chosen by Shift; for a key whose two symbols are the two cases of one letter, by Shift xor
//! Caps Lock. That is the behaviour of the `TWO_LEVEL` and `ALPHABETIC` key types, which cover Latin layouts.
//! Not read: other groups (a second layout), levels three and up (AltGr), the keypad's Num Lock level, compose
//! and dead keys. A key this reader cannot resolve produces no event rather than a wrong one. A full xkb
//! implementation is the platform lane's when it is needed; this one exists so that nothing is `dlopen`ed
//! and the binary can be linked statically.
use crate::event::Sym;

pub struct Keymap {
    /// By xkb key code (evdev code + 8): the symbols at levels one and two.
    keys: Vec<[Option<Sym>; 2]>,
}

impl Keymap {
    pub fn parse(text: &[u8]) -> Keymap {
        let text = core::str::from_utf8(text.split(|&b| b == 0).next().unwrap_or(&[])).unwrap_or("");
        let mut names: Vec<(&str, u32)> = Vec::new();
        let mut keys: Vec<[Option<Sym>; 2]> = Vec::new();
        let mut section = "";
        let mut t = Tok { s: text, at: 0 };
        while let Some(w) = t.next() {
            match w {
                "xkb_keycodes" | "xkb_types" | "xkb_compatibility" | "xkb_compat" | "xkb_symbols" | "xkb_geometry" => section = w,
                _ if section == "xkb_keycodes" && w.starts_with('<') => {
                    // <NAME> = code ;
                    if t.next() == Some("=")
                        && let Some(v) = t.next().and_then(|v| v.parse::<u32>().ok())
                    {
                        names.push((w, v));
                    }
                }
                "alias" if section == "xkb_keycodes" => {
                    // alias <A> = <B> ;
                    if let (Some(a), Some("="), Some(b)) = (t.next(), t.next(), t.next())
                        && let Some(&(_, code)) = names.iter().find(|n| n.0 == b)
                    {
                        names.push((a, code));
                    }
                }
                "key" if section == "xkb_symbols" => {
                    let Some(name) = t.next() else { break };
                    let code = names.iter().find(|n| n.0 == name).map(|n| n.1);
                    // the first bracketed list inside the braces is group one's symbols
                    let mut syms: [Option<Sym>; 2] = [None, None];
                    let (mut depth, mut seen, mut level) = (0i32, false, 0usize);
                    while let Some(w) = t.next() {
                        match w {
                            "{" => depth += 1,
                            "}" => {
                                depth -= 1;
                                if depth <= 0 {
                                    break;
                                }
                            }
                            "[" if !seen && t.prev_is_symbols() => {
                                while let Some(w) = t.next() {
                                    match w {
                                        "]" => break,
                                        "," => {}
                                        s => {
                                            if level < 2 {
                                                syms[level] = sym(s);
                                            }
                                            level += 1;
                                        }
                                    }
                                }
                                seen = true;
                            }
                            _ => {}
                        }
                    }
                    if let Some(code) = code
                        && code < 1024
                    {
                        if keys.len() <= code as usize {
                            keys.resize(code as usize + 1, [None, None]);
                        }
                        keys[code as usize] = syms;
                    }
                }
                _ => {}
            }
        }
        Keymap { keys }
    }

    /// The symbol of xkb key code `code` under Shift and Caps Lock.
    pub fn sym(&self, code: u32, shift: bool, lock: bool) -> Option<Sym> {
        let k = self.keys.get(code as usize)?;
        let alpha = match (k[0], k[1]) {
            (Some(Sym::Char(a)), Some(Sym::Char(b))) => a != b && a.to_uppercase().eq(core::iter::once(b)),
            _ => false,
        };
        let two = if alpha { shift ^ lock } else { shift };
        if two && k[1].is_some() { k[1] } else { k[0] }
    }
    pub fn len(&self) -> usize {
        self.keys.iter().filter(|k| k[0].is_some()).count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

struct Tok<'a> {
    s: &'a str,
    at: usize,
}
impl<'a> Tok<'a> {
    /// The next token: a punctuation character, a quoted string (returned with its quotes), `<NAME>`, or a
    /// run of anything else. Comments (`//`, `#`) are skipped.
    fn next(&mut self) -> Option<&'a str> {
        let b = self.s.as_bytes();
        loop {
            while self.at < b.len() && b[self.at].is_ascii_whitespace() {
                self.at += 1;
            }
            if self.at + 1 < b.len() && b[self.at] == b'/' && b[self.at + 1] == b'/' || self.at < b.len() && b[self.at] == b'#' {
                while self.at < b.len() && b[self.at] != b'\n' {
                    self.at += 1;
                }
                continue;
            }
            break;
        }
        if self.at >= b.len() {
            return None;
        }
        let a = self.at;
        match b[a] {
            b'{' | b'}' | b'[' | b']' | b',' | b';' | b'=' | b'(' | b')' => self.at += 1,
            b'"' => {
                self.at += 1;
                while self.at < b.len() && b[self.at] != b'"' {
                    self.at += 1;
                }
                self.at = (self.at + 1).min(b.len());
            }
            _ => {
                while self.at < b.len() && !b[self.at].is_ascii_whitespace() && !matches!(b[self.at], b'{' | b'}' | b'[' | b']' | b',' | b';' | b'=' | b'(' | b')' | b'"') {
                    self.at += 1;
                }
            }
        }
        Some(&self.s[a..self.at])
    }
    /// Whether the `[` just read opens a list of symbols: it follows `{`, or `symbols[..] =`; not `actions[..] =`,
    /// and not the `[Group1]` of an index.
    fn prev_is_symbols(&self) -> bool {
        let before = self.s[..self.at - 1].trim_end();
        if before.ends_with('{') || before.ends_with(',') {
            return true;
        }
        if let Some(b) = before.strip_suffix('=') {
            let b = b.trim_end();
            // `symbols[Group1]` or `symbols`
            let b = b.strip_suffix(']').and_then(|b| b.rfind('[').map(|i| b[..i].trim_end())).unwrap_or(b);
            return b.ends_with("symbols");
        }
        false
    }
}

/// A keysym name as a symbol.
fn sym(name: &str) -> Option<Sym> {
    let mut cs = name.chars();
    if let (Some(c), None) = (cs.next(), cs.next()) {
        return Some(Sym::Char(c));
    }
    if let Some(n) = name.strip_prefix('F')
        && let Ok(n) = n.parse::<u8>()
    {
        return (1..=35).contains(&n).then_some(Sym::F(n));
    }
    if let Some(h) = name.strip_prefix("U+").or_else(|| name.strip_prefix('U').filter(|h| h.len() >= 4 && h.bytes().all(|b| b.is_ascii_hexdigit()))) {
        return u32::from_str_radix(h, 16).ok().and_then(char::from_u32).map(Sym::Char);
    }
    if let Some(h) = name.strip_prefix("0x") {
        // a keymap may be serialised with keysyms as numbers (fakeland's is)
        return keysym(u32::from_str_radix(h, 16).ok()?);
    }
    Some(match name {
        "BackSpace" => Sym::Backspace,
        "Tab" | "ISO_Left_Tab" => Sym::Tab,
        "Return" | "KP_Enter" => Sym::Enter,
        "Escape" => Sym::Escape,
        "Delete" => Sym::Delete,
        "Left" => Sym::Left,
        "Right" => Sym::Right,
        "Up" => Sym::Up,
        "Down" => Sym::Down,
        "Home" => Sym::Home,
        "End" => Sym::End,
        "Prior" => Sym::PageUp,
        "Next" => Sym::PageDown,
        _ => Sym::Char(match name {
            "space" => ' ',
            "exclam" => '!',
            "quotedbl" => '"',
            "numbersign" => '#',
            "dollar" => '$',
            "percent" => '%',
            "ampersand" => '&',
            "apostrophe" | "quoteright" => '\'',
            "parenleft" => '(',
            "parenright" => ')',
            "asterisk" => '*',
            "plus" => '+',
            "comma" => ',',
            "minus" => '-',
            "period" => '.',
            "slash" => '/',
            "colon" => ':',
            "semicolon" => ';',
            "less" => '<',
            "equal" => '=',
            "greater" => '>',
            "question" => '?',
            "at" => '@',
            "bracketleft" => '[',
            "backslash" => '\\',
            "bracketright" => ']',
            "asciicircum" => '^',
            "underscore" => '_',
            "grave" | "quoteleft" => '`',
            "braceleft" => '{',
            "bar" => '|',
            "braceright" => '}',
            "asciitilde" => '~',
            "nobreakspace" => '\u{a0}',
            "sterling" => '£',
            "section" => '§',
            "degree" => '°',
            "mu" => 'µ',
            "periodcentered" => '·',
            "EuroSign" => '€',
            "adiaeresis" => 'ä',
            "Adiaeresis" => 'Ä',
            "odiaeresis" => 'ö',
            "Odiaeresis" => 'Ö',
            "udiaeresis" => 'ü',
            "Udiaeresis" => 'Ü',
            "ssharp" => 'ß',
            "eacute" => 'é',
            "Eacute" => 'É',
            "egrave" => 'è',
            "agrave" => 'à',
            "ccedilla" => 'ç',
            "ntilde" => 'ñ',
            "aring" => 'å',
            "Aring" => 'Å',
            "oslash" => 'ø',
            "ae" => 'æ',
            _ => return None,
        }),
    })
}

/// A keysym by number.
fn keysym(v: u32) -> Option<Sym> {
    Some(match v {
        0xff08 => Sym::Backspace,
        0xff09 | 0xfe20 => Sym::Tab,
        0xff0d | 0xff8d => Sym::Enter,
        0xff1b => Sym::Escape,
        0xffff => Sym::Delete,
        0xff50 => Sym::Home,
        0xff51 => Sym::Left,
        0xff52 => Sym::Up,
        0xff53 => Sym::Right,
        0xff54 => Sym::Down,
        0xff55 => Sym::PageUp,
        0xff56 => Sym::PageDown,
        0xff57 => Sym::End,
        0xffbe..=0xffe0 => Sym::F((v - 0xffbe + 1) as u8),
        // Latin-1 keysyms are their code points; Unicode keysyms are 0x01000000 + the code point
        0x20..=0x7e | 0xa0..=0xff => Sym::Char(char::from_u32(v)?),
        0x0100_0000..=0x0110_ffff => Sym::Char(char::from_u32(v - 0x0100_0000)?),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const MAP: &str = r#"xkb_keymap {
xkb_keycodes "(unnamed)" {
	minimum = 8;
	maximum = 708;
	<ESC>                = 9;
	<AE01>               = 10;
	<AD01>               = 24;
	<AC01>               = 38;
	<BKSP>               = 22;
	<FK01>               = 67;
	<SPCE>               = 65;
	<LFSH>               = 50;
	<AB10>               = 61;
	<KP7>                = 79;
	<FK03> = 69;
	<TAB> = 23;
	<AB01> = 52;
	<DELE> = 119;
	alias <LatQ>         = <AD01>;
	indicator 1 = "Caps Lock";
};
xkb_types "(unnamed)" {
	virtual_modifiers NumLock,Alt;
	type "TWO_LEVEL" {
		modifiers= Shift;
		map[Shift]= 2;
		level_name[1]= "Base";
	};
};
xkb_compatibility "(unnamed)" {
	interpret Shift_L+AnyOf(all) { action= SetMods(modifiers=Shift,clearLocks); };
};
xkb_symbols "(unnamed)" {
	name[Group1]="English (US)";
	key <ESC>                {	[          Escape ] };
	key <AE01>               {	[               1,          exclam ] };
	key <AD01>               {
		type= "ALPHABETIC",
		symbols[Group1]= [               q,               Q ]
	};
	key <AC01>               {	type= "ALPHABETIC", symbols[Group1]= [ a, A ], actions[Group1]= [ NoAction(), NoAction() ] };
	key <BKSP>               {	[       BackSpace,       BackSpace ] };
	key <FK01>               {
		type= "CTRL+ALT",
		symbols[Group1]= [              F1,              F1,              F1,              F1, XF86Switch_VT_1 ]
	};
	key <SPCE>               {	[           space ] };
	key <LFSH>               {	[         Shift_L ] };
	key <AB10>               {	[           slash,        question ] };
	key <KP7>                {	[         KP_Home,            KP_7 ] };
	key <FK03> {
		type= "CTRL+ALT",
		symbols[1]= [ 0xffc0, 0xffc0, 0xffc0, 0xffc0, 0x1008fe03 ]
	};
	key <TAB> {	[ 0xff09, 0xfe20 ] };
	key <AB01> {	[ 0x7a, 0x5a ] };
	key <DELE> {	[ 0xffff ] };
	modifier_map Shift { <LFSH> };
};
};
"#;
    #[test]
    fn a_us_keymap_resolves() {
        let k = Keymap::parse(MAP.as_bytes());
        assert_eq!(k.sym(24, false, false), Some(Sym::Char('q')));
        assert_eq!(k.sym(24, true, false), Some(Sym::Char('Q')));
        assert_eq!(k.sym(24, false, true), Some(Sym::Char('Q')), "caps lock raises a letter");
        assert_eq!(k.sym(24, true, true), Some(Sym::Char('q')));
        assert_eq!(k.sym(38, false, false), Some(Sym::Char('a')), "symbols before an actions list");
        assert_eq!(k.sym(10, false, true), Some(Sym::Char('1')), "caps lock leaves a digit");
        assert_eq!(k.sym(10, true, false), Some(Sym::Char('!')));
        assert_eq!(k.sym(22, false, false), Some(Sym::Backspace));
        assert_eq!(k.sym(67, false, false), Some(Sym::F(1)));
        assert_eq!(k.sym(65, true, false), Some(Sym::Char(' ')), "one level: shift changes nothing");
        assert_eq!(k.sym(61, true, false), Some(Sym::Char('?')));
        assert_eq!(k.sym(9, false, false), Some(Sym::Escape));
        assert_eq!(k.sym(50, false, false), None, "a modifier key types nothing");
        assert_eq!(k.sym(79, false, false), None, "the keypad's levels are not read");
        assert_eq!(k.sym(999, false, false), None);
        // keysyms written as numbers, and a group index written `[1]`
        assert_eq!(k.sym(69, false, false), Some(Sym::F(3)));
        assert_eq!(k.sym(23, false, false), Some(Sym::Tab));
        assert_eq!(k.sym(52, true, false), Some(Sym::Char('Z')));
        assert_eq!(k.sym(52, false, true), Some(Sym::Char('Z')));
        assert_eq!(k.sym(119, false, false), Some(Sym::Delete));
    }
}
