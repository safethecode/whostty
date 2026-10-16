use alacritty_terminal::{
    event::EventListener,
    grid::Dimensions,
    term::{Config, Term},
    vte::ansi::Processor,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub columns: usize,
    pub rows: usize,
}

impl Size {
    pub fn new(columns: usize, rows: usize) -> Self {
        Self {
            columns: columns.clamp(2, 1000),
            rows: rows.clamp(1, 1000),
        }
    }
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

pub struct Terminal<T: EventListener> {
    pub term: Term<T>,
    parser: Processor,
}

impl<T: EventListener> Terminal<T> {
    pub fn new(size: Size, scrollback: usize, listener: T) -> Self {
        let config = Config {
            scrolling_history: scrollback,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &size, listener),
            parser: Processor::new(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn text(&self) -> String {
        self.term.grid().display_iter().map(|cell| cell.c).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::{
        event::VoidListener,
        index::{Column, Line},
        term::TermMode,
        vte::ansi::{Color, Rgb},
    };

    #[test]
    fn utf8_split_chunks_truecolor_and_alternate_screen() {
        let mut terminal = Terminal::new(Size::new(80, 24), 100, VoidListener);
        let data = "\x1b[38;2;12;34;56m한글".as_bytes();
        for byte in data {
            terminal.feed(&[*byte]);
        }
        assert!(terminal.text().contains('한'));
        assert_eq!(
            terminal.term.grid()[Line(0)][Column(0)].fg,
            Color::Spec(Rgb {
                r: 12,
                g: 34,
                b: 56
            })
        );
        terminal.feed(b"\x1b[?1049hALT");
        assert!(terminal.term.mode().contains(TermMode::ALT_SCREEN));
        terminal.feed(b"\x1b[?1049l");
        assert!(terminal.text().contains('한'));
        assert!(!terminal.term.mode().contains(TermMode::ALT_SCREEN));
    }

    #[test]
    fn resize_and_scrollback_preserve_terminal_state() {
        let mut terminal = Terminal::new(Size::new(20, 3), 10, VoidListener);
        terminal.feed(b"one\r\ntwo\r\nthree\r\nfour");
        assert!(terminal.term.grid().history_size() > 0);
        terminal.term.resize(Size::new(40, 5));
        assert_eq!(terminal.term.columns(), 40);
        assert!(terminal.text().contains("four"));
    }
}
