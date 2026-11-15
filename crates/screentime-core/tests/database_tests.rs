use rusqlite::Connection;
use screentime_core::{DbActorHandle, Session};
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn test_wal_pragmas_and_persistence() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("screentime_wal_test.db");

    let handle = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let session = Session::new(1000.0, 1005.0, "alacritty")
        .with_title("terminal")
        .with_device_id("test-host");

    let session_id = handle
        .insert_session(session.clone())
        .await
        .expect("Insert session failed");
    assert!(session_id > 0);

    handle
        .update_end_time(session_id, 1010.0)
        .await
        .expect("Update end time failed");

    let journal_mode = handle.query_pragma("journal_mode").await.unwrap();
    assert_eq!(journal_mode.to_lowercase(), "wal");

    let synchronous = handle.query_pragma("synchronous").await.unwrap();
    assert_eq!(synchronous, "1");

    let foreign_keys = handle.query_pragma("foreign_keys").await.unwrap();
    assert_eq!(foreign_keys, "1");

    let wal_checkpoint = handle.query_pragma("wal_autocheckpoint").await.unwrap();
    assert_eq!(wal_checkpoint, "1000");

    let busy_timeout = handle.query_pragma("busy_timeout").await.unwrap();
    assert_eq!(busy_timeout, "10000");

    let direct_conn = Connection::open(&db_path).expect("Direct connection open failed");
    let header_journal_mode: String = direct_conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .expect("Failed to read journal_mode");
    assert_eq!(header_journal_mode.to_lowercase(), "wal");

    drop(direct_conn);
    drop(handle);

    let reopened = DbActorHandle::spawn(&db_path).expect("Reopen DbActor failed");
    let fetched = reopened
        .get_session(session_id)
        .await
        .expect("Failed to get session")
        .expect("Session not found after reopen");

    assert_eq!(fetched.id, Some(session_id));
    assert_eq!(fetched.start_time, 1000.0);
    assert_eq!(fetched.end_time, 1010.0);
    assert_eq!(fetched.app_class, "alacritty");
    assert_eq!(fetched.app_title.as_deref(), Some("terminal"));
    assert_eq!(fetched.device_id, "test-host");
}

#[tokio::test]
async fn test_concurrent_read_write_access() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("screentime_concurrency_test.db");

    let handle = Arc::new(DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor"));

    let num_writers = 8;
    let sessions_per_writer = 25;
    let mut writer_handles = Vec::new();

    for w in 0..num_writers {
        let h = Arc::clone(&handle);
        writer_handles.push(tokio::spawn(async move {
            for i in 0..sessions_per_writer {
                let start_time = (w * 1000 + i * 10) as f64;
                let end_time = start_time + 5.0;
                let session = Session::new(start_time, end_time, format!("app_{w}"))
                    .with_title(format!("title_{i}"))
                    .with_device_id("worker-dev");

                let id = h.insert_session(session).await.expect("Insert failed");
                h.update_end_time(id, end_time + 1.0)
                    .await
                    .expect("Update failed");
            }
        }));
    }

    let num_readers = 4;
    let mut reader_handles = Vec::new();
    for _ in 0..num_readers {
        let path = db_path.clone();
        reader_handles.push(tokio::spawn(async move {
            let conn = Connection::open(&path).expect("Reader connection failed");
            conn.busy_timeout(std::time::Duration::from_millis(10000))
                .expect("Failed to set reader busy timeout");

            for _ in 0..20 {
                let count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
                    .expect("Reader query failed");
                assert!(count >= 0);
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }));
    }

    for wh in writer_handles {
        wh.await.expect("Writer task panicked");
    }

    for rh in reader_handles {
        rh.await.expect("Reader task panicked");
    }

    let all_sessions = handle
        .get_sessions_in_range(0.0, 1_000_000.0)
        .await
        .expect("Query all sessions failed");

    assert_eq!(all_sessions.len(), num_writers * sessions_per_writer);
}

#[tokio::test]
async fn test_migration_baselining_against_legacy_python_db() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("screentime_legacy.db");

    {
        let conn = Connection::open(&db_path).expect("Failed to open legacy db");
        conn.execute_batch(
            r#"
            PRAGMA user_version = 0;

            CREATE TABLE sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                start_time REAL NOT NULL,
                end_time REAL NOT NULL,
                app_class TEXT NOT NULL,
                app_title TEXT,
                website_domain TEXT,
                website_title TEXT
            );

            INSERT INTO sessions (start_time, end_time, app_class, app_title, website_domain)
            VALUES (100.0, 200.0, 'firefox', 'Mozilla Firefox', 'https://github.com');

            INSERT INTO sessions (start_time, end_time, app_class, app_title, website_domain)
            VALUES (300.0, 400.0, 'google-chrome', 'Chrome', 'https://crates.io');

            INSERT INTO sessions (start_time, end_time, app_class, app_title, website_domain)
            VALUES (300.0, 400.0, 'google-chrome', 'Chrome', 'https://crates.io');
            "#,
        )
        .expect("Failed to set up legacy schema");
    }

    let handle = DbActorHandle::spawn(&db_path).expect("DbActor baselining failed");

    let direct_conn = Connection::open(&db_path).expect("Direct connection failed");

    let user_version: u32 = direct_conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("Failed to query user_version");
    assert_eq!(user_version, 1);

    let count: i64 = direct_conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .expect("Failed to count rows");
    assert_eq!(count, 2);

    let (url, device_id): (Option<String>, String) = direct_conn
        .query_row(
            "SELECT website_url, device_id FROM sessions WHERE start_time = 100.0",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("Query migrated row failed");

    assert_eq!(url.as_deref(), Some("https://github.com"));
    assert_eq!(device_id, "hyprland-pc");

    let table_count: i64 = direct_conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('sessions', 'title_rules', 'categories', 'app_categories', 'app_groups')",
            [],
            |r| r.get(0),
        )
        .expect("Table count query failed");
    assert_eq!(table_count, 5);

    let new_session = Session::new(500.0, 510.0, "slack").with_title("General");
    let inserted_id = handle
        .insert_session(new_session)
        .await
        .expect("Insert into baselined DB failed");
    assert!(inserted_id > 0);
}

#[tokio::test]
async fn test_noise_filtering_and_session_lifecycle() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("screentime_noise_test.db");

    let handle = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    let noise1 = Session::new(10.0, 10.4, "alt-tab-noise-1");
    let valid1 = Session::new(20.0, 25.0, "code");
    let noise2 = Session::new(30.0, 30.8, "alt-tab-noise-2");

    handle.insert_session(noise1).await.unwrap();
    handle.insert_session(valid1).await.unwrap();
    handle.insert_session(noise2).await.unwrap();

    let deleted_count = handle
        .filter_noise_sessions(1.0)
        .await
        .expect("Filter noise failed");
    assert_eq!(deleted_count, 2);

    let remaining = handle
        .get_sessions_in_range(0.0, 100.0)
        .await
        .expect("Query failed");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].app_class, "code");

    let transient = Session::new(50.0, 50.0, "quick-popup");
    let transient_id = handle.insert_session(transient).await.unwrap();

    let kept_transient = handle
        .close_session(transient_id, 50.5, 1.0)
        .await
        .expect("Close session failed");
    assert!(!kept_transient);
    assert!(handle.get_session(transient_id).await.unwrap().is_none());

    let persistent = Session::new(60.0, 60.0, "editor");
    let persistent_id = handle.insert_session(persistent).await.unwrap();

    let kept_persistent = handle
        .close_session(persistent_id, 63.5, 1.0)
        .await
        .expect("Close session failed");
    assert!(kept_persistent);

    let fetched = handle
        .get_session(persistent_id)
        .await
        .unwrap()
        .expect("Session must exist");
    assert_eq!(fetched.end_time, 63.5);
    assert_eq!(fetched.duration(), 3.5);
}

#[tokio::test]
async fn test_title_rules_crud_and_reload() {
    let dir = tempdir().expect("Failed to create tempdir");
    let db_path = dir.path().join("screentime_rules_test.db");

    let handle = DbActorHandle::spawn(&db_path).expect("Failed to spawn DbActor");

    handle
        .add_title_rule("kitty", "nvim")
        .await
        .expect("Add rule failed");
    handle
        .add_title_rule("kitty", "cargo")
        .await
        .expect("Add rule failed");
    handle
        .add_title_rule("google-chrome", "GitHub")
        .await
        .expect("Add rule failed");

    let cache = handle
        .reload_title_rules()
        .await
        .expect("Reload title rules failed");

    assert_eq!(
        cache.resolve_app_class("kitty", Some("nvim src/db/actor.rs")),
        "nvim"
    );
    assert_eq!(
        cache.resolve_app_class("kitty", Some("CARGO test --all")),
        "cargo"
    );
    assert_eq!(
        cache.resolve_app_class("google-chrome", Some("github - issues")),
        "GitHub"
    );
    assert_eq!(
        cache.resolve_app_class("kitty", Some("fish shell")),
        "kitty"
    );

    handle
        .remove_title_rule("kitty", "cargo")
        .await
        .expect("Remove rule failed");

    let updated_cache = handle
        .reload_title_rules()
        .await
        .expect("Reload title rules failed");
    assert_eq!(
        updated_cache.resolve_app_class("kitty", Some("cargo test")),
        "kitty"
    );
    assert_eq!(
        updated_cache.resolve_app_class("kitty", Some("nvim src/lib.rs")),
        "nvim"
    );
}
