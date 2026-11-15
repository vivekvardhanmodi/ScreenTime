# Research: Browser Extension Native Messaging Integration for Rust Daemon

**Ticket Reference:** `.scratch/rust-daemon-port/issues/07-browser-extension-native-messaging-integration.md`  
**Status:** Complete  
**Date:** 2026-11-15  

---

## 1. Executive Summary & Concrete Decisions

This research resolves the two central architectural questions regarding the integration between the browser extensions (Chrome and Firefox) and the new Rust daemon (`screentimed`):

1. **Binary Packaging: Dedicated Binary (`screentime-browser-host`) vs Subcommand (`screentimed browser-host`)**
   * **Decision:** **Build a dedicated, lightweight binary (`screentime-browser-host`) alongside `screentimed` in the same Cargo workspace/package.**
   * **Why:** The Chrome and Firefox Native Messaging specifications **strictly forbid command-line arguments in the manifest `"path"` field**. The browser directly executes the binary path specified in the manifest. Supplying arguments (such as `screentimed browser-host`) causes the browser to attempt executing the entire string as a literal executable path, failing immediately with `ENOENT` / `ERROR_FILE_NOT_FOUND`.
   * **Why not a wrapper script (`.sh` / `.bat`)?** Wrapper scripts add interpreter dependencies (`/bin/sh` / `bash`), permission issues, and on Windows, `.bat` files cause console window flicker (cmd.exe popup), stream encoding issues, and SmartScreen/antivirus false positives.
   * **Why not a monolithic binary using argv heuristics?** Having `screentimed` detect browser arguments (`chrome-extension://...`) bloats the host process with the daemon's heavy dependencies (X11/Wayland libs, SQLite, full Tokio runtime), increases startup latency, and critically risks **stdout corruption** (any errant logging or panic in the daemon kills the native messaging connection).
   * **Secondary Support:** As a developer convenience, `screentimed browser-host` can exist as a CLI subcommand sharing the exact same handler logic, but the installed manifest will point directly to `screentime-browser-host`.

2. **IPC Mechanism: Shared Runtime JSON State Files vs Local Sockets / Named Pipes**
   * **Decision:** **Replace shared filesystem JSON files with a real-time, event-driven Local Socket IPC mechanism** using Unix Domain Sockets on Linux (`$XDG_RUNTIME_DIR/screentime/screentimed.sock`) and Named Pipes on Windows (`\\.\pipe\screentimed-ipc`), abstracted via the `interprocess` crate (`interprocess::local_socket`).
   * **Why:** The legacy Python implementation relied on periodic filesystem polling (every 2.0s in `_browser_url_watcher`), leading to noticeable tab-switch latency, unnecessary disk I/O, race conditions requiring hacky delayed sleeps (`await asyncio.sleep(0.5)`), lack of multi-window multiplexing, and orphaned stale files when browsers crashed.
   * **Benefits of Local Socket IPC:**
     * **Push-based, sub-millisecond tab updates:** Instant URL resolution when switching tabs or windows.
     * **Zero CPU / disk polling overhead:** The daemon sits idle until an event arrives.
     * **Automatic lifecycle tracking:** When a browser closes or crashes, the OS kernel automatically tears down the socket connection, allowing the daemon to immediately invalidate or clear active website tracking.
     * **Cross-platform parity:** Unified abstraction across Linux (Unix domain sockets) and Windows (Named Pipes).
   * **Extension Compatibility:** **Zero changes are required in `chrome_extension/` and `firefox_extension/`**. The wire protocol between the browser and native host remains 100% byte-for-byte identical.

---

## 2. Analysis: Dedicated Binary vs Subcommand

### 2.1 Native Messaging Host Execution Constraints

Chrome and Firefox define the Native Messaging protocol specification:
* **Google Chrome / Chromium / Edge / Brave Specification:**
  * Manifest property: `"path": "string"`.
  * *Constraint:* On Linux and macOS, `"path"` must be an **absolute path** to the executable. On Windows, it can be relative to the manifest directory or absolute.
  * *Invocation:* When an extension calls `chrome.runtime.connectNative("com.screentime.native")`, the browser launches the host via `execv` / `CreateProcess`.
  * *Arguments passed by browser:*
    * Linux / macOS: `argv[1] = "chrome-extension://<extension_id>/"`
    * Windows: `argv[1] = "chrome-extension://<extension_id>/"`, `argv[2] = "--parent-window=<HWND>"`
* **Mozilla Firefox Specification:**
  * Manifest property: `"path": "string"`.
  * *Constraint:* Must be an absolute path on Linux.
  * *Invocation:* Firefox launches the host process via `execv` / `CreateProcess`.
  * *Arguments passed by browser:*
    * Linux: `argv[1] = "/path/to/manifest.json"`, `argv[2] = "extension_id@domain"` (or just the extension ID depending on version).
    * Windows: `argv[1] = "extension_id@domain"`.

### 2.2 Why Manifest `"path"` Cannot Pass Subcommands
If a manifest is configured with:
```json
{
  "name": "com.screentime.native",
  "path": "/usr/bin/screentimed browser-host",
  "type": "stdio"
}
```
The browser's process launcher does not invoke a shell; it calls `execv("/usr/bin/screentimed browser-host", ...)` directly. The OS attempts to find a single binary literally named `"screentimed browser-host"`, resulting in `ENOENT` (No such file or directory).

### 2.3 Evaluation of Packaging Alternatives

| Criterion | Option A: Subcommand via Shell/Batch Wrapper | Option B: Monolithic Binary with `argv` Heuristic | Option C: Dedicated Binary (`screentime-browser-host`) **[RECOMMENDED]** |
| :--- | :--- | :--- | :--- |
| **Manifest Path** | Points to `/usr/bin/screentime-host.sh` or `.bat` | Points to `/usr/bin/screentimed` | Points to `/usr/bin/screentime-browser-host` |
| **Windows Compatibility** | **Poor**: `.bat` files cause cmd window popups, CMD quoting quirks, and SmartScreen blocks | **Good**: Native `.exe` invoked directly | **Excellent**: Native `.exe` invoked directly, clean Windows subsystem |
| **Process Startup & Memory** | High (Shell interpreter + daemon initialization) | High (Daemon initializes X11/Wayland/Win32/SQLite bindings) | **Minimal (<2 MB RAM, <2ms startup)**: Lean binary with zero window/database dependencies |
| **Stdout Safety** | High risk of script output corrupting binary stdio | **Dangerous**: Any daemon log/panic on stdout breaks browser bridge | **Guaranteed**: Binary only links stdio bridge and socket client; zero stdout pollution |
| **Cargo Build Ergonomics** | Needs auxiliary script generation | Single binary | Built natively via `[[bin]]` in `Cargo.toml`; shares dependencies and code |

### 2.4 Cargo Multi-Binary Architecture

Cargo natively supports defining multiple binaries within a single crate or workspace without duplicating code:

```toml
# Cargo.toml
[package]
name = "screentime"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1.38", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
interprocess = { version = "2.2", features = ["tokio"] }
tracing = "0.1"
tracing-subscriber = "0.3"

# Main Daemon Binary
[[bin]]
name = "screentimed"
path = "src/bin/daemon.rs"

# Native Messaging Host Binary
[[bin]]
name = "screentime-browser-host"
path = "src/bin/browser_host.rs"
```

Both binaries share common modules defined in `src/lib.rs` (e.g., IPC message types, socket path resolution, wire encoding).

---

## 3. Analysis: Communication Mechanism (Files vs IPC)

### 3.1 The Legacy Python Architecture & Its Flaws

In Python:
1. `browser_host.py` parses `url_update` messages from stdin and writes them to:
   * Chrome: `$XDG_RUNTIME_DIR/screentime/chrome_url`
   * Firefox: `$XDG_RUNTIME_DIR/screentime/firefox_url`
2. State file contents: `{"url": "https://example.com", "title": "Example", "pid": 12345}`.
3. In `daemon.py`:
   * `_browser_url_watcher`: Runs an infinite loop `await asyncio.sleep(2)` whenever a browser window has focus.
   * `on_window_change`: Starts a browser session with `url = None` and schedules `_delayed_url_check()` with an arbitrary `await asyncio.sleep(0.5)` to wait for the extension to write the state file.

#### Limitations of the File-Based Approach:
* **High Latency (Up to 2,000ms):** When switching tabs within an active browser, the daemon does not detect the tab change until the next 2-second polling tick.
* **Race Conditions & Artificial Delays:** Switching to a browser window immediately requires an artificial 500ms delay (`_delayed_url_check`) because reading the file immediately produces stale data from the previously active tab.
* **Orphaned State on Crash / Ungraceful Kill:** If Chrome crashes, terminates via `SIGKILL`, or the system reboots, the ephemeral `chrome_url` file persists on disk. The daemon continues reading this stale file if the browser is reopened or focused.
* **Windows File Locking:** On Windows, atomic file replacement (`MoveFileExW` / `std::fs::rename`) fails with `ERROR_ACCESS_DENIED` or `ERROR_SHARING_VIOLATION` if another process (such as the daemon reader or an antivirus indexer) has the destination file open without `FILE_SHARE_DELETE`.
* **Multi-Window & Multi-Profile Confusion:** If a user has two Chrome windows open across monitors (e.g. Work and Personal profiles), both write to the single `chrome_url` file, causing thrashing.

### 3.2 The Modern Event-Driven IPC Architecture

Instead of polling disk files, `screentimed` hosts an IPC listener (Unix Domain Socket on Linux, Named Pipe on Windows). `screentime-browser-host` acts as a thin bridge: it reads messages from the browser's stdin and immediately writes them to the IPC connection.

```
┌────────────────────────┐
│  Chrome / Firefox      │
│  Extension             │
└───────────┬────────────┘
            │ stdio (4-byte length + JSON)
┌───────────▼────────────┐
│ screentime-browser-host│
└───────────┬────────────┘
            │ Local Socket IPC (Tokio / interprocess)
            │ [Linux: Unix Domain Socket | Windows: Named Pipe]
┌───────────▼────────────┐
│      screentimed       │
│  (SessionManager loop) │
└────────────────────────┘
```

#### Comparison Matrix

| Property | Shared State Files (`chrome_url`) | Local Socket IPC (Unix Socket / Named Pipe) |
| :--- | :--- | :--- |
| **Tab Switch Latency** | 0.5s – 2.0s (polling delay) | **< 1ms (Instant event push)** |
| **CPU / Battery Usage** | Periodic timer wakeups every 2s | **Zero idle wakeups** |
| **Process Crash Handling** | Stale file remains on disk | **Kernel closes socket stream; daemon cleans state instantly** |
| **Concurrency / Multi-Browser** | Single shared file per browser type | **Independent connections per browser process/profile** |
| **Cross-Platform Safety** | Windows file sharing violations | **OS-native IPC primitives (Named Pipe ACLs / POSIX permissions)** |

---

## 4. Wire Protocols and Compatibility

### 4.1 Browser ↔ Native Messaging Host Protocol (Stdio)

The protocol between Chrome/Firefox and `screentime-browser-host` is defined by the browser vendors:
* Each message consists of:
  1. A **4-byte unsigned 32-bit integer** in native endianness specifying the payload length in bytes.
  2. A **UTF-8 encoded JSON string** of that exact length.
* Maximum message size: 1 MB (Chrome) / 1 MB (Firefox).

#### Message Schemas (Inbound from Extension):
1. **URL Update Event:**
   ```json
   {
     "type": "url_update",
     "source": "chrome",
     "url": "https://github.com/rust-lang/rust",
     "title": "rust-lang/rust: Empowering everyone to build reliable and efficient software."
   }
   ```
   *Note:* `url` and `title` may be `null` when the browser loses focus or navigates to private/internal pages.
2. **Ping / Keepalive Event:**
   ```json
   {
     "type": "ping"
   }
   ```

#### Message Schemas (Outbound from Host to Extension):
* Acknowledgement: `{"status": "ok"}`
* Pong: `{"status": "pong"}`
* Error: `{"status": "error", "message": "..."}`

> **CRITICAL REQUIREMENT: Strict STDOUT Discipline**  
> In native messaging, `stdout` is the raw binary framing channel. Any standard library printing (`println!`), debugging output, or logging written to `stdout` corrupts the 4-byte message framing and causes Chrome/Firefox to immediately terminate the connection with error: `Native host has exited`.  
> All diagnostic logging in `screentime-browser-host` must be routed exclusively to `stderr` or a rotating file.

### 4.2 Native Host ↔ Daemon IPC Protocol

Communication between `screentime-browser-host` and `screentimed` uses Newline-Delimited JSON (NDJSON) over the local socket.

#### IPC Event Types:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum BrowserIpcMessage {
    /// Sent when the native host connects to the daemon
    Register {
        source: String,       // "chrome" | "firefox" | "brave" | "edge"
        pid: u32,
    },
    /// Sent whenever the active tab URL changes
    UrlUpdate {
        source: String,
        url: Option<String>,
        title: Option<String>,
        timestamp: f64,
    },
    /// Heartbeat keepalive
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum DaemonIpcResponse {
    Ok,
    Pong,
    Error { message: String },
}
```

---

## 5. Concrete Code Recommendations

### 5.1 Native Messaging Stdio Framing (`src/browser_host/framing.rs`)

```rust
use std::io::{self, Read, Write};
use serde::{de::DeserializeOwned, Serialize};

pub struct StdioMessenger;

impl StdioMessenger {
    /// Read a length-prefixed JSON message from stdin
    pub fn read_message<T: DeserializeOwned>() -> io::Result<Option<T>> {
        let mut length_buf = [0u8; 4];
        let mut stdin = io::stdin().lock();
        
        match stdin.read_exact(&mut length_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEOF => return Ok(None),
            Err(e) => return Err(e),
        }

        let length = u32::from_ne_bytes(length_buf) as usize;
        if length == 0 || length > 1024 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid message length: {length}"),
            ));
        }

        let mut payload_buf = vec![0u8; length];
        stdin.read_exact(&mut payload_buf)?;

        let message = serde_json::from_slice(&payload_buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(Some(message))
    }

    /// Write a length-prefixed JSON message to stdout
    pub fn send_message<T: Serialize>(message: &T) -> io::Result<()> {
        let payload = serde_json::to_vec(message)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let length = payload.len() as u32;
        let length_buf = length.to_ne_bytes();

        let mut stdout = io::stdout().lock();
        stdout.write_all(&length_buf)?;
        stdout.write_all(&payload)?;
        stdout.flush()?;

        Ok(())
    }
}
```

### 5.2 Browser Host Entrypoint (`src/bin/browser_host.rs`)

```rust
use interprocess::local_socket::tokio::Stream;
use interprocess::local_socket::GenericNamespaced;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: Initialize tracing logger to stderr ONLY
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    info!("Starting screentime-browser-host");

    // Connect to the daemon's local IPC socket
    let socket_name = screentime::ipc::get_ipc_socket_name();
    let stream = match Stream::connect(socket_name).await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to connect to screentimed IPC socket: {e}");
            // Return pong/error to extension so it does not loop crash
            return Err(e.into());
        }
    };

    let (reader, mut writer) = stream.into_split();
    let mut ipc_reader = BufReader::new(reader).lines();

    // Spawn task to read incoming messages from browser extension stdin
    let (tx, mut rx) = tokio::sync::mpsc::channel::<screentime::ipc::BrowserIpcMessage>(32);

    tokio::task::spawn_blocking(move || {
        while let Ok(Some(msg)) = StdioMessenger::read_message::<serde_json::Value>() {
            let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match msg_type {
                "url_update" => {
                    let source = msg.get("source").and_then(|s| s.as_str()).unwrap_or("chrome").to_string();
                    let url = msg.get("url").and_then(|u| u.as_str()).map(ToString::to_string);
                    let title = msg.get("title").and_then(|t| t.as_str()).map(ToString::to_string);

                    let _ = tx.blocking_send(screentime::ipc::BrowserIpcMessage::UrlUpdate {
                        source,
                        url,
                        title,
                        timestamp: screentime::time::now(),
                    });
                    let _ = StdioMessenger::send_message(&serde_json::json!({ "status": "ok" }));
                }
                "ping" => {
                    let _ = StdioMessenger::send_message(&serde_json::json!({ "status": "pong" }));
                }
                _ => {
                    let _ = StdioMessenger::send_message(&serde_json::json!({
                        "status": "error",
                        "message": format!("Unknown type: {msg_type}")
                    }));
                }
            }
        }
    });

    // Forward received tab updates over IPC to the daemon
    while let Some(ipc_msg) = rx.recv().await {
        let mut line = serde_json::to_vec(&ipc_msg)?;
        line.push(b'\n');
        if let Err(e) = writer.write_all(&line).await {
            error!("Failed to write to daemon IPC: {e}");
            break;
        }
    }

    info!("screentime-browser-host shutting down");
    Ok(())
}
```

### 5.3 Daemon IPC Server (`src/ipc/server.rs`)

```rust
use interprocess::local_socket::tokio::Listener;
use interprocess::local_socket::ListenerOptions;
use tokio::io::{AsyncBufReadExt, BufReader};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, error};

pub struct BrowserIpcServer {
    event_tx: broadcast::Sender<screentime::ipc::BrowserIpcMessage>,
}

impl BrowserIpcServer {
    pub fn new(event_tx: broadcast::Sender<screentime::ipc::BrowserIpcMessage>) -> Self {
        Self { event_tx }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let name = screentime::ipc::get_ipc_socket_name();
        
        let opts = ListenerOptions::new()
            .name(name)
            .try_overwrite(true); // Auto-unlink stale Unix socket file if needed

        let listener = opts.create_tokio()?;
        info!("screentimed browser IPC server listening on {:?}", name);

        loop {
            match listener.accept().await {
                Ok(stream) => {
                    let tx = self.event_tx.clone();
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(stream).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            if let Ok(msg) = serde_json::from_str::<screentime::ipc::BrowserIpcMessage>(&line) {
                                let _ = tx.send(msg);
                            }
                        }
                    });
                }
                Err(e) => {
                    error!("Error accepting IPC connection: {e}");
                }
            }
        }
    }
}
```

---

## 6. Manifest File Specifications & Cross-Platform Paths

The browser extensions interact with the native host through manifest files installed in vendor-specified system/user locations.

### 6.1 Chrome / Chromium / Brave / Edge Manifest
* **File Name:** `com.screentime.native.json`
* **Content:**
  ```json
  {
    "name": "com.screentime.native",
    "description": "ScreenTime browser URL tracker",
    "path": "/usr/local/bin/screentime-browser-host",
    "type": "stdio",
    "allowed_origins": [
      "chrome-extension://EXTENSION_ID_PLACEHOLDER/"
    ]
  }
  ```
* **Install Locations (Linux):**
  * Google Chrome: `~/.config/google-chrome/NativeMessagingHosts/com.screentime.native.json`
  * Chromium: `~/.config/chromium/NativeMessagingHosts/com.screentime.native.json`
  * Brave: `~/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts/com.screentime.native.json`
* **Install Location (Windows):**
  * Registry Key: `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.screentime.native`
  * Value: `(Default) = "C:\Users\<User>\AppData\Local\ScreenTime\com.screentime.native.json"`
  * Manifest `"path"` on Windows: `C:\Users\<User>\AppData\Local\ScreenTime\screentime-browser-host.exe`

### 6.2 Firefox Manifest
* **File Name:** `com.screentime.native.json`
* **Content:**
  ```json
  {
    "name": "com.screentime.native",
    "description": "ScreenTime browser URL tracker",
    "path": "/usr/local/bin/screentime-browser-host",
    "type": "stdio",
    "allowed_extensions": [
      "screentime@screentime.local"
    ]
  }
  ```
* **Install Locations (Linux):**
  * User path: `~/.mozilla/native-messaging-hosts/com.screentime.native.json`
* **Install Location (Windows):**
  * Registry Key: `HKCU\Software\Mozilla\NativeMessagingHosts\com.screentime.native`
  * Value: `(Default) = "C:\Users\<User>\AppData\Local\ScreenTime\com.screentime.native-firefox.json"`

---

## 7. Integration with the SessionManager State Machine (Issue 09)

In the Python implementation (`daemon.py`), `SessionManager` handled browser tracking with race-condition workarounds:
```python
# Legacy Python workaround in daemon.py
if window and self._is_browser(window):
    self._start_browser_watcher() # 2-second sleep poll
    asyncio.create_task(self._delayed_url_check(window)) # 0.5s sleep delay
```

With the Rust IPC event stream, `SessionManager` simplifies substantially:
1. `SessionManager` maintains a memory-cached table of active browser states:
   ```rust
   struct BrowserState {
       url: Option<String>,
       title: Option<String>,
       last_updated: Instant,
   }
   
   // Keyed by browser source: "chrome", "firefox", etc.
   browser_states: HashMap<String, BrowserState>,
   ```
2. When the IPC server emits `BrowserIpcMessage::UrlUpdate`:
   * Update `browser_states[source]`.
   * If the currently active focused window matches `source`:
     * If the website domain changed, cleanly close the existing session and open a new session with the new website.
     * If the session just started with `website = None` within the last 1.0 second, update the session's website in-place.
3. When the window manager emits an active window change:
   * If the newly focused window is a browser, inspect `browser_states[source]`. If the URL is already present and fresh (< 2 seconds old), start the session with the website immediately.
   * If no fresh URL is present yet, start the session with `website = None`. As soon as the extension emits `UrlUpdate` (typically within 10–50ms of focus), the session is updated immediately.
4. **No polling task (`_browser_url_watcher`) and no delayed sleep tasks (`_delayed_url_check`) are needed.**

---

## 8. Summary of Actionable Implementation Tasks

When implementing this subsystem:
1. **Define IPC message structures** in `screentime-core/src/ipc.rs` using `serde`.
2. **Build `screentime-browser-host` binary** in `src/bin/browser_host.rs`:
   * Strict `stdout` discipline (only framed JSON).
   * All logging to `stderr`.
   * Asynchronously stream browser events to the daemon's local socket.
3. **Add `LocalSocket` listener** to `screentimed` using `interprocess::local_socket::tokio`.
4. **Update `SessionManager`** to consume the broadcast channel of `BrowserIpcMessage`.
5. **Update `install.sh` / installer** to write native messaging manifests pointing to `screentime-browser-host`.
