# Arx — Claude Code project notes

Short-form status + architectural notes for Claude Code sessions
working on this repo. The long-form vision lives in
[`docs/spec.md`](docs/spec.md).

If this is your first session on the repo: read this file, then skim
`docs/spec.md` §1–§5 and §18 for vision and current-phase scope.
Everything else in the spec is forward-looking.

## Current status (end of Phase 1)

Phase 1 per spec §18 is complete. The editor has:

- A working daemon/client split with Unix-domain-socket and
  Windows-named-pipe IPC, verified by cross-compilation.
- Ropey-backed buffers with interval-tree property layers.
- A single-writer async event loop + command bus.
- A pure-function `View → RenderTree → Diff → CrosstermBackend` pipeline.
- Emacs and Vim keymap profiles plus a stock command catalogue
  (cursor motion incl. word/buffer-edge, page scroll, self-insert,
  delete, newline, save, quit, mode switch).
- File open/save, Level-1 session persistence (save on clean
  shutdown, reload on next start), and an `M-x` fuzzy command palette.
- A real extension SDK: `arx-sdk` with `Extension` trait +
  `declare_extension!` macro, and `arx-driver::ext_host` +
  `ext_watcher` that load `cdylib` extensions via `libloading` with
  hot-reload on file change.

**382 tests green.** `cargo clippy --workspace --all-targets` clean
under the workspace pedantic lint set.
`cargo check --workspace --target x86_64-pc-windows-gnu` clean.

## Crate map

| Crate | Role |
|---|---|
| `arx-buffer` | Rope buffer, property map, interval tree, buffer snapshots. |
| `arx-core` | `Editor`, event loop, command bus, buffer/window managers, session, palette, stock commands. Single-writer state lives here. |
| `arx-keymap` | Keymap engine, chord parser, Emacs + Vim profiles, command name constants. |
| `arx-render` | `ViewState → RenderTree → Diff → Backend`. Includes `CrosstermBackend` and `TestBackend`. |
| `arx-protocol` | Wire types, postcard framing, cross-platform IPC transport. |
| `arx-driver` | Ties it all together: input task, render task, daemon, client, extension host, file watcher. |
| `arx-sdk` | Extension author SDK — `Extension` trait, `ActivationContext`, `declare_extension!`. Depends on `arx-core`; re-exports it as `arx_sdk::core`. |
| `arx` | Binary. `arx`, `arx daemon`, `arx client`. |
| `examples/ext-hello` | Reference `cdylib` extension. |

## Architectural invariants

These are the load-bearing rules of the codebase. Don't break them
without a very good reason and a commit message that says why.

- **Single writer.** Only the event loop task holds `&mut Editor`.
  Everything else dispatches through `CommandBus::invoke` /
  `CommandBus::dispatch`. Don't add a second mutable-access path.
- **View is a pure function.** `arx_render::render(&ViewState) ->
  RenderTree` has no side effects. The driver's
  `arx_driver::render::build_view_state` is the *only* place that
  flattens editor state into a `ViewState`.
- **No `unsafe` except in `arx-driver::ext_host`.** Workspace lint
  is `unsafe_code = "deny"`. `ext_host` has a module-local
  `#![allow(unsafe_code)]` with `SAFETY:` comments on every
  `unsafe` block (it's required for the libloading FFI). If you
  need unsafe elsewhere, stop and justify it explicitly.
- **Same-compiler contract for extensions.** v0.1 of the SDK relies
  on the host and extensions being built from the same `rustc`
  toolchain. The `arx_sdk_version()` check catches the common case;
  a later milestone will swap to `abi_stable` for cross-toolchain
  interop.
- **Loaded extension libraries are never dropped**
  (`ManuallyDrop<Library>`). Rust cdylibs hit a null-fn-ptr segfault
  in glibc's `_dl_call_fini` on `dlclose`. Hot-reload leaks the old
  library; commands are unregistered from the editor so they're
  unreachable. The extension hot-reload integration test registers
  `atexit(libc::_exit(0))` to dodge the same issue at process
  teardown.

## Gotchas (sharp edges to know)

- **Initial paint must touch every cell.**
  `arx_render::diff::initial_paint` emits a `SetCell` for every grid
  position even if it equals `Cell::blank()`. Skipping blanks lets
  the terminal's own default background bleed through for spaces and
  produces a mottled light/dark pattern. See commit `66dd27f`.
- **Crossterm normalises shift on printable chars.** The conversion
  in `arx_keymap::key::KeyChord::from(&KeyEvent)` strips the shift
  bit for `Char` keys so `M-<` matches on terminals like Kitty that
  report `Shift+<` with the shift bit set. Named keys (F-keys, Tab,
  arrows) keep shift.
- **Render task writes viewport dimensions back to `WindowData`.**
  `arx_driver::render::build_view_state` updates
  `visible_rows` / `visible_cols` on the active window inside the
  same `bus.invoke` closure as the state read. This is how
  page-down, word-nav, and `Editor::ensure_active_cursor_visible`
  know the actual text area size.
- **Stock command descriptions are `&str`, not `&'static str`.**
  The trait method takes `&self` so extension commands can return
  borrows of their runtime-owned `String`. Stock commands still
  return literals; the lifetime change is invisible to them.

## Phase 2 roadmap (spec §18)

Recommended implementation order based on dependencies:

1. **Window splits + layout tree** — unblocks everything else.
   Touches `arx-core::window`, `arx-render::view_state::LayoutTree`
   (which already has a `Split` variant stubbed out), and the
   render loop. The existing `LayoutTree::Split` branch in
   `arx_render::view::render` is marked `TODO(phase-2)` and ready
   to be filled in.
2. **Undo tree** — self-contained in `arx-buffer` + `arx-core`.
   Doesn't depend on splits; can run in parallel.
3. **Tree-sitter highlighting** — plugs into
   `arx_buffer::PropertyMap` as a new property layer. Spec §4.2.
4. **LSP client** — needs splits for hover/diagnostic popups.
5. **Completion framework** — needs LSP.
6. **Embedded terminal** — mostly standalone (termwiz-based).
7. **Session management (attach/detach/list)** — builds on the
   existing Level-1 persistence + daemon architecture. Mostly
   CLI + daemon protocol work.

**First task recommendation: window splits.** Single biggest blocker
for every other Phase 2 feature.

## How to work here

Common commands:

- Run all tests: `cargo test --workspace`
- Clippy (workspace pedantic lint set): `cargo clippy --workspace --all-targets`
- Windows cross-check: `cargo check --workspace --target x86_64-pc-windows-gnu`
  (one-off `rustup target add x86_64-pc-windows-gnu` first).
- Run the daemon locally: `cargo run --bin arx -- daemon --no-session --no-extensions`
  (skips persistence + extension loading for quick iteration).
- Run the client: `cargo run --bin arx -- client` (in another terminal).
- Run the embedded editor: `cargo run --bin arx -- <file>`.

Conventions for new code:

- New stock commands go in `crates/arx-core/src/stock.rs` via the
  `stock_cmd!` macro; their name constants live in
  `crates/arx-keymap/src/commands.rs`; profile bindings live in
  `crates/arx-keymap/src/profiles.rs`.
- New tests live next to the code they test (`#[cfg(test)] mod tests`)
  for unit-level coverage. Cross-task integration tests live in
  `crates/<crate>/tests/*.rs`.
- Don't bypass the `CommandBus` to mutate `Editor`. If you find
  yourself wanting to, you're probably reaching for the wrong
  abstraction.

## Doc pointers

- [`docs/spec.md`](docs/spec.md) — long-term vision (1764 lines).
  Phase 1 = §18. Phase 2 starts at §18 too.
- `crates/arx-core/src/session.rs` — persistence ladder (Level 0–3).
- `crates/arx-driver/src/ext_host.rs` — extension ABI caveats and
  the `ManuallyDrop<Library>` story.
- `crates/arx-sdk/src/lib.rs` — how to write an extension.
- `examples/ext-hello/src/lib.rs` — copy-pasteable starting point
  for a new extension.
