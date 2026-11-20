use screentimed::config::{
    default_config_path, default_data_dir, default_db_path, default_log_dir, Config, IniDocument,
};
use screentimed::{RunOptions, SessionManager};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tempfile::tempdir;

const CLI_BIN: &str = env!("CARGO_BIN_EXE_screentimed");

#[test]
fn test_default_configuration() {
    let empty_env = HashMap::new();
    let config = Config::load_with_overrides(&RunOptions::default(), &empty_env, None);

    assert_eq!(config.paths.data_dir, default_data_dir());
    assert_eq!(config.paths.log_dir, default_log_dir());
    assert_eq!(config.db_path, default_db_path());
    assert_eq!(config.daemon.heartbeat_interval, 10);
    assert_eq!(config.daemon.idle_timeout, 90);
    assert_eq!(config.daemon.chrome_window_class, "google-chrome");
    assert_eq!(config.daemon.firefox_window_class, "firefox");
    assert_eq!(config.logging.level, "INFO");
    assert_eq!(config.logging.max_size_mb, 10);
    assert_eq!(config.logging.backup_count, 5);
    assert!(!config.logging.console);
}

#[test]
fn test_config_ini_overrides_defaults() {
    let temp = tempdir().expect("tempdir");
    let ini_path = temp.path().join("config.ini");
    let ini_content = r#"
[Paths]
data_dir = /tmp/ini_data
log_dir = /tmp/ini_logs

[Daemon]
heartbeat_interval = 25
idle_timeout = 180
chrome_window_class = chromium-custom
firefox_window_class = floorp-custom

[Logging]
level = DEBUG
max_size_mb = 50
backup_count = 10
console = true
"#;
    fs::write(&ini_path, ini_content).expect("write ini");

    let empty_env = HashMap::new();
    let config = Config::load_with_overrides(&RunOptions::default(), &empty_env, Some(&ini_path));

    assert_eq!(config.paths.data_dir, PathBuf::from("/tmp/ini_data"));
    assert_eq!(config.paths.log_dir, PathBuf::from("/tmp/ini_logs"));
    assert_eq!(config.db_path, PathBuf::from("/tmp/ini_data/screentime.db"));
    assert_eq!(config.daemon.heartbeat_interval, 25);
    assert_eq!(config.daemon.idle_timeout, 180);
    assert_eq!(config.daemon.chrome_window_class, "chromium-custom");
    assert_eq!(config.daemon.firefox_window_class, "floorp-custom");
    assert_eq!(config.logging.level, "DEBUG");
    assert_eq!(config.logging.max_size_mb, 50);
    assert_eq!(config.logging.backup_count, 10);
    assert!(config.logging.console);
}

#[test]
fn test_environment_variables_override_ini() {
    let temp = tempdir().expect("tempdir");
    let ini_path = temp.path().join("config.ini");
    let ini_content = r#"
[Paths]
data_dir = /tmp/ini_data
log_dir = /tmp/ini_logs

[Daemon]
heartbeat_interval = 20
idle_timeout = 100
chrome_window_class = ini-chrome
firefox_window_class = ini-firefox

[Logging]
level = INFO
max_size_mb = 10
backup_count = 5
console = false
"#;
    fs::write(&ini_path, ini_content).expect("write ini");

    let mut env = HashMap::new();
    env.insert(
        "SCREENTIME_DATA_DIR".to_string(),
        "/tmp/env_data".to_string(),
    );
    env.insert(
        "SCREENTIME_LOG_DIR".to_string(),
        "/tmp/env_logs".to_string(),
    );
    env.insert(
        "SCREENTIME_DB_PATH".to_string(),
        "/tmp/env_data/custom.db".to_string(),
    );
    env.insert(
        "SCREENTIME_HEARTBEAT_INTERVAL".to_string(),
        "35".to_string(),
    );
    env.insert("SCREENTIME_IDLE_TIMEOUT".to_string(), "240".to_string());
    env.insert(
        "SCREENTIME_CHROME_CLASS".to_string(),
        "env-chrome".to_string(),
    );
    env.insert(
        "SCREENTIME_FIREFOX_CLASS".to_string(),
        "env-firefox".to_string(),
    );
    env.insert("SCREENTIME_LOG_LEVEL".to_string(), "WARN".to_string());
    env.insert("SCREENTIME_LOG_MAX_SIZE_MB".to_string(), "40".to_string());
    env.insert("SCREENTIME_LOG_BACKUP_COUNT".to_string(), "8".to_string());
    env.insert("SCREENTIME_LOG_CONSOLE".to_string(), "true".to_string());

    let config = Config::load_with_overrides(&RunOptions::default(), &env, Some(&ini_path));

    assert_eq!(config.paths.data_dir, PathBuf::from("/tmp/env_data"));
    assert_eq!(config.paths.log_dir, PathBuf::from("/tmp/env_logs"));
    assert_eq!(config.db_path, PathBuf::from("/tmp/env_data/custom.db"));
    assert_eq!(config.daemon.heartbeat_interval, 35);
    assert_eq!(config.daemon.idle_timeout, 240);
    assert_eq!(config.daemon.chrome_window_class, "env-chrome");
    assert_eq!(config.daemon.firefox_window_class, "env-firefox");
    assert_eq!(config.logging.level, "WARN");
    assert_eq!(config.logging.max_size_mb, 40);
    assert_eq!(config.logging.backup_count, 8);
    assert!(config.logging.console);
}

#[test]
fn test_cli_arguments_override_environment_and_ini() {
    let temp = tempdir().expect("tempdir");
    let ini_path = temp.path().join("config.ini");
    let ini_content = r#"
[Paths]
data_dir = /tmp/ini_data
log_dir = /tmp/ini_logs

[Daemon]
heartbeat_interval = 10
idle_timeout = 60
chrome_window_class = ini-chrome
firefox_window_class = ini-firefox

[Logging]
level = INFO
max_size_mb = 10
backup_count = 5
console = false
"#;
    fs::write(&ini_path, ini_content).expect("write ini");

    let mut env = HashMap::new();
    env.insert(
        "SCREENTIME_DATA_DIR".to_string(),
        "/tmp/env_data".to_string(),
    );
    env.insert(
        "SCREENTIME_HEARTBEAT_INTERVAL".to_string(),
        "20".to_string(),
    );
    env.insert("SCREENTIME_LOG_LEVEL".to_string(), "WARN".to_string());
    env.insert("SCREENTIME_LOG_CONSOLE".to_string(), "false".to_string());

    let cli_opts = RunOptions {
        data_dir: Some(PathBuf::from("/tmp/cli_data")),
        log_dir: Some(PathBuf::from("/tmp/cli_logs")),
        db_path: Some(PathBuf::from("/tmp/cli_data/cli.db")),
        heartbeat_interval: Some(50),
        idle_timeout: Some(300),
        chrome_window_class: Some("cli-chrome".to_string()),
        firefox_window_class: Some("cli-firefox".to_string()),
        log_level: Some("ERROR".to_string()),
        max_size_mb: Some(100),
        backup_count: Some(15),
        log_console: Some(true),
        ..Default::default()
    };

    let config = Config::load_with_overrides(&cli_opts, &env, Some(&ini_path));

    assert_eq!(config.paths.data_dir, PathBuf::from("/tmp/cli_data"));
    assert_eq!(config.paths.log_dir, PathBuf::from("/tmp/cli_logs"));
    assert_eq!(config.db_path, PathBuf::from("/tmp/cli_data/cli.db"));
    assert_eq!(config.daemon.heartbeat_interval, 50);
    assert_eq!(config.daemon.idle_timeout, 300);
    assert_eq!(config.daemon.chrome_window_class, "cli-chrome");
    assert_eq!(config.daemon.firefox_window_class, "cli-firefox");
    assert_eq!(config.logging.level, "ERROR");
    assert_eq!(config.logging.max_size_mb, 100);
    assert_eq!(config.logging.backup_count, 15);
    assert!(config.logging.console);
}

#[test]
fn test_missing_config_ini_clean_fallback() {
    let missing_path = Path::new("/path/that/definitely/does/not/exist/config.ini");
    let empty_env = HashMap::new();

    let config =
        Config::load_with_overrides(&RunOptions::default(), &empty_env, Some(missing_path));

    assert_eq!(config.daemon.heartbeat_interval, 10);
    assert_eq!(config.daemon.idle_timeout, 90);
    assert_eq!(config.daemon.chrome_window_class, "google-chrome");
    assert_eq!(config.daemon.firefox_window_class, "firefox");
    assert_eq!(config.logging.level, "INFO");
    assert_eq!(config.paths.data_dir, default_data_dir());
}

#[test]
fn test_malformed_config_ini_graceful_recovery() {
    let temp = tempdir().expect("tempdir");
    let malformed_path = temp.path().join("malformed.ini");
    let malformed_content = r#"
    [Paths
    data_dir = /tmp/partial_data
    malformed line without delimiter
    = value without key

    [Daemon]
    heartbeat_interval = invalid_number
    idle_timeout = 150
    chrome_window_class = "brave"

    [Logging]
    level = TRACE
    max_size_mb = not_a_num
    backup_count = 9
    console = not_a_bool

    [UnknownSection]
    some_key = some_value
    "#;
    fs::write(&malformed_path, malformed_content).expect("write malformed");

    let empty_env = HashMap::new();
    let config =
        Config::load_with_overrides(&RunOptions::default(), &empty_env, Some(&malformed_path));

    // Invalid heartbeat_interval falls back to default 10
    assert_eq!(config.daemon.heartbeat_interval, 10);
    // Valid idle_timeout is parsed
    assert_eq!(config.daemon.idle_timeout, 150);
    // Valid chrome_window_class is parsed
    assert_eq!(config.daemon.chrome_window_class, "brave");
    // firefox_window_class missing from INI falls back to default
    assert_eq!(config.daemon.firefox_window_class, "firefox");
    // Valid logging level
    assert_eq!(config.logging.level, "TRACE");
    // Invalid max_size_mb falls back to default 10
    assert_eq!(config.logging.max_size_mb, 10);
    // Valid backup_count
    assert_eq!(config.logging.backup_count, 9);
    // Invalid console boolean falls back to default false
    assert!(!config.logging.console);
}

#[tokio::test]
async fn test_session_manager_custom_browser_classes_and_heartbeat() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("sm_test.db");
    let handle = screentime_core::DbActorHandle::spawn(&db_path).unwrap();

    let sm = SessionManager::new(handle)
        .with_browser_classes("custom-chrome", "custom-firefox")
        .with_heartbeat_interval(Duration::from_secs(12));

    assert_eq!(sm.chrome_window_class(), "custom-chrome");
    assert_eq!(sm.firefox_window_class(), "custom-firefox");
    assert_eq!(sm.heartbeat_interval(), Duration::from_secs(12));

    // Test browser window detection
    assert!(sm.is_browser("custom-chrome"));
    assert!(sm.is_browser("My-Custom-Firefox-Build"));
    assert!(sm.is_browser("firefox"));
    assert!(sm.is_browser("google-chrome"));
    assert!(!sm.is_browser("alacritty"));
    assert!(!sm.is_browser("vlc"));
}

#[test]
fn test_config_ini_round_trip_format() {
    let config = Config::default();
    let ini_str = config.to_ini_string();

    assert!(ini_str.contains("[Paths]"));
    assert!(ini_str.contains("[Daemon]"));
    assert!(ini_str.contains("[Logging]"));
    assert!(ini_str.contains("heartbeat_interval = 10"));
    assert!(ini_str.contains("idle_timeout = 90"));
    assert!(ini_str.contains("chrome_window_class = google-chrome"));
    assert!(ini_str.contains("firefox_window_class = firefox"));

    let parsed = IniDocument::parse(&ini_str);
    assert_eq!(parsed.get_u64("daemon", "heartbeat_interval"), Some(10));
    assert_eq!(parsed.get_u64("daemon", "idle_timeout"), Some(90));
    assert_eq!(
        parsed.get_string("daemon", "chrome_window_class"),
        Some("google-chrome".to_string())
    );
    assert_eq!(
        parsed.get_string("daemon", "firefox_window_class"),
        Some("firefox".to_string())
    );
}

#[test]
fn test_default_config_path_structure() {
    let path = default_config_path();
    assert!(path.ends_with("config.ini"));
    assert!(path.to_string_lossy().contains("screentime"));
}

#[test]
fn test_cli_binary_help_displays_config_options() {
    let output = Command::new(CLI_BIN)
        .arg("--help")
        .output()
        .expect("screentimed --help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--config"));
    assert!(stdout.contains("--data-dir"));
    assert!(stdout.contains("--log-dir"));
    assert!(stdout.contains("--heartbeat-interval"));
    assert!(stdout.contains("--idle-timeout"));
    assert!(stdout.contains("--chrome-class"));
    assert!(stdout.contains("--firefox-class"));
    assert!(stdout.contains("--log-level"));
    assert!(stdout.contains("--log-console"));
}
