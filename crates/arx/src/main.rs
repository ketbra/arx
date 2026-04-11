//! Arx editor binary.
//!
//! Thin shim: parse CLI arguments, build an [`arx_driver::Driver`] with an
//! async hook that opens the requested files through [`arx_core::open_file`],
//! and run it to completion. All the real work lives in the library crates.
//!
//! Phase 1 only ships a tiny flag surface:
//!
//! ```text
//! arx [FILES...]
//! ```
//!
//! Later phases will grow this into the full CLI reference described in
//! `docs/spec.md` (sessions, daemon, headless commands, package manager).
//! For now `arx foo.rs bar.rs` opens the files and drops you into the
//! TUI; `Ctrl+S` saves the active buffer, `Ctrl+Q` quits.

use std::path::PathBuf;
use std::process::ExitCode;

use arx_driver::Driver;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "arx",
    about = "Arx editor",
    version,
    disable_help_subcommand = true
)]
struct Cli {
    /// Files to open. Each file becomes a buffer; the last one is the
    /// active window. Missing paths are opened as empty buffers bound
    /// to that path, so `arx new_file.rs` works as expected.
    files: Vec<PathBuf>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // The terminal guard has already been torn down by the time
            // we get here, so it's safe to print to stderr.
            eprintln!("arx: {err}");
            let mut source = err.source();
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let files = cli.files;

    let driver = Driver::new(|_editor| {
        // Nothing synchronous to seed — windows/buffers are opened by
        // the async hook below once the event loop is up.
    })
    .with_async_hook(move |bus| async move {
        for path in files {
            match arx_core::open_file(&bus, path.clone()).await {
                Ok((buffer_id, window_id)) => {
                    tracing::info!(
                        ?buffer_id,
                        ?window_id,
                        path = %path.display(),
                        "opened file"
                    );
                }
                Err(err) => {
                    tracing::warn!(%err, path = %path.display(), "failed to open file");
                }
            }
        }
        // If nothing opened (either because the CLI had no args or
        // every `open_file` failed), fall back to a scratch buffer so
        // the editor has something to render.
        let _ = bus
            .dispatch(|editor| {
                if editor.windows().active().is_none() {
                    let buf = editor.buffers_mut().create_scratch();
                    editor.windows_mut().open(buf);
                    editor.mark_dirty();
                }
            })
            .await;
    });

    driver.run().await?;
    Ok(())
}
