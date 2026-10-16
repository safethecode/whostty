use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    NewTab,
    ClosePane,
    SplitRight,
    SplitDown,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    NextTab,
    PreviousTab,
    Find,
    FindNext,
    FindPrevious,
    Preferences,
    IncreaseFont,
    DecreaseFont,
    ResetFont,
    Copy,
    Paste,
    ScrollPageUp,
    ScrollPageDown,
    ScrollTop,
    ScrollBottom,
    Unbind,
}

impl Action {
    pub fn repeatable(self) -> bool {
        !matches!(
            self,
            Self::NewTab
                | Self::ClosePane
                | Self::SplitRight
                | Self::SplitDown
                | Self::Preferences
                | Self::Paste
        )
    }

    pub fn terminal_only(self) -> bool {
        matches!(
            self,
            Self::Copy
                | Self::Paste
                | Self::ScrollPageUp
                | Self::ScrollPageDown
                | Self::ScrollTop
                | Self::ScrollBottom
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub key: String,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Chord {
    key: String,
    ctrl: bool,
    alt: bool,
    shift: bool,
}

impl Chord {
    fn parse(value: &str) -> Result<Self> {
        let mut parts: Vec<_> = value.split('+').map(str::trim).collect();
        let key = parts.pop().unwrap_or_default();
        let key = match key.to_ascii_lowercase().as_str() {
            "arrowleft" | "left" => "Left",
            "arrowright" | "right" => "Right",
            "arrowup" | "up" => "Up",
            "arrowdown" | "down" => "Down",
            "esc" | "escape" => "Escape",
            "return" | "enter" => "Enter",
            "tab" => "Tab",
            "space" => "Space",
            "backspace" => "Backspace",
            "insert" => "Insert",
            "delete" => "Delete",
            "home" => "Home",
            "end" => "End",
            "pageup" => "PageUp",
            "pagedown" => "PageDown",
            "plus" => "Plus",
            "equals" | "equal" => "Equals",
            "minus" => "Minus",
            "comma" => "Comma",
            "period" => "Period",
            "slash" => "Slash",
            "backslash" => "Backslash",
            "semicolon" => "Semicolon",
            "quote" => "Quote",
            "backtick" => "Backtick",
            "openbracket" => "OpenBracket",
            "closebracket" => "CloseBracket",
            _ if key.len() == 1 && key.bytes().all(|byte| byte.is_ascii_alphanumeric()) => key,
            _ if key
                .to_ascii_uppercase()
                .strip_prefix('F')
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (1..=12).contains(&n) && key.len() == 1 + n.to_string().len()) =>
            {
                key
            }
            _ => bail!("Unknown shortcut key '{key}'; use key names such as T, F3, PageUp or Plus"),
        }
        .to_ascii_uppercase();
        let mut chord = Self {
            key,
            ctrl: false,
            alt: false,
            shift: false,
        };
        for modifier in parts {
            let flag = match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => &mut chord.ctrl,
                "alt" => &mut chord.alt,
                "shift" => &mut chord.shift,
                _ => bail!("Unknown shortcut modifier '{modifier}'; use Ctrl, Alt or Shift"),
            };
            if *flag {
                bail!("Repeated shortcut modifier '{modifier}'");
            }
            *flag = true;
        }
        Ok(chord)
    }

    fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

#[derive(Clone, Debug)]
pub struct Shortcuts(Vec<(Chord, Action)>);

impl Shortcuts {
    pub fn new(overrides: &[Binding]) -> Result<Self> {
        if overrides.len() > 128 {
            bail!("keybindings must contain at most 128 entries");
        }
        let mut shortcuts = Vec::new();
        for (key, action) in DEFAULTS {
            shortcuts.push((
                Chord::parse(key).expect("Built-in shortcut must be valid"),
                *action,
            ));
        }
        let mut seen = HashSet::new();
        for binding in overrides {
            let chord = Chord::parse(&binding.key)
                .with_context(|| format!("Invalid keybinding '{}'", binding.key))?;
            if !seen.insert(chord.clone()) {
                bail!("Duplicate keybinding '{}'", binding.key);
            }
            shortcuts.retain(|(existing, _)| existing != &chord);
            shortcuts.push((chord, binding.action));
        }
        Ok(Self(shortcuts))
    }

    pub fn action(&self, key: &str, ctrl: bool, alt: bool, shift: bool) -> Option<Action> {
        self.0.iter().find_map(|(chord, action)| {
            (chord.key.eq_ignore_ascii_case(key)
                && chord.ctrl == ctrl
                && chord.alt == alt
                && chord.shift == shift
                && *action != Action::Unbind)
                .then_some(*action)
        })
    }

    pub fn label(&self, action: Action) -> String {
        self.0
            .iter()
            .rev()
            .find(|(_, candidate)| *candidate == action)
            .map(|(chord, _)| chord.label())
            .unwrap_or_else(|| "Unbound".into())
    }
}

const DEFAULTS: &[(&str, Action)] = &[
    ("Ctrl+Shift+T", Action::NewTab),
    ("Ctrl+Shift+W", Action::ClosePane),
    ("Ctrl+Shift+D", Action::SplitRight),
    ("Ctrl+Shift+E", Action::SplitDown),
    ("Ctrl+Alt+Left", Action::FocusLeft),
    ("Ctrl+Alt+Right", Action::FocusRight),
    ("Ctrl+Alt+Up", Action::FocusUp),
    ("Ctrl+Alt+Down", Action::FocusDown),
    ("Ctrl+Tab", Action::NextTab),
    ("Ctrl+Shift+Tab", Action::PreviousTab),
    ("Ctrl+F", Action::Find),
    ("F3", Action::FindNext),
    ("Shift+F3", Action::FindPrevious),
    ("Ctrl+Comma", Action::Preferences),
    ("Ctrl+Equals", Action::IncreaseFont),
    ("Ctrl+Shift+Equals", Action::IncreaseFont),
    ("Ctrl+Plus", Action::IncreaseFont),
    ("Ctrl+Shift+Plus", Action::IncreaseFont),
    ("Ctrl+Minus", Action::DecreaseFont),
    ("Ctrl+0", Action::ResetFont),
    ("Ctrl+Shift+C", Action::Copy),
    ("Shift+PageUp", Action::ScrollPageUp),
    ("Shift+PageDown", Action::ScrollPageDown),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_unbind_exact_chords_and_reject_ambiguous_configuration() {
        let bindings = [
            Binding {
                key: "control+shift+t".into(),
                action: Action::Unbind,
            },
            Binding {
                key: "Alt+N".into(),
                action: Action::NewTab,
            },
        ];
        let shortcuts = Shortcuts::new(&bindings).unwrap();
        assert_eq!(shortcuts.action("T", true, false, true), None);
        assert_eq!(
            shortcuts.action("N", false, true, false),
            Some(Action::NewTab)
        );
        assert_eq!(shortcuts.action("N", false, true, true), None);
        assert_eq!(shortcuts.action("F", true, true, false), None);
        assert_eq!(shortcuts.label(Action::NewTab), "Alt+N");
        for key in [
            "Ctrl++",
            "Super+T",
            "Ctrl+Ctrl+T",
            "Shift+",
            "Ctrl+F13",
            "Ctrl+한",
        ] {
            assert!(
                Shortcuts::new(&[Binding {
                    key: key.into(),
                    action: Action::Find
                }])
                .is_err(),
                "Accepted {key}"
            );
        }
        assert!(Shortcuts::new(&[
            Binding {
                key: "Ctrl+Left".into(),
                action: Action::Find
            },
            Binding {
                key: "Control+ArrowLeft".into(),
                action: Action::Copy
            }
        ])
        .is_err());
    }
}
