use interprocess::local_socket::tokio::Stream;
use interprocess::local_socket::traits::tokio::{Listener as _, Stream as _};
use interprocess::local_socket::{ListenerOptions, Name};
use screentime_core::ipc::{
    default_socket_name, default_socket_path, socket_name_from_path, BrowserIpcMessage,
    DaemonStatusResponse, UrlUpdate,
};
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

#[derive(Debug, Error)]
pub enum IpcServerError {
    #[error("Another instance of screentimed is already running")]
    AlreadyRunning,
    #[error("IPC I/O error: {0}")]
    Io(#[from] io::Error),
}

#[cfg(unix)]
struct UnixFlockGuard {
    _file: std::fs::File,
}

#[cfg(unix)]
impl UnixFlockGuard {
    fn acquire(socket_path: &Path) -> Result<Self, IpcServerError> {
        use std::os::unix::io::AsRawFd;

        let lock_path = PathBuf::from(format!("{}.lock", socket_path.display()));
        if let Some(parent) = lock_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;

        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock
                || err.raw_os_error() == Some(libc::EWOULDBLOCK)
                || err.raw_os_error() == Some(libc::EAGAIN)
            {
                return Err(IpcServerError::AlreadyRunning);
            }
            return Err(IpcServerError::Io(err));
        }

        Ok(Self { _file: file })
    }
}

pub struct IpcServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    join_handle: Option<tokio::task::JoinHandle<Result<(), IpcServerError>>>,
    #[cfg(unix)]
    socket_path: Option<PathBuf>,
    #[cfg(unix)]
    _lock: Option<UnixFlockGuard>,
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
        {
            self._lock.take();
            if let Some(ref path) = self.socket_path {
                let _ = std::fs::remove_file(path);
                let lock_path = PathBuf::from(format!("{}.lock", path.display()));
                let _ = std::fs::remove_file(lock_path);
            }
        }
        #[cfg(windows)]
        {
            self._mutex.take();
        }
    }
}

impl Drop for IpcServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        #[cfg(unix)]
        {
            self._lock.take();
            if let Some(ref path) = self.socket_path {
                let _ = std::fs::remove_file(path);
                let lock_path = PathBuf::from(format!("{}.lock", path.display()));
                let _ = std::fs::remove_file(lock_path);
            }
        }
        #[cfg(windows)]
        {
            self._mutex.take();
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
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
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
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        if !self.handle.is_null() && self.handle != INVALID_HANDLE_VALUE {
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
        #[allow(unused_variables)] socket_path: Option<PathBuf>,
        url_tx: mpsc::Sender<UrlUpdate>,
    ) -> Result<IpcServerHandle, IpcServerError> {
        #[cfg(unix)]
        let lock = match socket_path.as_deref() {
            Some(path) => Some(UnixFlockGuard::acquire(path)?),
            None => None,
        };

        #[cfg(windows)]
        let mutex = {
            let suffix = socket_path
                .as_deref()
                .map(screentime_core::ipc::windows_pipe_name_for_path)
                .unwrap_or_else(|| "ScreenTimeDaemonSingleInstance".to_string());
            let global_name = format!("Global\\ScreenTime-{suffix}");
            let local_name = format!("Local\\ScreenTime-{suffix}");
            match Win32NamedMutex::acquire(&global_name) {
                Ok(m) => m,
                Err(IpcServerError::AlreadyRunning) => return Err(IpcServerError::AlreadyRunning),
                Err(IpcServerError::Io(_)) => Win32NamedMutex::acquire(&local_name)?,
            }
        };

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
            #[cfg(unix)]
            _lock: lock,
            #[cfg(windows)]
            _mutex: Some(mutex),
        })
    }

    async fn handle_client(stream: Stream, tx: mpsc::Sender<UrlUpdate>) {
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };

            let is_status = value.get("event").and_then(|v| v.as_str()) == Some("status")
                || value.get("type").and_then(|v| v.as_str()) == Some("status");

            if is_status {
                let resp = DaemonStatusResponse {
                    status: "running".to_string(),
                    pid: std::process::id(),
                };
                if let Ok(mut encoded) = serde_json::to_string(&resp) {
                    encoded.push('\n');
                    let _ = writer.write_all(encoded.as_bytes()).await;
                    let _ = writer.flush().await;
                }
                continue;
            }

            if value.get("event").is_some() {
                if let Ok(msg) = serde_json::from_value::<BrowserIpcMessage>(value) {
                    if let Some(update) = msg.into_url_update() {
                        if tx.send(update).await.is_err() {
                            break;
                        }
                    }
                }
            } else if value.get("url").is_some()
                || value.get("browser_type").is_some()
                || value.get("source").is_some()
            {
                if let Ok(update) = serde_json::from_value::<UrlUpdate>(value) {
                    if tx.send(update).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}
