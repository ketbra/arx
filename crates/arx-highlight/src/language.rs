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
    #[allow(clippy::too_many_lines)]
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

        // C++
        let cpp = Arc::new(LanguageConfig {
            name: "C++",
            language: tree_sitter_cpp::LANGUAGE.into(),
            highlights_query: tree_sitter_cpp::HIGHLIGHT_QUERY,
        });
        by_extension.insert("cpp", Arc::clone(&cpp));
        by_extension.insert("cc", Arc::clone(&cpp));
        by_extension.insert("cxx", Arc::clone(&cpp));
        by_extension.insert("hpp", Arc::clone(&cpp));
        by_extension.insert("hh", Arc::clone(&cpp));
        by_extension.insert("hxx", Arc::clone(&cpp));

        // JSON
        let json = Arc::new(LanguageConfig {
            name: "JSON",
            language: tree_sitter_json::LANGUAGE.into(),
            highlights_query: tree_sitter_json::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("json", Arc::clone(&json));
        by_extension.insert("jsonc", Arc::clone(&json));

        // JavaScript
        let javascript = Arc::new(LanguageConfig {
            name: "JavaScript",
            language: tree_sitter_javascript::LANGUAGE.into(),
            highlights_query: tree_sitter_javascript::HIGHLIGHT_QUERY,
        });
        by_extension.insert("js", Arc::clone(&javascript));
        by_extension.insert("jsx", Arc::clone(&javascript));
        by_extension.insert("mjs", Arc::clone(&javascript));
        by_extension.insert("cjs", Arc::clone(&javascript));

        // TypeScript
        let typescript = Arc::new(LanguageConfig {
            name: "TypeScript",
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            highlights_query: tree_sitter_typescript::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("ts", Arc::clone(&typescript));
        by_extension.insert("mts", Arc::clone(&typescript));
        by_extension.insert("cts", Arc::clone(&typescript));

        // TSX
        let tsx = Arc::new(LanguageConfig {
            name: "TSX",
            language: tree_sitter_typescript::LANGUAGE_TSX.into(),
            highlights_query: tree_sitter_typescript::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("tsx", Arc::clone(&tsx));

        // Go
        let go = Arc::new(LanguageConfig {
            name: "Go",
            language: tree_sitter_go::LANGUAGE.into(),
            highlights_query: tree_sitter_go::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("go", Arc::clone(&go));

        // TOML
        let toml = Arc::new(LanguageConfig {
            name: "TOML",
            language: tree_sitter_toml_ng::LANGUAGE.into(),
            highlights_query: tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("toml", Arc::clone(&toml));

        // Bash / Shell
        let bash = Arc::new(LanguageConfig {
            name: "Bash",
            language: tree_sitter_bash::LANGUAGE.into(),
            highlights_query: tree_sitter_bash::HIGHLIGHT_QUERY,
        });
        by_extension.insert("sh", Arc::clone(&bash));
        by_extension.insert("bash", Arc::clone(&bash));
        by_extension.insert("zsh", Arc::clone(&bash));

        // Java
        let java = Arc::new(LanguageConfig {
            name: "Java",
            language: tree_sitter_java::LANGUAGE.into(),
            highlights_query: tree_sitter_java::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("java", Arc::clone(&java));

        // C# — the crate doesn't expose HIGHLIGHTS_QUERY, so we bundle
        // a copy of its highlights.scm ourselves.
        let csharp = Arc::new(LanguageConfig {
            name: "C#",
            language: tree_sitter_c_sharp::LANGUAGE.into(),
            highlights_query: include_str!("../queries/csharp.scm"),
        });
        by_extension.insert("cs", Arc::clone(&csharp));

        // HTML
        let html = Arc::new(LanguageConfig {
            name: "HTML",
            language: tree_sitter_html::LANGUAGE.into(),
            highlights_query: tree_sitter_html::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("html", Arc::clone(&html));
        by_extension.insert("htm", Arc::clone(&html));

        // CSS
        let css = Arc::new(LanguageConfig {
            name: "CSS",
            language: tree_sitter_css::LANGUAGE.into(),
            highlights_query: tree_sitter_css::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("css", Arc::clone(&css));

        // Ruby
        let ruby = Arc::new(LanguageConfig {
            name: "Ruby",
            language: tree_sitter_ruby::LANGUAGE.into(),
            highlights_query: tree_sitter_ruby::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("rb", Arc::clone(&ruby));
        by_extension.insert("rake", Arc::clone(&ruby));
        by_extension.insert("gemspec", Arc::clone(&ruby));

        // Markdown
        let markdown = Arc::new(LanguageConfig {
            name: "Markdown",
            language: tree_sitter_md::LANGUAGE.into(),
            highlights_query: tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
        });
        by_extension.insert("md", Arc::clone(&markdown));
        by_extension.insert("markdown", Arc::clone(&markdown));

        // YAML
        let yaml = Arc::new(LanguageConfig {
            name: "YAML",
            language: tree_sitter_yaml::LANGUAGE.into(),
            highlights_query: tree_sitter_yaml::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("yaml", Arc::clone(&yaml));
        by_extension.insert("yml", Arc::clone(&yaml));

        // Lua
        let lua = Arc::new(LanguageConfig {
            name: "Lua",
            language: tree_sitter_lua::LANGUAGE.into(),
            highlights_query: tree_sitter_lua::HIGHLIGHTS_QUERY,
        });
        by_extension.insert("lua", Arc::clone(&lua));

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

    /// Verify every bundled grammar can compile its highlight query.
    /// This catches regressions where a grammar crate's query uses
    /// capture predicates not supported by our tree-sitter version.
    #[test]
    fn every_grammar_compiles_its_query() {
        let reg = LanguageRegistry::new();
        // Test one representative extension for each language.
        let extensions = [
            "rs", "py", "c", "cpp", "json", "js", "ts", "tsx", "go",
            "toml", "sh", "java", "cs", "html", "css", "rb", "md",
            "yaml", "lua",
        ];
        for ext in extensions {
            let config = reg
                .config_for_extension(ext)
                .unwrap_or_else(|| panic!("no config for extension {ext}"));
            let query = tree_sitter::Query::new(&config.language, config.highlights_query);
            assert!(
                query.is_ok(),
                "grammar {} failed to compile query: {:?}",
                config.name,
                query.err()
            );
        }
    }

    #[test]
    fn unknown_extension_returns_none() {
        let reg = LanguageRegistry::new();
        assert!(reg.config_for_extension("zzz").is_none());
    }
}
