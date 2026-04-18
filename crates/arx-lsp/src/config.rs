//! LSP server configuration: which command to run for which language.
//!
//! Two shapes coexist:
//!
//! * [`LspServerConfig`] — the `&'static`-backed built-in registry
//!   baked into the binary for well-known languages.
//! * [`OwnedLspServerConfig`] — heap-owned entries sourced from the
//!   user's config file.
//!
//! [`LspRegistry`] merges both. [`ResolvedLspConfig`] is the borrow
//! the client uses to spawn a server; callers should use
//! [`LspRegistry::config_for_extension`] /
//! [`LspRegistry::config_for_language_id`] rather than the raw
//! lookups directly.

/// Configuration for one built-in LSP server.
#[derive(Debug, Clone)]
pub struct LspServerConfig {
    pub name: &'static str,
    pub language_id: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub root_markers: &'static [&'static str],
    pub extensions: &'static [&'static str],
}

/// User-supplied override/extension of the registry. Wins over a
/// built-in with the same `language_id`; adds a new entry when the
/// `language_id` doesn't match any built-in.
#[derive(Debug, Clone)]
pub struct OwnedLspServerConfig {
    pub name: String,
    pub language_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub root_markers: Vec<String>,
    pub extensions: Vec<String>,
    pub initialization_options: Option<serde_json::Value>,
}

/// A resolved config ready to be spawned. Borrows from either the
/// static registry or a heap-owned override so the hot path doesn't
/// allocate.
#[derive(Debug)]
pub enum ResolvedLspConfig<'a> {
    Builtin(&'a LspServerConfig),
    Override(&'a OwnedLspServerConfig),
}

impl ResolvedLspConfig<'_> {
    pub fn name(&self) -> &str {
        match self {
            Self::Builtin(c) => c.name,
            Self::Override(c) => &c.name,
        }
    }

    pub fn language_id(&self) -> &str {
        match self {
            Self::Builtin(c) => c.language_id,
            Self::Override(c) => &c.language_id,
        }
    }

    pub fn command(&self) -> &str {
        match self {
            Self::Builtin(c) => c.command,
            Self::Override(c) => &c.command,
        }
    }

    /// Arguments to pass to the server process. Iterator form so
    /// callers don't have to distinguish `&[&str]` vs `Vec<String>`.
    pub fn args(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            Self::Builtin(c) => Box::new(c.args.iter().copied()),
            Self::Override(c) => Box::new(c.args.iter().map(String::as_str)),
        }
    }

    pub fn root_markers(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            Self::Builtin(c) => Box::new(c.root_markers.iter().copied()),
            Self::Override(c) => Box::new(c.root_markers.iter().map(String::as_str)),
        }
    }

    pub fn extensions(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            Self::Builtin(c) => Box::new(c.extensions.iter().copied()),
            Self::Override(c) => Box::new(c.extensions.iter().map(String::as_str)),
        }
    }

    /// The user-specified `initializationOptions` to send with
    /// `initialize`. Built-ins never carry these today — only
    /// overrides can.
    pub fn initialization_options(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Builtin(_) => None,
            Self::Override(c) => c.initialization_options.as_ref(),
        }
    }
}

/// Merged view over built-in and user-provided server configs. Cheap
/// to construct; clones of the underlying data are not made.
#[derive(Debug, Clone)]
pub struct LspRegistry {
    overrides: Vec<OwnedLspServerConfig>,
}

impl LspRegistry {
    /// Registry with no user overrides — identical to the legacy
    /// behaviour.
    pub fn builtin_only() -> Self {
        Self { overrides: Vec::new() }
    }

    /// Registry with a user-supplied override list. Overrides are
    /// searched before built-ins.
    pub fn with_overrides(overrides: Vec<OwnedLspServerConfig>) -> Self {
        Self { overrides }
    }

    /// Borrow the user overrides (for diagnostics/tests).
    pub fn overrides(&self) -> &[OwnedLspServerConfig] {
        &self.overrides
    }

    /// Resolve a server by file extension. Overrides win; built-ins
    /// are the fallback.
    pub fn config_for_extension(&self, ext: &str) -> Option<ResolvedLspConfig<'_>> {
        if let Some(o) = self
            .overrides
            .iter()
            .find(|o| o.extensions.iter().any(|e| e == ext))
        {
            return Some(ResolvedLspConfig::Override(o));
        }
        BUILTIN_CONFIGS
            .iter()
            .find(|c| c.extensions.contains(&ext))
            .map(ResolvedLspConfig::Builtin)
    }

    /// Resolve a server by LSP `languageId`. Overrides win.
    pub fn config_for_language_id(&self, id: &str) -> Option<ResolvedLspConfig<'_>> {
        if let Some(o) = self.overrides.iter().find(|o| o.language_id == id) {
            return Some(ResolvedLspConfig::Override(o));
        }
        BUILTIN_CONFIGS
            .iter()
            .find(|c| c.language_id == id)
            .map(ResolvedLspConfig::Builtin)
    }
}

impl Default for LspRegistry {
    fn default() -> Self {
        Self::builtin_only()
    }
}

/// Look up the server config for a file extension in the built-in
/// registry only. Kept for backward-compat; prefer
/// [`LspRegistry::config_for_extension`].
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

    fn py_override() -> OwnedLspServerConfig {
        OwnedLspServerConfig {
            name: "Python (pylsp)".into(),
            language_id: "python".into(),
            command: "pylsp".into(),
            args: vec!["--stdio".into()],
            root_markers: vec!["pyproject.toml".into()],
            extensions: vec!["py".into()],
            initialization_options: None,
        }
    }

    #[test]
    fn registry_override_wins_over_builtin_by_language_id() {
        let reg = LspRegistry::with_overrides(vec![py_override()]);
        let got = reg.config_for_language_id("python").unwrap();
        assert_eq!(got.command(), "pylsp");
        assert!(matches!(got, ResolvedLspConfig::Override(_)));
    }

    #[test]
    fn registry_override_wins_over_builtin_by_extension() {
        let reg = LspRegistry::with_overrides(vec![py_override()]);
        let got = reg.config_for_extension("py").unwrap();
        assert_eq!(got.command(), "pylsp");
    }

    #[test]
    fn registry_falls_back_to_builtin() {
        let reg = LspRegistry::with_overrides(vec![py_override()]);
        let got = reg.config_for_extension("rs").unwrap();
        assert_eq!(got.command(), "rust-analyzer");
        assert!(matches!(got, ResolvedLspConfig::Builtin(_)));
    }

    #[test]
    fn registry_extends_for_new_language_id() {
        let elm = OwnedLspServerConfig {
            name: "Elm".into(),
            language_id: "elm".into(),
            command: "elm-language-server".into(),
            args: vec![],
            root_markers: vec!["elm.json".into()],
            extensions: vec!["elm".into()],
            initialization_options: None,
        };
        let reg = LspRegistry::with_overrides(vec![elm]);
        let got = reg.config_for_extension("elm").unwrap();
        assert_eq!(got.command(), "elm-language-server");
    }

    #[test]
    fn builtin_only_matches_static() {
        let reg = LspRegistry::builtin_only();
        assert_eq!(
            reg.config_for_extension("rs").unwrap().command(),
            "rust-analyzer"
        );
        assert!(reg.config_for_extension("zzz").is_none());
    }

    #[test]
    fn args_iter_works_for_both_variants() {
        let reg = LspRegistry::with_overrides(vec![py_override()]);
        let ov = reg.config_for_language_id("python").unwrap();
        let ov_args: Vec<&str> = ov.args().collect();
        assert_eq!(ov_args, vec!["--stdio"]);
        let bi = reg.config_for_extension("rs").unwrap();
        let bi_args: Vec<&str> = bi.args().collect();
        assert!(bi_args.is_empty());
    }
}
