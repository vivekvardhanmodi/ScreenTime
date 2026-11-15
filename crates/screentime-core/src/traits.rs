use crate::domain::{IdleState, WindowInfo};
use async_trait::async_trait;
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
}
