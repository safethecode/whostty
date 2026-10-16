use alacritty_terminal::{
    term::color::{Colors, COUNT},
    vte::ansi::{Color, NamedColor, Rgb},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HexColor(pub [u8; 3]);

impl TryFrom<String> for HexColor {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let text = value.strip_prefix('#').unwrap_or(&value);
        if text.len() != 6 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Colors must use #RRGGBB".into());
        }
        Ok(Self(std::array::from_fn(|index| {
            u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap()
        })))
    }
}

impl From<HexColor> for String {
    fn from(value: HexColor) -> Self {
        format!("#{:02x}{:02x}{:02x}", value.0[0], value.0[1], value.0[2])
    }
}

impl From<HexColor> for Rgb {
    fn from(value: HexColor) -> Self {
        Rgb {
            r: value.0[0],
            g: value.0[1],
            b: value.0[2],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    #[default]
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Theme {
    Preset(Preset),
    Custom(Palette),
}

impl Default for Theme {
    fn default() -> Self {
        Self::Preset(Preset::Dark)
    }
}

impl Theme {
    pub fn colors(&self) -> Palette {
        match self {
            Self::Preset(Preset::Dark) => Palette::default(),
            Self::Custom(palette) => *palette,
            Self::Preset(Preset::Light) => Palette {
                background: HexColor([247, 249, 252]),
                foreground: HexColor([28, 37, 51]),
                cursor: HexColor([31, 100, 68]),
                selection: HexColor([191, 219, 204]),
                search: HexColor([247, 214, 143]),
                ansi: [
                    [28, 37, 51],
                    [171, 42, 53],
                    [28, 107, 62],
                    [133, 90, 12],
                    [34, 85, 163],
                    [115, 59, 154],
                    [16, 107, 120],
                    [191, 200, 211],
                    [91, 105, 125],
                    [201, 57, 69],
                    [38, 129, 78],
                    [157, 109, 20],
                    [45, 109, 196],
                    [144, 78, 184],
                    [18, 130, 144],
                    [255, 255, 255],
                ]
                .map(HexColor),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Palette {
    pub background: HexColor,
    pub foreground: HexColor,
    pub cursor: HexColor,
    pub selection: HexColor,
    pub search: HexColor,
    pub ansi: [HexColor; 16],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            background: HexColor([19, 23, 28]),
            foreground: HexColor([219, 224, 231]),
            cursor: HexColor([151, 219, 174]),
            selection: HexColor([51, 77, 70]),
            search: HexColor([103, 78, 35]),
            ansi: [
                [35, 40, 48],
                [232, 122, 130],
                [151, 219, 174],
                [231, 199, 133],
                [133, 178, 231],
                [189, 160, 225],
                [128, 206, 211],
                [219, 224, 231],
                [104, 116, 131],
                [247, 148, 154],
                [176, 234, 192],
                [246, 217, 159],
                [163, 199, 244],
                [209, 184, 241],
                [158, 226, 230],
                [247, 249, 252],
            ]
            .map(HexColor),
        }
    }
}

impl Palette {
    pub fn resolve(&self, color: Color, overrides: &Colors) -> Rgb {
        match color {
            Color::Spec(rgb) => rgb,
            Color::Named(named) => self.indexed(named as usize, overrides),
            Color::Indexed(index) => self.indexed(index as usize, overrides),
        }
    }

    pub fn indexed(&self, index: usize, overrides: &Colors) -> Rgb {
        if index < COUNT {
            if let Some(rgb) = overrides[index] {
                return rgb;
            }
        }
        match index {
            0..=15 => self.ansi[index].into(),
            16..=231 => {
                let value = index - 16;
                let channel = |value| {
                    if value == 0 {
                        0
                    } else {
                        (55 + value * 40) as u8
                    }
                };
                Rgb {
                    r: channel(value / 36),
                    g: channel((value / 6) % 6),
                    b: channel(value % 6),
                }
            }
            232..=255 => {
                let gray = (8 + (index - 232) * 10) as u8;
                Rgb {
                    r: gray,
                    g: gray,
                    b: gray,
                }
            }
            i if i == NamedColor::Background as usize => self.background.into(),
            i if i == NamedColor::Cursor as usize => self.cursor.into(),
            259..=266 => dim(self.indexed(index - 259, overrides)),
            i if i == NamedColor::BrightForeground as usize => self.foreground.into(),
            i if i == NamedColor::DimForeground as usize => dim(self.foreground.into()),
            _ => self.foreground.into(),
        }
    }
}

fn dim(rgb: Rgb) -> Rgb {
    Rgb {
        r: (rgb.r as f32 * 0.6) as u8,
        g: (rgb.g as f32 * 0.6) as u8,
        b: (rgb.b as f32 * 0.6) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    #[test]
    fn presets_and_custom_colors_round_trip_and_invalid_colors_are_rejected() {
        let settings: Settings = toml::from_str("theme = 'light'").unwrap();
        assert_eq!(
            settings.theme.colors().background,
            HexColor([247, 249, 252])
        );
        let settings: Settings =
            toml::from_str("[theme]\nbackground = '#123456'\nforeground = 'aBcDeF'").unwrap();
        assert_eq!(
            settings.theme.colors().foreground,
            HexColor([171, 205, 239])
        );
        let encoded = toml::to_string(&settings).unwrap();
        assert_eq!(
            toml::from_str::<Settings>(&encoded).unwrap().theme,
            settings.theme
        );
        for text in [
            "theme='unknown'",
            "[theme]\nbackground='#fff'",
            "[theme]\nforeground='한글'",
            "[theme]\nansi=['#ffffff']",
            "[theme]\nforeground='transparent'",
        ] {
            assert!(toml::from_str::<Settings>(text).is_err(), "Accepted {text}");
        }
    }

    #[test]
    fn osc_overrides_truecolor_and_indexed_cube_have_consistent_precedence() {
        let palette = Theme::Preset(Preset::Light).colors();
        let mut overrides = Colors::default();
        let custom = Rgb { r: 1, g: 2, b: 3 };
        overrides[1] = Some(custom);
        overrides[NamedColor::Background] = Some(custom);
        assert_eq!(
            palette.resolve(Color::Named(NamedColor::Red), &overrides),
            custom
        );
        assert_eq!(
            palette.indexed(NamedColor::Background as usize, &overrides),
            custom
        );
        assert_eq!(palette.resolve(Color::Spec(custom), &overrides), custom);
        assert_eq!(palette.indexed(16, &overrides), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            palette.indexed(231, &overrides),
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(palette.indexed(232, &overrides), Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(
            palette.indexed(usize::MAX, &overrides),
            palette.foreground.into()
        );
    }

    #[test]
    fn osc_dynamic_colors_reset_to_the_selected_theme() {
        use crate::terminal::{Size, Terminal};
        use alacritty_terminal::event::VoidListener;
        let palette = Theme::Preset(Preset::Light).colors();
        let mut terminal = Terminal::new(Size::new(40, 3), 100, VoidListener);
        terminal.feed(b"\x1b]10;#102030\x07\x1b]11;#405060\x07\x1b]12;#708090\x07");
        assert_eq!(
            palette.indexed(NamedColor::Foreground as usize, terminal.term.colors()),
            Rgb {
                r: 16,
                g: 32,
                b: 48
            }
        );
        assert_eq!(
            palette.indexed(NamedColor::Background as usize, terminal.term.colors()),
            Rgb {
                r: 64,
                g: 80,
                b: 96
            }
        );
        assert_eq!(
            palette.indexed(NamedColor::Cursor as usize, terminal.term.colors()),
            Rgb {
                r: 112,
                g: 128,
                b: 144
            }
        );
        terminal.feed(b"\x1b]110\x07\x1b]111\x07\x1b]112\x07");
        assert_eq!(
            palette.indexed(NamedColor::Foreground as usize, terminal.term.colors()),
            palette.foreground.into()
        );
        assert_eq!(
            palette.indexed(NamedColor::Background as usize, terminal.term.colors()),
            palette.background.into()
        );
        assert_eq!(
            palette.indexed(NamedColor::Cursor as usize, terminal.term.colors()),
            palette.cursor.into()
        );
    }
}
