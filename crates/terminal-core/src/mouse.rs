use crate::input::Modifiers;
use alacritty_terminal::term::TermMode;

/// Xterm button codes: 0/1/2 buttons, +32 motion, 64/65 wheel.
pub fn encode_mouse(
    button: u8,
    column: usize,
    row: usize,
    pressed: bool,
    modifiers: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let modifiers =
        u8::from(modifiers.shift) * 4 + u8::from(modifiers.alt) * 8 + u8::from(modifiers.ctrl) * 16;
    if mode.contains(TermMode::SGR_MOUSE) {
        return Some(
            format!(
                "\x1b[<{};{};{}{}",
                button + modifiers,
                column.checked_add(1)?,
                row.checked_add(1)?,
                if pressed { 'M' } else { 'm' }
            )
            .into_bytes(),
        );
    }
    let code = 32
        + if pressed {
            button + modifiers
        } else {
            3 + modifiers
        };
    if mode.contains(TermMode::UTF8_MOUSE) && column < 2015 && row < 2015 {
        let mut result = b"\x1b[M".to_vec();
        for value in [code as u32, column as u32 + 33, row as u32 + 33] {
            let mut buffer = [0; 4];
            result.extend_from_slice(char::from_u32(value)?.encode_utf8(&mut buffer).as_bytes());
        }
        Some(result)
    } else if !mode.contains(TermMode::UTF8_MOUSE) && column < 223 && row < 223 {
        Some(vec![
            27,
            b'[',
            b'M',
            code,
            (column + 33) as u8,
            (row + 33) as u8,
        ])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_tracks_the_released_button_and_legacy_uses_release_code() {
        let mods = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            encode_mouse(2, 4, 8, false, mods, TermMode::SGR_MOUSE).unwrap(),
            b"\x1b[<18;5;9m"
        );
        assert_eq!(
            encode_mouse(2, 4, 8, false, mods, TermMode::empty()).unwrap(),
            [27, b'[', b'M', 51, 37, 41]
        );
    }

    #[test]
    fn utf8_mouse_supports_large_coordinates_without_invalid_utf8() {
        let report = encode_mouse(
            64,
            1000,
            999,
            true,
            Modifiers::default(),
            TermMode::UTF8_MOUSE,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(report).unwrap(),
            format!(
                "\x1b[M`{}{}",
                char::from_u32(1033).unwrap(),
                char::from_u32(1032).unwrap()
            )
        );
        assert!(encode_mouse(0, 223, 0, true, Modifiers::default(), TermMode::empty()).is_none());
        assert!(
            encode_mouse(0, 2015, 0, true, Modifiers::default(), TermMode::UTF8_MOUSE).is_none()
        );
    }
}
