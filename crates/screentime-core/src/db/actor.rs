use crate::db::error::DbError;
use crate::db::migrations::apply_migrations_with_baseline;
use crate::domain::{Session, TitleRule};
use crate::rules::TitleRulesCache;
use rusqlite::{Connection, TransactionBehavior};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

pub enum DbCommand {
    InsertSession {
        session: Session,
        responder: oneshot::Sender<Result<i64, DbError>>,
    },
    UpdateEndTime {
        session_id: i64,
        end_time: f64,
        responder: oneshot::Sender<Result<(), DbError>>,
    },
    UpdateSessionTitle {
        session_id: i64,
        title: String,
        responder: oneshot::Sender<Result<(), DbError>>,
    },
    UpdateWebsiteUrl {
        session_id: i64,
        url: Option<String>,
        responder: oneshot::Sender<Result<(), DbError>>,
    },
    CloseSession {
        session_id: i64,
        end_time: f64,
        min_duration: f64,
        responder: oneshot::Sender<Result<bool, DbError>>,
    },
    FilterNoiseSessions {
        min_duration: f64,
        responder: oneshot::Sender<Result<usize, DbError>>,
    },
    ReloadTitleRules {
        responder: oneshot::Sender<Result<TitleRulesCache, DbError>>,
    },
    GetTitleRulesRaw {
        responder: oneshot::Sender<Result<HashMap<String, Vec<String>>, DbError>>,
    },
    AddTitleRule {
        rule: TitleRule,
        responder: oneshot::Sender<Result<(), DbError>>,
    },
    RemoveTitleRule {
        rule: TitleRule,
        responder: oneshot::Sender<Result<(), DbError>>,
    },
    GetSession {
        id: i64,
        responder: oneshot::Sender<Result<Option<Session>, DbError>>,
    },
    GetSessionsInRange {
        start_time: f64,
        end_time: f64,
        responder: oneshot::Sender<Result<Vec<Session>, DbError>>,
    },
    QueryPragma {
        pragma: String,
        responder: oneshot::Sender<Result<String, DbError>>,
    },
}

#[derive(Clone, Debug)]
pub struct DbActorHandle {
    sender: mpsc::Sender<DbCommand>,
}

impl DbActorHandle {
    pub fn spawn<P: AsRef<Path>>(db_path: P) -> Result<Self, DbError> {
        let path_buf = db_path.as_ref().to_path_buf();
        let (init_tx, init_rx) = std_mpsc::sync_channel::<Result<(), DbError>>(1);
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<DbCommand>(256);

        std::thread::Builder::new()
            .name("screentime-db-actor".into())
            .spawn(move || {
                let mut conn = match Self::open_and_configure_connection(&path_buf) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };

                if let Err(e) = apply_migrations_with_baseline(&mut conn) {
                    let _ = init_tx.send(Err(e));
                    return;
                }

                if init_tx.send(Ok(())).is_err() {
                    return;
                }

                while let Some(cmd) = cmd_rx.blocking_recv() {
                    match cmd {
                        DbCommand::InsertSession { session, responder } => {
                            let res = Self::handle_insert_session(&mut conn, session);
                            let _ = responder.send(res);
                        }
                        DbCommand::UpdateEndTime {
                            session_id,
                            end_time,
                            responder,
                        } => {
                            let res = Self::handle_update_end_time(&mut conn, session_id, end_time);
                            let _ = responder.send(res);
                        }
                        DbCommand::UpdateSessionTitle {
                            session_id,
                            title,
                            responder,
                        } => {
                            let res =
                                Self::handle_update_session_title(&mut conn, session_id, title);
                            let _ = responder.send(res);
                        }
                        DbCommand::UpdateWebsiteUrl {
                            session_id,
                            url,
                            responder,
                        } => {
                            let res = Self::handle_update_website_url(&mut conn, session_id, url);
                            let _ = responder.send(res);
                        }
                        DbCommand::CloseSession {
                            session_id,
                            end_time,
                            min_duration,
                            responder,
                        } => {
                            let res = Self::handle_close_session(
                                &mut conn,
                                session_id,
                                end_time,
                                min_duration,
                            );
                            let _ = responder.send(res);
                        }
                        DbCommand::FilterNoiseSessions {
                            min_duration,
                            responder,
                        } => {
                            let res = Self::handle_filter_noise_sessions(&mut conn, min_duration);
                            let _ = responder.send(res);
                        }
                        DbCommand::ReloadTitleRules { responder } => {
                            let res = Self::handle_reload_title_rules(&conn);
                            let _ = responder.send(res);
                        }
                        DbCommand::GetTitleRulesRaw { responder } => {
                            let res = Self::handle_get_title_rules_raw(&conn);
                            let _ = responder.send(res);
                        }
                        DbCommand::AddTitleRule { rule, responder } => {
                            let res = Self::handle_add_title_rule(&mut conn, rule);
                            let _ = responder.send(res);
                        }
                        DbCommand::RemoveTitleRule { rule, responder } => {
                            let res = Self::handle_remove_title_rule(&mut conn, rule);
                            let _ = responder.send(res);
                        }
                        DbCommand::GetSession { id, responder } => {
                            let res = Self::handle_get_session(&conn, id);
                            let _ = responder.send(res);
                        }
                        DbCommand::GetSessionsInRange {
                            start_time,
                            end_time,
                            responder,
                        } => {
                            let res =
                                Self::handle_get_sessions_in_range(&conn, start_time, end_time);
                            let _ = responder.send(res);
                        }
                        DbCommand::QueryPragma { pragma, responder } => {
                            let res = Self::handle_query_pragma(&conn, &pragma);
                            let _ = responder.send(res);
                        }
                    }
                }
            })
            .map_err(|e| DbError::Other(format!("Failed to spawn db worker thread: {e}")))?;

        init_rx.recv().map_err(|_| DbError::ChannelClosed)??;

        Ok(Self { sender: cmd_tx })
    }

    fn open_and_configure_connection(path: &Path) -> Result<Connection, DbError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let conn = Connection::open(path)?;

        conn.busy_timeout(Duration::from_millis(10000))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "wal_autocheckpoint", 1000)?;

        Ok(conn)
    }

    fn handle_insert_session(conn: &mut Connection, s: Session) -> Result<i64, DbError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id: i64 = tx.query_row(
            r#"
            INSERT INTO sessions (
                start_time, end_time, app_class, app_title, website_url, device_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(start_time, app_class, device_id)
            DO UPDATE SET
                end_time = excluded.end_time,
                app_title = excluded.app_title,
                website_url = excluded.website_url
            RETURNING id;
            "#,
            rusqlite::params![
                s.start_time,
                s.end_time,
                s.app_class,
                s.app_title,
                s.website_url,
                s.device_id
            ],
            |row| row.get(0),
        )?;
        tx.commit()?;
        Ok(id)
    }

    fn handle_update_end_time(
        conn: &mut Connection,
        session_id: i64,
        end_time: f64,
    ) -> Result<(), DbError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE sessions SET end_time = ?1 WHERE id = ?2",
            rusqlite::params![end_time, session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn handle_update_session_title(
        conn: &mut Connection,
        session_id: i64,
        title: String,
    ) -> Result<(), DbError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE sessions SET app_title = ?1 WHERE id = ?2",
            rusqlite::params![title, session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn handle_update_website_url(
        conn: &mut Connection,
        session_id: i64,
        url: Option<String>,
    ) -> Result<(), DbError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE sessions SET website_url = ?1 WHERE id = ?2",
            rusqlite::params![url, session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn handle_close_session(
        conn: &mut Connection,
        session_id: i64,
        end_time: f64,
        min_duration: f64,
    ) -> Result<bool, DbError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE sessions SET end_time = ?1 WHERE id = ?2",
            rusqlite::params![end_time, session_id],
        )?;
        let deleted = tx.execute(
            "DELETE FROM sessions WHERE id = ?1 AND (end_time - start_time) < ?2",
            rusqlite::params![session_id, min_duration],
        )?;
        tx.commit()?;
        Ok(deleted == 0)
    }

    fn handle_filter_noise_sessions(
        conn: &mut Connection,
        min_duration: f64,
    ) -> Result<usize, DbError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let deleted = tx.execute(
            "DELETE FROM sessions WHERE (end_time - start_time) < ?1",
            rusqlite::params![min_duration],
        )?;
        tx.commit()?;
        Ok(deleted)
    }

    fn handle_reload_title_rules(conn: &Connection) -> Result<TitleRulesCache, DbError> {
        let raw = Self::handle_get_title_rules_raw(conn)?;
        Ok(TitleRulesCache::new(raw))
    }

    fn handle_get_title_rules_raw(
        conn: &Connection,
    ) -> Result<HashMap<String, Vec<String>>, DbError> {
        let mut stmt = conn.prepare(
            "SELECT app_class, target_title FROM title_rules ORDER BY app_class, target_title",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for r in rows {
            let (app, target) = r?;
            map.entry(app).or_default().push(target);
        }
        Ok(map)
    }

    fn handle_add_title_rule(conn: &mut Connection, rule: TitleRule) -> Result<(), DbError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT OR IGNORE INTO title_rules (app_class, target_title) VALUES (?1, ?2)",
            rusqlite::params![rule.app_class, rule.target_title],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn handle_remove_title_rule(conn: &mut Connection, rule: TitleRule) -> Result<(), DbError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM title_rules WHERE app_class = ?1 AND target_title = ?2",
            rusqlite::params![rule.app_class, rule.target_title],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn handle_get_session(conn: &Connection, id: i64) -> Result<Option<Session>, DbError> {
        let mut stmt = conn.prepare(
            "SELECT id, start_time, end_time, app_class, app_title, website_url, device_id FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Session {
                id: Some(row.get(0)?),
                start_time: row.get(1)?,
                end_time: row.get(2)?,
                app_class: row.get(3)?,
                app_title: row.get(4)?,
                website_url: row.get(5)?,
                device_id: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    fn handle_get_sessions_in_range(
        conn: &Connection,
        start_time: f64,
        end_time: f64,
    ) -> Result<Vec<Session>, DbError> {
        let mut stmt = conn.prepare(
            "SELECT id, start_time, end_time, app_class, app_title, website_url, device_id FROM sessions WHERE end_time >= ?1 AND start_time <= ?2 ORDER BY start_time ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![start_time, end_time], |row| {
            Ok(Session {
                id: Some(row.get(0)?),
                start_time: row.get(1)?,
                end_time: row.get(2)?,
                app_class: row.get(3)?,
                app_title: row.get(4)?,
                website_url: row.get(5)?,
                device_id: row.get(6)?,
            })
        })?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    fn handle_query_pragma(conn: &Connection, pragma: &str) -> Result<String, DbError> {
        if !pragma
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(DbError::Other("Invalid pragma name".into()));
        }
        let sql = format!("PRAGMA {pragma}");
        let val: String = conn.query_row(&sql, [], |row| {
            let val: rusqlite::types::Value = row.get(0)?;
            match val {
                rusqlite::types::Value::Integer(i) => Ok(i.to_string()),
                rusqlite::types::Value::Text(s) => Ok(s),
                rusqlite::types::Value::Real(f) => Ok(f.to_string()),
                _ => Ok(String::new()),
            }
        })?;
        Ok(val)
    }

    pub async fn insert_session(&self, session: Session) -> Result<i64, DbError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(DbCommand::InsertSession { session, responder })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn update_end_time(&self, session_id: i64, end_time: f64) -> Result<(), DbError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(DbCommand::UpdateEndTime {
                session_id,
                end_time,
                responder,
            })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn update_session_title(
        &self,
        session_id: i64,
        title: impl Into<String>,
    ) -> Result<(), DbError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(DbCommand::UpdateSessionTitle {
                session_id,
                title: title.into(),
                responder,
            })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn update_website_url(
        &self,
        session_id: i64,
        url: Option<String>,
    ) -> Result<(), DbError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(DbCommand::UpdateWebsiteUrl {
                session_id,
                url,
                responder,
            })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn close_session(
        &self,
        session_id: i64,
        end_time: f64,
        min_duration: f64,
    ) -> Result<bool, DbError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(DbCommand::CloseSession {
                session_id,
                end_time,
                min_duration,
                responder,
            })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn filter_noise_sessions(&self, min_duration: f64) -> Result<usize, DbError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(DbCommand::FilterNoiseSessions {
                min_duration,
                responder,
            })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn reload_title_rules(&self) -> Result<TitleRulesCache, DbError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(DbCommand::ReloadTitleRules { responder })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn get_title_rules_raw(&self) -> Result<HashMap<String, Vec<String>>, DbError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(DbCommand::GetTitleRulesRaw { responder })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn add_title_rule(
        &self,
        app_class: impl Into<String>,
        target_title: impl Into<String>,
    ) -> Result<(), DbError> {
        let (responder, receiver) = oneshot::channel();
        let rule = TitleRule::new(app_class, target_title);
        self.sender
            .send(DbCommand::AddTitleRule { rule, responder })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn remove_title_rule(
        &self,
        app_class: impl Into<String>,
        target_title: impl Into<String>,
    ) -> Result<(), DbError> {
        let (responder, receiver) = oneshot::channel();
        let rule = TitleRule::new(app_class, target_title);
        self.sender
            .send(DbCommand::RemoveTitleRule { rule, responder })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn get_session(&self, id: i64) -> Result<Option<Session>, DbError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(DbCommand::GetSession { id, responder })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn get_sessions_in_range(
        &self,
        start_time: f64,
        end_time: f64,
    ) -> Result<Vec<Session>, DbError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(DbCommand::GetSessionsInRange {
                start_time,
                end_time,
                responder,
            })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }

    pub async fn query_pragma(&self, pragma: impl Into<String>) -> Result<String, DbError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(DbCommand::QueryPragma {
                pragma: pragma.into(),
                responder,
            })
            .await
            .map_err(|_| DbError::ChannelClosed)?;
        receiver.await.map_err(|_| DbError::ChannelClosed)?
    }
}
