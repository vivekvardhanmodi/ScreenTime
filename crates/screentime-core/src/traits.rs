use crate::domain::{IdleState, WindowInfo};
use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
pub enum TrackerError {
    #[error("Tracker error: {0}")]
    Tracker(String),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

#[async_trait]
pub trait WindowTracker: Send + Sync {
    async fn start(&mut self, tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError>;
    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError>;
    async fn stop(&mut self) -> Result<(), TrackerError>;
}

#[async_trait]
pub trait IdleDetector: Send + Sync {
    async fn start(&mut self, tx: mpsc::Sender<IdleState>) -> Result<(), TrackerError>;
    fn is_idle(&self) -> bool;
    async fn stop(&mut self) -> Result<(), TrackerError>;

    /// Returns the configured idle timeout/threshold, if supported by the detector.
    fn idle_timeout(&self) -> Option<Duration> {
        None
    }
}

#[async_trait]
impl<T: ?Sized + WindowTracker + Send + Sync> WindowTracker for Box<T> {
    async fn start(&mut self, tx: mpsc::Sender<Option<WindowInfo>>) -> Result<(), TrackerError> {
        (**self).start(tx).await
    }

    async fn query_active_window(&self) -> Result<Option<WindowInfo>, TrackerError> {
        (**self).query_active_window().await
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        (**self).stop().await
    }
}

#[async_trait]
impl<T: ?Sized + IdleDetector + Send + Sync> IdleDetector for Box<T> {
    async fn start(&mut self, tx: mpsc::Sender<IdleState>) -> Result<(), TrackerError> {
        (**self).start(tx).await
    }

    fn is_idle(&self) -> bool {
        (**self).is_idle()
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        (**self).stop().await
    }

    fn idle_timeout(&self) -> Option<Duration> {
        (**self).idle_timeout()
    }
}

#[derive(Debug, Default, Clone)]
pub struct NoopIdleDetector;

#[async_trait]
impl IdleDetector for NoopIdleDetector {
    async fn start(&mut self, _tx: mpsc::Sender<IdleState>) -> Result<(), TrackerError> {
        Ok(())
    }

    fn is_idle(&self) -> bool {
        false
    }

    async fn stop(&mut self) -> Result<(), TrackerError> {
        Ok(())
    }
}
