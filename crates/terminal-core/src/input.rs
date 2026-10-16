#[derive(Clone, Copy, Debug, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum Key {
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,
    Function(u8),
    Character(char),
}

pub fn encode_key(key: Key, mods: Modifiers, application_cursor: bool) -> Option<Vec<u8>> {
    let modifier = 1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
    let sequence = match key {
        Key::Character(c) if mods.ctrl => {
            let c = c.to_ascii_uppercase();
            let byte = match c {
                '@' | ' ' | '2' => 0,
                'A'..='Z' | '['..='_' => c as u8 & 0x1f,
                '?' => 0x7f,
                _ => return None,
            };
            return Some(if mods.alt {
                vec![0x1b, byte]
            } else {
                vec![byte]
            });
        }
        Key::Character(_) => return None,
        Key::Enter => "\r".into(),
        Key::Backspace => if mods.ctrl { "\x08" } else { "\x7f" }.into(),
        Key::Escape => "\x1b".into(),
        Key::Tab => if mods.shift { "\x1b[Z" } else { "\t" }.into(),
        Key::Up | Key::Down | Key::Right | Key::Left | Key::Home | Key::End => {
            let final_char = match key {
                Key::Up => 'A',
                Key::Down => 'B',
                Key::Right => 'C',
                Key::Left => 'D',
                Key::Home => 'H',
                _ => 'F',
            };
            if modifier > 1 {
                format!("\x1b[1;{modifier}{final_char}")
            } else if application_cursor {
                format!("\x1bO{final_char}")
            } else {
                format!("\x1b[{final_char}")
            }
        }
        Key::Function(number @ 1..=4) => {
            let final_char = (b'P' + number - 1) as char;
            if modifier > 1 {
                format!("\x1b[1;{modifier}{final_char}")
            } else {
                format!("\x1bO{final_char}")
            }
        }
        other => {
            let code = match other {
                Key::Insert => 2,
                Key::Delete => 3,
                Key::PageUp => 5,
                Key::PageDown => 6,
                Key::Function(5) => 15,
                Key::Function(6) => 17,
                Key::Function(7) => 18,
                Key::Function(8) => 19,
                Key::Function(9) => 20,
                Key::Function(10) => 21,
                Key::Function(11) => 23,
                Key::Function(12) => 24,
                _ => return None,
            };
            if modifier > 1 {
                format!("\x1b[{code};{modifier}~")
            } else {
                format!("\x1b[{code}~")
            }
        }
    };
    let mut bytes = sequence.into_bytes();
    if mods.alt && matches!(key, Key::Enter | Key::Tab | Key::Backspace) {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    // Strip escape characters so pasted text cannot terminate bracketed paste early.
    let clean = text
        .replace('\x1b', "")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if bracketed {
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        clean.replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_and_tui_key_sequences() {
        assert_eq!(
            encode_key(Key::Up, Modifiers::default(), true).unwrap(),
            b"\x1bOA"
        );
        assert_eq!(
            encode_key(
                Key::Left,
                Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
                false
            )
            .unwrap(),
            b"\x1b[1;5D"
        );
        assert_eq!(
            encode_key(
                Key::Character('c'),
                Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
                false
            )
            .unwrap(),
            [3]
        );
        assert_eq!(
            encode_key(Key::Function(12), Modifiers::default(), false).unwrap(),
            b"\x1b[24~"
        );
    }

    #[test]
    fn paste_normalizes_newlines_and_cannot_inject_escape() {
        assert_eq!(encode_paste("a\r\nb\rc", false), b"a\rb\rc");
        assert_eq!(
            encode_paste("x\x1b[201~", true),
            b"\x1b[200~x[201~\x1b[201~"
        );
    }
}
