# Arx — Claude Code project notes

Short-form status + architectural notes for Claude Code sessions
working on this repo. The long-form vision lives in
[`docs/spec.md`](docs/spec.md).

If this is your first session on the repo: read this file, then skim
`docs/spec.md` §1–§5 and §18 for vision and current-phase scope.
Everything else in the spec is forward-looking.

## Current status (Phase 2 in progress — splits + undo tree + tree-sitter + LSP + completion)

Phase 1 per spec §18 is complete. Phase 2 has five of seven items
done: **window splits** (item 1), **undo tree** (item 2),
**tree-sitter highlighting** (item 3), **LSP client** (item 4),
and **completion framework** (item 5). The editor has:

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
- **(Phase 2)** Multi-pane window splits via an `arx_core::Layout`
  tree. Horizontal / vertical splits nest arbitrarily, dividers are
  painted between panes, the active pane is the only one that shows a
  cursor. New stock commands: `window.split-horizontal`,
  `window.split-vertical`, `window.close`, `window.focus-next`,
  `window.focus-prev` — bound to `C-x 2/3/0/o` in Emacs and
  `C-w s/v/c/q/w/W` in Vim.
- **(Phase 2)** Split layouts now survive a daemon restart. The
  session format is `SessionFile` v2, which adds an optional
  `SerializedLayout` tree; v1 files still load through a backward-
  compat path and come back as the single-leaf layout they always
  were. On restore the saved layout is rehydrated against the
  freshly-reopened windows via an old-id → new-id remap, and splits
  whose leaves couldn't be restored (buffer missing, etc.) collapse
  into the surviving sibling.
- **(Phase 2)** Per-buffer **undo tree** (not stack) in
  `arx_buffer::history::UndoTree`. User edits that go through the
  stock commands (self-insert, newline, delete-backward/forward)
  push an `EditRecord` (offset + removed + inserted + cursor
  before/after + timestamp) via a single `user_edit` helper in
  `arx-core::stock`. `buffer.undo` walks the tree toward the root
  and inverts the edit; `buffer.redo` follows `last_active_child`
  and replays it. Typing after an undo creates a new *branch* under
  the current node rather than discarding the redo branch, so
  history isn't lost. Bound to `C-/`, `C-_`, `C-x u` (undo) and
  `M-_` (redo) in Emacs; `u` and `C-r` in Vim normal mode.

- **(Phase 2)** **Tree-sitter syntax highlighting** via the new
  `arx-highlight` crate. Bundled grammars for Rust, Python, C, and
  JSON; a `LanguageRegistry` maps file extensions to grammars; a
  `Theme` (One-Dark-flavoured) maps capture names to
  `arx_buffer::Face` values with hierarchical fallback. A per-buffer
  `Highlighter` holds the parser + tree + compiled query; incremental
  re-parse on each edit is typically sub-millisecond. Highlights are
  written as `PropertyValue::Decoration(Face)` into the buffer's
  `"syntax"` property layer with `InvalidateOnEdit` policy — the
  render pipeline picks them up via the existing
  `PropertyMap::styled_runs` path with zero changes to `arx-render`.
  `Editor::edit_with_highlight` and `Editor::attach_highlight` wire
  it into the edit and file-open paths; gated behind a `syntax`
  Cargo feature on `arx-core` (default-on) so the workspace can
  still cross-compile for `x86_64-pc-windows-gnu` without a MinGW
  C compiler.

- **(Phase 2)** **LSP client** via the new `arx-lsp` crate.
  Hand-rolled JSON-RPC 2.0 transport over stdio (no heavy framework
  deps — just `lsp-types` for protocol structs). `LspClient` wraps
  the transport with typed methods: `initialize`, `did_open`,
  `did_change`, `did_close`, `hover`, `shutdown`. `LspTransport`
  spawns the server process and runs reader/writer tokio tasks;
  responses are dispatched to per-request oneshot channels,
  notifications to an mpsc channel. Position helpers translate
  between LSP UTF-16 offsets and Arx byte offsets. Diagnostic
  converter maps `lsp_types::Diagnostic` → `arx_buffer::Diagnostic`
  + underline face via the property map. Hardcoded server configs
  for rust-analyzer, pyright, clangd, gopls. Editor gains an
  `LspNotifier` channel (feature-gated behind `lsp` on `arx-core`)
  that pushes `BufferOpened`/`BufferEdited`/`BufferClosed` events
  from the edit and file-open paths. Diagnostic navigation commands
  `lsp.next-diagnostic` / `lsp.prev-diagnostic` bound to `M-n`/`M-p`
  in Emacs, `]d`/`[d` in Vim normal mode.

- **(Phase 2)** **Completion framework** with a popup overlay near
  the cursor. `CompletionPopup` on `Editor` holds the item list,
  selection index, and anchor byte offset. The renderer paints a
  floating box with highlighted-row selection (scrollable, max 8
  rows). MVP trigger (`completion.trigger` / `M-/` in Emacs,
  `C-x C-o` in Vim) collects word-based completions from the buffer
  itself; LSP-based `textDocument/completion` is ready to plug in
  (the transport supports it, just needs an async dispatch path).
  A `completion` keymap layer routes `<Tab>`/`<Enter>` to accept,
  `<Esc>` to dismiss, `<Up>`/`<Down>` (`C-p`/`C-n`) to navigate.
  Accepting replaces the prefix (`anchor..cursor`) with the selected
  item's insert text via the `user_edit` path (so it's undoable).

**352 tests green** (up from Phase 1's 274).
`cargo clippy --workspace --all-targets` clean under the workspace
pedantic lint set.
`cargo check --workspace --target x86_64-pc-windows-gnu` clean.

## Crate map

| Crate | Role |
|---|---|
| `arx-buffer` | Rope buffer, property map, interval tree, buffer snapshots. |
| `arx-core` | `Editor`, event loop, command bus, buffer/window managers, session, palette, stock commands. Single-writer state lives here. Depends on `arx-highlight` (feature-gated behind `syntax`). |
| `arx-highlight` | Tree-sitter syntax highlighting. `HighlightManager`, per-buffer `Highlighter`, `LanguageRegistry`, `Theme`. Depends on `tree-sitter` + grammar crates (C build via `cc`). |
| `arx-lsp` | LSP client. JSON-RPC transport over stdio, `LspClient`, position helpers, diagnostic conversion, server config registry. Depends on `lsp-types`. |
| `arx-terminal` | Embedded terminal emulator. `TerminalPane` wraps `alacritty_terminal::Term` + `portable-pty` PTY, with a grid bridge to convert terminal cells to renderer-agnostic `TerminalCell`s. Renderer-agnostic so the same engine works for TUI and future GUI. |
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
  `visible_rows` / `visible_cols` for **every visible pane** (not
  just the active one) inside the same `bus.invoke` closure as the
  state read. This is how page-down, word-nav, and
  `Editor::ensure_active_cursor_visible` know the actual text-area
  size *in whichever pane is currently active* after a split.
- **Stock command descriptions are `&str`, not `&'static str`.**
  The trait method takes `&self` so extension commands can return
  borrows of their runtime-owned `String`. Stock commands still
  return literals; the lifetime change is invisible to them.
- **Two layout trees.** `arx_core::Layout` is the logical tree the
  editor mutates (splits, closes, focus cycling). `arx_render::LayoutTree`
  is the display projection the render layer consumes. The driver's
  `build_view_layout` helper is the only place that converts between
  them. Both have a `walk_pane_rects` / `walk_divider_rects`-shaped
  API so splitters and tests agree on geometry.
- **Splits share a buffer by default.** `window.split-horizontal` /
  `window.split-vertical` create a new window on *the same buffer*
  as the pane being split, giving two views of the same content.
  Switching one of them to a different buffer is follow-up work.
- **`window.close` refuses the last pane.** Closing the only leaf
  would leave the render task with nothing to draw, so the command
  is a no-op when `layout.leaves().len() <= 1`. `editor.quit` is the
  command for exiting the editor.
- **Inactive panes have no cursor.** `arx_render::view::render` only
  emits a cursor for the window whose id matches
  `ViewState::active_window`. Inactive panes still paint their text
  but have no blinking caret, which matches every other terminal
  editor's convention for "which pane will take my keystrokes".
- **Session schema is versioned and not self-describing.** Postcard
  has no field delimiters, so adding / removing a field on `Session`
  is a wire break. Bump `SessionFile::CURRENT_VERSION` and add a
  compat branch in `Session::load_from_path` — the current load path
  peels the version varint off with `postcard::take_from_bytes::<u32>`
  and then decodes the rest against the right schema. v1 files are
  read through `LegacySessionV1` and lifted into v2 with
  `layout = None`.
- **Undo tree is pure data; the buffer never touches it.**
  `Buffer::edit` is origin-agnostic and does *not* automatically push
  into `UndoTree`. Stock user-visible edit commands
  (`insert_at_cursor`, `buffer.delete-backward`, etc.) route through
  `arx_core::stock::user_edit`, which captures the pre-edit bytes,
  applies the edit, updates the window cursor, and *then* pushes
  an `EditRecord` to the buffer's tree. `buffer.undo` and
  `buffer.redo` apply the record back through `Buffer::edit` with
  `EditOrigin::System` so the inversion itself doesn't re-enter
  the tree. If you add a new user-facing edit path, route it
  through `user_edit` or undo/redo will skip it.
- **Undo is per-buffer, cursor is per-window.** Records carry
  `cursor_before` / `cursor_after` as raw byte offsets. Undo applies
  them to the *invoking* window only; other windows viewing the
  same buffer have their cursors clamped to `len_bytes` (so a
  shortened buffer doesn't leave them past the end) but otherwise
  keep their position. Good enough for Phase 2; a future anchor
  system (spec §8) would let every cursor follow edits precisely.

## Phase 2 roadmap (spec §18)

Recommended implementation order based on dependencies:

1. ~~**Window splits + layout tree**~~ — **DONE.** Nested splits
   render correctly, commands for split / close / focus-cycle are
   wired, per-pane viewport dimensions flow back through
   `build_view_state`, the pure view renderer paints both panes and
   a divider glyph. See `arx_core::Layout`,
   `arx_render::LayoutTree::walk_pane_rects`, and
   `arx_driver::render::build_view_layout`.
2. ~~**Undo tree**~~ — **DONE.** `arx_buffer::history::UndoTree`
   stores one `EditRecord` per user-visible edit with cursor
   before/after. `arx_core::stock::user_edit` is the single entry
   point that applies a user edit and pushes to the tree; the
   `buffer.undo` / `buffer.redo` commands invert / replay records.
   Branches aren't discarded on new edits after undo —
   `last_active_child` picks the most recently visited branch for
   redo. Not yet exposed: branch-next/prev keybindings and an undo
   visualiser (both straightforward follow-ups).
3. ~~**Tree-sitter highlighting**~~ — **DONE.** `arx-highlight`
   crate with bundled grammars (Rust, Python, C, JSON), One-Dark
   theme, per-buffer `Highlighter` with incremental re-parse.
   Highlights flow through `PropertyMap::styled_runs` into the
   render pipeline with no renderer changes. Gated behind
   `arx-core`'s `syntax` Cargo feature for environments without
   a C cross-compiler.
4. ~~**LSP client**~~ — **DONE.** `arx-lsp` crate with JSON-RPC
   transport, typed `LspClient` (initialize/didOpen/didChange/hover
   /shutdown), position helpers (UTF-16 ↔ byte), diagnostic
   conversion, server config registry (rust-analyzer/pyright/clangd
   /gopls). Editor wired with `LspNotifier` channel for buffer
   events. Diagnostic navigation commands `lsp.next-diagnostic` /
   `lsp.prev-diagnostic`. Feature-gated behind `lsp` on `arx-core`.
   The driver-side `LspManager` task spawns servers, sends
   `didOpen`/`didChange`/`didClose` as buffers are opened and
   edited, and processes incoming `publishDiagnostics` into the
   `"diagnostics"` property layer via the `CommandBus`. A dedicated
   per-server notification task handles server → editor dispatching
   so the main event loop is never blocked on LSP I/O.
5. ~~**Completion framework**~~ — **DONE.** `CompletionPopup` state
   on `Editor`, floating popup overlay in the renderer, word-based
   trigger as MVP, `completion` keymap layer for navigation/accept/
   dismiss. LSP-completion-request plumbing is ready for a follow-up
   async dispatch.
6. ~~**Embedded terminal**~~ — **DONE.** `arx-terminal` crate wraps
   `alacritty_terminal` (0.26) + `portable-pty` (0.9) into a
   renderer-agnostic `TerminalPane`. Terminal panes live alongside
   buffer windows in the layout tree (identified by a side-table
   on `Editor`). The render path branches per pane:
   `render_window` for buffers, `render_terminal_pane` for
   terminals. Input routing forwards keystrokes to the PTY when a
   terminal pane is focused; `C-\` breaks back to the editor's
   keymap for pane switching. `terminal.open` (`C-x t` in Emacs)
   splits the active pane and spawns a shell.
7. **Session management (attach/detach/list)** — builds on the
   existing Level-1 persistence + daemon architecture. Level-1 now
   persists the layout tree too (SessionFile v2), so restarts come
   back with splits intact. Remaining work here is mostly CLI +
   daemon protocol (attach / detach / list commands) rather than
   state capture. Undo trees are **not** persisted to disk yet;
   that's a follow-up.

**Next task recommendation: embedded terminal.** The remaining items
(embedded terminal and session management) are mostly standalone.
The embedded terminal would use a PTY crate to run a shell inside a
split pane; session management is CLI + daemon protocol work
(`arx attach` / `arx detach` / `arx list`).

## How to work here

Common commands:

- Run all tests: `cargo test --workspace`
- Clippy (workspace pedantic lint set): `cargo clippy --workspace --all-targets`
- Windows cross-check (pure-Rust crates only — `arx-highlight` needs
  MinGW for tree-sitter's C build):
  `cargo check --workspace --target x86_64-pc-windows-gnu --exclude arx-highlight --exclude arx-core --exclude arx-driver --exclude arx-sdk --exclude ext-hello --exclude arx`
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
