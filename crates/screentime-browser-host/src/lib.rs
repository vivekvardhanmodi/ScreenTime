use interprocess::local_socket::tokio::Stream;
use interprocess::local_socket::traits::tokio::Stream as _;
use interprocess::local_socket::Name;
use screentime_core::ipc::{
    default_socket_name, read_length_prefixed, socket_name_from_path, write_length_prefixed,
    BrowserExtensionMessage, HostExtensionResponse, UrlUpdate,
};
use std::io::{self, Read, Write};
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tracing::{debug, warn};

pub fn resolve_socket_name() -> io::Result<Name<'static>> {
    if let Ok(path_str) = std::env::var("SCREENTIME_SOCKET") {
        if !path_str.is_empty() {
            return socket_name_from_path(Path::new(&path_str));
        }
    }
    default_socket_name()
}

pub async fn run_host<R, W>(
    mut reader: R,
    mut writer: W,
    socket_name: Name<'static>,
) -> (io::Result<()>, W)
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel::<UrlUpdate>(32);

    let socket_task = tokio::spawn(async move {
        let mut ipc_stream: Option<Stream> = None;

        while let Some(update) = rx.recv().await {
            let line = match serde_json::to_string(&update) {
                Ok(mut s) => {
                    s.push('\n');
                    s
                }
                Err(e) => {
                    warn!("Failed to serialize UrlUpdate: {e}");
                    continue;
                }
            };

            let mut sent = false;
            for _ in 0..2 {
                if ipc_stream.is_none() {
                    match Stream::connect(socket_name.clone()).await {
                        Ok(stream) => {
                            debug!("Connected to screentimed IPC socket");
                            ipc_stream = Some(stream);
                        }
                        Err(e) => {
                            warn!("Failed to connect to screentimed IPC socket: {e}");
                            break;
                        }
                    }
                }

                if let Some(ref mut stream) = ipc_stream {
                    if stream.write_all(line.as_bytes()).await.is_ok() {
                        sent = true;
                        break;
                    }
                    ipc_stream = None;
                }
            }

            if !sent {
                debug!("Dropped UrlUpdate because screentimed is unreachable");
            }
        }
    });

    let read_task = tokio::task::spawn_blocking(move || -> (io::Result<()>, W) {
        let result = (|| -> io::Result<()> {
            while let Some(msg) = read_length_prefixed::<_, BrowserExtensionMessage>(&mut reader)? {
                match msg {
                    BrowserExtensionMessage::UrlUpdate {
                        browser_type,
                        url,
                        title,
                    } => {
                        let update = UrlUpdate::new(browser_type, url, title);
                        let _ = tx.blocking_send(update);
                        write_length_prefixed(&mut writer, &HostExtensionResponse::Ok)?;
                    }
                    BrowserExtensionMessage::Ping => {
                        write_length_prefixed(&mut writer, &HostExtensionResponse::Pong)?;
                    }
                    BrowserExtensionMessage::Unknown => {
                        write_length_prefixed(
                            &mut writer,
                            &HostExtensionResponse::Error {
                                message: "Unknown message type".to_string(),
                            },
                        )?;
                    }
                }
            }
            debug!("Extension stdin reached EOF");
            Ok(())
        })();

        (result, writer)
    });

    let (res, writer) = read_task.await.expect("Browser host read task panicked");
    let _ = socket_task.await;
    (res, writer)
}

pub async fn run_host_with_path<R, W>(reader: R, writer: W, path: &Path) -> (io::Result<()>, W)
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    match socket_name_from_path(path) {
        Ok(name) => run_host(reader, writer, name).await,
        Err(e) => (Err(e), writer),
    }
}
