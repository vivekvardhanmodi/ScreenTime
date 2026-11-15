use screentime_core::TrackerError;
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream as StdUnixStream;
use tokio::io::unix::AsyncFd;
use tokio::sync::{mpsc, watch};
use tracing::warn;
use wayland_client::backend::WaylandError;
use wayland_client::{Connection, EventQueue};

/// File descriptor wrapper implementing AsRawFd for Tokio's AsyncFd.
pub struct WaylandFd(pub RawFd);

impl AsRawFd for WaylandFd {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

/// Source of the Wayland connection: standard environment or an existing Unix socket stream.
pub enum WaylandConnectionSource {
    Env,
    CustomStream(Option<StdUnixStream>),
}

impl WaylandConnectionSource {
    pub fn connect(&mut self) -> Result<Connection, TrackerError> {
        match self {
            Self::Env => Connection::connect_to_env().map_err(|e| {
                TrackerError::Tracker(format!("Failed to connect to Wayland display: {e}"))
            }),
            Self::CustomStream(ref mut stream_opt) => {
                let stream = stream_opt.take().ok_or_else(|| {
                    TrackerError::Tracker(
                        "Custom Wayland socket stream was already consumed".to_string(),
                    )
                })?;
                Connection::from_socket(stream).map_err(|e| {
                    TrackerError::Tracker(format!(
                        "Failed to create Wayland connection from socket: {e}"
                    ))
                })
            }
        }
    }
}

/// Trait implemented by Wayland states to drain buffered outgoing events into an mpsc channel.
pub trait OutgoingDrainer {
    type Event: Send + 'static;
    fn pop_outgoing(&mut self) -> Option<Self::Event>;
}

async fn drain_outgoing<S: OutgoingDrainer>(state: &mut S, tx: &mpsc::Sender<S::Event>) -> bool {
    while let Some(evt) = state.pop_outgoing() {
        if tx.send(evt).await.is_err() {
            return false;
        }
    }
    true
}

/// Runs the Wayland dispatch event loop asynchronously using Tokio's AsyncFd.
///
/// This provides non-blocking Wayland event dispatching integrated directly
/// into Tokio's async reactor with instant cooperative cancellation.
pub async fn run_wayland_event_loop<S>(
    conn: Connection,
    mut event_queue: EventQueue<S>,
    mut state: S,
    tx: mpsc::Sender<S::Event>,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<S, TrackerError>
where
    S: OutgoingDrainer + 'static,
{
    // First, dispatch any events currently pending in the queue
    let _ = event_queue.dispatch_pending(&mut state);
    let _ = conn.flush();

    if !drain_outgoing(&mut state, &tx).await {
        return Ok(state);
    }

    let raw_fd = conn.backend().poll_fd().as_raw_fd();
    // Ensure the socket is in non-blocking mode as required by Tokio's AsyncFd.
    unsafe {
        use std::os::unix::io::FromRawFd;
        let stream = std::mem::ManuallyDrop::new(StdUnixStream::from_raw_fd(raw_fd));
        let _ = stream.set_nonblocking(true);
    }

    // SAFETY: WaylandFd wraps the active socket fd whose lifetime is managed by `conn`.
    let async_fd = unsafe {
        AsyncFd::register(WaylandFd(raw_fd)).map_err(|e| TrackerError::Io(e.into_parts().1))?
    };

    while !*stop_rx.borrow() {
        // Dispatch pending events
        if let Err(e) = event_queue.dispatch_pending(&mut state) {
            warn!("Error dispatching pending Wayland events: {e}");
            return Err(TrackerError::Other(e.to_string()));
        }

        // Drain outgoing events
        if !drain_outgoing(&mut state, &tx).await {
            break;
        }

        // Flush any queued outgoing requests
        if let Err(e) = conn.flush() {
            warn!("Error flushing Wayland connection: {e}");
            return Err(TrackerError::Other(e.to_string()));
        }

        // Prepare read
        let guard = match conn.prepare_read() {
            Some(guard) => guard,
            None => {
                // If None, events were buffered during prepare; dispatch them next iteration
                continue;
            }
        };

        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    // Dropping guard safely cancels the read intent in libwayland
                    break;
                }
            }
            res = async_fd.readable() => {
                match res {
                    Ok(mut ready_guard) => {
                        match guard.read() {
                            Ok(_) => {
                                ready_guard.clear_ready();
                                if let Err(e) = event_queue.dispatch_pending(&mut state) {
                                    warn!("Error dispatching Wayland events: {e}");
                                    return Err(TrackerError::Other(e.to_string()));
                                }
                                if !drain_outgoing(&mut state, &tx).await {
                                    break;
                                }
                            }
                            Err(WaylandError::Io(e))
                                if e.kind() == std::io::ErrorKind::WouldBlock =>
                            {
                                ready_guard.clear_ready();
                            }
                            Err(e) => {
                                warn!("Wayland socket read error: {e}");
                                return Err(TrackerError::Other(e.to_string()));
                            }
                        }
                    }
                    Err(e) => {
                        warn!("AsyncFd readable error: {e}");
                        return Err(TrackerError::Io(e));
                    }
                }
            }
        }
    }

    Ok(state)
}
