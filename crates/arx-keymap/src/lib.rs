//! Arx editor keymap engine.
//!
//! Holds the keys, keymap trie, and stateful engine that turns a stream
//! of terminal key events into command names the editor can dispatch
//! through its [`CommandRegistry`](https://docs.rs/arx-core). Designed
//! to be shared across Emacs-style, Vim-style, and KEDIT-style profiles
//! — see `docs/spec.md` §15 and the `profiles` module for stock maps.
//!
//! ## Architecture
//!
//! ```text
//!   crossterm::KeyEvent
//!         │
//!         │ KeyChord::from
//!         ▼
//!   ┌──────────────┐     pending sequence + count prefix
//!   │ KeymapEngine │◄───── mode stack (Arc<Keymap> per layer)
//!   └──────┬───────┘
//!          │ FeedOutcome::{Execute(name, count), Pending, Unbound}
//!          ▼
//!   arx_core::CommandRegistry → named command invocation
//! ```
//!
//! The engine is:
//!
//! * **Modeless-or-modal**: a single stack of keymap layers. Emacs pushes
//!   nothing; Vim pushes its `normal` map on top of the global map at
//!   startup and swaps to `insert` when the user presses `i`.
//! * **Prefix-aware**: each keymap is a trie, so `C-x C-s` and `C-x C-c`
//!   share their first chord cleanly.
//! * **Count-aware**: the engine can accumulate a numeric prefix (Vim
//!   `3j`, `22gg`) or ignore digits (Emacs).
//! * **Fallthrough-aware**: a key unbound in the top layer falls through
//!   to the next layer, unless an explicit [`Keymap::unbind`] blocks it.

pub mod commands;
pub mod engine;
pub mod key;
pub mod keymap;
pub mod parse;
pub mod profiles;

pub use engine::{CountMode, FeedOutcome, KeymapEngine, Layer, LayerId};
pub use key::{Key, KeyChord, KeyModifiers, NamedKey};
pub use keymap::{CommandRef, Keymap, Lookup};
pub use parse::{ParseError, parse_chord, parse_sequence};
pub use profiles::Profile;
