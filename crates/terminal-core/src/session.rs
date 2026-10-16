use std::{
    io::{Read, Write},
    sync::{
        mpsc::{self, Receiver, Sender, SyncSender},
        Arc, Mutex,
    },
    thread,
};

use alacritty_terminal::event::{Event, EventListener};
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::{
    config::Profile,
    terminal::{Size, Terminal},
};

#[derive(Debug)]
pub enum SessionEvent {
    Terminal(Event),
    Exited(u32),
    Error(String),
}

#[derive(Clone)]
pub struct Listener {
    events: Sender<SessionEvent>,
    input: SyncSender<Vec<u8>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            if self.input.try_send(text.into_bytes()).is_err() {
                let _ = self.events.send(SessionEvent::Error(
                    "PTY response queue is full or closed".into(),
                ));
            }
        } else {
            let _ = self.events.send(SessionEvent::Terminal(event));
        }
    }
}

pub struct Session {
    pub terminal: Arc<Mutex<Terminal<Listener>>>,
    pub events: Receiver<SessionEvent>,
    master: Box<dyn MasterPty + Send>,
    input: SyncSender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    size: Size,
}

impl Session {
    pub fn spawn(
        profile: &Profile,
        size: Size,
        scrollback: usize,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let pair = native_pty_system()
            .openpty(pty_size(size))
            .context("Create native PTY (ConPTY on Windows)")?;
        // Acquire handles before spawning so a setup error cannot orphan a process.
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let mut command = CommandBuilder::new(&profile.program);
        command.args(&profile.args);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "Whostty");
        if let Some(directory) = &profile.working_directory {
            command.cwd(directory);
        } else if let Some(home) = dirs::home_dir() {
            command.cwd(home);
        }
        let mut child = pair
            .slave
            .spawn_command(command)
            .with_context(|| format!("Start {} ({})", profile.name, profile.program))?;
        let killer = child.clone_killer();
        drop(pair.slave);

        let (events_tx, events) = mpsc::channel();
        let (input, input_rx) = mpsc::sync_channel::<Vec<u8>>(64);
        let terminal = Arc::new(Mutex::new(Terminal::new(
            size,
            scrollback,
            Listener {
                events: events_tx.clone(),
                input: input.clone(),
            },
        )));

        let writer_events = events_tx.clone();
        let writer_wake = wake.clone();
        thread::spawn(move || {
            while let Ok(bytes) = input_rx.recv() {
                if let Err(error) = writer.write_all(&bytes).and_then(|()| writer.flush()) {
                    let _ = writer_events.send(SessionEvent::Error(format!("PTY write: {error}")));
                    writer_wake();
                    break;
                }
            }
        });

        let output_terminal = terminal.clone();
        let output_events = events_tx.clone();
        let output_wake = wake.clone();
        thread::spawn(move || {
            let mut buffer = [0; 16 * 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        let Ok(mut terminal) = output_terminal.lock() else {
                            break;
                        };
                        terminal.feed(&buffer[..count]);
                        drop(terminal);
                        output_wake();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    // Unix PTYs report EIO after the slave closes.
                    #[cfg(unix)]
                    Err(error) if error.raw_os_error() == Some(5) => break,
                    Err(error) => {
                        let _ =
                            output_events.send(SessionEvent::Error(format!("PTY read: {error}")));
                        output_wake();
                        break;
                    }
                }
            }
        });

        thread::spawn(move || {
            let event = match child.wait() {
                Ok(status) => SessionEvent::Exited(status.exit_code()),
                Err(error) => SessionEvent::Error(format!("Wait for shell: {error}")),
            };
            let _ = events_tx.send(event);
            wake();
        });
        Ok(Self {
            terminal,
            events,
            master: pair.master,
            input,
            killer,
            size,
        })
    }

    pub fn write(&self, bytes: Vec<u8>) -> Result<()> {
        self.input
            .try_send(bytes)
            .context("PTY input queue is full or the session has closed")
    }

    pub fn resize(&mut self, size: Size) -> Result<()> {
        if self.size == size {
            return Ok(());
        }
        // Serialize grid resizing with output parsing so ConPTY reflow uses the new dimensions.
        let mut terminal = self
            .terminal
            .lock()
            .map_err(|_| anyhow::anyhow!("Terminal lock poisoned"))?;
        self.master.resize(pty_size(size)).context("Resize PTY")?;
        terminal.term.resize(size);
        self.size = size;
        Ok(())
    }

    pub fn size(&self) -> Size {
        self.size
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // The wait thread reaps the process. Do not block the GUI while waiting for exit.
        let _ = self.killer.kill();
    }
}

fn pty_size(size: Size) -> PtySize {
    PtySize {
        rows: size.rows as u16,
        cols: size.columns as u16,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !predicate() {
            assert!(Instant::now() < deadline, "PTY test timed out");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn native_pty_runs_command_resizes_and_reports_exit() {
        #[cfg(windows)]
        let profile = Profile::new(
            "test",
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "[Console]::Write('whostty-pty-ok'); exit 7",
            ],
        );
        #[cfg(not(windows))]
        let profile = Profile::new("test", "/bin/sh", &["-c", "printf whostty-pty-ok; exit 7"]);
        let mut session =
            Session::spawn(&profile, Size::new(80, 24), 100, Arc::new(|| {})).unwrap();
        session.resize(Size::new(100, 30)).unwrap();
        assert_eq!(session.size(), Size::new(100, 30));
        wait_until(|| {
            session
                .terminal
                .lock()
                .unwrap()
                .text()
                .contains("whostty-pty-ok")
        });
        wait_until(|| {
            session
                .events
                .try_iter()
                .any(|event| matches!(event, SessionEvent::Exited(7)))
        });
    }

    #[test]
    fn interactive_input_reaches_the_child() {
        #[cfg(windows)]
        let profile = Profile::new(
            "test",
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "$v = [Console]::ReadLine(); [Console]::Write('received-' + $v)",
            ],
        );
        #[cfg(not(windows))]
        let profile = Profile::new(
            "test",
            "/bin/sh",
            &["-c", "read value; printf received-%s \"$value\""],
        );
        let session = Session::spawn(&profile, Size::new(80, 24), 100, Arc::new(|| {})).unwrap();
        session.write(b"hello\r".to_vec()).unwrap();
        wait_until(|| {
            session
                .terminal
                .lock()
                .unwrap()
                .text()
                .contains("received-hello")
        });
    }
}
