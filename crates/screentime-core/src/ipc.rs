use interprocess::local_socket::Name;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;
pub const DEFAULT_SOCKET_NAME: &str = "screentimed.sock";
pub const DEFAULT_WINDOWS_PIPE_NAME: &str = "screentimed-ipc";

fn default_browser_type() -> String {
    "browser".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrlUpdate {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default = "default_browser_type", alias = "source")]
    pub browser_type: String,
}

impl UrlUpdate {
    pub fn new(
        browser_type: impl Into<String>,
        url: Option<String>,
        title: Option<String>,
    ) -> Self {
        Self {
            browser_type: browser_type.into(),
            url,
            title,
        }
    }
}

impl From<String> for UrlUpdate {
    fn from(url: String) -> Self {
        Self {
            url: Some(url),
            title: None,
            browser_type: default_browser_type(),
        }
    }
}

impl From<&str> for UrlUpdate {
    fn from(url: &str) -> Self {
        Self {
            url: Some(url.to_string()),
            title: None,
            browser_type: default_browser_type(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserExtensionMessage {
    UrlUpdate {
        #[serde(default = "default_browser_type", alias = "source")]
        browser_type: String,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        title: Option<String>,
    },
    Ping,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HostExtensionResponse {
    Ok,
    Pong,
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum BrowserIpcMessage {
    #[serde(rename = "url_update")]
    UrlUpdate {
        #[serde(default = "default_browser_type", alias = "source")]
        browser_type: String,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        timestamp: Option<f64>,
    },
    #[serde(rename = "register")]
    Register {
        #[serde(default = "default_browser_type", alias = "browser_type")]
        source: String,
        pid: u32,
    },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "status")]
    Status,
}

impl BrowserIpcMessage {
    pub fn into_url_update(self) -> Option<UrlUpdate> {
        match self {
            Self::UrlUpdate {
                browser_type,
                url,
                title,
                ..
            } => Some(UrlUpdate {
                browser_type,
                url,
                title,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatusResponse {
    pub status: String,
    pub pid: u32,
}

pub fn read_length_prefixed<R: Read, T: serde::de::DeserializeOwned>(
    mut reader: R,
) -> io::Result<Option<T>> {
    let mut len_buf = [0u8; 4];
    let mut n = 0;
    while n < 4 {
        match reader.read(&mut len_buf[n..]) {
            Ok(0) => {
                if n == 0 {
                    return Ok(None);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Incomplete 4-byte message length prefix",
                ));
            }
            Ok(read_bytes) => n += read_bytes,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }

    let len = u32::from_ne_bytes(len_buf) as usize;
    if len == 0 || len > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid message length: {len}"),
        ));
    }

    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;

    let msg = serde_json::from_slice(&payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(Some(msg))
}

pub fn write_length_prefixed<W: Write, T: Serialize>(mut writer: W, msg: &T) -> io::Result<()> {
    let payload =
        serde_json::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let len = payload.len();
    if len > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Message size {len} exceeds max {MAX_MESSAGE_SIZE}"),
        ));
    }

    let len_bytes = (len as u32).to_ne_bytes();
    writer.write_all(&len_bytes)?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

pub fn default_runtime_dir() -> PathBuf {
    #[cfg(unix)]
    {
        if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
            if !xdg.is_empty() {
                return PathBuf::from(xdg).join("screentime");
            }
        }
        let uid = unsafe { libc::getuid() };
        PathBuf::from(format!("/tmp/screentime-{}", uid))
    }
    #[cfg(not(unix))]
    {
        std::env::temp_dir().join("screentime")
    }
}

pub fn default_socket_path() -> PathBuf {
    default_runtime_dir().join(DEFAULT_SOCKET_NAME)
}

/// Derives a deterministic Windows named pipe identifier from a user-supplied socket/pipe path.
pub fn windows_pipe_name_for_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return DEFAULT_WINDOWS_PIPE_NAME.to_string();
    }

    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix(r"\\.\pipe\") {
        if !rest.is_empty() {
            return trimmed[9..].to_string();
        }
        return DEFAULT_WINDOWS_PIPE_NAME.to_string();
    }

    if !trimmed.contains('\\') && !trimmed.contains('/') && !trimmed.contains(':') {
        return trimmed.to_string();
    }

    let default_path = default_socket_path();
    if path == default_path {
        return DEFAULT_WINDOWS_PIPE_NAME.to_string();
    }

    // Stable 64-bit FNV-1a hash so distinct temp/custom paths map to distinct named pipes
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in trimmed.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{DEFAULT_WINDOWS_PIPE_NAME}-{hash:016x}")
}

pub fn socket_name_from_path(path: &Path) -> io::Result<Name<'static>> {
    #[cfg(unix)]
    {
        use interprocess::local_socket::ToFsName;
        path.to_fs_name::<interprocess::local_socket::GenericFilePath>()
            .map(|n| n.into_owned())
    }
    #[cfg(windows)]
    {
        use interprocess::local_socket::ToNsName;
        let pipe_name = windows_pipe_name_for_path(path);
        pipe_name
            .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
            .map(|n| n.into_owned())
    }
}

pub fn default_socket_name() -> io::Result<Name<'static>> {
    #[cfg(unix)]
    {
        let path = default_socket_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        socket_name_from_path(&path)
    }
    #[cfg(windows)]
    {
        use interprocess::local_socket::ToNsName;
        DEFAULT_WINDOWS_PIPE_NAME.to_ns_name::<interprocess::local_socket::GenericNamespaced>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_framing_round_trip() {
        let update = BrowserExtensionMessage::UrlUpdate {
            browser_type: "chrome".to_string(),
            url: Some("https://example.com".to_string()),
            title: Some("Example Domain".to_string()),
        };

        let mut buffer = Vec::new();
        write_length_prefixed(&mut buffer, &update).expect("Write failed");

        let read_msg: Option<BrowserExtensionMessage> =
            read_length_prefixed(Cursor::new(&buffer)).expect("Read failed");
        assert_eq!(read_msg, Some(update));
    }

    #[test]
    fn test_framing_clean_eof() {
        let empty_cursor = Cursor::new(Vec::new());
        let result: io::Result<Option<BrowserExtensionMessage>> =
            read_length_prefixed(empty_cursor);
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn test_framing_truncated_length() {
        let truncated = vec![1, 2];
        let result: io::Result<Option<BrowserExtensionMessage>> =
            read_length_prefixed(Cursor::new(truncated));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn test_framing_truncated_payload() {
        let mut buffer = Vec::new();
        let len: u32 = 50;
        buffer.extend_from_slice(&len.to_ne_bytes());
        buffer.extend_from_slice(br#"{"too": "short"}"#);

        let result: io::Result<Option<BrowserExtensionMessage>> =
            read_length_prefixed(Cursor::new(buffer));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn test_framing_size_limit_rejection() {
        let mut buffer = Vec::new();
        let len: u32 = (MAX_MESSAGE_SIZE + 10) as u32;
        buffer.extend_from_slice(&len.to_ne_bytes());

        let result: io::Result<Option<BrowserExtensionMessage>> =
            read_length_prefixed(Cursor::new(buffer));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_url_update_deserialization_compatibility() {
        let raw_from_chrome = r#"{"type":"url_update","source":"chrome","url":"https://github.com","title":"GitHub"}"#;
        let parsed: UrlUpdate = serde_json::from_str(raw_from_chrome).expect("Chrome JSON parse");
        assert_eq!(parsed.browser_type, "chrome");
        assert_eq!(parsed.url.as_deref(), Some("https://github.com"));
        assert_eq!(parsed.title.as_deref(), Some("GitHub"));

        let raw_direct =
            r#"{"url":"https://rust-lang.org","title":"Rust","browser_type":"firefox"}"#;
        let parsed_direct: UrlUpdate = serde_json::from_str(raw_direct).expect("Direct JSON parse");
        assert_eq!(parsed_direct.browser_type, "firefox");
        assert_eq!(parsed_direct.url.as_deref(), Some("https://rust-lang.org"));

        let raw_nulls = r#"{"type":"url_update","source":"chrome","url":null,"title":null}"#;
        let parsed_nulls: UrlUpdate = serde_json::from_str(raw_nulls).expect("Nulls JSON parse");
        assert_eq!(parsed_nulls.url, None);
        assert_eq!(parsed_nulls.title, None);
    }

    #[test]
    fn test_browser_ipc_message_into_url_update() {
        let msg = BrowserIpcMessage::UrlUpdate {
            browser_type: "brave".to_string(),
            url: Some("https://brave.com".to_string()),
            title: Some("Brave".to_string()),
            timestamp: Some(12345.0),
        };
        let update = msg.into_url_update().expect("Expected UrlUpdate");
        assert_eq!(update.browser_type, "brave");
        assert_eq!(update.url.as_deref(), Some("https://brave.com"));
    }

    #[test]
    fn test_windows_pipe_name_for_path() {
        assert_eq!(
            windows_pipe_name_for_path(Path::new(r"\\.\pipe\custom-pipe")),
            "custom-pipe"
        );
        assert_eq!(
            windows_pipe_name_for_path(Path::new("my-pipe-name")),
            "my-pipe-name"
        );
        let p1 = windows_pipe_name_for_path(Path::new(r"C:\Temp\a\test.sock"));
        let p2 = windows_pipe_name_for_path(Path::new(r"C:\Temp\b\test.sock"));
        assert_ne!(p1, p2);
        assert_eq!(
            p1,
            windows_pipe_name_for_path(Path::new(r"C:\Temp\a\test.sock"))
        );
    }
}
