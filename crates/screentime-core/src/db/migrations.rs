use crate::db::error::DbError;
use rusqlite::{Connection, TransactionBehavior};
use rusqlite_migration::{Migrations, M};
use std::collections::HashSet;

pub const MIGRATION_1_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    start_time REAL NOT NULL,
    end_time REAL NOT NULL,
    app_class TEXT NOT NULL,
    app_title TEXT,
    website_url TEXT,
    device_id TEXT DEFAULT 'hyprland-pc'
);

CREATE INDEX IF NOT EXISTS idx_sessions_time ON sessions(start_time, end_time);
CREATE INDEX IF NOT EXISTS idx_sessions_app ON sessions(app_class);
CREATE INDEX IF NOT EXISTS idx_sessions_url ON sessions(website_url);
CREATE INDEX IF NOT EXISTS idx_sessions_device ON sessions(device_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_unique ON sessions(start_time, app_class, device_id);

CREATE TABLE IF NOT EXISTS title_rules (
    app_class TEXT NOT NULL,
    target_title TEXT NOT NULL,
    PRIMARY KEY(app_class, target_title)
);

CREATE TABLE IF NOT EXISTS categories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS app_categories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    app_identifier TEXT NOT NULL UNIQUE,
    identifier_type TEXT NOT NULL CHECK(identifier_type IN ('app', 'website')),
    category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_app_categories_ident ON app_categories(app_identifier);

CREATE TABLE IF NOT EXISTS app_groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    group_name TEXT NOT NULL,
    app_identifier TEXT NOT NULL UNIQUE,
    identifier_type TEXT NOT NULL CHECK(identifier_type IN ('app', 'website'))
);
CREATE INDEX IF NOT EXISTS idx_app_groups_name ON app_groups(group_name);
CREATE INDEX IF NOT EXISTS idx_app_groups_ident ON app_groups(app_identifier);
"#;

pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(MIGRATION_1_SQL)])
}

fn baseline_legacy_database(conn: &mut Connection) -> Result<(), DbError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let column_names: HashSet<String> = {
        let mut stmt = tx.prepare("PRAGMA table_info(sessions)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut names = HashSet::new();
        for name in rows {
            names.insert(name?);
        }
        names
    };

    if column_names.contains("website_domain") && !column_names.contains("website_url") {
        tx.execute(
            "ALTER TABLE sessions RENAME COLUMN website_domain TO website_url",
            [],
        )?;
    }

    if column_names.contains("website_title") {
        tx.execute("ALTER TABLE sessions DROP COLUMN website_title", [])?;
    }

    if !column_names.contains("device_id") && !column_names.is_empty() {
        tx.execute(
            "ALTER TABLE sessions ADD COLUMN device_id TEXT DEFAULT 'hyprland-pc'",
            [],
        )?;
    }

    tx.execute(
        r#"
        DELETE FROM sessions
        WHERE id NOT IN (
            SELECT MIN(id)
            FROM sessions
            GROUP BY start_time, app_class, device_id
        )
        "#,
        [],
    )?;

    tx.execute_batch(MIGRATION_1_SQL)?;
    tx.execute_batch("PRAGMA user_version = 1;")?;
    tx.commit()?;

    Ok(())
}

pub fn apply_migrations_with_baseline(conn: &mut Connection) -> Result<(), DbError> {
    let user_version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if user_version == 0 {
        let has_sessions: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'sessions')",
            [],
            |row| row.get(0),
        )?;

        if has_sessions {
            baseline_legacy_database(conn)?;
        }
    }

    let migs = migrations();
    migs.to_latest(conn)
        .map_err(|e| DbError::Migration(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fresh_database_migration() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_migrations_with_baseline(&mut conn).unwrap();

        let user_version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 1);

        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('sessions', 'title_rules', 'categories', 'app_categories', 'app_groups')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 5);
    }

    #[test]
    fn test_legacy_database_baselining() {
        let mut conn = Connection::open_in_memory().unwrap();

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
            INSERT INTO sessions (start_time, end_time, app_class, website_domain)
            VALUES (100.0, 150.0, 'firefox', 'https://rust-lang.org');
            INSERT INTO sessions (start_time, end_time, app_class, website_domain)
            VALUES (100.0, 150.0, 'firefox', 'https://rust-lang.org');
            "#,
        )
        .unwrap();

        apply_migrations_with_baseline(&mut conn).unwrap();

        let user_version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 1);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let (url, device_id): (Option<String>, String) = conn
            .query_row(
                "SELECT website_url, device_id FROM sessions WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(url.as_deref(), Some("https://rust-lang.org"));
        assert_eq!(device_id, "hyprland-pc");
    }
}
