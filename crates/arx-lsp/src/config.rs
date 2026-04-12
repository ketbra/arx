//! LSP server configuration: which command to run for which language.

/// Configuration for one LSP server.
#[derive(Debug, Clone)]
pub struct LspServerConfig {
    /// Human-readable language name (`"Rust"`, `"Python"`, ...).
    pub name: &'static str,
    /// The LSP `languageId` string sent in `textDocument/didOpen`.
    pub language_id: &'static str,
    /// Command to spawn (e.g. `"rust-analyzer"`).
    pub command: &'static str,
    /// Arguments to pass to the command.
    pub args: &'static [&'static str],
    /// File-name patterns used to detect the workspace root. Walk up
    /// from the file's directory and stop at the first directory that
    /// contains any of these.
    pub root_markers: &'static [&'static str],
    /// File extensions this server handles (without the leading dot).
    pub extensions: &'static [&'static str],
}

/// Look up the server config for a file extension. Returns `None` for
/// extensions that don't have a known LSP server.
pub fn config_for_extension(ext: &str) -> Option<&'static LspServerConfig> {
    BUILTIN_CONFIGS.iter().find(|c| c.extensions.contains(&ext))
}

static BUILTIN_CONFIGS: &[LspServerConfig] = &[
    LspServerConfig {
        name: "Rust",
        language_id: "rust",
        command: "rust-analyzer",
        args: &[],
        root_markers: &["Cargo.toml", "Cargo.lock"],
        extensions: &["rs"],
    },
    LspServerConfig {
        name: "Python",
        language_id: "python",
        command: "pyright-langserver",
        args: &["--stdio"],
        root_markers: &["pyproject.toml", "setup.py", "setup.cfg", "requirements.txt"],
        extensions: &["py", "pyi"],
    },
    LspServerConfig {
        name: "C/C++",
        language_id: "c",
        command: "clangd",
        args: &[],
        root_markers: &["compile_commands.json", "CMakeLists.txt", ".clangd", "Makefile"],
        extensions: &["c", "h", "cc", "cpp", "cxx", "hpp"],
    },
    LspServerConfig {
        name: "Go",
        language_id: "go",
        command: "gopls",
        args: &[],
        root_markers: &["go.mod", "go.sum"],
        extensions: &["go"],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_extension_resolves() {
        let config = config_for_extension("rs").unwrap();
        assert_eq!(config.command, "rust-analyzer");
    }

    #[test]
    fn unknown_returns_none() {
        assert!(config_for_extension("zzz").is_none());
    }
}
