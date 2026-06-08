//! Directory policy for Context Harness CLI state.
//!
//! Workspace-local files live under `.ctx/`. User-global files use XDG base
//! directories with an app directory named `ctx` (without a leading dot).

use std::env;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "ctx";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSourceKind {
    Explicit,
    Env,
    Workspace,
    LegacyWorkspace,
    Global,
    BuiltIn,
}

#[derive(Debug, Clone)]
pub struct ConfigSource {
    pub kind: ConfigSourceKind,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub explicit: Option<PathBuf>,
    pub env_config: Option<PathBuf>,
    pub workspace: PathBuf,
    pub legacy_workspace: PathBuf,
    pub global: PathBuf,
}

impl ConfigPaths {
    pub fn resolve(&self) -> ConfigSource {
        if let Some(path) = &self.explicit {
            return ConfigSource {
                kind: ConfigSourceKind::Explicit,
                path: Some(path.clone()),
            };
        }
        if let Some(path) = &self.env_config {
            return ConfigSource {
                kind: ConfigSourceKind::Env,
                path: Some(path.clone()),
            };
        }
        if self.workspace.exists() {
            return ConfigSource {
                kind: ConfigSourceKind::Workspace,
                path: Some(self.workspace.clone()),
            };
        }
        if self.legacy_workspace.exists() {
            return ConfigSource {
                kind: ConfigSourceKind::LegacyWorkspace,
                path: Some(self.legacy_workspace.clone()),
            };
        }
        if self.global.exists() {
            return ConfigSource {
                kind: ConfigSourceKind::Global,
                path: Some(self.global.clone()),
            };
        }
        ConfigSource {
            kind: ConfigSourceKind::BuiltIn,
            path: None,
        }
    }

    pub fn has_explicit_source(&self) -> bool {
        self.explicit.is_some() || self.env_config.is_some()
    }

    pub fn has_workspace_source(&self) -> bool {
        self.workspace.exists() || self.legacy_workspace.exists()
    }
}

pub fn config_paths(explicit: Option<PathBuf>) -> ConfigPaths {
    ConfigPaths {
        explicit,
        env_config: env::var_os("CTX_CONFIG").map(PathBuf::from),
        workspace: workspace_config_path(),
        legacy_workspace: legacy_workspace_config_path(),
        global: config_dir().join("config.toml"),
    }
}

pub fn workspace_dir() -> PathBuf {
    PathBuf::from(".ctx")
}

pub fn workspace_config_path() -> PathBuf {
    workspace_dir().join("config.toml")
}

pub fn legacy_workspace_config_path() -> PathBuf {
    PathBuf::from("config").join("ctx.toml")
}

pub fn workspace_data_dir() -> PathBuf {
    workspace_dir().join("data")
}

pub fn workspace_db_path() -> PathBuf {
    workspace_data_dir().join("ctx.sqlite")
}

pub fn workspace_vector_index_dir() -> PathBuf {
    workspace_data_dir().join("vector-index").join("zvec")
}

pub fn workspace_cache_dir() -> PathBuf {
    workspace_dir().join("cache")
}

pub fn workspace_git_cache_dir() -> PathBuf {
    workspace_cache_dir().join("git")
}

pub fn config_dir() -> PathBuf {
    xdg_app_dir("CTX_CONFIG_DIR", "XDG_CONFIG_HOME", ".config")
}

pub fn data_dir() -> PathBuf {
    xdg_app_dir("CTX_DATA_DIR", "XDG_DATA_HOME", ".local/share")
}

#[allow(dead_code)]
pub fn cache_dir() -> PathBuf {
    xdg_app_dir("CTX_CACHE_DIR", "XDG_CACHE_HOME", ".cache")
}

pub fn state_dir() -> PathBuf {
    xdg_app_dir("CTX_STATE_DIR", "XDG_STATE_HOME", ".local/state")
}

#[allow(dead_code)]
pub fn models_dir() -> PathBuf {
    cache_dir().join("models")
}

pub fn telemetry_state_path() -> PathBuf {
    state_dir().join("telemetry.json")
}

pub fn registries_dir() -> PathBuf {
    data_dir().join("registries")
}

#[allow(dead_code)]
pub fn legacy_registries_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ctx")
        .join("registries")
}

fn xdg_app_dir(override_var: &str, xdg_var: &str, default_suffix: &str) -> PathBuf {
    if let Some(path) = absolute_env_path(override_var) {
        return path;
    }
    if let Some(base) = absolute_env_path(xdg_var) {
        return base.join(APP_DIR);
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(default_suffix)
        .join(APP_DIR)
}

fn absolute_env_path(var: &str) -> Option<PathBuf> {
    let value = env::var_os(var)?;
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Some(path)
    } else {
        eprintln!(
            "Warning: ignoring relative path in {}: {}",
            var,
            path.display()
        );
        None
    }
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

pub fn is_default_workspace_db_path(path: &Path) -> bool {
    path == workspace_db_path()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn with_env<F: FnOnce(&Path)>(vars: &[(&str, Option<&str>)], f: F) {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let old: Vec<(&str, Option<OsString>)> =
            vars.iter().map(|(k, _)| (*k, env::var_os(k))).collect();
        for (key, value) in vars {
            match value {
                Some(value) => env::set_var(key, value),
                None => env::remove_var(key),
            }
        }
        let tmp = TempDir::new().unwrap();
        let old_cwd = env::current_dir().unwrap();
        env::set_current_dir(tmp.path()).unwrap();
        f(tmp.path());
        env::set_current_dir(old_cwd).unwrap();
        for (key, value) in old {
            match value {
                Some(value) => env::set_var(key, value),
                None => env::remove_var(key),
            }
        }
    }

    #[test]
    fn xdg_defaults_use_ctx_subdirectories() {
        with_env(
            &[
                ("HOME", Some("/tmp/ctx-home")),
                ("XDG_CONFIG_HOME", None),
                ("XDG_DATA_HOME", None),
                ("XDG_CACHE_HOME", None),
                ("XDG_STATE_HOME", None),
                ("CTX_CONFIG_DIR", None),
                ("CTX_DATA_DIR", None),
                ("CTX_CACHE_DIR", None),
                ("CTX_STATE_DIR", None),
            ],
            |_| {
                assert_eq!(config_dir(), PathBuf::from("/tmp/ctx-home/.config/ctx"));
                assert_eq!(data_dir(), PathBuf::from("/tmp/ctx-home/.local/share/ctx"));
                assert_eq!(cache_dir(), PathBuf::from("/tmp/ctx-home/.cache/ctx"));
                assert_eq!(state_dir(), PathBuf::from("/tmp/ctx-home/.local/state/ctx"));
            },
        );
    }

    #[test]
    fn ctx_dir_overrides_win_over_xdg() {
        with_env(
            &[
                ("CTX_CACHE_DIR", Some("/tmp/ctx-cache")),
                ("XDG_CACHE_HOME", Some("/tmp/xdg-cache")),
            ],
            |_| {
                assert_eq!(cache_dir(), PathBuf::from("/tmp/ctx-cache"));
            },
        );
    }

    #[test]
    fn relative_env_overrides_are_ignored() {
        with_env(
            &[
                ("HOME", Some("/tmp/ctx-home")),
                ("CTX_DATA_DIR", Some("relative-data")),
                ("XDG_DATA_HOME", Some("relative-xdg")),
            ],
            |_| {
                assert_eq!(data_dir(), PathBuf::from("/tmp/ctx-home/.local/share/ctx"));
            },
        );
    }

    #[test]
    fn explicit_and_env_config_bypass_discovery() {
        with_env(&[("CTX_CONFIG", Some("/tmp/from-env.toml"))], |_| {
            let source = config_paths(Some(PathBuf::from("/tmp/explicit.toml"))).resolve();
            assert_eq!(source.kind, ConfigSourceKind::Explicit);
            assert_eq!(source.path, Some(PathBuf::from("/tmp/explicit.toml")));

            let source = config_paths(None).resolve();
            assert_eq!(source.kind, ConfigSourceKind::Env);
            assert_eq!(source.path, Some(PathBuf::from("/tmp/from-env.toml")));
        });
    }
}
