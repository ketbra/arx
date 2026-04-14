//! Vendored Perl tree-sitter grammar.
//!
//! The upstream `tree-sitter-perl` crate (v1.1.2) depends on
//! `tree-sitter = "0.26"`, which conflicts with our workspace's
//! `tree-sitter = "0.25"` because both claim `links = "tree-sitter"`.
//! Vendoring the grammar source (`parser.c` + `scanner.c`) avoids the
//! conflict — the C parser is API-stable across tree-sitter versions
//! and only depends on the `LanguageFn` ABI exported by
//! `tree-sitter-language`.
//!
//! Source: <https://github.com/ganezdragon/tree-sitter-perl>
//! (vendored at v1.1.2 to match the upstream crate.)
//!
//! Usage:
//! ```no_run
//! let mut parser = tree_sitter::Parser::new();
//! parser
//!     .set_language(&arx_tree_sitter_perl::LANGUAGE.into())
//!     .expect("Error loading Perl parser");
//! ```

// SAFETY: this crate is a thin wrapper over a C parser produced by
// tree-sitter. The `extern "C"` declaration and `LanguageFn::from_raw`
// call are the standard tree-sitter binding pattern — see e.g.
// tree-sitter-rust's lib.rs for the same construction.
#![allow(unsafe_code)]

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_perl() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for the Perl grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_perl) };

/// The contents of `node-types.json` for this grammar.
pub const NODE_TYPES: &str = include_str!("node-types.json");

/// Highlight query for Perl. Hand-written to match the
/// [`ganezdragon/tree-sitter-perl`] grammar's named nodes.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../queries/highlights.scm");

#[cfg(test)]
mod tests {
    #[test]
    fn loads_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("Error loading Perl parser");
        let tree = parser.parse("print \"hello\\n\";\n", None).unwrap();
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn highlights_query_compiles() {
        let q = tree_sitter::Query::new(&super::LANGUAGE.into(), super::HIGHLIGHTS_QUERY);
        assert!(q.is_ok(), "highlights.scm failed to compile: {:?}", q.err());
    }
}
