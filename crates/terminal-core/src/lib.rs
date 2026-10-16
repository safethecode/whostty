pub mod config;
mod directory;
pub mod input;
pub mod links;
pub mod mouse;
pub mod search;
mod search_worker;
pub mod session;
pub mod shortcuts;
pub mod terminal;
pub mod theme;

pub use alacritty_terminal;
mod atomic_file;
