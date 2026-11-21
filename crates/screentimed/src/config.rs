use crate::manifests::default_home_dir;
use crate::platform::PlatformBackend;
use screentime_core::ipc::default_socket_path;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::warn;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IniDocument {
    sections: HashMap<String, HashMap<String, String>>,
}

impl IniDocument {
    pub fn parse(input: &str) -> Self {
        let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current_section = String::new();

        for raw_line in input.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            if line.starts_with('[') {
                if let Some(end_idx) = line.find(']') {
                    let section_name = line[1..end_idx].trim().to_lowercase();
                    current_section = section_name;
                    sections.entry(current_section.clone()).or_default();
                    continue;
                }
            }

            let delimiter_pos = line.find('=').or_else(|| line.find(':'));
            if let Some(pos) = delimiter_pos {
                let key = line[..pos].trim().to_lowercase();
                let mut val = line[pos + 1..].trim();

                if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
                    || (val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2)
                {
                    val = &val[1..val.len() - 1];
                }

                if !key.is_empty() {
                    sections
                        .entry(current_section.clone())
                        .or_default()
                        .insert(key, val.to_string());
                }
            }
        }

        Self { sections }
    }

    pub fn load_file(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(content) => Self::parse(&content),
            Err(e) => {
                if path.exists() {
                    warn!("Failed to read config file {}: {e}", path.display());
                }
                Self::default()
            }
        }
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .get(&section.to_lowercase())?
            .get(&key.to_lowercase())
            .map(|s| s.as_str())
    }

    pub fn get_string(&self, section: &str, key: &str) -> Option<String> {
        self.get(section, key).map(|s| s.to_string())
    }

    pub fn get_u64(&self, section: &str, key: &str) -> Option<u64> {
        self.get(section, key)?.parse::<u64>().ok()
    }

    pub fn get_usize(&self, section: &str, key: &str) -> Option<usize> {
        self.get(section, key)?.parse::<usize>().ok()
    }

    pub fn get_bool(&self, section: &str, key: &str) -> Option<bool> {
        match self.get(section, key)?.to_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    }

    pub fn get_path(&self, section: &str, key: &str) -> Option<PathBuf> {
        self.get(section, key).map(|s| expand_tilde(Path::new(s)))
    }
}

pub fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix("~/") {
        default_home_dir().join(stripped)
    } else if s == "~" {
        default_home_dir()
    } else {
        path.to_path_buf()
    }
}

pub trait EnvLookup {
    fn get_var(&self, key: &str) -> Option<String>;
}

pub struct SystemEnv;

impl EnvLookup for SystemEnv {
    fn get_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

impl EnvLookup for HashMap<String, String> {
    fn get_var(&self, key: &str) -> Option<String> {
        self.get(key).cloned()
    }
}

impl<F> EnvLookup for F
where
    F: Fn(&str) -> Option<String>,
{
    fn get_var(&self, key: &str) -> Option<String> {
        self(key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathsConfig {
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    pub heartbeat_interval: u64,
    pub idle_timeout: u64,
    pub chrome_window_class: String,
    pub firefox_window_class: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingConfig {
    pub level: String,
    pub max_size_mb: u64,
    pub backup_count: usize,
    pub console: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub paths: PathsConfig,
    pub daemon: DaemonConfig,
    pub logging: LoggingConfig,
    pub db_path: PathBuf,
    pub socket_path: PathBuf,
    pub platform: Option<PlatformBackend>,
    pub device_id: Option<String>,
}

pub fn default_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            if !local_app_data.trim().is_empty() {
                return PathBuf::from(local_app_data)
                    .join("screentime")
                    .join("config.ini");
            }
        }
        if let Ok(app_data) = std::env::var("APPDATA") {
            if !app_data.trim().is_empty() {
                return PathBuf::from(app_data)
                    .join("screentime")
                    .join("config.ini");
            }
        }
    }

    if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg_config.trim().is_empty() {
            return PathBuf::from(xdg_config)
                .join("screentime")
                .join("config.ini");
        }
    }

    default_home_dir()
        .join(".config")
        .join("screentime")
        .join("config.ini")
}

pub fn default_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            if !local_app_data.trim().is_empty() {
                return PathBuf::from(local_app_data).join("screentime");
            }
        }
        if let Ok(app_data) = std::env::var("APPDATA") {
            if !app_data.trim().is_empty() {
                return PathBuf::from(app_data).join("screentime");
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        return default_home_dir()
            .join("Library")
            .join("Application Support")
            .join("screentime");
    }

    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.trim().is_empty() {
            return PathBuf::from(xdg_data).join("screentime");
        }
    }

    default_home_dir()
        .join(".local")
        .join("share")
        .join("screentime")
}

pub fn default_log_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(app_data) = std::env::var("APPDATA") {
            if !app_data.trim().is_empty() {
                return PathBuf::from(app_data).join("screentime").join("logs");
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        return default_home_dir()
            .join("Library")
            .join("Application Support")
            .join("screentime")
            .join("logs");
    }

    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.trim().is_empty() {
            return PathBuf::from(xdg_data).join("screentime").join("logs");
        }
    }

    default_home_dir()
        .join(".local")
        .join("share")
        .join("screentime")
        .join("logs")
}

pub fn default_db_path() -> PathBuf {
    if let Ok(env_path) = std::env::var("SCREENTIME_DB_PATH") {
        if !env_path.trim().is_empty() {
            return PathBuf::from(env_path);
        }
    }

    default_data_dir().join("screentime.db")
}

impl Default for PathsConfig {
    fn default() -> Self {
        let data = default_data_dir();
        let log = default_log_dir();
        Self {
            data_dir: data,
            log_dir: log,
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: 10,
            idle_timeout: 90,
            chrome_window_class: "google-chrome".to_string(),
            firefox_window_class: "firefox".to_string(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "INFO".to_string(),
            max_size_mb: 10,
            backup_count: 5,
            console: false,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let paths = PathsConfig::default();
        let daemon = DaemonConfig::default();
        let logging = LoggingConfig::default();
        let db_path = paths.data_dir.join("screentime.db");
        let socket_path = default_socket_path();
        Self {
            paths,
            daemon,
            logging,
            db_path,
            socket_path,
            platform: None,
            device_id: None,
        }
    }
}

impl Config {
    pub fn to_ini_string(&self) -> String {
        format!(
            "[Paths]\ndata_dir = {}\nlog_dir = {}\n\n[Daemon]\nheartbeat_interval = {}\nidle_timeout = {}\nchrome_window_class = {}\nfirefox_window_class = {}\n\n[Logging]\nlevel = {}\nmax_size_mb = {}\nbackup_count = {}\nconsole = {}\n",
            self.paths.data_dir.display(),
            self.paths.log_dir.display(),
            self.daemon.heartbeat_interval,
            self.daemon.idle_timeout,
            self.daemon.chrome_window_class,
            self.daemon.firefox_window_class,
            self.logging.level,
            self.logging.max_size_mb,
            self.logging.backup_count,
            self.logging.console
        )
    }

    pub fn load_from_env_and_cli(cli_opts: &crate::RunOptions) -> Self {
        Self::load_with_overrides(cli_opts, &SystemEnv, None)
    }

    pub fn load_with_overrides(
        cli_opts: &crate::RunOptions,
        env: &impl EnvLookup,
        config_override: Option<&Path>,
    ) -> Self {
        let resolved_config_path = cli_opts
            .config_path
            .clone()
            .or_else(|| {
                env.get_var("SCREENTIME_CONFIG")
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| expand_tilde(Path::new(&s)))
            })
            .or_else(|| config_override.map(|p| p.to_path_buf()))
            .unwrap_or_else(default_config_path);

        let ini = IniDocument::load_file(&resolved_config_path);

        let data_dir = cli_opts
            .data_dir
            .clone()
            .or_else(|| {
                env.get_var("SCREENTIME_DATA_DIR")
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| expand_tilde(Path::new(&s)))
            })
            .or_else(|| ini.get_path("Paths", "data_dir"))
            .unwrap_or_else(default_data_dir);

        let log_dir = cli_opts
            .log_dir
            .clone()
            .or_else(|| {
                env.get_var("SCREENTIME_LOG_DIR")
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| expand_tilde(Path::new(&s)))
            })
            .or_else(|| ini.get_path("Paths", "log_dir"))
            .unwrap_or_else(|| data_dir.join("logs"));

        let db_path = cli_opts
            .db_path
            .clone()
            .or_else(|| {
                env.get_var("SCREENTIME_DB_PATH")
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| expand_tilde(Path::new(&s)))
            })
            .unwrap_or_else(|| data_dir.join("screentime.db"));

        let socket_path = cli_opts
            .socket_path
            .clone()
            .or_else(|| {
                env.get_var("SCREENTIME_SOCKET")
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| expand_tilde(Path::new(&s)))
            })
            .or_else(|| {
                ini.get_path("Paths", "socket_path")
                    .or_else(|| ini.get_path("Daemon", "socket_path"))
            })
            .unwrap_or_else(default_socket_path);

        let heartbeat_interval = cli_opts
            .heartbeat_interval
            .or_else(|| {
                env.get_var("SCREENTIME_HEARTBEAT_INTERVAL")
                    .and_then(|s| s.trim().parse::<u64>().ok())
            })
            .or_else(|| ini.get_u64("Daemon", "heartbeat_interval"))
            .unwrap_or(10);

        let idle_timeout = cli_opts
            .idle_timeout
            .or_else(|| {
                env.get_var("SCREENTIME_IDLE_TIMEOUT")
                    .and_then(|s| s.trim().parse::<u64>().ok())
            })
            .or_else(|| ini.get_u64("Daemon", "idle_timeout"))
            .unwrap_or(90);

        let chrome_window_class = cli_opts
            .chrome_window_class
            .clone()
            .or_else(|| {
                env.get_var("SCREENTIME_CHROME_CLASS")
                    .or_else(|| env.get_var("SCREENTIME_CHROME_WINDOW_CLASS"))
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| ini.get_string("Daemon", "chrome_window_class"))
            .unwrap_or_else(|| "google-chrome".to_string());

        let firefox_window_class = cli_opts
            .firefox_window_class
            .clone()
            .or_else(|| {
                env.get_var("SCREENTIME_FIREFOX_CLASS")
                    .or_else(|| env.get_var("SCREENTIME_FIREFOX_WINDOW_CLASS"))
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| ini.get_string("Daemon", "firefox_window_class"))
            .unwrap_or_else(|| "firefox".to_string());

        let level = cli_opts
            .log_level
            .clone()
            .or_else(|| {
                env.get_var("SCREENTIME_LOG_LEVEL")
                    .or_else(|| env.get_var("RUST_LOG"))
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| ini.get_string("Logging", "level"))
            .unwrap_or_else(|| "INFO".to_string());

        let max_size_mb = cli_opts
            .max_size_mb
            .or_else(|| {
                env.get_var("SCREENTIME_LOG_MAX_SIZE_MB")
                    .or_else(|| env.get_var("SCREENTIME_MAX_SIZE_MB"))
                    .and_then(|s| s.trim().parse::<u64>().ok())
            })
            .or_else(|| ini.get_u64("Logging", "max_size_mb"))
            .unwrap_or(10);

        let backup_count = cli_opts
            .backup_count
            .or_else(|| {
                env.get_var("SCREENTIME_LOG_BACKUP_COUNT")
                    .or_else(|| env.get_var("SCREENTIME_BACKUP_COUNT"))
                    .and_then(|s| s.trim().parse::<usize>().ok())
            })
            .or_else(|| ini.get_usize("Logging", "backup_count"))
            .unwrap_or(5);

        let console = cli_opts
            .log_console
            .or_else(|| {
                env.get_var("SCREENTIME_LOG_CONSOLE").and_then(|s| {
                    match s.trim().to_lowercase().as_str() {
                        "1" | "true" | "yes" | "on" => Some(true),
                        "0" | "false" | "no" | "off" => Some(false),
                        _ => None,
                    }
                })
            })
            .or_else(|| ini.get_bool("Logging", "console"))
            .unwrap_or(false);

        let platform = cli_opts
            .platform
            .or_else(|| {
                env.get_var("SCREENTIME_PLATFORM")
                    .and_then(|s| s.trim().parse::<PlatformBackend>().ok())
            })
            .or_else(|| {
                ini.get_string("Daemon", "platform")
                    .and_then(|s| s.trim().parse::<PlatformBackend>().ok())
            });

        let device_id = cli_opts
            .device_id
            .clone()
            .or_else(|| {
                env.get_var("SCREENTIME_DEVICE_ID")
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| ini.get_string("Daemon", "device_id"));

        Self {
            paths: PathsConfig { data_dir, log_dir },
            daemon: DaemonConfig {
                heartbeat_interval,
                idle_timeout,
                chrome_window_class,
                firefox_window_class,
            },
            logging: LoggingConfig {
                level,
                max_size_mb,
                backup_count,
                console,
            },
            db_path,
            socket_path,
            platform,
            device_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ini_parser_basic_and_normalization() {
        let ini_text = r#"
        # Comment line
        ; Another comment line

        [Paths]
        data_dir = /custom/data
        log_dir = /custom/logs

        [Daemon]
        heartbeat_interval = 25
        idle_timeout = 120
        chrome_window_class = "brave-browser"
        firefox_window_class = 'floorp'

        [Logging]
        level = DEBUG
        max_size_mb = 50
        backup_count = 10
        console = true
        "#;

        let ini = IniDocument::parse(ini_text);
        assert_eq!(ini.get("paths", "DATA_DIR"), Some("/custom/data"));
        assert_eq!(ini.get("PATHS", "log_dir"), Some("/custom/logs"));
        assert_eq!(ini.get_u64("daemon", "heartbeat_interval"), Some(25));
        assert_eq!(ini.get_u64("daemon", "idle_timeout"), Some(120));
        assert_eq!(
            ini.get_string("daemon", "chrome_window_class"),
            Some("brave-browser".to_string())
        );
        assert_eq!(
            ini.get_string("daemon", "firefox_window_class"),
            Some("floorp".to_string())
        );
        assert_eq!(
            ini.get_string("logging", "level"),
            Some("DEBUG".to_string())
        );
        assert_eq!(ini.get_u64("logging", "max_size_mb"), Some(50));
        assert_eq!(ini.get_usize("logging", "backup_count"), Some(10));
        assert_eq!(ini.get_bool("logging", "console"), Some(true));
    }

    #[test]
    fn test_ini_parser_malformed_tolerance() {
        let malformed = r#"
        not a section or key value
        [BrokenSection
        key_without_value
        = value_without_key
        [Valid]
        good_key = 12345
        invalid_number = not_a_num
        invalid_bool = maybe
        "#;

        let ini = IniDocument::parse(malformed);
        assert_eq!(ini.get_u64("valid", "good_key"), Some(12345));
        assert_eq!(ini.get_u64("valid", "invalid_number"), None);
        assert_eq!(ini.get_bool("valid", "invalid_bool"), None);
    }

    #[test]
    fn test_expand_tilde() {
        let expanded = expand_tilde(Path::new("~/test/path"));
        assert!(!expanded.starts_with("~/"));
        assert!(expanded.ends_with("test/path"));
    }
}
