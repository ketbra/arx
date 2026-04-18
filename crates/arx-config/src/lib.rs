//! User-facing TOML configuration for the arx editor.
//!
//! Loads `~/.config/arx/config.toml` (or the platform equivalent) into
//! a [`Config`] struct that the binary feeds into [`Driver`] and
//! [`Editor`]. Keymap profile choice, keybinding overrides, runtime
//! feature toggles, appearance settings, and LSP server overrides all
//! flow through this crate.
//!
//! Intentional non-goals:
//!
//! * Scripting. The programmatic escape hatch is the Rust extension
//!   SDK (`arx-sdk`), not a Lua runtime.
//! * Hot-reload. v1 loads once at startup.
//! * Direct coupling to `arx-core`. Appliers take predicates so this
//!   crate stays at the bottom of the dep graph.

mod discovery;
mod merge;
mod schema;
mod warning;

pub use discovery::{default_config_path, load, load_or_default, LoadError};
pub use merge::{apply_keymap_overrides, CommandExistsFn};
pub use schema::{
    AppearanceSection, BindingEntry, Config, FeaturesSection, KeymapProfile, KeymapSection,
    LspSection, LspServerOverride, RuntimeFeatures, UnbindEntry,
};
pub use warning::Warning;
