//! Language registry: maps file extensions to tree-sitter grammars and
//! highlight queries.

use std::collections::HashMap;
use std::sync::Arc;

/// Everything needed to parse and highlight one language.
#[derive(Debug)]
pub struct LanguageConfig {
    /// Human-readable name (`"Rust"`, `"Python"`, ...).
    pub name: &'static str,
    /// The tree-sitter language object.
    pub language: tree_sitter::Language,
    /// The `highlights.scm` query text for this language.
    pub highlights_query: &'static str,
}

/// Registry of bundled grammars, keyed by file extension.
#[derive(Debug)]
pub struct LanguageRegistry {
    by_extension: HashMap<&'static str, Arc<LanguageConfig>>,
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageRegistry {
    /// Build a registry with every bundled grammar.
    pub fn new() -> Self {
        let mut by_extension = HashMap::new();

        // Rust
        let rust = Arc::new(LanguageConfig {
            name: "Rust",
            language: tree_sitter_rust::LANGUAGE.into(),
            highlights_query: tree_sitter_rust::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("rs", Arc::clone(&rust));

        // Python
        let python = Arc::new(LanguageConfig {
            name: "Python",
            language: tree_sitter_python::LANGUAGE.into(),
            highlights_query: tree_sitter_python::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("py", Arc::clone(&python));
        by_extension.insert("pyi", Arc::clone(&python));

        // C
        let c = Arc::new(LanguageConfig {
            name: "C",
            language: tree_sitter_c::LANGUAGE.into(),
            highlights_query: tree_sitter_c::HIGHLIGHT_QUERY,
        });
        by_extension.insert("c", Arc::clone(&c));
        by_extension.insert("h", Arc::clone(&c));

        // JSON
        let json = Arc::new(LanguageConfig {
            name: "JSON",
            language: tree_sitter_json::LANGUAGE.into(),
            highlights_query: tree_sitter_json::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("json", Arc::clone(&json));

        Self { by_extension }
    }

    /// Look up the grammar for a file extension (without the leading
    /// dot). Returns `None` for unrecognised extensions.
    pub fn config_for_extension(&self, ext: &str) -> Option<Arc<LanguageConfig>> {
        self.by_extension.get(ext).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_extension_resolves() {
        let reg = LanguageRegistry::new();
        let config = reg.config_for_extension("rs").unwrap();
        assert_eq!(config.name, "Rust");
        assert!(!config.highlights_query.is_empty());
    }

    #[test]
    fn unknown_extension_returns_none() {
        let reg = LanguageRegistry::new();
        assert!(reg.config_for_extension("zzz").is_none());
    }
}
