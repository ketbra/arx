//! Terminal backends.
//!
//! A [`Backend`] applies a sequence of [`DiffOp`]s to some output target.
//! Two implementations ship with Phase 1:
//!
//! * [`TestBackend`] — in-memory [`CellGrid`] that mirrors what a real
//!   terminal would display. Tests diff a rendered [`RenderTree`] into a
//!   fresh backend and then assert on its `to_debug_text()`. No TTY
//!   required.
//!
//! * [`CrosstermBackend`] — thin shim over `crossterm`'s queued-writes API.
//!   Translates [`DiffOp`]s into crossterm commands. Takes any
//!   `io::Write` target (`io::stdout()`, a `Vec<u8>` for byte-level tests,
//!   a pipe to a pty for golden-master tests, etc.).
//!
//! Both satisfy the same [`Backend`] trait so callers (the editor's
//! renderer task) don't care which they're driving.

use std::io::{self, Write};

use crossterm::{QueueableCommand, cursor, style, terminal};

use crate::cell::{Cell, CellFlags, CellGrid};
use crate::diff::DiffOp;
use crate::face::{Color, ResolvedFace};
use crate::render_tree::{CursorRender, CursorStyle};
use arx_buffer::UnderlineStyle;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// A renderer backend. Implementations apply a stream of low-level diff
/// operations to some output (in-memory grid, crossterm-backed terminal,
/// a future wgpu surface, …).
pub trait Backend {
    /// Current backend size in cells.
    fn size(&self) -> (u16, u16);

    /// Apply a batch of [`DiffOp`]s.
    fn apply(&mut self, ops: &[DiffOp]) -> io::Result<()>;

    /// Flush any buffered output to the underlying target.
    fn present(&mut self) -> io::Result<()>;

    /// Reset the backend to an empty state (blank cells, no cursor). Used
    /// on terminal resume or mode switches.
    fn clear(&mut self) -> io::Result<()>;
}

// ---------------------------------------------------------------------------
// TestBackend
// ---------------------------------------------------------------------------

/// An in-memory backend that mirrors what a real terminal would show.
///
/// The renderer writes [`DiffOp`]s into it; tests read back the resulting
/// [`CellGrid`] / cursor via the public accessors.
#[derive(Debug, Clone)]
pub struct TestBackend {
    grid: CellGrid,
    cursor: Option<CursorRender>,
}

impl TestBackend {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            grid: CellGrid::new(width, height),
            cursor: None,
        }
    }

    pub fn grid(&self) -> &CellGrid {
        &self.grid
    }

    pub fn cursor(&self) -> Option<CursorRender> {
        self.cursor
    }

    /// Convenience: render the backend's current state as a
    /// newline-separated string with trailing whitespace trimmed.
    pub fn to_text(&self) -> String {
        self.grid.to_debug_text()
    }
}

impl Backend for TestBackend {
    fn size(&self) -> (u16, u16) {
        (self.grid.width(), self.grid.height())
    }

    fn apply(&mut self, ops: &[DiffOp]) -> io::Result<()> {
        for op in ops {
            match op {
                DiffOp::Resize { width, height } => {
                    self.grid = CellGrid::new(*width, *height);
                    self.cursor = None;
                }
                DiffOp::SetCell { x, y, cell } => {
                    self.grid.set(*x, *y, cell.clone());
                }
                DiffOp::MoveCursor(cr) => {
                    self.cursor = Some(*cr);
                }
                DiffOp::HideCursor => {
                    self.cursor = None;
                }
            }
        }
        Ok(())
    }

    fn present(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        self.grid.clear();
        self.cursor = None;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CrosstermBackend
// ---------------------------------------------------------------------------

/// Drives a `crossterm`-compatible `io::Write` target.
///
/// Translates [`DiffOp`]s into crossterm queued commands and flushes on
/// `present`. Style changes are only emitted when they actually differ
/// from the last cell's face, so a big run of same-colour text becomes
/// one colour change plus a sequence of raw characters.
#[derive(Debug)]
pub struct CrosstermBackend<W: Write> {
    out: W,
    width: u16,
    height: u16,
    /// The last resolved face we emitted — used to avoid redundant
    /// style-reset sequences.
    last_face: Option<ResolvedFace>,
}

impl<W: Write> CrosstermBackend<W> {
    pub fn new(out: W, width: u16, height: u16) -> Self {
        Self {
            out,
            width,
            height,
            last_face: None,
        }
    }

    /// Set the terminal size hint used by the backend. Useful after
    /// handling a `SIGWINCH`.
    pub fn set_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    /// Borrow the underlying writer. Useful for tests that want to
    /// inspect the raw byte output.
    pub fn writer(&self) -> &W {
        &self.out
    }

    fn queue_face(&mut self, face: ResolvedFace) -> io::Result<()> {
        if self.last_face == Some(face) {
            return Ok(());
        }
        self.out.queue(style::ResetColor)?;
        self.out.queue(style::SetForegroundColor(color_to_xterm(face.fg)))?;
        self.out.queue(style::SetBackgroundColor(color_to_xterm(face.bg)))?;
        let mut attrs = style::Attributes::default();
        if face.bold {
            attrs.set(style::Attribute::Bold);
        }
        if face.italic {
            attrs.set(style::Attribute::Italic);
        }
        if face.strikethrough {
            attrs.set(style::Attribute::CrossedOut);
        }
        if let Some(u) = face.underline {
            match u {
                UnderlineStyle::Double => attrs.set(style::Attribute::DoubleUnderlined),
                // Crossterm doesn't distinguish curly/dashed/dotted from
                // straight — fall back to Underlined for all of them.
                UnderlineStyle::Straight
                | UnderlineStyle::Curly
                | UnderlineStyle::Dashed
                | UnderlineStyle::Dotted => attrs.set(style::Attribute::Underlined),
            }
        }
        if !attrs.is_empty() {
            self.out.queue(style::SetAttributes(attrs))?;
        }
        self.last_face = Some(face);
        Ok(())
    }

    fn write_cell(&mut self, x: u16, y: u16, cell: &Cell) -> io::Result<()> {
        if cell.flags.contains(CellFlags::WIDE_CONTINUATION) {
            return Ok(());
        }
        self.queue_face(cell.face)?;
        self.out.queue(cursor::MoveTo(x, y))?;
        // We intentionally write the whole grapheme cluster rather than
        // per-codepoint — the terminal is responsible for joining ZWJ
        // sequences.
        write!(self.out, "{}", cell.grapheme)?;
        Ok(())
    }

    fn move_cursor(&mut self, cr: CursorRender) -> io::Result<()> {
        self.out.queue(cursor::Show)?;
        self.out.queue(cursor::MoveTo(cr.col, cr.row))?;
        let shape = match cr.style {
            CursorStyle::Block => cursor::SetCursorStyle::SteadyBlock,
            CursorStyle::Bar => cursor::SetCursorStyle::SteadyBar,
            CursorStyle::Underline => cursor::SetCursorStyle::SteadyUnderScore,
        };
        self.out.queue(shape)?;
        Ok(())
    }
}

impl<W: Write> Backend for CrosstermBackend<W> {
    fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn apply(&mut self, ops: &[DiffOp]) -> io::Result<()> {
        for op in ops {
            match op {
                DiffOp::Resize { width, height } => {
                    self.width = *width;
                    self.height = *height;
                    // `Clear(All)` erases to the terminal's *current*
                    // SGR background, which could be anything
                    // `self.last_face` drifted to (e.g. the modeline's
                    // light-grey face). Explicitly set our default bg
                    // before clearing so the erased area is in a known
                    // state regardless of prior writes.
                    self.out
                        .queue(style::SetBackgroundColor(color_to_xterm(
                            ResolvedFace::DEFAULT.bg,
                        )))?;
                    self.out.queue(terminal::Clear(terminal::ClearType::All))?;
                    self.last_face = None;
                }
                DiffOp::SetCell { x, y, cell } => {
                    self.write_cell(*x, *y, cell)?;
                }
                DiffOp::MoveCursor(cr) => self.move_cursor(*cr)?,
                DiffOp::HideCursor => {
                    self.out.queue(cursor::Hide)?;
                }
            }
        }
        Ok(())
    }

    fn present(&mut self) -> io::Result<()> {
        self.out.flush()
    }

    fn clear(&mut self) -> io::Result<()> {
        self.out.queue(terminal::Clear(terminal::ClearType::All))?;
        self.out.queue(cursor::MoveTo(0, 0))?;
        self.last_face = None;
        self.out.flush()
    }
}

/// Convert our 24-bit RGB into a crossterm colour. All modern terminals
/// we target (Kitty, `WezTerm`, Ghostty, `iTerm2`, Windows Terminal) support
/// truecolour.
fn color_to_xterm(c: Color) -> style::Color {
    style::Color::Rgb {
        r: c.r(),
        g: c.g(),
        b: c.b(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::CellFlags;
    use crate::diff::{DiffOp, initial_paint};
    use crate::render_tree::{CursorRender, CursorStyle};
    use compact_str::CompactString;
    use smallvec::smallvec;

    fn cell(ch: &str) -> Cell {
        Cell {
            grapheme: CompactString::new(ch),
            face: ResolvedFace::DEFAULT,
            flags: CellFlags::empty(),
        }
    }

    #[test]
    fn test_backend_applies_setcell_in_place() {
        let mut backend = TestBackend::new(4, 2);
        backend
            .apply(&[
                DiffOp::SetCell {
                    x: 0,
                    y: 0,
                    cell: cell("A"),
                },
                DiffOp::SetCell {
                    x: 1,
                    y: 0,
                    cell: cell("B"),
                },
            ])
            .unwrap();
        assert_eq!(backend.to_text(), "AB  \n    ");
    }

    #[test]
    fn test_backend_handles_resize() {
        let mut backend = TestBackend::new(2, 2);
        backend
            .apply(&[
                DiffOp::SetCell {
                    x: 0,
                    y: 0,
                    cell: cell("X"),
                },
                DiffOp::Resize {
                    width: 3,
                    height: 1,
                },
                DiffOp::SetCell {
                    x: 2,
                    y: 0,
                    cell: cell("Z"),
                },
            ])
            .unwrap();
        assert_eq!(backend.size(), (3, 1));
        assert_eq!(backend.to_text(), "  Z");
    }

    #[test]
    fn test_backend_tracks_cursor() {
        let mut backend = TestBackend::new(4, 2);
        backend
            .apply(&[DiffOp::MoveCursor(CursorRender {
                col: 2,
                row: 1,
                style: CursorStyle::Block,
            })])
            .unwrap();
        assert_eq!(backend.cursor().unwrap().col, 2);
        backend.apply(&[DiffOp::HideCursor]).unwrap();
        assert!(backend.cursor().is_none());
    }

    #[test]
    fn crossterm_backend_writes_something_for_initial_paint() {
        use crate::render_tree::RenderTree;
        let mut tree = RenderTree::blank(4, 1, 0);
        tree.cells.set(0, 0, cell("H"));
        tree.cells.set(1, 0, cell("i"));
        tree.cursors = smallvec![CursorRender {
            col: 1,
            row: 0,
            style: CursorStyle::Block,
        }];

        let mut out: Vec<u8> = Vec::new();
        let mut backend = CrosstermBackend::new(&mut out, 4, 1);
        let ops = initial_paint(&tree);
        backend.apply(&ops).unwrap();
        backend.present().unwrap();

        // Crude smoke test: the raw bytes include our two letters and a
        // cursor style sequence. We don't pin the exact escape codes
        // because crossterm may update them.
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains('H'), "output: {s:?}");
        assert!(s.contains('i'), "output: {s:?}");
    }
}
