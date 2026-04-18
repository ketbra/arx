//! Where the config file lives and how it's read.
//!
//! Lookup order (first hit wins):
//!
//! 1. Explicit `--config <path>` (handled by the caller, passed here
//!    via [`load_or_default`]'s `explicit` arg).
//! 2. `$ARX_CONFIG` env var (absolute path).
//! 3. Platform default:
//!    * Linux/macOS: `$XDG_CONFIG_HOME/arx/config.toml`, else
//!      `$HOME/.config/arx/config.toml`.
//!    * Windows: `%APPDATA%\arx\config.toml`, else
//!      `%USERPROFILE%\arx\config.toml`.
//!
//! A missing *default-path* file is not an error — it returns
//! [`Config::default`] silently. A missing or malformed *explicit*
//! file is an error.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::schema::Config;
use crate::warning::Warning;

const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("config file not found: {0}")]
    NotFound(PathBuf),
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// Resolve the default config file path for the current platform.
/// Does not check whether the file exists.
#[must_use]
pub fn default_config_path() -> Option<PathBuf> {
    default_config_path_with(|var| std::env::var_os(var).map(PathBuf::from))
}

fn default_config_path_with<F>(get_env: F) -> Option<PathBuf>
where
    F: Fn(&str) -> Option<PathBuf>,
{
    #[cfg(target_os = "windows")]
    {
        let base = get_env("APPDATA").or_else(|| get_env("USERPROFILE"))?;
        Some(base.join("arx").join(CONFIG_FILE_NAME))
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(xdg) = get_env("XDG_CONFIG_HOME") {
            return Some(xdg.join("arx").join(CONFIG_FILE_NAME));
        }
        let home = get_env("HOME")?;
        Some(home.join(".config").join("arx").join(CONFIG_FILE_NAME))
    }
}

/// Load a config from a specific path. Always errors on missing or
/// malformed files.
pub fn load(path: &Path) -> Result<Config, LoadError> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(LoadError::NotFound(path.to_path_buf()));
        }
        Err(e) => {
            return Err(LoadError::Io {
                path: path.to_path_buf(),
                source: e,
            });
        }
    };
    toml::from_str::<Config>(&text).map_err(|source| LoadError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Discover + load a config, following the lookup order documented
/// at the module level. `explicit` takes precedence (hard errors if
/// missing); `skip` short-circuits to the default. Returns the
/// loaded config alongside any non-fatal warnings (currently empty
/// — `apply_keymap_overrides` is where warnings accumulate).
pub fn load_or_default(
    explicit: Option<&Path>,
    skip: bool,
) -> Result<(Config, Vec<Warning>), LoadError> {
    if skip {
        return Ok((Config::default(), Vec::new()));
    }
    if let Some(path) = explicit {
        // Explicit path: every failure is hard.
        return Ok((load(path)?, Vec::new()));
    }
    // Env override, still explicit-semantics (hard errors).
    if let Some(env_path) = std::env::var_os("ARX_CONFIG") {
        let p = PathBuf::from(env_path);
        return Ok((load(&p)?, Vec::new()));
    }
    // Default path: missing file is silent; parse errors still hard.
    let Some(default) = default_config_path() else {
        return Ok((Config::default(), Vec::new()));
    };
    match load(&default) {
        Ok(cfg) => Ok((cfg, Vec::new())),
        Err(LoadError::NotFound(_)) => Ok((Config::default(), Vec::new())),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_config(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("config.toml");
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn load_parses_valid_file() {
        let dir = TempDir::new().unwrap();
        let path = write_config(dir.path(), "[keymap]\nprofile = \"vim\"\n");
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.keymap.profile.as_str(), "vim");
    }

    #[test]
    fn load_errors_on_missing() {
        let dir = TempDir::new().unwrap();
        let err = load(&dir.path().join("nope.toml")).unwrap_err();
        assert!(matches!(err, LoadError::NotFound(_)));
    }

    #[test]
    fn load_errors_on_parse_failure() {
        let dir = TempDir::new().unwrap();
        let path = write_config(dir.path(), "garbage = = =");
        let err = load(&path).unwrap_err();
        assert!(matches!(err, LoadError::Parse { .. }));
    }

    #[test]
    fn load_or_default_skip_returns_default() {
        let (cfg, warnings) = load_or_default(None, true).unwrap();
        assert_eq!(cfg, Config::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn load_or_default_explicit_missing_is_hard_error() {
        let dir = TempDir::new().unwrap();
        let err = load_or_default(Some(&dir.path().join("x.toml")), false).unwrap_err();
        assert!(matches!(err, LoadError::NotFound(_)));
    }

    #[test]
    fn load_or_default_explicit_ok() {
        let dir = TempDir::new().unwrap();
        let path = write_config(dir.path(), "[features]\nlsp = false\n");
        let (cfg, _) = load_or_default(Some(&path), false).unwrap();
        assert!(!cfg.features.lsp);
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn default_path_respects_xdg_config_home() {
        let p = default_config_path_with(|v| {
            if v == "XDG_CONFIG_HOME" {
                Some(PathBuf::from("/tmp/xdg"))
            } else {
                None
            }
        })
        .unwrap();
        assert_eq!(p, PathBuf::from("/tmp/xdg/arx/config.toml"));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn default_path_falls_back_to_home_dot_config() {
        let p = default_config_path_with(|v| {
            if v == "HOME" {
                Some(PathBuf::from("/home/test"))
            } else {
                None
            }
        })
        .unwrap();
        assert_eq!(p, PathBuf::from("/home/test/.config/arx/config.toml"));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn default_path_none_when_no_env() {
        assert!(default_config_path_with(|_| None).is_none());
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn default_path_respects_appdata() {
        let p = default_config_path_with(|v| {
            if v == "APPDATA" {
                Some(PathBuf::from("C:\\Users\\test\\AppData\\Roaming"))
            } else {
                None
            }
        })
        .unwrap();
        assert_eq!(
            p,
            PathBuf::from("C:\\Users\\test\\AppData\\Roaming\\arx\\config.toml")
        );
    }
}
