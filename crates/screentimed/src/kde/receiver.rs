use screentime_core::WindowInfo;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::debug;
use zbus::interface;

pub fn parse_window_changed(app_class: &str, title: &str) -> Option<WindowInfo> {
    let trimmed_class = app_class.trim();
    if trimmed_class.is_empty() {
        return None;
    }
    Some(WindowInfo::new(trimmed_class, title.trim()))
}

pub struct KdeTrackerReceiver {
    tx: mpsc::Sender<Option<WindowInfo>>,
    current: Arc<Mutex<Option<WindowInfo>>>,
}

impl KdeTrackerReceiver {
    pub fn new(
        tx: mpsc::Sender<Option<WindowInfo>>,
        current: Arc<Mutex<Option<WindowInfo>>>,
    ) -> Self {
        Self { tx, current }
    }
}

#[interface(name = "org.screentime.Tracker")]
impl KdeTrackerReceiver {
    #[zbus(name = "WindowChanged")]
    pub async fn window_changed(&self, app_class: String, title: String) {
        let info = parse_window_changed(&app_class, &title);
        debug!("KDE D-Bus WindowChanged received: {:?}", info);

        {
            let mut cur = self.current.lock().await;
            *cur = info.clone();
        }

        let _ = self.tx.send(info).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_window_changed_normal() {
        let win = parse_window_changed("kitty", "fish").unwrap();
        assert_eq!(win.app_class, "kitty");
        assert_eq!(win.title, "fish");

        let win2 = parse_window_changed("firefox", "Issue #42: test, comma").unwrap();
        assert_eq!(win2.app_class, "firefox");
        assert_eq!(win2.title, "Issue #42: test, comma");
    }

    #[test]
    fn test_parse_window_changed_empty_and_whitespace() {
        assert_eq!(parse_window_changed("", ""), None);
        assert_eq!(parse_window_changed("   ", "   "), None);
        assert_eq!(parse_window_changed("", "Only Title"), None);
        assert_eq!(parse_window_changed("   ", "Only Title"), None);
    }

    #[test]
    fn test_parse_window_changed_empty_title() {
        let win = parse_window_changed("alacritty", "").unwrap();
        assert_eq!(win.app_class, "alacritty");
        assert_eq!(win.title, "");
    }
}
