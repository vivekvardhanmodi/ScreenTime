use crate::sleep::SleepEvent;
use screentime_core::ipc::UrlUpdate;
use screentime_core::{
    DbActorHandle, DbError, IdleDetector, IdleState, Session, TitleRulesCache, WindowInfo,
    WindowTracker,
};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{debug, info};

pub fn current_time() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn browser_family_from_tokens(lower: &str) -> Option<&'static str> {
    for token in lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
    {
        match token {
            "firefox" | "zen" | "waterfox" | "librewolf" | "floorp" => return Some("firefox"),
            "chrome" | "chromium" | "googlechrome" | "brave" | "vivaldi" | "opera" | "edge"
            | "msedge" => return Some("chrome"),
            _ => {}
        }
    }
    None
}

/// Helper to determine if an application class corresponds to a known web browser.
pub fn is_browser_app(app_class: &str) -> bool {
    browser_family_from_tokens(&app_class.to_lowercase()).is_some()
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
    current_title: Option<String>,
    current_url: Option<String>,
    is_idle: bool,
    is_sleeping: bool,
    is_lid_closed: bool,
    has_external_display: bool,
    sleep_rx: Option<mpsc::Receiver<SleepEvent>>,
    last_heartbeat: f64,
    browser_urls: HashMap<String, String>,
    chrome_window_class: String,
    firefox_window_class: String,
    heartbeat_interval: Duration,
    min_duration: f64,
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
            current_title: None,
            current_url: None,
            is_idle: false,
            is_sleeping: false,
            is_lid_closed: false,
            has_external_display: false,
            sleep_rx: None,
            last_heartbeat: 0.0,
            browser_urls: HashMap::new(),
            chrome_window_class: "google-chrome".to_string(),
            firefox_window_class: "firefox".to_string(),
            heartbeat_interval: Duration::from_secs(10),
            min_duration: 1.0,
        }
    }

    pub fn with_min_duration(mut self, min_duration: f64) -> Self {
        self.min_duration = min_duration;
        self
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

    pub fn with_browser_classes(
        mut self,
        chrome: impl Into<String>,
        firefox: impl Into<String>,
    ) -> Self {
        self.chrome_window_class = chrome.into();
        self.firefox_window_class = firefox.into();
        self
    }

    pub fn with_heartbeat_interval(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = interval;
        self
    }

    pub fn with_daemon_config(mut self, config: &crate::config::DaemonConfig) -> Self {
        self.chrome_window_class = config.chrome_window_class.clone();
        self.firefox_window_class = config.firefox_window_class.clone();
        self.heartbeat_interval = Duration::from_secs(config.heartbeat_interval);
        self
    }

    pub fn with_sleep_receiver(mut self, rx: mpsc::Receiver<SleepEvent>) -> Self {
        self.sleep_rx = Some(rx);
        self
    }

    pub fn chrome_window_class(&self) -> &str {
        &self.chrome_window_class
    }

    pub fn firefox_window_class(&self) -> &str {
        &self.firefox_window_class
    }

    pub fn heartbeat_interval(&self) -> Duration {
        self.heartbeat_interval
    }

    fn browser_family_for_class(&self, app_class: &str) -> Option<&'static str> {
        let lower = app_class.to_lowercase();
        let chrome_lower = self.chrome_window_class.to_lowercase();
        let firefox_lower = self.firefox_window_class.to_lowercase();

        if !firefox_lower.is_empty() && lower.contains(&firefox_lower) {
            Some("firefox")
        } else if !chrome_lower.is_empty() && lower.contains(&chrome_lower) {
            Some("chrome")
        } else {
            browser_family_from_tokens(&lower)
        }
    }

    fn normalize_browser_source(&self, source: &str) -> String {
        let lower = source.trim().to_lowercase();
        if lower.is_empty() || lower == "browser" {
            return "browser".to_string();
        }
        if let Some(family) = self.browser_family_for_class(&lower) {
            family.to_string()
        } else {
            lower
        }
    }

    fn store_browser_url(&mut self, source_key: &str, url: Option<String>) {
        if let Some(u) = url {
            self.browser_urls.insert(source_key.to_string(), u);
        } else {
            self.browser_urls.remove(source_key);
        }
    }

    pub fn is_browser(&self, app_class: &str) -> bool {
        self.browser_family_for_class(app_class).is_some()
    }

    pub fn current_session_id(&self) -> Option<i64> {
        self.current_session_id
    }

    pub fn current_window(&self) -> Option<&WindowInfo> {
        self.current_window.as_ref()
    }

    pub fn sync_current_window(&mut self, window: Option<WindowInfo>) {
        self.current_window = window;
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

    pub fn is_sleeping(&self) -> bool {
        self.is_sleeping
    }

    pub fn is_lid_closed(&self) -> bool {
        self.is_lid_closed
    }

    pub fn has_external_display(&self) -> bool {
        self.has_external_display
    }

    pub fn is_tracking_active(&self) -> bool {
        !self.is_idle && !self.is_sleeping && (!self.is_lid_closed || self.has_external_display)
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

    pub async fn on_sleep_event(&mut self, event: SleepEvent) -> Result<(), DbError> {
        self.handle_sleep_event(event, current_time()).await
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

    async fn maybe_reload_title_rules(&mut self, now: f64) {
        if (now - self.last_rules_reload) > self.rules_ttl {
            if let Ok(reloaded) = self.db.reload_title_rules().await {
                self.title_rules = reloaded;
                self.last_rules_reload = now;
            }
        }
    }

    async fn close_active_session(&mut self, end_time: f64) -> Result<(), DbError> {
        if let Some(id) = self.current_session_id.take() {
            self.db.close_session(id, end_time, self.min_duration).await?;
        }
        self.current_start_time = 0.0;
        self.current_url = None;
        self.current_title = None;
        self.current_effective_class = None;
        self.last_heartbeat = 0.0;
        Ok(())
    }

    async fn start_session(&mut self, win: WindowInfo, now: f64) -> Result<i64, DbError> {
        self.maybe_reload_title_rules(now).await;

        let effective_class = self
            .title_rules
            .resolve_app_class(&win.app_class, Some(&win.title));

        let initial_url = self
            .browser_family_for_class(&win.app_class)
            .and_then(|family| {
                self.browser_urls
                    .get(family)
                    .or_else(|| self.browser_urls.get("browser"))
                    .cloned()
            });

        let mut session = Session::new(now, now, effective_class.clone())
            .with_title(win.title.clone())
            .with_device_id(self.device_id.clone());

        if let Some(ref u) = initial_url {
            session = session.with_url(u.clone());
        }

        let new_id = self.db.insert_session(session).await?;
        self.current_title = Some(win.title.clone());
        self.current_window = Some(win);
        self.current_effective_class = Some(effective_class);
        self.current_start_time = now;
        self.current_url = initial_url;
        self.current_session_id = Some(new_id);
        self.last_heartbeat = now;

        Ok(new_id)
    }

    async fn resume_tracking_if_active(&mut self, now: f64) -> Result<(), DbError> {
        if !self.is_tracking_active() || self.current_session_id.is_some() {
            return Ok(());
        }

        if let Some(win) = self.current_window.clone() {
            self.start_session(win, now).await?;
        }

        Ok(())
    }

    pub async fn handle_sleep_event(&mut self, event: SleepEvent, now: f64) -> Result<(), DbError> {
        match event {
            SleepEvent::Sleep => {
                self.is_sleeping = true;
                self.close_active_session(now).await?;
            }
            SleepEvent::Wake => {
                self.is_sleeping = false;
                self.resume_tracking_if_active(now).await?;
            }
            SleepEvent::LidClosed {
                has_external_display,
            } => {
                self.is_lid_closed = true;
                self.has_external_display = has_external_display;
                if has_external_display {
                    self.resume_tracking_if_active(now).await?;
                } else {
                    self.close_active_session(now).await?;
                }
            }
            SleepEvent::LidOpened => {
                self.is_lid_closed = false;
                self.resume_tracking_if_active(now).await?;
            }
        }
        Ok(())
    }

    pub async fn handle_window_change(
        &mut self,
        window: Option<WindowInfo>,
        now: f64,
    ) -> Result<Option<i64>, DbError> {
        if !self.is_tracking_active() {
            self.current_window = window;
            return Ok(None);
        }

        if self.current_session_id.is_some() && self.current_window.as_ref() == window.as_ref() {
            return Ok(self.current_session_id);
        }

        let Some(win) = window else {
            self.close_active_session(now).await?;
            self.current_window = None;
            return Ok(None);
        };

        self.close_active_session(now).await?;
        let new_id = self.start_session(win, now).await?;
        Ok(Some(new_id))
    }

    pub async fn handle_idle_change(&mut self, state: IdleState, now: f64) -> Result<(), DbError> {
        match state {
            IdleState::Idle => {
                let was_idle = self.is_idle;
                self.is_idle = true;
                if !was_idle {
                    self.close_active_session(now).await?;
                }
            }
            IdleState::Active => {
                let was_idle = self.is_idle;
                self.is_idle = false;
                if was_idle {
                    self.resume_tracking_if_active(now).await?;
                }
            }
        }
        Ok(())
    }

    pub async fn handle_url_update(&mut self, update: UrlUpdate, now: f64) -> Result<(), DbError> {
        let source_key = self.normalize_browser_source(&update.browser_type);

        let Some(session_id) = self.current_session_id else {
            self.store_browser_url(&source_key, update.url);
            return Ok(());
        };

        if !self.is_tracking_active() || self.current_window.is_none() {
            self.store_browser_url(&source_key, update.url);
            return Ok(());
        }

        let active_family = self
            .current_window
            .as_ref()
            .and_then(|win| self.browser_family_for_class(&win.app_class));

        let Some(active_family) = active_family else {
            self.store_browser_url(&source_key, update.url);
            return Ok(());
        };

        // Ignore updates from a different browser family while buffering them for later focus
        if source_key != "browser" && source_key != active_family {
            self.store_browser_url(&source_key, update.url);
            return Ok(());
        }

        let cache_key = if source_key == "browser" {
            active_family
        } else {
            source_key.as_str()
        };

        let Some(url) = update.url else {
            self.store_browser_url(cache_key, None);
            if self.current_url.is_some() {
                let win_clone = self.current_window.clone().unwrap();
                let effective_class = self
                    .current_effective_class
                    .clone()
                    .unwrap_or_else(|| win_clone.app_class.clone());

                self.close_active_session(now).await?;

                let session = Session::new(now, now, effective_class.clone())
                    .with_title(win_clone.title.clone())
                    .with_device_id(self.device_id.clone());

                let new_id = self.db.insert_session(session).await?;
                self.current_effective_class = Some(effective_class);
                self.current_title = Some(win_clone.title);
                self.current_start_time = now;
                self.current_url = None;
                self.current_session_id = Some(new_id);
                self.last_heartbeat = now;
            }
            return Ok(());
        };

        self.store_browser_url(cache_key, Some(url.clone()));

        let trimmed_title = update
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(ToString::to_string);

        if self.current_url.as_deref() == Some(&url) {
            if let Some(title) = trimmed_title {
                if self.current_title.as_deref() != Some(title.as_str()) {
                    self.db
                        .update_session_title(session_id, title.clone())
                        .await?;
                    self.current_title = Some(title);
                }
            }
            return Ok(());
        }

        let elapsed = now - self.current_start_time;

        if elapsed <= 2.0 && self.current_url.is_none() {
            if let Some(title) = trimmed_title {
                self.db
                    .update_session_title(session_id, title.clone())
                    .await?;
                self.current_title = Some(title);
            }
            self.db
                .update_website_url(session_id, Some(url.clone()))
                .await?;
            self.current_url = Some(url);
        } else {
            let win_clone = self.current_window.clone().unwrap();
            let effective_class = self
                .current_effective_class
                .clone()
                .unwrap_or_else(|| win_clone.app_class.clone());
            let title = trimmed_title.unwrap_or(win_clone.title);

            self.close_active_session(now).await?;

            let session = Session::new(now, now, effective_class.clone())
                .with_title(title.clone())
                .with_url(url.clone())
                .with_device_id(self.device_id.clone());

            let new_id = self.db.insert_session(session).await?;
            self.current_effective_class = Some(effective_class);
            self.current_title = Some(title);
            self.current_start_time = now;
            self.current_url = Some(url);
            self.current_session_id = Some(new_id);
            self.last_heartbeat = now;
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
        if !self.is_tracking_active() {
            return Ok(());
        }

        let sleep_threshold = (self.heartbeat_interval.as_secs_f64() * 3.0).max(30.0);
        if self.last_heartbeat > 0.0 && (now - self.last_heartbeat) > sleep_threshold {
            let prev_hb = self.last_heartbeat;
            self.close_active_session(prev_hb).await?;

            if let Some(win) = self.current_window.clone() {
                self.start_session(win, now).await?;
            }
        } else if let Some(id) = self.current_session_id {
            self.db.update_end_time(id, now).await?;
            self.last_heartbeat = now;
        }

        Ok(())
    }

    pub async fn handle_shutdown(&mut self, now: f64) -> Result<(), DbError> {
        self.close_active_session(now).await?;
        self.current_window = None;
        Ok(())
    }

    pub async fn run_loop_with_sleep<W, I, U>(
        mut self,
        tracker: W,
        idle: I,
        url_rx: mpsc::Receiver<U>,
        sleep_rx: mpsc::Receiver<SleepEvent>,
        shutdown_rx: mpsc::Receiver<()>,
    ) -> Result<(), DbError>
    where
        W: WindowTracker,
        I: IdleDetector,
        U: Into<UrlUpdate> + Send + 'static,
    {
        self.sleep_rx = Some(sleep_rx);
        self.run_loop(tracker, idle, url_rx, shutdown_rx).await
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

        let mut sleep_rx = self.sleep_rx.take();

        if let Ok(initial_win) = tracker.query_active_window().await {
            self.on_window_change(initial_win).await?;
        }

        let mut heartbeat_ticker = tokio::time::interval(self.heartbeat_interval);
        heartbeat_ticker.tick().await;

        loop {
            tokio::select! {
                Some(win) = win_rx.recv() => {
                    debug!("Window focus changed: {:?}", win);
                    self.on_window_change(win).await?;
                }
                Some(idle_state) = idle_rx.recv() => {
                    debug!("Idle state changed: {:?}", idle_state);
                    let queried = if idle_state.is_active() && self.is_idle() {
                        tracker.query_active_window().await.ok()
                    } else {
                        None
                    };
                    if let Some(active_win) = queried {
                        self.sync_current_window(active_win);
                    }
                    self.on_idle_change(idle_state).await?;
                }
                Some(url) = url_rx.recv() => {
                    let update = url.into();
                    debug!("Browser URL received: {:?}", update);
                    self.on_url_update(update).await?;
                }
                sleep_event = async {
                    match sleep_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match sleep_event {
                        Some(event) => {
                            debug!("Sleep event received: {:?}", event);
                            let queried = if matches!(event, SleepEvent::Wake | SleepEvent::LidOpened) {
                                tracker.query_active_window().await.ok()
                            } else {
                                None
                            };
                            if let Some(active_win) = queried {
                                self.sync_current_window(active_win);
                            }
                            self.on_sleep_event(event).await?;
                        }
                        None => {
                            sleep_rx = None;
                        }
                    }
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
