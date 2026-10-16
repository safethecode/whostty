use crate::terminal::Terminal;
use alacritty_terminal::{
    event::EventListener,
    grid::Dimensions,
    index::{Column, Line, Point},
    term::{
        cell::{Cell, Flags},
        search::Match,
    },
};
use regex::RegexBuilder;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
    Arc, Mutex,
};

// Bound individual lock holds and the memory used by a single wrapped logical line.
const ROW_BATCH: i32 = 16;
const MAX_LINE_BYTES: usize = 256 * 1024;

pub(crate) struct Request {
    pub pattern: String,
    pub origin: Point,
    pub backwards: bool,
    pub revision: u64,
    pub top: i32,
    pub bottom: i32,
    pub debounce: bool,
}

#[derive(Debug)]
pub(crate) enum Failure {
    Stale,
    Canceled,
    Closed,
    InvalidRegex,
    LongLine,
}

pub(crate) struct Outcome {
    pub revision: u64,
    pub result: Result<Option<Match>, Failure>,
    pub wrapped: bool,
}

pub(crate) struct SearchJob {
    canceled: Arc<AtomicBool>,
    receiver: Receiver<Outcome>,
    worker: std::thread::Thread,
}

impl Drop for SearchJob {
    fn drop(&mut self) {
        self.canceled.store(true, Ordering::Release);
        self.worker.unpark();
    }
}

impl SearchJob {
    pub fn spawn<T: EventListener + Send + 'static>(
        request: Request,
        terminal: &Arc<Mutex<Terminal<T>>>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let terminal = Arc::downgrade(terminal);
        let canceled = Arc::new(AtomicBool::new(false));
        let cancellation = canceled.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            if request.debounce {
                std::thread::park_timeout(std::time::Duration::from_millis(100));
            }
            let outcome = run(&request, &cancellation, |first, last| {
                let terminal = terminal.upgrade().ok_or(Failure::Closed)?;
                let terminal = terminal.lock().map_err(|_| Failure::Closed)?;
                if terminal.revision() != request.revision {
                    return Err(Failure::Stale);
                }
                Ok(copy_rows(&terminal, first, last))
            });
            if !cancellation.load(Ordering::Acquire) {
                let _ = sender.send(outcome);
                wake();
            }
        });
        Self {
            canceled,
            receiver,
            worker: worker.thread().clone(),
        }
    }

    pub fn poll(&self) -> Option<Outcome> {
        match self.receiver.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Outcome {
                revision: 0,
                result: Err(Failure::Closed),
                wrapped: false,
            }),
        }
    }
}

pub(crate) fn copy_rows<T: EventListener>(
    terminal: &Terminal<T>,
    first: i32,
    last: i32,
) -> Vec<(i32, Vec<Cell>)> {
    (first..last)
        .map(|line| {
            let cells = (0..terminal.term.columns())
                .map(|col| terminal.term.grid()[Line(line)][Column(col)].clone())
                .collect();
            (line, cells)
        })
        .collect()
}

pub(crate) fn run(
    request: &Request,
    canceled: &AtomicBool,
    mut rows: impl FnMut(i32, i32) -> Result<Vec<(i32, Vec<Cell>)>, Failure>,
) -> Outcome {
    let mut wrapped = false;
    let result = (|| {
        if canceled.load(Ordering::Acquire) {
            return Err(Failure::Canceled);
        }
        let regex = RegexBuilder::new(&request.pattern)
            .size_limit(2 * 1024 * 1024)
            .dfa_size_limit(2 * 1024 * 1024)
            .build()
            .map_err(|_| Failure::InvalidRegex)?;
        let mut text = String::new();
        let mut points: Vec<(usize, Point, Point)> = Vec::new();
        let mut preferred = None;
        let mut fallback = None;
        for first in (request.top..request.bottom).step_by(ROW_BATCH as usize) {
            if canceled.load(Ordering::Acquire) {
                return Err(Failure::Canceled);
            }
            // No parsing, regex work or large string allocation happens under the grid lock.
            let batch = rows(first, (first + ROW_BATCH).min(request.bottom))?;
            for (line, cells) in batch {
                let continues = cells
                    .last()
                    .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE));
                for (column, cell) in cells.iter().enumerate() {
                    if cell
                        .flags
                        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                    {
                        continue;
                    }
                    let start = Point::new(Line(line), Column(column));
                    let end = Point::new(
                        Line(line),
                        Column(
                            (column + usize::from(cell.flags.contains(Flags::WIDE_CHAR)))
                                .min(cells.len() - 1),
                        ),
                    );
                    for c in std::iter::once(&cell.c).chain(cell.zerowidth().unwrap_or_default()) {
                        points.push((text.len(), start, end));
                        text.push(*c);
                        if text.len() > MAX_LINE_BYTES {
                            return Err(Failure::LongLine);
                        }
                    }
                }
                if continues && line + 1 < request.bottom {
                    continue;
                }
                for found in regex.find_iter(&text).filter(|found| !found.is_empty()) {
                    if canceled.load(Ordering::Acquire) {
                        return Err(Failure::Canceled);
                    }
                    let start_index =
                        points.partition_point(|(byte, _, _)| *byte <= found.start()) - 1;
                    let end_index = points.partition_point(|(byte, _, _)| *byte < found.end()) - 1;
                    let found = points[start_index].1..=points[end_index].2;
                    if request.backwards {
                        fallback = Some(found.clone());
                        if *found.end() <= request.origin {
                            preferred = Some(found);
                        }
                    } else {
                        if fallback.is_none() {
                            fallback = Some(found.clone());
                        }
                        if *found.start() >= request.origin {
                            return Ok(Some(found));
                        }
                    }
                }
                text.clear();
                points.clear();
            }
        }
        if preferred.is_some() {
            return Ok(preferred);
        }
        wrapped = fallback.is_some();
        Ok(fallback)
    })();
    Outcome {
        revision: request.revision,
        result,
        wrapped,
    }
}
