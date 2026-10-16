use crate::terminal::Terminal;
use alacritty_terminal::{
    event::EventListener,
    grid::Dimensions,
    index::{Boundary, Column, Direction, Line, Point, Side},
    term::search::{Match, RegexSearch},
};

#[derive(Default)]
pub struct Search {
    pub query: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub error: Option<String>,
    pub wrapped: bool,
    result: Option<Match>,
    revision: u64,
    expression: Option<(String, RegexSearch)>,
}

impl Search {
    pub fn result(&self, revision: u64) -> Option<&Match> {
        (revision == self.revision)
            .then_some(self.result.as_ref())
            .flatten()
    }

    pub fn find<T: EventListener>(
        &mut self,
        terminal: &mut Terminal<T>,
        backwards: bool,
        reset: bool,
    ) {
        self.error = None;
        self.wrapped = false;
        if self.query.is_empty() {
            self.result = None;
            self.expression = None;
            return;
        }
        if self.query.chars().count() > 256 {
            self.result = None;
            self.error = Some("Use at most 256 characters.".into());
            return;
        }
        let text = if self.regex {
            self.query.clone()
        } else {
            escape_literal(&self.query)
        };
        // Explicit flags override the engine's smart-case default.
        let pattern = format!("(?{}){text}", if self.case_sensitive { "-i" } else { "i" });
        if self
            .expression
            .as_ref()
            .is_none_or(|(previous, _)| previous != &pattern)
        {
            match RegexSearch::new(&pattern) {
                Ok(regex) => self.expression = Some((pattern, regex)),
                Err(_) => {
                    self.expression = None;
                    self.result = None;
                    self.error = Some("Invalid regular expression.".into());
                    return;
                }
            }
        }
        let revision = terminal.revision();
        let term = &mut terminal.term;
        let current = if reset || self.revision != revision {
            None
        } else {
            self.result.as_ref()
        };
        let origin = if let Some(current) = current {
            if backwards {
                current.start().sub(term, Boundary::None, 1)
            } else {
                current.end().add(term, Boundary::None, 1)
            }
        } else if backwards {
            Point::new(
                Line(-(term.grid().display_offset() as i32) + term.screen_lines() as i32 - 1),
                term.last_column(),
            )
        } else {
            Point::new(Line(-(term.grid().display_offset() as i32)), Column(0))
        };
        let direction = if backwards {
            Direction::Left
        } else {
            Direction::Right
        };
        let (_, expression) = self.expression.as_mut().unwrap();
        self.result = term.search_next(
            expression,
            origin,
            direction,
            if backwards { Side::Right } else { Side::Left },
            None,
        );
        if let Some(result) = &self.result {
            self.wrapped = if backwards {
                *result.end() > origin
            } else {
                *result.start() < origin
            };
            term.scroll_to_point(*result.start());
        }
        self.revision = revision;
    }

    pub fn stale(&self, revision: u64) -> bool {
        !self.query.is_empty() && self.revision != revision
    }
}

fn escape_literal(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if ".+*?()|[]{}^$\\".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::Size;
    use alacritty_terminal::event::VoidListener;

    #[test]
    fn search_traverses_scrollback_and_wraps_in_both_directions() {
        let mut terminal = Terminal::new(Size::new(20, 2), 20, VoidListener);
        terminal.feed(b"first target\r\nsecond\r\nlast TARGET");
        let mut search = Search {
            query: "target".into(),
            ..Default::default()
        };
        search.find(&mut terminal, false, true);
        let last = search.result(terminal.revision()).unwrap().clone();
        search.find(&mut terminal, false, false);
        assert!(search.wrapped);
        let first = search.result(terminal.revision()).unwrap().clone();
        assert!(first.start() < last.start());
        assert!(terminal.term.grid().display_offset() > 0);
        search.find(&mut terminal, true, false);
        assert!(search.wrapped);
        assert_eq!(search.result(terminal.revision()), Some(&last));
    }

    #[test]
    fn literal_regex_unicode_and_stale_matches_are_handled() {
        let mut terminal = Terminal::new(Size::new(12, 4), 20, VoidListener);
        terminal.feed("abc 한글 e\u{301}\r\na.b aaab".as_bytes());
        let mut search = Search {
            query: "한글".into(),
            ..Default::default()
        };
        search.find(&mut terminal, false, true);
        let found = search.result(terminal.revision()).unwrap();
        assert_eq!(found.end().column.0 - found.start().column.0 + 1, 4);
        search.query = "a.b".into();
        search.find(&mut terminal, false, true);
        assert_eq!(
            search.result(terminal.revision()).unwrap().start().line,
            Line(1)
        );
        search.query = "[".into();
        search.regex = true;
        search.find(&mut terminal, false, true);
        assert!(search.error.is_some());
        assert!(search.result(terminal.revision()).is_none());
        search.query = "aa.b".into();
        search.find(&mut terminal, false, true);
        assert!(search.result(terminal.revision()).is_some());
        terminal.feed(b"new output");
        assert!(search.result(terminal.revision()).is_none());
    }
}
