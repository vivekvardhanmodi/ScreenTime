use interprocess::local_socket::tokio::Stream;
use interprocess::local_socket::traits::tokio::{Listener as _, Stream as _};
use interprocess::local_socket::{ListenerOptions, Name};
use screentime_core::ipc::{
    default_socket_name, default_socket_path, socket_name_from_path, BrowserIpcMessage, UrlUpdate,
};
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

#[derive(Debug, Error)]
pub enum IpcServerError {
    #[error("Another instance of screentimed is already running")]
    AlreadyRunning,
    #[error("IPC I/O error: {0}")]
    Io(#[from] io::Error),
}

pub struct IpcServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    join_handle: Option<tokio::task::JoinHandle<Result<(), IpcServerError>>>,
    #[cfg(unix)]
    socket_path: Option<PathBuf>,
    #[cfg(windows)]
    _mutex: Option<Win32NamedMutex>,
}

impl IpcServerHandle {
    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.await;
        }
        #[cfg(unix)]
        if let Some(ref path) = self.socket_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for IpcServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        #[cfg(unix)]
        if let Some(ref path) = self.socket_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(windows)]
struct Win32NamedMutex {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl Win32NamedMutex {
    fn acquire(name: &str) -> Result<Self, IpcServerError> {
        use windows_sys::Win32::Foundation::{
            CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, INVALID_HANDLE_VALUE,
        };
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, wide.as_ptr()) };
        if handle == 0 || handle == INVALID_HANDLE_VALUE {
            return Err(IpcServerError::Io(io::Error::last_os_error()));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return Err(IpcServerError::AlreadyRunning);
        }
        Ok(Self { handle })
    }
}

#[cfg(windows)]
impl Drop for Win32NamedMutex {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
        }
    }
}

pub struct IpcServer;

impl IpcServer {
    pub async fn start_default(
        url_tx: mpsc::Sender<UrlUpdate>,
    ) -> Result<IpcServerHandle, IpcServerError> {
        let name = default_socket_name()?;
        let path = default_socket_path();
        Self::start(name, Some(path), url_tx).await
    }

    pub async fn start_with_path(
        path: &Path,
        url_tx: mpsc::Sender<UrlUpdate>,
    ) -> Result<IpcServerHandle, IpcServerError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let name = socket_name_from_path(path)?;
        Self::start(name, Some(path.to_path_buf()), url_tx).await
    }

    pub async fn start(
        name: Name<'static>,
        socket_path: Option<PathBuf>,
        url_tx: mpsc::Sender<UrlUpdate>,
    ) -> Result<IpcServerHandle, IpcServerError> {
        #[cfg(windows)]
        let mutex = Win32NamedMutex::acquire("Global\\ScreenTimeDaemonSingleInstance")
            .or_else(|_| Win32NamedMutex::acquire("Local\\ScreenTimeDaemonSingleInstance"))?;

        if Stream::connect(name.clone()).await.is_ok() {
            return Err(IpcServerError::AlreadyRunning);
        }

        let opts = ListenerOptions::new().name(name).try_overwrite(true);

        let listener = opts.create_tokio()?;
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        let join_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        debug!("IPC server received shutdown signal");
                        break;
                    }
                    accept_res = listener.accept() => {
                        match accept_res {
                            Ok(stream) => {
                                let tx = url_tx.clone();
                                tokio::spawn(async move {
                                    Self::handle_client(stream, tx).await;
                                });
                            }
                            Err(e) => {
                                warn!("IPC listener accept error: {e}");
                            }
                        }
                    }
                }
            }
            Ok(())
        });

        Ok(IpcServerHandle {
            shutdown_tx: Some(shutdown_tx),
            join_handle: Some(join_handle),
            #[cfg(unix)]
            socket_path,
            #[cfg(windows)]
            _mutex: Some(mutex),
        })
    }

    async fn handle_client(stream: Stream, tx: mpsc::Sender<UrlUpdate>) {
        let (reader, _) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Ok(update) = serde_json::from_str::<UrlUpdate>(line) {
                if tx.send(update).await.is_err() {
                    break;
                }
            } else if let Ok(msg) = serde_json::from_str::<BrowserIpcMessage>(line) {
                if let Some(update) = msg.into_url_update() {
                    if tx.send(update).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}
