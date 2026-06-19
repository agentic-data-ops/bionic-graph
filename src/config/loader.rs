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
/// 1. Environment variables (`BGRAPH_HOST`, `BGRAPH_PORT`, `BGRAPH_LLM_API_KEY`, etc.)
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
        if let Ok(json) = serde_json::to_string_pretty(&s) {
            if let Err(e) = std::fs::write(&path, &json) {
                log::warn!("Failed to write default config to {:?}: {}", path, e);
            } else {
                log::info!("Default config written to {:?}", path);
            }
        }
        s
    };

    // Environment variable overrides
    apply_env_overrides(&mut settings);

    settings
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

    // Extraction
    if std::env::var("BGRAPH_LLM_API_KEY").is_ok() || std::env::var("BGRAPH_EXTRACT_API_KEY").is_ok() {
        log::info!("BGRAPH_LLM_API_KEY set via environment");
    }
    if let Ok(val) = std::env::var("BGRAPH_LLM_BASE_URL").or_else(|_| std::env::var("BGRAPH_EXTRACT_BASE_URL")) {
        settings.extraction.api_base_url = val;
    }
    if let Ok(val) = std::env::var("BGRAPH_LLM_MODEL").or_else(|_| std::env::var("BGRAPH_EXTRACT_MODEL")) {
        settings.extraction.model = val;
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
        assert_eq!(parsed.extraction.model, "deepseek-v4-flash");
        assert_eq!(parsed.storage.data_dir, "data");
        assert_eq!(parsed.neural.default_threshold, 0.7);
    }

    #[test]
    fn test_load_nonexistent_creates_default() {
        // Temporarily change HOME to a temp dir so no real config is touched
        let dir = tempdir().unwrap();
        std::env::set_var("HOME", dir.path());
        let s = load_or_create_settings();
        assert_eq!(s.server.port, 8080);
        // Config file should now exist
        let path = config_file_path();
        assert!(path.exists());
    }
}
