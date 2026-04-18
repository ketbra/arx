//! Tree-sitter syntax highlighting for the Arx editor.
//!
//! This crate sits between [`arx_buffer`] (which provides the rope,
//! property map, and face types) and [`arx_core`] (which owns the
//! editor state and wires highlighting into the edit flow). It
//! provides:
//!
//! * [`HighlightManager`] — the per-editor orchestrator. Owns a
//!   [`LanguageRegistry`], a [`Theme`], and a map of per-buffer
//!   [`Highlighter`]s. Call [`HighlightManager::attach_buffer`] when a
//!   file is opened (detects language from extension, full-parses, and
//!   populates the `"syntax"` property layer) and
//!   [`HighlightManager::on_edit`] after every buffer edit (incremental
//!   re-parse + re-highlight of the affected range).
//! * [`Highlighter`] — per-buffer parser, parse tree, and compiled
//!   highlight query. Incremental re-parses are typically sub-ms.
//! * [`LanguageRegistry`] — maps file extensions to bundled tree-sitter
//!   grammars and their highlight queries.
//! * [`Theme`] — maps tree-sitter capture names (`@keyword`,
//!   `@function`, etc.) to [`arx_buffer::Face`] values.
//!
//! The rendering pipeline doesn't change: `arx-render` already reads
//! [`arx_buffer::PropertyMap::styled_runs`] and merges the resulting
//! [`arx_buffer::Face`]s into cell colours. This crate just populates
//! the `"syntax"` layer that feeds those runs.

pub mod highlighter;
pub mod language;
pub mod theme;

use std::collections::HashMap;

use arx_buffer::{AdjustmentPolicy, Buffer, BufferId, Edit};

pub use highlighter::{HighlightError, Highlighter};
pub use language::{LanguageConfig, LanguageRegistry};
pub use theme::Theme;

/// The syntax layer name used in [`arx_buffer::PropertyMap`]. Matches
/// the layer name already used in `arx_buffer`'s own property tests.
const SYNTAX_LAYER: &str = "syntax";

/// Per-editor highlight orchestrator.
#[derive(Debug)]
pub struct HighlightManager {
    registry: LanguageRegistry,
    theme: Theme,
    highlighters: HashMap<BufferId, Highlighter>,
}

impl Default for HighlightManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HighlightManager {
    /// Create a manager with the default language registry and dark
    /// theme.
    pub fn new() -> Self {
        Self {
            registry: LanguageRegistry::new(),
            theme: Theme::default_dark(),
            highlighters: HashMap::new(),
        }
    }

    /// Swap the active theme. Existing attached highlighters keep
    /// their parse trees; the next edit re-paints with the new theme.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Attach a highlighter to `buffer`, detecting the language from
    /// `extension` (without the leading dot). Does a full parse and
    /// populates the `"syntax"` property layer.
    ///
    /// No-op if `extension` is `None` or doesn't map to a known
    /// grammar.
    pub fn attach_buffer(
        &mut self,
        buffer: &mut Buffer,
        extension: Option<&str>,
    ) {
        let Some(ext) = extension else { return };
        let Some(config) = self.registry.config_for_extension(ext) else {
            return;
        };
        let mut hl = match Highlighter::new(config) {
            Ok(hl) => hl,
            Err(err) => {
                tracing::warn!(%err, "failed to create highlighter");
                return;
            }
        };
        hl.parse_full(buffer.rope());
        let intervals = hl.highlight_all(buffer.rope(), &self.theme);
        let layer = buffer
            .properties_mut()
            .ensure_layer(SYNTAX_LAYER, AdjustmentPolicy::InvalidateOnEdit);
        layer.clear();
        for iv in intervals {
            layer.insert(iv);
        }
        layer.clear_dirty();
        self.highlighters.insert(buffer.id(), hl);
    }

    /// Detach the highlighter for `buffer_id`, if any. The `"syntax"`
    /// layer stays on the buffer (stale highlights until the next
    /// render clears them) but won't be updated on subsequent edits.
    pub fn detach_buffer(&mut self, buffer_id: BufferId) {
        self.highlighters.remove(&buffer_id);
    }

    /// Incrementally update the syntax highlights after `edit` was
    /// applied to `buffer`. Runs the tree-sitter incremental re-parse
    /// and re-highlights the dirty range.
    ///
    /// No-op if the buffer has no attached highlighter.
    pub fn on_edit(&mut self, buffer: &mut Buffer, edit: &Edit) {
        let Some(hl) = self.highlighters.get_mut(&buffer.id()) else {
            return;
        };
        hl.apply_edit(edit, buffer.rope());
        // Re-highlight the full buffer for now. A smarter
        // implementation would query only the dirty/changed ranges,
        // but tree-sitter's query cursor with set_byte_range makes
        // this cheap even for large files, and correctness is easier
        // to verify with a full pass. Profile later.
        let intervals = hl.highlight_all(buffer.rope(), &self.theme);
        let layer = buffer
            .properties_mut()
            .ensure_layer(SYNTAX_LAYER, AdjustmentPolicy::InvalidateOnEdit);
        layer.clear();
        for iv in intervals {
            layer.insert(iv);
        }
        layer.clear_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_buffer::{Buffer, BufferId, EditOrigin};

    #[test]
    fn attach_populates_syntax_layer() {
        let mut mgr = HighlightManager::new();
        let mut buf = Buffer::from_str(BufferId(1), "fn main() {}");
        mgr.attach_buffer(&mut buf, Some("rs"));
        let layer = buf.properties().layer(SYNTAX_LAYER);
        assert!(layer.is_some(), "syntax layer should exist");
        let count = layer.unwrap().tree().iter().count();
        assert!(count > 0, "syntax layer should have intervals");
    }

    #[test]
    fn on_edit_updates_highlights() {
        let mut mgr = HighlightManager::new();
        let mut buf = Buffer::from_str(BufferId(1), "fn f() {}");
        mgr.attach_buffer(&mut buf, Some("rs"));
        let before = buf.properties().layer(SYNTAX_LAYER).unwrap().tree().iter().count();

        let edit = buf.edit(4..4, "oo", EditOrigin::User);
        mgr.on_edit(&mut buf, &edit);
        let after = buf.properties().layer(SYNTAX_LAYER).unwrap().tree().iter().count();
        // The exact count may differ, but the layer should still be
        // populated (not empty after re-highlight).
        assert!(after > 0, "should still have highlights after edit");
        let _ = before;
    }

    #[test]
    fn attach_on_unknown_extension_is_noop() {
        let mut mgr = HighlightManager::new();
        let mut buf = Buffer::from_str(BufferId(1), "hello");
        mgr.attach_buffer(&mut buf, Some("zzz"));
        assert!(buf.properties().layer(SYNTAX_LAYER).is_none());
    }

    #[test]
    fn detach_removes_highlighter() {
        let mut mgr = HighlightManager::new();
        let mut buf = Buffer::from_str(BufferId(1), "fn f() {}");
        mgr.attach_buffer(&mut buf, Some("rs"));
        assert!(mgr.highlighters.contains_key(&BufferId(1)));
        mgr.detach_buffer(BufferId(1));
        assert!(!mgr.highlighters.contains_key(&BufferId(1)));
    }
}
