use std::path::PathBuf;

use super::settings::Settings;

/// Resolve the config file path (`~/.config/bionic-graph/settings.json`).
pub fn config_file_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE")) // Windows
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("bionic-graph")
        .join("settings.json")
}

/// Load settings from the config file, or create and save defaults.
///
/// Priority (highest wins):
/// 1. Environment variables (`BGRAPH_HOST`, `BGRAPH_PORT`, etc.)
/// 2. `~/.config/bionic-graph/settings.json`
/// 3. Built-in defaults
pub fn load_or_create_settings() -> Settings {
    let path = config_file_path();

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

/// Save settings back to the config file at the default path.
pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = config_file_path();
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
        settings.storage.data_dir = val;
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
