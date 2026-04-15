//! Suspend / resume support for the interactive editor.
//!
//! Ctrl-Z is the standard Unix mechanism to suspend a process and
//! return to the shell. Since the editor runs in raw mode, Ctrl-Z is
//! captured as a regular keystroke and not auto-translated to SIGTSTP
//! by the tty driver. The `editor.suspend` command sets
//! `Editor::suspend_requested`, and after each keystroke the input
//! task checks the flag and, if set, runs the suspend/resume dance
//! implemented here:
//!
//! 1. Pop the Kitty keyboard protocol flags, disable mouse capture,
//!    leave the alternate screen, show the cursor, and disable raw
//!    mode — restoring the terminal to the state the shell expects.
//! 2. Flush stdout.
//! 3. Raise `SIGTSTP` on the current process. The Unix tty driver
//!    suspends the whole process group; control returns to the
//!    shell, which shows its prompt. The user types `fg` to resume.
//! 4. When the process is resumed (SIGCONT), the kernel returns
//!    from `raise()`. We re-enter the alternate screen, hide the
//!    cursor, re-enable mouse capture and the Kitty protocol, and
//!    re-enter raw mode — exactly the setup the `TerminalGuard`
//!    did at startup.
//! 5. Mark the editor dirty so the render task paints the full
//!    screen on the next cycle.
//!
//! On Windows there's no SIGTSTP, so the whole flow is a no-op that
//! just clears the suspend flag and shows a status message.

use std::io::{self, Write};

use arx_core::{CommandBus, Editor};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal;
use crossterm::{ExecutableCommand, cursor};
use tracing::warn;

/// Tear down the terminal, raise SIGTSTP to suspend the process,
/// and rebuild the terminal state on resume. Called by the input
/// task when `editor.suspend_requested()` is true.
pub async fn suspend_and_resume(bus: &CommandBus) {
    #[cfg(unix)]
    {
        if let Err(err) = do_suspend_unix() {
            warn!(%err, "suspend/resume failed");
        }
    }
    #[cfg(not(unix))]
    {
        let _ = bus;
        // On non-Unix platforms there's no SIGTSTP. Just clear the
        // flag and show a status message so the user knows the key
        // was received but isn't supported here.
        let _ = bus
            .dispatch(|editor| {
                editor.clear_suspend_request();
                editor.set_status("Suspend is not supported on this platform");
            })
            .await;
        return;
    }

    // Clear the flag and force a full redraw on resume. Uses the bus
    // so the mutation happens on the event-loop thread, keeping the
    // single-writer invariant.
    let _ = bus
        .dispatch(|editor: &mut Editor| {
            editor.clear_suspend_request();
            editor.mark_dirty();
        })
        .await;
}

#[cfg(unix)]
fn do_suspend_unix() -> io::Result<()> {
    use crossterm::event::{
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    };

    let mut out = io::stdout();

    // 1) Tear down interactive terminal state.
    let _ = out.execute(PopKeyboardEnhancementFlags);
    let _ = out.execute(DisableMouseCapture);
    let _ = out.execute(cursor::Show);
    let _ = out.execute(terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    out.flush()?;

    // 2) Raise SIGTSTP on ourselves. The kernel suspends the whole
    // process; this call returns when the user runs `fg`.
    //
    // SAFETY: `libc::raise` takes a signal number and is safe to
    // call from any thread. SIGTSTP is a valid POSIX signal that
    // suspends the process group.
    #[allow(unsafe_code)]
    let rc = unsafe { libc::raise(libc::SIGTSTP) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }

    // 3) Process resumed. Re-initialise the terminal exactly like
    // TerminalGuard::enable did on startup.
    terminal::enable_raw_mode()?;
    out.execute(terminal::EnterAlternateScreen)?;
    out.execute(cursor::Hide)?;
    out.execute(EnableMouseCapture)?;
    // Best-effort: re-enable the Kitty protocol. If the terminal
    // doesn't support it we still have a working editor.
    let _ = out.execute(PushKeyboardEnhancementFlags(
        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
    ));
    out.flush()?;

    Ok(())
}
