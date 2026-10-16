use std::{hint::black_box, time::Instant};
use whostty_core::{
    alacritty_terminal::{
        event::VoidListener,
        grid::{Dimensions, Indexed},
        term::cell::Cell,
    },
    search::Search,
    terminal::{Size, Terminal},
};

fn throughput(line: &str) -> f64 {
    let mut terminal = Terminal::new(Size::new(120, 40), 10_000, VoidListener);
    let chunk = line.repeat(128);
    for _ in 0..16 {
        terminal.feed(chunk.as_bytes());
    }
    let start = Instant::now();
    for _ in 0..512 {
        terminal.feed(black_box(chunk.as_bytes()));
    }
    let elapsed = start.elapsed().as_secs_f64();
    black_box(terminal.term.grid());
    chunk.len() as f64 * 512.0 / 1024.0 / 1024.0 / elapsed
}

fn main() {
    let ascii = throughput(
        "\x1b[38;2;151;219;174mterminal output 0123456789 abcdefghijklmnopqrstuvwxyz\x1b[0m\r\n",
    );
    let unicode = throughput("\x1b[32m한글 터미널 출력 日本語 中文 e\u{301} wide text\x1b[0m\r\n");
    let mut terminal = Terminal::new(Size::new(120, 40), 10_000, VoidListener);
    for row in 0..10_000 {
        terminal.feed(format!("line {row:05} terminal benchmark text\r\n").as_bytes());
    }
    let mut search = Search::new("absent needle");
    search.find(&mut terminal, false, true);
    let start = Instant::now();
    for _ in 0..30 {
        search.find(black_box(&mut terminal), false, true);
    }
    let search_ms = start.elapsed().as_secs_f64() * 1000.0 / 30.0;
    let mut cells = Vec::<Indexed<Cell>>::new();
    let start = Instant::now();
    for _ in 0..1000 {
        cells.clear();
        cells.extend(terminal.term.grid().display_iter().map(|cell| Indexed {
            point: cell.point,
            cell: cell.cell.clone(),
        }));
        black_box(&cells);
    }
    let snapshot_us = start.elapsed().as_secs_f64() * 1_000_000.0 / 1000.0;
    println!("{{\"os\":\"{}\",\"arch\":\"{}\",\"debug\":{},\"columns\":120,\"rows\":40,\"history_lines\":{},\"ascii_mib_per_second\":{ascii:.2},\"unicode_mib_per_second\":{unicode:.2},\"search_no_match_10k_lines_avg_ms\":{search_ms:.3},\"snapshot_4800_cells_avg_us\":{snapshot_us:.3}}}",std::env::consts::OS,std::env::consts::ARCH,cfg!(debug_assertions),terminal.term.grid().history_size());
}
