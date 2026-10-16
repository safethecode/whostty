use crate::{
    search_worker::{self, Failure, Outcome, Request, SearchJob},
    terminal::Terminal,
};
use alacritty_terminal::{
    event::EventListener,
    grid::Dimensions,
    index::{Boundary, Column, Line, Point},
    term::search::Match,
};
use std::sync::{atomic::AtomicBool, Arc, Mutex};

#[derive(Default)]
pub struct Search {
    pub query: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub error: Option<String>,
    pub wrapped: bool,
    result: Option<Match>,
    revision: u64,
    job: Option<SearchJob>,
}

impl Search {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            ..Self::default()
        }
    }

    pub fn result(&self, revision: u64) -> Option<&Match> {
        (revision == self.revision && !self.busy())
            .then_some(self.result.as_ref())
            .flatten()
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub fn cancel(&mut self) {
        self.job = None;
    }

    pub fn start<T: EventListener + Send + 'static>(
        &mut self,
        terminal: &Arc<Mutex<Terminal<T>>>,
        backwards: bool,
        reset: bool,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) {
        self.cancel();
        let request = {
            let terminal = terminal.lock().unwrap_or_else(|e| e.into_inner());
            self.prepare(&terminal, backwards, reset)
        };
        if let Some(request) = request {
            self.job = Some(SearchJob::spawn(request, terminal, wake));
        }
    }

    /// Poll from the UI; only apply results from the current output/resize revision.
    pub fn poll<T: EventListener>(&mut self, terminal: &mut Terminal<T>) {
        if let Some(outcome) = self.job.as_ref().and_then(SearchJob::poll) {
            self.job = None;
            self.apply(terminal, outcome);
        }
    }

    /// Blocking entry point for tests and performance examples. Desktop uses start/poll.
    pub fn find<T: EventListener>(
        &mut self,
        terminal: &mut Terminal<T>,
        backwards: bool,
        reset: bool,
    ) {
        self.cancel();
        if let Some(request) = self.prepare(terminal, backwards, reset) {
            let outcome = search_worker::run(&request, &AtomicBool::new(false), |first, last| {
                Ok(search_worker::copy_rows(terminal, first, last))
            });
            self.apply(terminal, outcome);
        }
    }

    fn prepare<T: EventListener>(
        &mut self,
        terminal: &Terminal<T>,
        backwards: bool,
        reset: bool,
    ) -> Option<Request> {
        self.error = None;
        self.wrapped = false;
        if self.query.is_empty() {
            self.result = None;
            self.revision = terminal.revision();
            return None;
        }
        if self.query.chars().count() > 256 {
            self.result = None;
            self.error = Some("Use at most 256 characters.".into());
            return None;
        }
        let text = if self.regex {
            self.query.clone()
        } else {
            regex::escape(&self.query)
        };
        let pattern = format!("(?{}){text}", if self.case_sensitive { "-i" } else { "i" });
        let term = &terminal.term;
        let origin = if let Some(current) = self
            .result
            .as_ref()
            .filter(|_| !reset && self.revision == terminal.revision())
        {
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
        self.result = None;
        self.revision = terminal.revision();
        Some(Request {
            pattern,
            origin,
            backwards,
            revision: terminal.revision(),
            top: -(term.grid().history_size() as i32),
            bottom: term.screen_lines() as i32,
            debounce: reset,
        })
    }

    fn apply<T: EventListener>(&mut self, terminal: &mut Terminal<T>, outcome: Outcome) {
        self.result = None;
        self.revision = outcome.revision;
        self.wrapped = false;
        if terminal.revision() != outcome.revision {
            return;
        }
        match outcome.result {
            Ok(result) => {
                self.result = result;
                self.wrapped = outcome.wrapped;
                if let Some(found) = &self.result {
                    terminal.term.scroll_to_point(*found.start());
                }
            }
            Err(Failure::Stale | Failure::Canceled) => {}
            Err(Failure::Closed) => self.error = Some("The terminal closed during search.".into()),
            Err(Failure::InvalidRegex) => {
                self.error = Some("Invalid or overly complex regular expression.".into())
            }
            Err(Failure::LongLine) => {
                self.error = Some("A wrapped line exceeds the 256 KiB search limit.".into())
            }
        }
    }

    pub fn stale(&self, revision: u64) -> bool {
        !self.query.is_empty() && !self.busy() && self.revision != revision
    }
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

    #[test]
    fn combining_text_and_matches_across_snapshot_batches_keep_cell_coordinates() {
        let mut terminal = Terminal::new(Size::new(10, 3), 100, VoidListener);
        terminal.feed(format!("{}bridge한e\u{301}Z", "x".repeat(155)).as_bytes());
        let mut search = Search::new("xxxbridge한e\u{301}");
        search.find(&mut terminal, false, true);
        let found = search.result(terminal.revision()).unwrap();
        assert_eq!(
            terminal.term.bounds_to_string(*found.start(), *found.end()),
            "xxxbridge한e\u{301}"
        );
        search.query = "e\u{301}".into();
        search.find(&mut terminal, true, true);
        assert!(search.result(terminal.revision()).is_some());
    }

    #[test]
    fn asynchronous_search_discards_old_revisions_and_only_applies_the_latest_request() {
        use std::time::{Duration, Instant};
        let terminal = Arc::new(Mutex::new(Terminal::new(
            Size::new(80, 5),
            5000,
            VoidListener,
        )));
        terminal
            .lock()
            .unwrap()
            .feed("old line\r\n".repeat(2000).as_bytes());
        let mut search = Search::new("old");
        search.start(&terminal, false, true, Arc::new(|| {}));
        terminal.lock().unwrap().feed(b"new-output");
        let deadline = Instant::now() + Duration::from_secs(15);
        while search.busy() {
            search.poll(&mut terminal.lock().unwrap());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(search.result(terminal.lock().unwrap().revision()).is_none());
        assert!(search.stale(terminal.lock().unwrap().revision()));
        search.query = "absent".into();
        search.start(&terminal, false, true, Arc::new(|| {}));
        search.query = "new-output".into();
        search.start(&terminal, false, true, Arc::new(|| {}));
        while search.busy() {
            search.poll(&mut terminal.lock().unwrap());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let terminal = terminal.lock().unwrap();
        let found = search.result(terminal.revision()).unwrap();
        assert_eq!(
            terminal.term.bounds_to_string(*found.start(), *found.end()),
            "new-output"
        );
    }

    #[test]
    fn canceling_search_does_not_keep_a_closed_terminal_alive() {
        use std::time::{Duration, Instant};
        let terminal = Arc::new(Mutex::new(Terminal::new(
            Size::new(80, 5),
            5000,
            VoidListener,
        )));
        terminal
            .lock()
            .unwrap()
            .feed("some line\r\n".repeat(2000).as_bytes());
        let weak = Arc::downgrade(&terminal);
        let mut search = Search::new("absent");
        search.start(&terminal, false, true, Arc::new(|| {}));
        search.cancel();
        drop(terminal);
        let deadline = Instant::now() + Duration::from_secs(15);
        while weak.upgrade().is_some() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!search.busy());
    }

    #[test]
    fn oversized_wrapped_lines_report_a_bounded_search_limit() {
        let mut terminal = Terminal::new(Size::new(1000, 1), 1000, VoidListener);
        terminal.feed("x".repeat(270_000).as_bytes());
        let mut search = Search::new("absent");
        search.find(&mut terminal, false, true);
        assert!(search.error.as_deref().unwrap().contains("256 KiB"));
    }
}
