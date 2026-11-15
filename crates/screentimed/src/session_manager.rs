use screentime_core::ipc::UrlUpdate;
use screentime_core::{
    DbActorHandle, DbError, IdleDetector, IdleState, Session, TitleRulesCache, WindowInfo,
    WindowTracker,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{debug, info};

pub fn current_time() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Helper to determine if an application class corresponds to a known web browser.
pub fn is_browser_app(app_class: &str) -> bool {
    let lower = app_class.to_lowercase();
    lower.contains("chrome")
        || lower.contains("chromium")
        || lower.contains("firefox")
        || lower.contains("brave")
        || lower.contains("vivaldi")
        || lower.contains("opera")
        || lower.contains("edge")
        || lower.contains("zen")
        || lower.contains("waterfox")
        || lower.contains("librewolf")
}

pub struct SessionManager {
    db: DbActorHandle,
    title_rules: TitleRulesCache,
    last_rules_reload: f64,
    rules_ttl: f64,
    device_id: String,
    current_session_id: Option<i64>,
    current_start_time: f64,
    current_window: Option<WindowInfo>,
    current_effective_class: Option<String>,
    current_url: Option<String>,
    is_idle: bool,
    last_heartbeat: f64,
    last_known_browser_url: Option<String>,
}

impl SessionManager {
    pub fn new(db: DbActorHandle) -> Self {
        Self {
            db,
            title_rules: TitleRulesCache::empty(),
            last_rules_reload: 0.0,
            rules_ttl: 60.0,
            device_id: "hyprland-pc".to_string(),
            current_session_id: None,
            current_start_time: 0.0,
            current_window: None,
            current_effective_class: None,
            current_url: None,
            is_idle: false,
            last_heartbeat: 0.0,
            last_known_browser_url: None,
        }
    }

    pub fn with_device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = device_id.into();
        self
    }

    pub fn with_title_rules(mut self, rules: TitleRulesCache) -> Self {
        self.title_rules = rules;
        self.last_rules_reload = current_time();
        self
    }

    pub fn with_rules_ttl(mut self, ttl_secs: f64) -> Self {
        self.rules_ttl = ttl_secs;
        self
    }

    pub fn current_session_id(&self) -> Option<i64> {
        self.current_session_id
    }

    pub fn current_window(&self) -> Option<&WindowInfo> {
        self.current_window.as_ref()
    }

    pub fn current_effective_class(&self) -> Option<&str> {
        self.current_effective_class.as_deref()
    }

    pub fn current_url(&self) -> Option<&str> {
        self.current_url.as_deref()
    }

    pub fn current_start_time(&self) -> f64 {
        self.current_start_time
    }

    pub fn is_idle(&self) -> bool {
        self.is_idle
    }

    pub async fn on_window_change(
        &mut self,
        window: Option<WindowInfo>,
    ) -> Result<Option<i64>, DbError> {
        self.handle_window_change(window, current_time()).await
    }

    pub async fn on_idle_change(&mut self, state: IdleState) -> Result<(), DbError> {
        self.handle_idle_change(state, current_time()).await
    }

    pub async fn on_browser_url(&mut self, url: impl Into<String>) -> Result<(), DbError> {
        self.handle_browser_url(url.into(), current_time()).await
    }

    pub async fn on_url_update(&mut self, update: UrlUpdate) -> Result<(), DbError> {
        self.handle_url_update(update, current_time()).await
    }

    pub async fn heartbeat(&mut self) -> Result<(), DbError> {
        self.handle_heartbeat(current_time()).await
    }

    pub async fn shutdown(&mut self) -> Result<(), DbError> {
        self.handle_shutdown(current_time()).await
    }

    pub async fn handle_window_change(
        &mut self,
        window: Option<WindowInfo>,
        now: f64,
    ) -> Result<Option<i64>, DbError> {
        if self.is_idle {
            self.current_window = window;
            return Ok(None);
        }

        if self.current_window.as_ref() == window.as_ref() {
            return Ok(self.current_session_id);
        }

        let Some(win) = window else {
            if let Some(id) = self.current_session_id.take() {
                self.db.close_session(id, now, 1.0).await?;
            }
            self.current_window = None;
            self.current_effective_class = None;
            self.current_url = None;
            self.current_start_time = 0.0;
            return Ok(None);
        };

        if (now - self.last_rules_reload) > self.rules_ttl {
            if let Ok(reloaded) = self.db.reload_title_rules().await {
                self.title_rules = reloaded;
                self.last_rules_reload = now;
            }
        }

        let effective_class = self
            .title_rules
            .resolve_app_class(&win.app_class, Some(&win.title));

        if let Some(id) = self.current_session_id.take() {
            self.db.close_session(id, now, 1.0).await?;
        }

        self.current_window = Some(win.clone());
        self.current_effective_class = Some(effective_class.clone());
        self.current_start_time = now;

        let initial_url = if is_browser_app(&win.app_class) {
            self.last_known_browser_url.clone()
        } else {
            None
        };
        self.current_url = initial_url.clone();

        let mut session = Session::new(now, now, effective_class)
            .with_title(win.title)
            .with_device_id(self.device_id.clone());

        if let Some(u) = initial_url {
            session = session.with_url(u);
        }

        let new_id = self.db.insert_session(session).await?;
        self.current_session_id = Some(new_id);

        Ok(Some(new_id))
    }

    pub async fn handle_idle_change(&mut self, state: IdleState, now: f64) -> Result<(), DbError> {
        match state {
            IdleState::Idle => {
                self.is_idle = true;
                if let Some(id) = self.current_session_id.take() {
                    self.db.close_session(id, now, 1.0).await?;
                }
                self.current_start_time = 0.0;
                self.current_url = None;
                self.current_effective_class = None;
            }
            IdleState::Active => {
                self.is_idle = false;
                if let Some(win) = self.current_window.clone() {
                    let effective_class = self
                        .title_rules
                        .resolve_app_class(&win.app_class, Some(&win.title));

                    self.current_effective_class = Some(effective_class.clone());
                    self.current_start_time = now;

                    let initial_url = if is_browser_app(&win.app_class) {
                        self.last_known_browser_url.clone()
                    } else {
                        None
                    };
                    self.current_url = initial_url.clone();

                    let mut session = Session::new(now, now, effective_class)
                        .with_title(win.title)
                        .with_device_id(self.device_id.clone());

                    if let Some(u) = initial_url {
                        session = session.with_url(u);
                    }

                    let new_id = self.db.insert_session(session).await?;
                    self.current_session_id = Some(new_id);
                }
            }
        }
        Ok(())
    }

    pub async fn handle_url_update(&mut self, update: UrlUpdate, now: f64) -> Result<(), DbError> {
        let Some(session_id) = self.current_session_id else {
            if let Some(url) = update.url {
                self.last_known_browser_url = Some(url);
            }
            return Ok(());
        };

        if self.is_idle || self.current_window.is_none() {
            if let Some(url) = update.url {
                self.last_known_browser_url = Some(url);
            }
            return Ok(());
        }

        let Some(url) = update.url else {
            self.last_known_browser_url = None;
            if self.current_url.is_some() {
                self.db.close_session(session_id, now, 1.0).await?;

                let win = self.current_window.as_ref().unwrap();
                let effective_class = self
                    .current_effective_class
                    .clone()
                    .unwrap_or_else(|| win.app_class.clone());

                self.current_start_time = now;
                self.current_url = None;

                let session = Session::new(now, now, effective_class)
                    .with_title(win.title.clone())
                    .with_device_id(self.device_id.clone());

                let new_id = self.db.insert_session(session).await?;
                self.current_session_id = Some(new_id);
            }
            return Ok(());
        };

        self.last_known_browser_url = Some(url.clone());

        if self.current_url.as_deref() == Some(&url) {
            return Ok(());
        }

        let elapsed = now - self.current_start_time;

        if elapsed <= 2.0 && self.current_url.is_none() {
            self.db
                .update_website_url(session_id, Some(url.clone()))
                .await?;
            self.current_url = Some(url);
        } else {
            self.db.close_session(session_id, now, 1.0).await?;

            let win = self.current_window.as_ref().unwrap();
            let effective_class = self
                .current_effective_class
                .clone()
                .unwrap_or_else(|| win.app_class.clone());

            let title = update.title.unwrap_or_else(|| win.title.clone());

            self.current_start_time = now;
            self.current_url = Some(url.clone());

            let session = Session::new(now, now, effective_class)
                .with_title(title)
                .with_url(url)
                .with_device_id(self.device_id.clone());

            let new_id = self.db.insert_session(session).await?;
            self.current_session_id = Some(new_id);
        }

        Ok(())
    }

    pub async fn handle_browser_url(&mut self, url: String, now: f64) -> Result<(), DbError> {
        self.handle_url_update(
            UrlUpdate {
                url: Some(url),
                title: None,
                browser_type: "browser".to_string(),
            },
            now,
        )
        .await
    }

    pub async fn handle_heartbeat(&mut self, now: f64) -> Result<(), DbError> {
        if self.is_idle {
            return Ok(());
        }

        if self.last_heartbeat > 0.0 && (now - self.last_heartbeat) > 30.0 {
            if let Some(id) = self.current_session_id.take() {
                self.db.close_session(id, self.last_heartbeat, 1.0).await?;
            }

            if let Some(win) = self.current_window.clone() {
                let effective_class = self
                    .title_rules
                    .resolve_app_class(&win.app_class, Some(&win.title));

                self.current_effective_class = Some(effective_class.clone());
                self.current_start_time = now;

                let session = Session::new(now, now, effective_class)
                    .with_title(win.title)
                    .with_device_id(self.device_id.clone());

                let new_id = self.db.insert_session(session).await?;
                self.current_session_id = Some(new_id);
            }
        } else if let Some(id) = self.current_session_id {
            self.db.update_end_time(id, now).await?;
        }

        self.last_heartbeat = now;
        Ok(())
    }

    pub async fn handle_shutdown(&mut self, now: f64) -> Result<(), DbError> {
        if let Some(id) = self.current_session_id.take() {
            self.db.close_session(id, now, 1.0).await?;
        }
        self.current_start_time = 0.0;
        self.current_url = None;
        self.current_window = None;
        self.current_effective_class = None;
        Ok(())
    }

    pub async fn run_loop<W, I, U>(
        mut self,
        mut tracker: W,
        mut idle: I,
        mut url_rx: mpsc::Receiver<U>,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) -> Result<(), DbError>
    where
        W: WindowTracker,
        I: IdleDetector,
        U: Into<UrlUpdate> + Send + 'static,
    {
        let (win_tx, mut win_rx) = mpsc::channel(64);
        tracker
            .start(win_tx)
            .await
            .map_err(|e| DbError::Other(e.to_string()))?;

        let (idle_tx, mut idle_rx) = mpsc::channel(16);
        idle.start(idle_tx)
            .await
            .map_err(|e| DbError::Other(e.to_string()))?;

        if let Ok(initial_win) = tracker.query_active_window().await {
            self.on_window_change(initial_win).await?;
        }

        let mut heartbeat_ticker = tokio::time::interval(Duration::from_secs(5));
        heartbeat_ticker.tick().await;

        loop {
            tokio::select! {
                Some(win) = win_rx.recv() => {
                    debug!("Window focus changed: {:?}", win);
                    self.on_window_change(win).await?;
                }
                Some(idle_state) = idle_rx.recv() => {
                    debug!("Idle state changed: {:?}", idle_state);
                    self.on_idle_change(idle_state).await?;
                }
                Some(url) = url_rx.recv() => {
                    let update = url.into();
                    debug!("Browser URL received: {:?}", update);
                    self.on_url_update(update).await?;
                }
                _ = heartbeat_ticker.tick() => {
                    self.heartbeat().await?;
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received, closing session");
                    self.shutdown().await?;
                    let _ = tracker.stop().await;
                    let _ = idle.stop().await;
                    break;
                }
            }
        }

        Ok(())
    }
}
