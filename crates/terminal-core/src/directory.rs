//! Bounded OSC working-directory observation, separate from the terminal's VT engine.
use std::path::{Path, PathBuf};

const LIMIT: usize = 8192;

#[derive(Default)]
enum State {
    #[default]
    Ground,
    Escape,
    Osc,
    OscEscape,
    String,
    StringEscape,
}

#[derive(Default)]
pub struct Directory {
    state: State,
    bytes: Vec<u8>,
    overflow: bool,
    current: Option<PathBuf>,
}

impl Directory {
    pub fn current(&self) -> Option<&Path> {
        self.current.as_deref()
    }

    pub fn feed(&mut self, mut input: &[u8]) {
        while !input.is_empty() {
            if matches!(self.state, State::Ground) {
                let Some(escape) = memchr::memchr(0x1b, input) else {
                    return;
                };
                input = &input[escape..];
            }
            let byte = input[0];
            input = &input[1..];
            self.state = match (&self.state, byte) {
                // CAN and SUB cancel a control string without interpreting its contents.
                (_, 0x18 | 0x1a) => State::Ground,
                (State::Ground, 0x1b) => State::Escape,
                (State::Escape, b']') => {
                    self.bytes.clear();
                    self.overflow = false;
                    State::Osc
                }
                (State::Escape, b'P' | b'X' | b'^' | b'_') => State::String,
                (State::Escape, b'c') => {
                    self.current = None;
                    State::Ground
                }
                (State::Escape, 0x1b) => State::Escape,
                (State::Escape, _) => State::Ground,
                (State::Osc, 0x07) | (State::OscEscape, b'\\') => {
                    self.dispatch();
                    State::Ground
                }
                (State::Osc, 0x1b) => State::OscEscape,
                // An escape other than ST aborts OSC and can start a new escape sequence.
                (State::OscEscape, _) => {
                    self.state = State::Escape;
                    self.feed(&[byte]);
                    std::mem::take(&mut self.state)
                }
                (State::Osc, _) => {
                    if self.bytes.len() < LIMIT {
                        self.bytes.push(byte);
                    } else {
                        self.overflow = true;
                    }
                    State::Osc
                }
                (State::String, 0x1b) | (State::StringEscape, 0x1b) => State::StringEscape,
                (State::StringEscape, b'\\') => State::Ground,
                (State::StringEscape, _) | (State::String, _) => State::String,
                _ => State::Ground,
            };
        }
    }

    fn dispatch(&mut self) {
        if self.overflow {
            return;
        }
        let Ok(text) = std::str::from_utf8(&self.bytes) else {
            return;
        };
        let path = if let Some(uri) = text.strip_prefix("7;") {
            local_file(uri)
        } else if let Some(path) = text.strip_prefix("9;9;") {
            // Windows Terminal's directory sequence may quote the path.
            let path = path
                .strip_prefix('"')
                .and_then(|p| p.strip_suffix('"'))
                .unwrap_or(path);
            local_path(path)
        } else {
            None
        };
        if let Some(path) = path {
            self.current = Some(path);
        }
    }
}

fn local_file(uri: &str) -> Option<PathBuf> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "file"
        || url
            .host_str()
            .is_some_and(|host| !host.eq_ignore_ascii_case("localhost"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let path = url.to_file_path().ok()?;
    local_path(path.to_str()?)
}

fn local_path(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.chars().any(char::is_control) {
        return None;
    }
    // Never turn an untrusted OSC value into a network/device path on pane creation.
    if path.starts_with(['\\', '/']) && path.get(1..).is_some_and(|p| p.starts_with(['\\', '/'])) {
        return None;
    }
    #[cfg(windows)]
    if !matches!(Path::new(path).components().next(), Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
    {
        return None;
    }
    let path = PathBuf::from(path);
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_utf8_and_terminators_preserve_a_local_directory() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("한글 space;%");
        let url = url::Url::from_directory_path(&path).unwrap();
        let mut directory = Directory::default();
        for byte in format!("\x1b]7;{url}\x1b\\").bytes() {
            directory.feed(&[byte]);
        }
        assert_eq!(directory.current(), Some(path.as_path()));
        directory.feed(format!("\x1b]9;9;\"{}\"\x07", temp.path().display()).as_bytes());
        assert_eq!(directory.current(), Some(temp.path()));
        directory.feed(b"\x1bc");
        assert_eq!(directory.current(), None);
    }

    #[test]
    fn untrusted_and_unterminated_strings_do_not_change_the_directory() {
        let temp = tempfile::tempdir().unwrap();
        let url = url::Url::from_directory_path(temp.path()).unwrap();
        let osc = format!("\x1b]7;{url}\x07");
        let mut directory = Directory::default();
        for prefix in ["\x1bP", "\x1bX", "\x1b^", "\x1b_"] {
            directory.feed(format!("{prefix}{osc}\x1b\\").as_bytes());
            assert_eq!(directory.current(), None);
        }
        directory.feed(format!("\x1b]7;{url}\x18").as_bytes());
        directory.feed(format!("\x1b]7;{url}\x1b[A").as_bytes());
        assert_eq!(directory.current(), None);
        let oversized = format!("\x1b]7;{url}{}\x07", "x".repeat(LIMIT));
        directory.feed(oversized.as_bytes());
        assert!(directory.bytes.len() <= LIMIT);
        assert_eq!(directory.current(), None);
        directory.feed(osc.as_bytes());
        for bad in [
            "file://remote-host/share",
            "https://example.com/",
            "file:///bad%00path",
            "file:///bad%0apath",
            "file:///ok?query",
            "file:///ok#fragment",
            "not a URI",
        ] {
            directory.feed(format!("\x1b]7;{bad}\x07").as_bytes());
            assert_eq!(directory.current(), Some(temp.path()));
        }
        directory.feed(b"\x1b]9;9;relative\x07\x1b]9;9;\\\\server\\share\x07");
        assert_eq!(directory.current(), Some(temp.path()));
    }
}
