use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line, Point},
    term::{cell::Flags, Term},
};
use std::ops::RangeInclusive;

#[derive(Clone, Debug)]
pub struct Link {
    pub uri: String,
    pub range: RangeInclusive<Point>,
}

const LIMIT: usize = 2048;

fn web_uri(text: &str) -> Option<String> {
    if text.len() > 8192 || text.chars().any(char::is_control) {
        return None;
    }
    let url = url::Url::parse(text).ok()?;
    (matches!(url.scheme(), "https" | "http") && url.host_str().is_some()).then(|| url.into())
}

fn previous<T>(term: &Term<T>, point: Point) -> Option<Point> {
    if point.column.0 > 0 {
        return Some(Point::new(point.line, point.column - 1));
    }
    let line: Line = point.line - 1;
    (line >= term.topmost_line()
        && term.grid()[line][term.last_column()]
            .flags
            .contains(Flags::WRAPLINE))
    .then(|| Point::new(line, term.last_column()))
}

fn next<T>(term: &Term<T>, point: Point) -> Option<Point> {
    if point.column < term.last_column() {
        return Some(Point::new(point.line, point.column + 1));
    }
    (point.line < term.bottommost_line() && term.grid()[point].flags.contains(Flags::WRAPLINE))
        .then(|| Point::new(point.line + 1, Column(0)))
}

pub fn at<T>(term: &Term<T>, point: Point) -> Option<Link> {
    if point.line < term.topmost_line()
        || point.line > term.bottommost_line()
        || point.column >= term.columns()
    {
        return None;
    }
    let cell = &term.grid()[point];
    if cell.flags.contains(Flags::HIDDEN) {
        return None;
    }
    if let Some(link) = cell.hyperlink() {
        let uri = web_uri(link.uri())?;
        let same = |point: Point| {
            term.grid()[point]
                .hyperlink()
                .is_some_and(|other| other.id() == link.id() && other.uri() == link.uri())
        };
        let mut start = point;
        let mut end = point;
        for _ in 0..LIMIT {
            if let Some(p) = previous(term, start).filter(|p| same(*p)) {
                start = p;
            } else {
                break;
            }
        }
        for _ in 0..LIMIT {
            if let Some(p) = next(term, end).filter(|p| same(*p)) {
                end = p;
            } else {
                break;
            }
        }
        return Some(Link {
            uri,
            range: start..=end,
        });
    }
    let token = |point: Point| {
        let cell = &term.grid()[point];
        (cell.flags.contains(Flags::WIDE_CHAR_SPACER) || !cell.c.is_whitespace())
            && !cell.flags.contains(Flags::HIDDEN)
            && !matches!(cell.c, '<' | '>' | '"' | '\'')
    };
    if !token(point) {
        return None;
    }
    let mut start = point;
    let mut count = 0;
    while let Some(p) = previous(term, start).filter(|p| token(*p)) {
        count += 1;
        if count >= LIMIT {
            return None;
        }
        start = p;
    }
    let mut text = String::new();
    let mut points = Vec::new();
    let mut cursor = Some(start);
    for _ in 0..LIMIT {
        let Some(p) = cursor else {
            break;
        };
        let cell = &term.grid()[p];
        if !token(p) {
            break;
        }
        if !cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            points.push((text.len(), p));
            text.push(cell.c);
            if let Some(extra) = cell.zerowidth() {
                for c in extra {
                    text.push(*c);
                    if text.len() > 8192 {
                        return None;
                    }
                }
            }
        }
        cursor = next(term, p);
    }
    if cursor.is_some_and(token) {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let from = [lower.find("https://"), lower.find("http://")]
        .into_iter()
        .flatten()
        .min()?;
    let mut to = text.trim_end_matches(['.', ',', ';', '!', ':']).len();
    for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
        while text[..to].ends_with(close)
            && text[from..to].chars().filter(|c| *c == close).count()
                > text[from..to].chars().filter(|c| *c == open).count()
        {
            to -= 1;
        }
    }
    let first = points.iter().find(|(byte, _)| *byte >= from)?.1;
    let mut last = points.iter().rev().find(|(byte, _)| *byte < to)?.1;
    if term.grid()[last].flags.contains(Flags::WIDE_CHAR) && last.column < term.last_column() {
        last.column += 1;
    }
    if !(first..=last).contains(&point) {
        return None;
    }
    Some(Link {
        uri: web_uri(&text[from..to])?,
        range: first..=last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{Size, Terminal};
    use alacritty_terminal::event::VoidListener;

    #[test]
    fn explicit_links_expose_the_real_target_and_reject_non_web_schemes() {
        let mut terminal = Terminal::new(Size::new(40, 4), 100, VoidListener);
        terminal.feed(b"\x1b]8;;https://example.com/path\x1b\\friendly label\x1b]8;;\x1b\\");
        let link = at(&terminal.term, Point::new(Line(0), Column(3))).unwrap();
        assert_eq!(link.uri, "https://example.com/path");
        assert_eq!(
            link.range,
            Point::new(Line(0), Column(0))..=Point::new(Line(0), Column(13))
        );
        terminal.feed(b"\r\x1b]8;;file://server/share\x1b\\files\x1b]8;;\x1b\\");
        assert!(at(&terminal.term, Point::new(Line(0), Column(1))).is_none());
        assert!(web_uri("javascript:alert(1)").is_none());
        assert!(web_uri("https://example.com/\nunsafe").is_none());
    }

    #[test]
    fn plain_urls_cross_soft_wrap_and_ignore_surrounding_punctuation() {
        let mut terminal = Terminal::new(Size::new(12, 5), 100, VoidListener);
        terminal.feed(b"(https://example.com/a(b)). next");
        let link = at(&terminal.term, Point::new(Line(1), Column(2))).unwrap();
        assert_eq!(link.uri, "https://example.com/a(b)");
        assert!(at(&terminal.term, Point::new(Line(0), Column(0))).is_none());
        assert!(at(&terminal.term, Point::new(Line(2), Column(1))).is_none());
        let mut terminal = Terminal::new(Size::new(40, 5), 100, VoidListener);
        terminal.feed(b"https://hard\r\nnewline.invalid");
        let link = at(&terminal.term, Point::new(Line(0), Column(3))).unwrap();
        assert_eq!(link.uri, "https://hard/");
    }
}
