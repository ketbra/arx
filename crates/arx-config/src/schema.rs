//! TOML schema types.
//!
//! The top-level [`Config`] mirrors the file layout documented in
//! `docs/spec.md`. Every section uses `#[serde(default)]` so a
//! partial config still deserialises cleanly; `deny_unknown_fields`
//! is applied section-by-section so a typo raises a hard error
//! instead of silently doing nothing.

use serde::Deserialize;

/// Root configuration object.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub keymap: KeymapSection,
    pub features: FeaturesSection,
    pub appearance: AppearanceSection,
    pub lsp: LspSection,
}

// ---------------------------------------------------------------------------
// [keymap]
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct KeymapSection {
    pub profile: KeymapProfile,
    #[serde(rename = "bindings")]
    pub bindings: Vec<BindingEntry>,
    #[serde(rename = "unbind")]
    pub unbind: Vec<UnbindEntry>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KeymapProfile {
    #[default]
    Emacs,
    Vim,
    Kedit,
}

impl KeymapProfile {
    /// The string form used in CLI flags and `[keymap].profile` values.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Emacs => "emacs",
            Self::Vim => "vim",
            Self::Kedit => "kedit",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BindingEntry {
    pub keys: String,
    pub command: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UnbindEntry {
    pub keys: String,
}

// ---------------------------------------------------------------------------
// [features]
// ---------------------------------------------------------------------------

/// Runtime feature toggles. All default to `true`; disabling one at
/// runtime short-circuits the corresponding subsystem inside the
/// existing `#[cfg(feature = "...")]` compile-gated arms (so
/// `--no-default-features` builds still work).
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct FeaturesSection {
    pub syntax: bool,
    pub lsp: bool,
    pub mouse: bool,
    pub kitty_keyboard_protocol: bool,
    pub extensions: bool,
}

impl Default for FeaturesSection {
    fn default() -> Self {
        Self {
            syntax: true,
            lsp: true,
            mouse: true,
            kitty_keyboard_protocol: true,
            extensions: true,
        }
    }
}

/// Simple POD carrier so downstream crates don't have to pull in all
/// of `arx-config` to read a toggle. Construct via `From<FeaturesSection>`.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeFeatures {
    pub syntax: bool,
    pub lsp: bool,
    pub mouse: bool,
    pub kitty_keyboard_protocol: bool,
    pub extensions: bool,
}

impl Default for RuntimeFeatures {
    fn default() -> Self {
        FeaturesSection::default().into()
    }
}

impl From<FeaturesSection> for RuntimeFeatures {
    fn from(s: FeaturesSection) -> Self {
        Self {
            syntax: s.syntax,
            lsp: s.lsp,
            mouse: s.mouse,
            kitty_keyboard_protocol: s.kitty_keyboard_protocol,
            extensions: s.extensions,
        }
    }
}

// ---------------------------------------------------------------------------
// [appearance]
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct AppearanceSection {
    pub theme: String,
    pub line_numbers: bool,
    /// Template string honoured by the modeline renderer. Tokens:
    /// `{name}`, `{modified}`, `{line}`, `{total}`, `{bytes}`,
    /// `{mode}`. `None` uses the built-in default format.
    pub status_format: Option<String>,
}

impl Default for AppearanceSection {
    fn default() -> Self {
        Self {
            theme: "one-dark".to_owned(),
            line_numbers: true,
            status_format: None,
        }
    }
}

// ---------------------------------------------------------------------------
// [lsp]
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LspSection {
    #[serde(rename = "servers")]
    pub servers: Vec<LspServerOverride>,
}

/// User-specified LSP server. Wins over the built-in registry when
/// `language_id` matches; introduces a new server entry when it
/// doesn't. `initialization_options` is an arbitrary TOML value,
/// converted to JSON at spawn time.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LspServerOverride {
    pub language_id: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extensions: Option<Vec<String>>,
    #[serde(default)]
    pub root_markers: Option<Vec<String>>,
    #[serde(default)]
    pub initialization_options: Option<toml::Value>,
}

// Manually implement Eq since `toml::Value` doesn't. `PartialEq` is
// derived above and is sufficient for our tests; `Eq` is not needed.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_sane() {
        let c = Config::default();
        assert_eq!(c.keymap.profile, KeymapProfile::Emacs);
        assert!(c.keymap.bindings.is_empty());
        assert!(c.features.syntax);
        assert!(c.features.lsp);
        assert!(c.features.mouse);
        assert!(c.features.kitty_keyboard_protocol);
        assert!(c.features.extensions);
        assert_eq!(c.appearance.theme, "one-dark");
        assert!(c.appearance.line_numbers);
        assert!(c.appearance.status_format.is_none());
        assert!(c.lsp.servers.is_empty());
    }

    #[test]
    fn empty_string_deserialises_to_default() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c, Config::default());
    }

    #[test]
    fn full_example_round_trip() {
        let src = r#"
[keymap]
profile = "vim"

[[keymap.bindings]]
keys = "C-c p"
command = "command-palette.open"

[[keymap.bindings]]
keys = "<F5>"
command = "buffer.save"

[[keymap.unbind]]
keys = "C-z"

[features]
syntax = true
lsp = false
mouse = true
kitty_keyboard_protocol = false
extensions = true

[appearance]
theme = "one-dark"
line_numbers = false
status_format = "{name}{modified}  ({line}/{total})"

[[lsp.servers]]
language_id = "python"
command = "pylsp"
args = ["--stdio"]
extensions = ["py"]

[lsp.servers.initialization_options]
"pylsp.plugins.ruff.enabled" = true
"#;
        let cfg: Config = toml::from_str(src).unwrap();
        assert_eq!(cfg.keymap.profile, KeymapProfile::Vim);
        assert_eq!(cfg.keymap.bindings.len(), 2);
        assert_eq!(cfg.keymap.bindings[0].keys, "C-c p");
        assert_eq!(cfg.keymap.bindings[0].command, "command-palette.open");
        assert_eq!(cfg.keymap.bindings[1].keys, "<F5>");
        assert_eq!(cfg.keymap.unbind.len(), 1);
        assert_eq!(cfg.keymap.unbind[0].keys, "C-z");
        assert!(!cfg.features.lsp);
        assert!(!cfg.features.kitty_keyboard_protocol);
        assert!(cfg.features.syntax);
        assert!(!cfg.appearance.line_numbers);
        assert_eq!(
            cfg.appearance.status_format.as_deref(),
            Some("{name}{modified}  ({line}/{total})")
        );
        assert_eq!(cfg.lsp.servers.len(), 1);
        let rust = &cfg.lsp.servers[0];
        assert_eq!(rust.language_id, "python");
        assert_eq!(rust.command, "pylsp");
        assert_eq!(rust.args, vec!["--stdio"]);
        assert!(rust.initialization_options.is_some());
    }

    #[test]
    fn unknown_top_level_key_is_rejected() {
        let src = "nonsense = 1\n";
        let err = toml::from_str::<Config>(src).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn unknown_feature_key_is_rejected() {
        let src = "[features]\nsyntx = true\n"; // typo
        let err = toml::from_str::<Config>(src).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn unknown_profile_is_rejected() {
        let src = "[keymap]\nprofile = \"nano\"\n";
        let err = toml::from_str::<Config>(src).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown variant") || msg.contains("nano"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn partial_features_inherit_defaults() {
        // Only override `lsp`; everything else should remain true.
        let src = "[features]\nlsp = false\n";
        let cfg: Config = toml::from_str(src).unwrap();
        assert!(!cfg.features.lsp);
        assert!(cfg.features.syntax);
        assert!(cfg.features.mouse);
        assert!(cfg.features.kitty_keyboard_protocol);
        assert!(cfg.features.extensions);
    }

    #[test]
    fn binding_requires_keys_and_command() {
        let src = "[[keymap.bindings]]\nkeys = \"C-a\"\n";
        let err = toml::from_str::<Config>(src).unwrap_err();
        assert!(err.to_string().contains("missing field"));
    }

    #[test]
    fn profile_as_str_round_trips() {
        for p in [KeymapProfile::Emacs, KeymapProfile::Vim, KeymapProfile::Kedit] {
            let ser = format!("[keymap]\nprofile = \"{}\"\n", p.as_str());
            let cfg: Config = toml::from_str(&ser).unwrap();
            assert_eq!(cfg.keymap.profile, p);
        }
    }
}
