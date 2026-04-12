//! Embedded terminal emulator for the Arx editor.
//!
//! Wraps [`alacritty_terminal`]'s terminal state machine and
//! [`portable_pty`]'s cross-platform PTY into a [`TerminalPane`].
//! The crate is renderer-agnostic: it exposes the terminal grid as a
//! matrix of [`TerminalCell`] values that callers paint however they
//! want (TUI cell grid today, GPU quads in a future GUI).

pub mod grid_bridge;
pub mod pty;

use std::sync::{Arc, Mutex};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
use tokio::sync::Notify;

/// Opaque identifier for a terminal pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct TerminalId(pub u64);

/// A cell in the terminal grid, ready for the renderer.
#[derive(Debug, Clone)]
pub struct TerminalCell {
    pub c: String,
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            c: " ".into(),
            fg: 0x00AB_B2BF,
            bg: 0x0028_2C34,
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

/// Snapshot of the terminal grid for rendering.
#[derive(Debug, Clone)]
pub struct TerminalSnapshot {
    pub cells: Vec<Vec<TerminalCell>>,
    pub cursor: Option<(u16, u16)>,
    pub cols: u16,
    pub rows: u16,
}

/// Simple size type that implements alacritty's `Dimensions` trait.
#[derive(Debug, Clone, Copy)]
struct TermSize {
    columns: usize,
    screen_lines: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }
    fn screen_lines(&self) -> usize {
        self.screen_lines
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

/// Event listener for the `Term`. Pokes the redraw notify on any
/// terminal state change.
#[derive(Clone)]
/// Event listener for the terminal state machine. Public because
/// [`grid_bridge::snapshot`] needs `Term<Listener>` in its signature.
#[derive(Debug)]
pub struct Listener {
    redraw: Arc<Notify>,
}

impl EventListener for Listener {
    fn send_event(&self, _event: alacritty_terminal::event::Event) {
        self.redraw.notify_one();
    }
}

/// An embedded terminal pane.
pub struct TerminalPane {
    term: Arc<Mutex<Term<Listener>>>,
    writer_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    redraw: Arc<Notify>,
    _child: Box<dyn portable_pty::Child + Send>,
}

impl std::fmt::Debug for TerminalPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalPane").finish_non_exhaustive()
    }
}

impl TerminalPane {
    /// Spawn a new terminal running `shell` with the given grid size.
    pub fn spawn(
        cols: u16,
        rows: u16,
        shell: Option<&str>,
        redraw: Arc<Notify>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system.openpty(portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let shell_cmd = shell
            .map(String::from)
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".into());
        let mut cmd = portable_pty::CommandBuilder::new(&shell_cmd);
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd)?;

        let listener = Listener {
            redraw: Arc::clone(&redraw),
        };
        let size = TermSize {
            columns: cols as usize,
            screen_lines: rows as usize,
        };
        let term = Term::new(TermConfig::default(), &size, listener);
        let term = Arc::new(Mutex::new(term));

        // Reader thread: PTY stdout → Term.
        let mut reader = pair.master.try_clone_reader()?;
        let term_clone = Arc::clone(&term);
        let redraw_clone = Arc::clone(&redraw);
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut processor: Processor<StdSyncHandler> = Processor::new();
            loop {
                match std::io::Read::read(&mut reader, &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut t = term_clone.lock().unwrap();
                        processor.advance(&mut *t, &buf[..n]);
                        redraw_clone.notify_one();
                    }
                }
            }
        });

        // Writer thread: keystrokes → PTY stdin.
        let (writer_tx, mut writer_rx) =
            tokio::sync::mpsc::channel::<Vec<u8>>(64);
        let mut master_writer = pair.master.take_writer()?;
        std::thread::spawn(move || {
            while let Some(bytes) = writer_rx.blocking_recv() {
                if std::io::Write::write_all(&mut master_writer, &bytes).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            term,
            writer_tx,
            redraw,
            _child: child,
        })
    }

    /// Take a snapshot of the terminal grid for rendering.
    pub fn snapshot(&self) -> TerminalSnapshot {
        let term = self.term.lock().unwrap();
        grid_bridge::snapshot(&term)
    }

    /// Write input bytes (keystrokes) to the PTY.
    pub fn write(&self, bytes: Vec<u8>) {
        let _ = self.writer_tx.try_send(bytes);
    }

    /// Resize the terminal grid and the underlying PTY.
    pub fn resize(&self, cols: u16, rows: u16) {
        let size = TermSize {
            columns: cols as usize,
            screen_lines: rows as usize,
        };
        let mut term = self.term.lock().unwrap();
        term.resize(size);
    }

    /// Get a clone of the redraw notifier.
    pub fn redraw_notify(&self) -> Arc<Notify> {
        Arc::clone(&self.redraw)
    }
}
