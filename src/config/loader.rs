use std::path::PathBuf;
use std::sync::Mutex;

use super::settings::Settings;

/// Tracks the active config path so `save_settings()` writes to the same file
/// that was loaded (custom or default).
static ACTIVE_CONFIG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Resolve the default config file path (`~/.config/bionic-graph/settings.json`).
pub fn config_file_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE")) // Windows
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("bionic-graph")
        .join("settings.json")
}

/// Load settings from the default config path, or create and save defaults.
///
/// Shortcut for `load_or_create_settings_from(None)`.
pub fn load_or_create_settings() -> Settings {
    load_or_create_settings_from(None)
}

/// Load settings from an optional custom config path, or create and save
/// defaults.
///
/// - If `config_path` is `Some(path)`, loads from that path.
/// - If `None`, uses `~/.config/bionic-graph/settings.json`.
/// - If the file does not exist, writes default settings to that path.
///
/// Priority (highest wins):
/// 1. Environment variables (`BGRAPH_HOST`, `BGRAPH_PORT`, etc.)
/// 2. Config file
/// 3. Built-in defaults
pub fn load_or_create_settings_from(config_path: Option<PathBuf>) -> Settings {
    let path = config_path.unwrap_or_else(config_file_path);

    // Record the active path so save_settings() can write back to the same file.
    if let Ok(mut active) = ACTIVE_CONFIG_PATH.lock() {
        *active = Some(path.clone());
    }

    let mut settings = if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<Settings>(&content) {
                Ok(s) => {
                    log::info!("Loaded settings from {:?}", path);
                    s
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse {:?}: {}. Using defaults.",
                        path,
                        e
                    );
                    Settings::default()
                }
            },
            Err(e) => {
                log::warn!("Failed to read {:?}: {}. Using defaults.", path, e);
                Settings::default()
            }
        }
    } else {
        log::info!("No config file at {:?}, creating defaults.", path);
        let s = Settings::default();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        write_settings_inner(&path, &s);
        s
    };

    // Environment variable overrides (only host/port/data-dir remain)
    apply_env_overrides(&mut settings);

    settings
}

/// Save settings back to the active config file path.
///
/// The active path is the one most recently passed to
/// `load_or_create_settings_from`. Falls back to the default
/// `~/.config/bionic-graph/settings.json` if none was loaded.
pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = ACTIVE_CONFIG_PATH
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(config_file_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config dir: {}", e))?;
    }
    write_settings_inner(&path, settings);
    Ok(())
}

/// Save settings to a specific path.
fn write_settings_inner(path: &std::path::Path, settings: &Settings) {
    match serde_json::to_string_pretty(settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, &json) {
                log::error!("Failed to write config to {:?}: {}", path, e);
            } else {
                log::info!("Config saved to {:?}", path);
            }
        }
        Err(e) => {
            log::error!("Failed to serialize config: {}", e);
        }
    }
}

/// Override settings from environment variables where set.
fn apply_env_overrides(settings: &mut Settings) {
    // Server
    if let Ok(val) = std::env::var("BGRAPH_HOST") {
        settings.server.host = val;
    }
    if let Ok(val) = std::env::var("BGRAPH_PORT") {
        if let Ok(port) = val.parse::<u16>() {
            settings.server.port = port;
        }
    }

    // Storage
    if let Ok(val) = std::env::var("BGRAPH_DATA_DIR") {
        settings.graph.storage.data_dir = val;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_config_path() {
        let path = config_file_path();
        assert!(path.ends_with(".config/bionic-graph/settings.json")
            || path.ends_with(".config\\bionic-graph\\settings.json"));
    }

    #[test]
    fn test_default_settings_roundtrip() {
        let s = Settings::default();
        let json = serde_json::to_string_pretty(&s).unwrap();
        let parsed: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.server.port, 8080);
        assert_eq!(parsed.llm.default_model, "DeepSeek/deepseek-v4-flash");
        assert_eq!(parsed.storage.data_dir, "data");
        assert!(parsed.search.greedy.traverse);
        assert_eq!(parsed.search.greedy.match_mode, "prefix");
        assert!((parsed.search.greedy.activate - 0.2).abs() < 0.001);
        assert!((parsed.search.greedy.decay - 0.95).abs() < 0.001);
        assert_eq!(parsed.search.greedy.depth, 16);
        assert!((parsed.search.greedy.score - 0.1).abs() < 0.001);
        assert_eq!(parsed.search.exact.match_mode, "word");
    }

    #[test]
    fn test_load_nonexistent_creates_default() {
        let dir = tempdir().unwrap();
        std::env::set_var("HOME", dir.path());
        let s = load_or_create_settings();
        assert_eq!(s.server.port, 8080);
        let path = config_file_path();
        assert!(path.exists());
    }

    #[test]
    fn test_save_and_reload() {
        let dir = tempdir().unwrap();
        std::env::set_var("HOME", dir.path());
        let mut s = Settings::default();
        s.llm.default_model = "TestProvider/gpt-4".to_string();
        save_settings(&s).unwrap();
        let loaded = load_or_create_settings();
        assert_eq!(loaded.llm.default_model, "TestProvider/gpt-4");
    }
}
