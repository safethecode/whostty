use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use alacritty_terminal::event::{Event, EventListener};
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExitState {
    Exited(u32),
    Failed(String),
}

#[derive(Clone)]
pub struct Listener {
    events: SyncSender<SessionEvent>,
    input: SyncSender<Vec<u8>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            if self.input.try_send(text.into_bytes()).is_err() {
                let _ = self.events.try_send(SessionEvent::Error(
                    "PTY response queue is full or closed".into(),
                ));
            }
        } else {
            // Never block while the parser holds the grid lock, even if the UI is busy.
            let _ = self.events.try_send(SessionEvent::Terminal(event));
        }
    }
}

#[derive(Default)]
struct PendingControl {
    resize: Option<Size>,
    closed: bool,
}

struct Control {
    pending: Arc<Mutex<PendingControl>>,
    wake: SyncSender<()>,
}

impl Control {
    fn resize(&self, size: Size) -> Result<()> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        anyhow::ensure!(!pending.closed, "The session has closed");
        pending.resize = Some(size);
        drop(pending);
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => Ok(()),
            Err(TrySendError::Disconnected(())) => anyhow::bail!("The session worker has closed"),
        }
    }

    fn close(&self) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.closed = true;
        pending.resize = None;
        drop(pending);
        let _ = self.wake.try_send(());
    }
}

pub struct Session {
    pub terminal: Arc<Mutex<Terminal<Listener>>>,
    pub events: Receiver<SessionEvent>,
    input: SyncSender<Vec<u8>>,
    control: Control,
    finished: Arc<AtomicBool>,
    exit: Arc<Mutex<Option<ExitState>>>,
    size: Size,
    initial_directory: Option<std::path::PathBuf>,
}

impl Session {
    pub fn spawn(
        profile: &Profile,
        size: Size,
        scrollback: usize,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        Self::spawn_with_master(profile, size, scrollback, wake, |master| master)
    }

    fn spawn_with_master(
        profile: &Profile,
        size: Size,
        scrollback: usize,
        wake: Arc<dyn Fn() + Send + Sync>,
        prepare: impl FnOnce(Box<dyn MasterPty + Send>) -> Box<dyn MasterPty + Send>,
    ) -> Result<Self> {
        let pair = native_pty_system()
            .openpty(pty_size(size))
            .context("Create native PTY (ConPTY on Windows)")?;
        // Acquire handles before spawning so a setup error cannot orphan a process.
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let mut command = CommandBuilder::new(&profile.program);
        command.args(&profile.args);
        if profile.shell_integration {
            anyhow::ensure!(
                profile.supports_shell_integration(),
                "Shell integration requires an interactive PowerShell profile"
            );
            if !profile
                .args
                .iter()
                .any(|arg| arg.eq_ignore_ascii_case("-NoExit"))
            {
                command.arg("-NoExit");
            }
            command.arg("-Command");
            command.arg(include_str!("../../../scripts/shell-integration.ps1"));
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "Whostty");
        let initial_directory = profile.working_directory.clone().or_else(dirs::home_dir);
        if let Some(directory) = &initial_directory {
            command.cwd(directory);
        }
        let mut child = pair
            .slave
            .spawn_command(command)
            .with_context(|| format!("Start {} ({})", profile.name, profile.program))?;
        drop(pair.slave);
        let master = prepare(pair.master);

        let (events_tx, events) = mpsc::sync_channel(1024);
        let (input, input_rx) = mpsc::sync_channel::<Vec<u8>>(64);
        let (control_wake, control_rx) = mpsc::sync_channel(1);
        let pending = Arc::new(Mutex::new(PendingControl::default()));
        let control = Control {
            pending: pending.clone(),
            wake: control_wake,
        };
        let finished = Arc::new(AtomicBool::new(false));
        let exit = Arc::new(Mutex::new(None));
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
        let writer_finished = finished.clone();
        thread::spawn(move || {
            while !writer_finished.load(Ordering::Acquire) {
                let bytes = match input_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(bytes) => bytes,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                if let Err(error) = writer.write_all(&bytes).and_then(|()| writer.flush()) {
                    let _ =
                        writer_events.try_send(SessionEvent::Error(format!("PTY write: {error}")));
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
                        let _ = output_events
                            .try_send(SessionEvent::Error(format!("PTY read: {error}")));
                        output_wake();
                        break;
                    }
                }
            }
            if let Ok(mut terminal) = output_terminal.lock() {
                terminal.finish_output();
            }
            output_wake();
        });

        let child_finished = finished.clone();
        let child_exit = exit.clone();
        thread::spawn(move || {
            // This thread alone owns/reaps the process, preventing signals to a reused PID.
            // Native resize and master destruction can wait for output to drain.
            // This worker owns both operations; neither may run on the GUI thread.
            let mut exited = false;
            let mut reaped = false;
            loop {
                if !exited {
                    let outcome = match poll_child(child.as_mut()) {
                        Ok(Some(status)) => Some(ExitState::Exited(status.exit_code())),
                        Ok(None) => None,
                        Err(error) => Some(ExitState::Failed(format!("Wait for shell: {error}"))),
                    };
                    if let Some(outcome) = outcome {
                        reaped = matches!(outcome, ExitState::Exited(_));
                        let event = match &outcome {
                            ExitState::Exited(code) => SessionEvent::Exited(*code),
                            ExitState::Failed(error) => SessionEvent::Error(error.clone()),
                        };
                        *child_exit.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
                        child_finished.store(true, Ordering::Release);
                        // The status cell remains observable even when terminal events fill the queue.
                        let _ = events_tx.try_send(event);
                        wake();
                        exited = true;
                    }
                }
                let (closed, resize) = {
                    let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
                    (pending.closed, pending.resize.take())
                };
                if closed {
                    break;
                }
                if !exited {
                    if let Some(size) = resize {
                        if let Err(error) = master.resize(pty_size(size)) {
                            let _ = events_tx
                                .try_send(SessionEvent::Error(format!("Resize PTY: {error:#}")));
                            wake();
                        }
                    }
                }
                let disconnected = if exited {
                    control_rx.recv().is_err()
                } else {
                    match control_rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(()) | Err(RecvTimeoutError::Timeout) => false,
                        Err(RecvTimeoutError::Disconnected) => true,
                    }
                };
                if disconnected {
                    break;
                }
            }
            child_finished.store(true, Ordering::Release);
            if !reaped {
                let _ = child.kill();
            }
            drop(master);
            if !reaped {
                let _ = child.wait();
            }
        });
        Ok(Self {
            terminal,
            events,
            input,
            control,
            finished,
            exit,
            size,
            initial_directory,
        })
    }

    pub fn write(&self, bytes: Vec<u8>) -> Result<()> {
        anyhow::ensure!(
            !self.finished.load(Ordering::Acquire),
            "The shell has exited"
        );
        self.input
            .try_send(bytes)
            .context("PTY input queue is full or the session has closed")
    }

    pub fn resize(&mut self, size: Size) -> Result<()> {
        if self.size == size {
            return Ok(());
        }
        // Only update the grid here; native resize is coalesced on the lifecycle worker.
        {
            let mut terminal = self
                .terminal
                .lock()
                .map_err(|_| anyhow::anyhow!("Terminal lock poisoned"))?;
            terminal.resize(size);
        }
        if !self.finished.load(Ordering::Acquire) {
            self.control.resize(size)?;
        }
        self.size = size;
        Ok(())
    }

    pub fn working_directory(&self) -> Option<std::path::PathBuf> {
        self.terminal
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .working_directory()
            .map(std::path::Path::to_owned)
            .or_else(|| self.initial_directory.clone())
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn exit_state(&self) -> Option<ExitState> {
        self.exit
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Release);
        self.control.close();
    }
}

fn poll_child(
    child: &mut dyn portable_pty::Child,
) -> std::io::Result<Option<portable_pty::ExitStatus>> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
            System::Threading::WaitForSingleObject,
        };
        let handle = child
            .as_raw_handle()
            .ok_or_else(|| std::io::Error::other("The shell has no Windows process handle"))?;
        // The worker owns the child throughout this nonblocking handle check. Checking the
        // signal avoids portable-pty 0.9 treating an actual exit code 259 as STILL_ACTIVE.
        match unsafe { WaitForSingleObject(handle, 0) } {
            WAIT_OBJECT_0 => child.wait().map(Some),
            WAIT_TIMEOUT => Ok(None),
            WAIT_FAILED => Err(std::io::Error::last_os_error()),
            value => Err(std::io::Error::other(format!(
                "Unexpected process wait result: {value}"
            ))),
        }
    }
    #[cfg(not(windows))]
    child.try_wait()
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
    use std::sync::mpsc::Sender;
    use std::time::{Duration, Instant};

    #[test]
    fn close_discards_pending_resize_even_when_wake_slot_is_full() {
        let (wake, receiver) = mpsc::sync_channel(1);
        let pending = Arc::new(Mutex::new(PendingControl::default()));
        let control = Control {
            pending: pending.clone(),
            wake,
        };
        for index in 0..1000 {
            control.resize(Size::new(80 + index % 20, 24)).unwrap();
        }
        assert_eq!(pending.lock().unwrap().resize, Some(Size::new(99, 24)));
        control.close();
        assert!(pending.lock().unwrap().closed);
        assert!(pending.lock().unwrap().resize.is_none());
        assert!(control.resize(Size::new(81, 25)).is_err());
        assert_eq!(
            receiver.try_iter().count(),
            1,
            "Wake requests grew without bound"
        );
    }

    struct DelayedResize {
        master: Box<dyn MasterPty + Send>,
        entered: Sender<Size>,
        release: Mutex<Receiver<()>>,
    }

    impl MasterPty for DelayedResize {
        fn resize(&self, size: PtySize) -> Result<()> {
            self.entered
                .send(Size::new(size.cols as usize, size.rows as usize))?;
            // A timeout bounds the old synchronous-path regression failure.
            let _ = self
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3));
            self.master.resize(size)
        }
        fn get_size(&self) -> Result<PtySize> {
            self.master.get_size()
        }
        fn try_clone_reader(&self) -> Result<Box<dyn Read + Send>> {
            self.master.try_clone_reader()
        }
        fn take_writer(&self) -> Result<Box<dyn Write + Send>> {
            self.master.take_writer()
        }
        #[cfg(unix)]
        fn process_group_leader(&self) -> Option<i32> {
            self.master.process_group_leader()
        }
        #[cfg(unix)]
        fn as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
            self.master.as_raw_fd()
        }
        #[cfg(unix)]
        fn tty_name(&self) -> Option<std::path::PathBuf> {
            self.master.tty_name()
        }
    }

    #[test]
    fn delayed_native_resize_does_not_block_caller_and_coalesces_pending_sizes() {
        #[cfg(windows)]
        let profile = Profile::new(
            "delayed-resize",
            "powershell.exe",
            &["-NoProfile", "-Command", "Start-Sleep 30"],
        );
        #[cfg(not(windows))]
        let profile = Profile::new("delayed-resize", "/bin/sh", &["-c", "sleep 30"]);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut session = Session::spawn_with_master(
            &profile,
            Size::new(80, 24),
            100,
            Arc::new(|| {}),
            |master| {
                Box::new(DelayedResize {
                    master,
                    entered: entered_tx,
                    release: Mutex::new(release_rx),
                })
            },
        )
        .unwrap();
        let first = Size::new(81, 25);
        let start = Instant::now();
        session.resize(first).unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "Native resize blocked its caller"
        );
        assert_eq!(
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            first
        );
        for index in 0..1000 {
            session.resize(Size::new(90 + index % 20, 30)).unwrap();
        }
        let latest = session.size();
        release_tx.send(()).unwrap();
        assert_eq!(
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            latest
        );
        let workers = Arc::downgrade(&session.finished);
        // Closing while the second native call is blocked must also stay off the caller.
        let start = Instant::now();
        drop(session);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "Session close blocked its caller"
        );
        release_tx.send(()).unwrap();
        wait_until(|| workers.upgrade().is_none());
        assert!(
            entered_rx.try_recv().is_err(),
            "Replayed intermediate resize requests"
        );
    }

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
    fn osc_directory_is_observed_through_the_native_pty() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("한글 space;%");
        std::fs::create_dir(&target).unwrap();
        let url = url::Url::from_directory_path(&target).unwrap();
        #[cfg(windows)]
        let profile = Profile::new(
            "directory",
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                &format!("[Console]::Write(([string][char]27) + ']7;{url}' + [char]7); exit 0"),
            ],
        );
        #[cfg(not(windows))]
        let profile = Profile::new(
            "directory",
            "/bin/sh",
            &[
                "-c",
                "printf '\\033]7;%s\\007' \"$1\"",
                "check",
                url.as_str(),
            ],
        );
        let session = Session::spawn(&profile, Size::new(80, 24), 100, Arc::new(|| {})).unwrap();
        wait_until(|| session.working_directory().as_ref() == Some(&target));
    }

    #[cfg(windows)]
    fn reported_directory_matches(session: &Session, expected: &std::path::Path) -> bool {
        // PowerShell expands Windows TEMP's 8.3 spelling and emits a trailing separator.
        session
            .terminal
            .lock()
            .unwrap()
            .working_directory()
            .and_then(|path| path.canonicalize().ok())
            .is_some_and(|path| Some(path) == expected.canonicalize().ok())
    }

    #[cfg(windows)]
    #[test]
    fn powershell_integration_reports_changed_unicode_directories() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("한글 space;%'quoted");
        std::fs::create_dir(&target).unwrap();
        let mut profile = Profile::new("integrated", "powershell.exe", &["-NoLogo", "-NoProfile"]);
        profile.shell_integration = true;
        profile.working_directory = Some(directory.path().to_owned());
        let session = Session::spawn(&profile, Size::new(120, 24), 100, Arc::new(|| {})).unwrap();
        // Inspect the OSC observer directly: Session's initial-directory fallback is not proof.
        wait_until(|| reported_directory_matches(&session, directory.path()));
        let command = format!(
            "Set-Location -LiteralPath '{}'; [Console]::WriteLine('changed-directory')\r",
            target.display().to_string().replace('\'', "''")
        );
        session.write(command.into_bytes()).unwrap();
        wait_until(|| reported_directory_matches(&session, &target));
        session.write(b"exit 0\r".to_vec()).unwrap();
        wait_until(|| session.exit_state() == Some(ExitState::Exited(0)));
    }

    #[cfg(windows)]
    #[test]
    fn powershell_integration_preserves_the_existing_prompt_error_state() {
        let directory = tempfile::tempdir().unwrap();
        let script = format!("function global:prompt {{ 'original-state:' + $? + ':' + $global:LASTEXITCODE }}; {} ; $global:LASTEXITCODE = 23; Write-Error 'expected fixture error' -ErrorAction Continue; prompt; exit 0", include_str!("../../../scripts/shell-integration.ps1"));
        let mut profile = Profile::new(
            "prompt",
            "powershell.exe",
            &["-NoLogo", "-NoProfile", "-Command", &script],
        );
        profile.working_directory = Some(directory.path().to_owned());
        let session = Session::spawn(&profile, Size::new(120, 24), 100, Arc::new(|| {})).unwrap();
        wait_until(|| session.exit_state() == Some(ExitState::Exited(0)));
        wait_until(|| {
            session
                .terminal
                .lock()
                .unwrap()
                .text()
                .contains("original-state:False:23")
        });
        assert!(reported_directory_matches(&session, directory.path()));
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

    #[test]
    fn title_event_flood_cannot_block_terminal_output() {
        #[cfg(windows)]
        let profile = Profile::new("flood", "powershell.exe", &[
            "-NoLogo", "-NoProfile", "-Command",
            "for ($i=0; $i -lt 2000; $i++) { [Console]::Write([string][char]27 + ']2;title' + [char]7) }; [Console]::Write('flood-ok')",
        ]);
        #[cfg(not(windows))]
        let profile = Profile::new("flood", "/bin/sh", &[
            "-c", "i=0; while [ \"$i\" -lt 2000 ]; do printf '\\033]2;title\\007'; i=$((i+1)); done; printf flood-ok",
        ]);
        let session = Session::spawn(&profile, Size::new(80, 24), 100, Arc::new(|| {})).unwrap();
        // Intentionally never drain session.events while thousands of OSC titles arrive.
        wait_until(|| session.terminal.lock().unwrap().text().contains("flood-ok"));
        wait_until(|| session.exit_state() == Some(ExitState::Exited(0)));
    }

    #[test]
    fn resize_while_output_is_flowing_does_not_deadlock() {
        #[cfg(windows)]
        let profile = Profile::new("resize", "powershell.exe", &[
            "-NoLogo", "-NoProfile", "-Command",
            "for ($i=0; $i -lt 1000; $i++) { [Console]::WriteLine('resize-output ' * 20) }; [Console]::Write('resize-finished')",
        ]);
        #[cfg(not(windows))]
        let profile = Profile::new("resize", "/bin/sh", &[
            "-c", "i=0; while [ \"$i\" -lt 1000 ]; do printf 'resize-output %.0s' 1 2 3 4 5 6 7 8 9 10; printf '\\r\\n'; i=$((i+1)); done; printf resize-finished",
        ]);
        let mut session =
            Session::spawn(&profile, Size::new(80, 24), 100, Arc::new(|| {})).unwrap();
        for index in 0..40 {
            session
                .resize(Size::new(80 + index % 20, 24 + index % 10))
                .unwrap();
            thread::sleep(Duration::from_millis(2));
        }
        wait_until(|| {
            session
                .terminal
                .lock()
                .unwrap()
                .text()
                .contains("resize-finished")
        });
    }

    #[test]
    fn closing_a_live_session_releases_its_reader_without_joining_the_ui() {
        #[cfg(windows)]
        let profile = Profile::new(
            "shutdown",
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "[Console]::Write('shutdown-ready'); Start-Sleep 60",
            ],
        );
        #[cfg(not(windows))]
        let profile = Profile::new(
            "shutdown",
            "/bin/sh",
            &[
                "-c",
                "trap '' HUP; printf shutdown-ready; while :; do sleep 1; done",
            ],
        );
        let session = Session::spawn(&profile, Size::new(80, 24), 100, Arc::new(|| {})).unwrap();
        wait_until(|| {
            session
                .terminal
                .lock()
                .unwrap()
                .text()
                .contains("shutdown-ready")
        });
        let terminal = Arc::downgrade(&session.terminal);
        let workers = Arc::downgrade(&session.finished);
        let start = Instant::now();
        drop(session);
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "Session drop blocked the caller"
        );
        wait_until(|| terminal.upgrade().is_none());
        wait_until(|| workers.upgrade().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn windows_exit_code_259_is_not_mistaken_for_a_live_process() {
        let profile = Profile::new(
            "exit-259",
            "powershell.exe",
            &["-NoLogo", "-NoProfile", "-Command", "exit 259"],
        );
        let session = Session::spawn(&profile, Size::new(80, 24), 100, Arc::new(|| {})).unwrap();
        wait_until(|| session.exit_state() == Some(ExitState::Exited(259)));
        assert!(session.write(b"input-after-exit\r".to_vec()).is_err());
    }
}
