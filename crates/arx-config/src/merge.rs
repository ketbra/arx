//! Apply parsed [`Config`] pieces to already-built state.
//!
//! The functions here stay free of an `arx-core` dependency by
//! accepting predicates/closures for anything they need to look up.
//! That keeps `arx-config` at the bottom of the dep graph.

use std::sync::Arc;

use arx_keymap::{parse::parse_sequence, profiles::Profile, Keymap};

use crate::schema::{BindingEntry, UnbindEntry};
use crate::warning::Warning;

/// Predicate a caller supplies so we can validate `command = "..."`
/// values against the editor's command registry.
pub type CommandExistsFn<'a> = &'a dyn Fn(&str) -> bool;

/// Layer user-provided bindings + unbinds on top of a profile's
/// global keymap.
///
/// Mutates `profile.global` via `Arc::make_mut` (cheap copy-on-write
/// if the profile's `Arc` isn't shared). Returns warnings for entries
/// that failed to apply; successful entries silently mutate the
/// profile.
pub fn apply_keymap_overrides(
    profile: &mut Profile,
    bindings: &[BindingEntry],
    unbind: &[UnbindEntry],
    command_exists: CommandExistsFn<'_>,
) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let map: &mut Keymap = Arc::make_mut(&mut profile.global);

    for entry in bindings {
        if let Err(err) = parse_sequence(&entry.keys) {
            warnings.push(Warning::InvalidKeySequence {
                keys: entry.keys.clone(),
                error: err.to_string(),
            });
            continue;
        }
        if !command_exists(&entry.command) {
            warnings.push(Warning::UnknownCommand {
                keys: entry.keys.clone(),
                command: entry.command.clone(),
            });
            continue;
        }
        if let Err(err) = map.bind_str(&entry.keys, entry.command.as_str()) {
            // Shouldn't happen — parse_sequence already succeeded —
            // but keep the branch for symmetry.
            warnings.push(Warning::InvalidKeySequence {
                keys: entry.keys.clone(),
                error: err.to_string(),
            });
        }
    }

    for entry in unbind {
        match parse_sequence(&entry.keys) {
            Err(err) => {
                warnings.push(Warning::InvalidKeySequence {
                    keys: entry.keys.clone(),
                    error: err.to_string(),
                });
            }
            Ok(chords) => {
                map.unbind(&chords);
            }
        }
    }

    warnings
}
