use serde::{Deserialize, Serialize};

pub fn extract_domain(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }

    let without_scheme = if let Some(idx) = url.find("://") {
        &url[idx + 3..]
    } else {
        url
    };

    let host_and_port = without_scheme.split(&['/', '?', '#'][..]).next()?;
    let host = host_and_port.split(':').next()?.trim();
    if host.is_empty() {
        return None;
    }

    let domain = if let Some(stripped) = host.strip_prefix("www.") {
        stripped
    } else {
        host
    };

    Some(domain.to_string())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: Option<i64>,
    pub start_time: f64,
    pub end_time: f64,
    pub app_class: String,
    pub app_title: Option<String>,
    pub website_url: Option<String>,
    pub device_id: String,
}

impl Session {
    pub fn new(start_time: f64, end_time: f64, app_class: impl Into<String>) -> Self {
        Self {
            id: None,
            start_time,
            end_time,
            app_class: app_class.into(),
            app_title: None,
            website_url: None,
            device_id: "hyprland-pc".to_string(),
        }
    }

    pub fn with_id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.app_title = Some(title.into());
        self
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.website_url = Some(url.into());
        self
    }

    pub fn with_device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = device_id.into();
        self
    }

    pub fn duration(&self) -> f64 {
        self.end_time - self.start_time
    }

    pub fn display_name(&self) -> String {
        if let Some(ref url) = self.website_url {
            if let Some(domain) = extract_domain(url) {
                return domain;
            }
        }
        self.app_class.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowInfo {
    pub app_class: String,
    pub title: String,
}

impl WindowInfo {
    pub fn new(app_class: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            app_class: app_class.into(),
            title: title.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IdleState {
    Active,
    Idle,
}

impl IdleState {
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TitleRule {
    pub app_class: String,
    pub target_title: String,
}

impl TitleRule {
    pub fn new(app_class: impl Into<String>, target_title: impl Into<String>) -> Self {
        Self {
            app_class: app_class.into(),
            target_title: target_title.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            extract_domain("https://www.github.com/rust-lang"),
            Some("github.com".to_string())
        );
        assert_eq!(
            extract_domain("http://crates.io:8080/search?q=rusqlite"),
            Some("crates.io".to_string())
        );
        assert_eq!(
            extract_domain("wikipedia.org/wiki/Rust"),
            Some("wikipedia.org".to_string())
        );
        assert_eq!(extract_domain(""), None);
    }

    #[test]
    fn test_session_display_name_and_duration() {
        let session =
            Session::new(100.0, 105.5, "firefox").with_url("https://www.youtube.com/watch?v=123");
        assert_eq!(session.duration(), 5.5);
        assert_eq!(session.display_name(), "youtube.com");

        let app_session = Session::new(100.0, 110.0, "alacritty");
        assert_eq!(app_session.display_name(), "alacritty");
    }

    #[test]
    fn test_idle_state_methods() {
        assert!(IdleState::Active.is_active());
        assert!(!IdleState::Active.is_idle());
        assert!(IdleState::Idle.is_idle());
        assert!(!IdleState::Idle.is_active());
    }
}
