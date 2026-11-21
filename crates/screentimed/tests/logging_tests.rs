use screentimed::{
    backup_path, parse_cli_args, strip_ansi, Config, RollingFileWriter, RunOptions,
    ScreenTimeLogWriter, Subcommand,
};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

#[test]
fn test_strip_ansi_all_scenarios() {
    assert_eq!(strip_ansi(b""), b"");
    assert_eq!(strip_ansi(b"Hello world"), b"Hello world");
    assert_eq!(
        strip_ansi("Hello, 🌍 and 世界!".as_bytes()),
        "Hello, 🌍 and 世界!".as_bytes()
    );

    let colored = b"\x1b[31mError\x1b[0m: connection failed\n";
    assert_eq!(strip_ansi(colored), b"Error: connection failed\n");

    let styled = b"\x1b[1;32;4mSUCCESS\x1b[0m - \x1b[2mcompleted\x1b[0m";
    assert_eq!(strip_ansi(styled), b"SUCCESS - completed");

    let color_256 = b"\x1b[38;5;196mBrightRed\x1b[0m";
    assert_eq!(strip_ansi(color_256), b"BrightRed");

    let rgb = b"\x1b[38;2;255;128;0mOrange\x1b[0m";
    assert_eq!(strip_ansi(rgb), b"Orange");

    let clear = b"\x1b[2J\x1b[H\x1b[2KHello clear screen";
    assert_eq!(strip_ansi(clear), b"Hello clear screen");

    let osc_bel = b"\x1b]0;Title Bar\x07Active content";
    assert_eq!(strip_ansi(osc_bel), b"Active content");

    let osc_st = b"\x1b]0;Title Bar\x1b\\Active content";
    assert_eq!(strip_ansi(osc_st), b"Active content");

    let charset = b"\x1b(BNormal text";
    assert_eq!(strip_ansi(charset), b"Normal text");

    assert_eq!(strip_ansi(b"Trailing escape \x1b"), b"Trailing escape ");
    assert_eq!(strip_ansi(b"Trailing csi \x1b["), b"Trailing csi ");
    assert_eq!(
        strip_ansi(b"\x1b[31m\x1b[1m\x1b[4mDanger\x1b[0m\x1b[0m"),
        b"Danger"
    );
}

#[test]
fn test_rolling_file_writer_basic_rotation() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("screentimed.log");

    let mut writer = RollingFileWriter::new(&log_path, 100, 3).unwrap();
    assert_eq!(writer.current_size(), 0);
    assert_eq!(writer.max_bytes(), 100);
    assert_eq!(writer.backup_count(), 3);

    writer
        .write_all(b"12345678901234567890123456789012345678901234567890")
        .unwrap();
    writer.flush().unwrap();
    assert_eq!(writer.current_size(), 50);

    let backup_1 = backup_path(&log_path, 1);
    assert!(!backup_1.exists());
    assert_eq!(fs::metadata(&log_path).unwrap().len(), 50);

    writer
        .write_all(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz12345678")
        .unwrap();
    writer.flush().unwrap();

    assert!(backup_1.exists());
    assert_eq!(fs::metadata(&backup_1).unwrap().len(), 50);
    assert_eq!(
        fs::read(&backup_1).unwrap(),
        b"12345678901234567890123456789012345678901234567890"
    );

    assert!(log_path.exists());
    assert_eq!(fs::metadata(&log_path).unwrap().len(), 60);
    assert_eq!(
        fs::read(&log_path).unwrap(),
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz12345678"
    );
}

#[test]
fn test_rolling_file_writer_retention_cap() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("screentimed.log");

    let mut writer = RollingFileWriter::new(&log_path, 100, 3).unwrap();

    for i in 0..8 {
        let chunk =
            format!("CHUNK-{i:02}------------------------------------------------------------\n");
        assert_eq!(chunk.len(), 69);
        writer.write_all(chunk.as_bytes()).unwrap();
        writer.flush().unwrap();
    }

    assert!(log_path.exists());
    assert!(backup_path(&log_path, 1).exists());
    assert!(backup_path(&log_path, 2).exists());
    assert!(backup_path(&log_path, 3).exists());

    assert!(!backup_path(&log_path, 4).exists());
    assert!(!backup_path(&log_path, 5).exists());

    let current = fs::read_to_string(&log_path).unwrap();
    assert!(current.starts_with("CHUNK-07"));

    let b1 = fs::read_to_string(backup_path(&log_path, 1)).unwrap();
    assert!(b1.starts_with("CHUNK-06"));

    let b2 = fs::read_to_string(backup_path(&log_path, 2)).unwrap();
    assert!(b2.starts_with("CHUNK-05"));

    let b3 = fs::read_to_string(backup_path(&log_path, 3)).unwrap();
    assert!(b3.starts_with("CHUNK-04"));
}

#[test]
fn test_rolling_file_writer_zero_backup_count() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("screentimed.log");

    let mut writer = RollingFileWriter::new(&log_path, 50, 0).unwrap();

    writer
        .write_all(b"First message that is forty bytes long!!")
        .unwrap();
    writer.flush().unwrap();
    assert_eq!(fs::metadata(&log_path).unwrap().len(), 40);

    writer
        .write_all(b"Second message that triggers rotation!!")
        .unwrap();
    writer.flush().unwrap();

    assert!(!backup_path(&log_path, 1).exists());
    assert_eq!(
        fs::read(&log_path).unwrap(),
        b"Second message that triggers rotation!!"
    );
}

#[test]
fn test_rolling_file_writer_preexisting_file() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("screentimed.log");

    fs::write(&log_path, [b'A'; 80]).unwrap();
    assert_eq!(fs::metadata(&log_path).unwrap().len(), 80);

    let mut writer = RollingFileWriter::new(&log_path, 100, 2).unwrap();
    assert_eq!(writer.current_size(), 80);

    writer.write_all(&[b'B'; 30]).unwrap();
    writer.flush().unwrap();

    let b1 = backup_path(&log_path, 1);
    assert!(b1.exists());
    assert_eq!(fs::read(&b1).unwrap(), [b'A'; 80]);

    assert_eq!(fs::read(&log_path).unwrap(), [b'B'; 30]);
}

#[test]
fn test_screen_time_log_writer_strips_ansi_to_file() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("screentimed.log");

    let rolling = RollingFileWriter::new(&log_path, 1024 * 1024, 5).unwrap();
    let mut writer = ScreenTimeLogWriter::new(Some(Arc::new(Mutex::new(rolling))), false);

    let log_entry = b"\x1b[2m2026-11-20T14:40:00Z\x1b[0m \x1b[32mINFO\x1b[0m \x1b[1mscreentimed\x1b[0m: daemon started\n";
    writer.write_all(log_entry).unwrap();
    writer.flush().unwrap();

    let disk_bytes = fs::read(&log_path).unwrap();
    let disk_str = String::from_utf8(disk_bytes).unwrap();

    assert_eq!(
        disk_str,
        "2026-11-20T14:40:00Z INFO screentimed: daemon started\n"
    );
    assert!(!disk_str.contains('\x1b'));
}

#[test]
fn test_cli_logging_flags_parsing() {
    let args = vec![
        "screentimed",
        "--log-level",
        "debug",
        "--log-console",
        "--max-size-mb",
        "25",
        "--backup-count",
        "7",
    ];

    let parsed = parse_cli_args(args).unwrap();
    match parsed {
        Subcommand::Run(opts) => {
            assert_eq!(opts.log_level, Some("debug".to_string()));
            assert_eq!(opts.log_console, Some(true));
            assert_eq!(opts.max_size_mb, Some(25));
            assert_eq!(opts.backup_count, Some(7));
        }
        other => panic!("Expected Subcommand::Run, got {:?}", other),
    }

    let upper_args = vec!["screentimed", "--log-level", "TRACE"];
    if let Subcommand::Run(opts) = parse_cli_args(upper_args).unwrap() {
        assert_eq!(opts.log_level, Some("TRACE".to_string()));
    } else {
        panic!("Expected Subcommand::Run");
    }

    assert!(parse_cli_args(vec!["screentimed", "--log-level", "invalid"]).is_err());
    assert!(parse_cli_args(vec!["screentimed", "--log-level"]).is_err());
}

#[test]
fn test_logging_hierarchical_precedence() {
    let dir = tempdir().unwrap();
    let ini_path = dir.path().join("config.ini");
    fs::write(
        &ini_path,
        "[Paths]\ndata_dir = /data\nlog_dir = /logs\n\n[Logging]\nlevel = INFO\nmax_size_mb = 10\nbackup_count = 5\nconsole = false\n",
    )
    .unwrap();

    let mut env = HashMap::new();
    env.insert("SCREENTIME_LOG_LEVEL".to_string(), "WARN".to_string());
    env.insert("SCREENTIME_LOG_MAX_SIZE_MB".to_string(), "40".to_string());
    env.insert("SCREENTIME_LOG_BACKUP_COUNT".to_string(), "9".to_string());
    env.insert("SCREENTIME_LOG_CONSOLE".to_string(), "true".to_string());

    let env_config = Config::load_with_overrides(&RunOptions::default(), &env, Some(&ini_path));
    assert_eq!(env_config.logging.level, "WARN");
    assert_eq!(env_config.logging.max_size_mb, 40);
    assert_eq!(env_config.logging.backup_count, 9);
    assert!(env_config.logging.console);

    let cli_opts = RunOptions {
        log_level: Some("ERROR".to_string()),
        max_size_mb: Some(100),
        backup_count: Some(20),
        log_console: Some(false),
        ..Default::default()
    };

    let cli_config = Config::load_with_overrides(&cli_opts, &env, Some(&ini_path));
    assert_eq!(cli_config.logging.level, "ERROR");
    assert_eq!(cli_config.logging.max_size_mb, 100);
    assert_eq!(cli_config.logging.backup_count, 20);
    assert!(!cli_config.logging.console);
}

#[test]
fn test_tracing_with_default_to_rolling_file() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("screentimed.log");

    let rolling = RollingFileWriter::new(&log_path, 100, 2).unwrap();
    let log_writer = ScreenTimeLogWriter::new(Some(Arc::new(Mutex::new(rolling))), false);

    let subscriber = tracing_subscriber::fmt()
        .with_ansi(true)
        .with_writer(move || log_writer.clone())
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("First log event message");
        tracing::warn!(
            "Second log event that will exceed one hundred bytes total and trigger rotation"
        );
    });

    assert!(log_path.exists());
    assert!(backup_path(&log_path, 1).exists());

    let active_content = fs::read_to_string(&log_path).unwrap();
    assert!(!active_content.contains('\x1b'));
    assert!(active_content.contains("Second log event"));

    let backup_content = fs::read_to_string(backup_path(&log_path, 1)).unwrap();
    assert!(!backup_content.contains('\x1b'));
    assert!(backup_content.contains("First log event"));
}
