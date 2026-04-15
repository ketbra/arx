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
        extensions: &["c", "h", "cc", "cpp", "cxx", "hpp", "hh", "hxx"],
    },
    LspServerConfig {
        name: "Go",
        language_id: "go",
        command: "gopls",
        args: &[],
        root_markers: &["go.mod", "go.sum"],
        extensions: &["go"],
    },
    LspServerConfig {
        name: "TypeScript",
        language_id: "typescript",
        command: "typescript-language-server",
        args: &["--stdio"],
        root_markers: &["tsconfig.json", "jsconfig.json", "package.json"],
        extensions: &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"],
    },
    LspServerConfig {
        name: "TOML",
        language_id: "toml",
        command: "taplo",
        args: &["lsp", "stdio"],
        root_markers: &["taplo.toml", ".taplo.toml", "Cargo.toml", "pyproject.toml"],
        extensions: &["toml"],
    },
    LspServerConfig {
        name: "Bash",
        language_id: "shellscript",
        command: "bash-language-server",
        args: &["start"],
        root_markers: &[".git"],
        extensions: &["sh", "bash", "zsh"],
    },
    LspServerConfig {
        name: "Java",
        language_id: "java",
        command: "jdtls",
        args: &[],
        root_markers: &["pom.xml", "build.gradle", "build.gradle.kts", ".project"],
        extensions: &["java"],
    },
    LspServerConfig {
        name: "C#",
        language_id: "csharp",
        command: "omnisharp",
        args: &["-lsp"],
        root_markers: &["*.sln", "*.csproj", "project.json", "omnisharp.json"],
        extensions: &["cs"],
    },
    LspServerConfig {
        name: "HTML",
        language_id: "html",
        command: "vscode-html-language-server",
        args: &["--stdio"],
        root_markers: &["package.json", ".git"],
        extensions: &["html", "htm"],
    },
    LspServerConfig {
        name: "CSS",
        language_id: "css",
        command: "vscode-css-language-server",
        args: &["--stdio"],
        root_markers: &["package.json", ".git"],
        extensions: &["css", "scss", "less"],
    },
    LspServerConfig {
        name: "Ruby",
        language_id: "ruby",
        command: "solargraph",
        args: &["stdio"],
        root_markers: &["Gemfile", "Rakefile", ".solargraph.yml"],
        extensions: &["rb", "rake", "gemspec"],
    },
    LspServerConfig {
        name: "Markdown",
        language_id: "markdown",
        command: "marksman",
        args: &["server"],
        root_markers: &[".marksman.toml", ".git"],
        extensions: &["md", "markdown"],
    },
    LspServerConfig {
        name: "YAML",
        language_id: "yaml",
        command: "yaml-language-server",
        args: &["--stdio"],
        root_markers: &[".git"],
        extensions: &["yaml", "yml"],
    },
    LspServerConfig {
        name: "Lua",
        language_id: "lua",
        command: "lua-language-server",
        args: &[],
        root_markers: &[".luarc.json", ".git"],
        extensions: &["lua"],
    },
    LspServerConfig {
        name: "Perl",
        language_id: "perl",
        command: "perlnavigator",
        args: &["--stdio"],
        root_markers: &["Makefile.PL", "Build.PL", "cpanfile", "dist.ini", ".git"],
        extensions: &["pl", "pm", "t"],
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
