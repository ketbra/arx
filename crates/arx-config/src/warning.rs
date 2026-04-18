//! Non-fatal config problems surfaced during load and apply.
//!
//! Kept in a single enum so the binary can iterate the full list,
//! print them to stderr before the alt-screen takes over, and
//! seed the first as a startup status message.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// `[[keymap.bindings]]` named a command the editor doesn't know.
    UnknownCommand { keys: String, command: String },
    /// `keys` failed to parse via `arx_keymap::parse::parse_sequence`.
    InvalidKeySequence { keys: String, error: String },
    /// `[appearance].theme` named a theme that isn't in the registry.
    UnknownTheme(String),
    /// `[[lsp.servers]]` referenced a `language_id` without providing
    /// `extensions`, so the override can never be looked up.
    UnknownLanguageId(String),
    /// `[lsp.servers.initialization_options]` couldn't be converted
    /// to JSON.
    InvalidInitOptions { language_id: String, error: String },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand { keys, command } => {
                write!(f, "unknown command `{command}` bound to `{keys}`")
            }
            Self::InvalidKeySequence { keys, error } => {
                write!(f, "invalid key sequence `{keys}`: {error}")
            }
            Self::UnknownTheme(name) => write!(f, "unknown theme `{name}`"),
            Self::UnknownLanguageId(id) => {
                write!(
                    f,
                    "lsp override for unknown language id `{id}` has no `extensions`; ignored"
                )
            }
            Self::InvalidInitOptions { language_id, error } => {
                write!(
                    f,
                    "lsp override for `{language_id}` has invalid initialization_options: {error}"
                )
            }
        }
    }
}
