# Linux KDE Plasma Wayland Window Tracking Research

**Ticket**: `05-linux-kde-plasma-window-tracking`  
**Date**: 2026-11-15  
**Status**: Completed  
**Target Environment**: KDE Plasma 5.27+ and KDE Plasma 6.x under Wayland  

---

## 1. Executive Summary & Recommended Architecture

Wayland compositors intentionally enforce client isolation: regular clients cannot query or monitor other applications' window titles, process IDs, or focus states through standard Wayland core protocols. On KDE Plasma Wayland, tracking active window changes (`app_class` and `title`) requires choosing among Wayland protocols, KWin native D-Bus interfaces, and KWin scripting.

### Key Findings
1. **Wayland Protocols are Inadequate or Blocked**:
   - `zwlr_foreign_toplevel_manager_v1` (the wlroots standard) is **not supported** by KWin.
   - `ext-foreign-toplevel-list-v1` (upstream standard staging protocol) is implemented in Plasma 6, but is strictly **read-only and omits active/focused window state**.
   - `org_kde_plasma_window_management` (KDE private Wayland protocol) provides active window state, but enforces a **strict singleton client restriction** (`Only one client can bind this interface at a time`). In a standard desktop session, `plasmashell` holds this binding exclusively; any secondary bind attempt causes a fatal protocol error.
2. **KWin D-Bus Root Interface Lacks Window Queries**:
   - `org.kde.KWin` has no direct `activeWindow` property or signal.
   - `queryWindowInfo` requires interactive user clicking (crosshair cursor).
   - `getWindowInfo` requires a `QUuid` internal handle which cannot be discovered without a script.
3. **The Proven Solution: Dynamic KWin Scripting with Bi-directional D-Bus**:
   - The optimal, robust approach used by modern tools like `2e3s/awatcher` and `kdotool` is **dynamically loading a transient JavaScript helper script into KWin via `org.kde.kwin.Scripting` over the session D-Bus**.
   - The script connects to KWin's internal `workspace.windowActivated` (or Plasma 5 `clientActivated`) and `window.captionChanged` signals.
   - When focus or title changes, the script invokes a D-Bus method (`WindowChanged`) on the Rust daemon via KWin's built-in `callDBus(...)` API.
   - **Zero root privileges** required (runs strictly on the user's `$DBUS_SESSION_BUS_ADDRESS`).
   - **Zero polling / zero journal log scraping** (pure event-driven push, <1 ms latency, negligible CPU).
   - **Zero persistent disk/configuration pollution** (transient script loaded on startup and cleanly unloaded on daemon shutdown).

---

## 2. Evaluation of Wayland Protocols on KDE Plasma

### 2.1 `zwlr_foreign_toplevel_manager_v1`
- **Specification**: Wayland protocol extension created by wlroots (`wlr-protocols`) providing window listing, title/app_id updates, and active state tracking.
- **KWin Support**: **None**. KDE developers explicitly chose not to implement `wlr-foreign-toplevel-management`.
- **Verdict**: Inapplicable on KDE Plasma Wayland. Any client attempting to bind to this global interface will fail with a protocol lookup error.

### 2.2 `ext-foreign-toplevel-list-v1`
- **Specification**: Upstream standardized staging protocol (`wayland-protocols`) introduced to provide a cross-compositor way to inspect toplevel surfaces.
- **KWin Support**: Supported in modern KWin (Plasma 6.0+ and recent Plasma 5 builds).
- **Interface Structure**:
  - Global `ext_foreign_toplevel_list_v1` emits `toplevel(id: ext_foreign_toplevel_handle_v1)`.
  - Handle `ext_foreign_toplevel_handle_v1` emits `app_id(string)`, `title(string)`, `identifier(string)`, `closed()`, and `done()`.
- **Fatal Limitation**: `ext-foreign-toplevel-list-v1` is deliberately designed as a minimalistic, observation-only protocol for window pickers. **It does not emit an active or focused window state**. There is no event indicating which window currently has keyboard focus.
- **Verdict**: Insufficient for active window tracking.

### 2.3 `org_kde_plasma_window_management`
- **Specification**: KDE's custom Wayland protocol (`plasma-wayland-protocols/src/protocols/plasma-window-management.xml`).
- **Features**: Full window management inspection including `active` boolean state flag, title, app_id, desktop, and geometry.
- **Fatal Limitation**: The XML specification explicitly mandates:
  > *"Only one client can bind this interface at a time."*
  
  In a standard KDE Plasma desktop session, `plasmashell` (the task manager / panel) binds this interface on login. If a daemon attempts to bind `org_kde_plasma_window_management`, KWin rejects the bind request with a protocol error.
- **Verdict**: Infeasible for third-party background daemons.

### Summary Protocol Matrix

| Protocol | Implemented in KWin? | Exposes Active State? | Third-Party Daemon Usable? |
| :--- | :---: | :---: | :---: |
| `zwlr_foreign_toplevel_manager_v1` | ❌ No | Yes | ❌ Not available |
| `ext-foreign-toplevel-list-v1` | ✅ Yes (Plasma 6) | ❌ No (no focus signal) | ❌ Cannot detect focus |
| `org_kde_plasma_window_management` | ✅ Yes | ✅ Yes | ❌ Blocked (singleton client) |

---

## 3. Evaluation of KWin D-Bus Interfaces (`org.kde.KWin`)

KWin exposes several D-Bus interfaces on the session bus (`org.kde.KWin`).

### 3.1 `/KWin` Object (`org.kde.KWin`)
Exposes global window manager commands:
- `currentDesktop() -> int`: Returns current virtual desktop index.
- `setCurrentDesktop(int) -> void`: Switches desktop.
- `queryWindowInfo() -> QVariantMap`: Turns cursor into interactive crosshair; **blocks until the user clicks a window**. Totally unsuitable for automated daemon operation.
- `getWindowInfo(QString uuid) -> QVariantMap`: Non-interactive query, but requires knowing the internal window `QUuid` beforehand.
- `activeWindow`: **Does not exist** on `/KWin`. Calling it results in `org.freedesktop.DBus.Error.UnknownMethod`.

### 3.2 `/Scripting` Object (`org.kde.kwin.Scripting`)
KWin includes an embedded ECMAScript (QJSEngine) execution engine specifically for scripting window management tasks. The `/Scripting` D-Bus interface exposes:
- `loadScript(QString filePath, QString pluginName) -> int`: Loads a JavaScript file from an absolute file path and registers it with KWin under `pluginName`. Returns an integer script ID.
- `unloadScript(QString pluginName) -> bool`: Unloads the specified script and stops all event hooks.
- `start() -> void`: Starts all loaded scripts.

Once loaded, KWin creates a child D-Bus object:
- In Plasma 6: `/Scripting/Script<id>` (implements `org.kde.kwin.Script` with `run()` and `stop()`).
- In Plasma 5: `/<id>` (and `/Scripting/Script<id>`).

This interface has full access to KWin's internal `workspace` object, which has real-time awareness of every window, focus transition, and title modification.

---

## 4. Prior Art & Ecosystem Analysis

### 4.1 ActivityWatch (`aw-watcher-window` & `awatcher`)
- Standard ActivityWatch (`aw-watcher-window`) relies strictly on X11 `XGetInputFocus` / `xprop`. Under Wayland, it fails or records generic XWayland surfaces.
- The Wayland variant `aw-watcher-window-wayland` relies on `zwlr_foreign_toplevel_manager_v1`, which fails on KDE Plasma.
- **`2e3s/awatcher` (Rust)**:
  `awatcher` is a high-performance Rust rewrite of the ActivityWatch watchers. For KDE Plasma Wayland, it introduced a dedicated `kwin_window` module that dynamically loads a KWin script via D-Bus to track window activations.

### 4.2 `kdotool` (Rust)
- `kdotool` (`jinliu/kdotool`) is an `xdotool` replacement written in Rust for KDE Plasma.
- **Evolution**: Early versions loaded KWin scripts that executed `print(...)` and scraped standard output from `journalctl -u plasma-kwin_wayland`. This was slow, fragile, and spammy.
- **Modern Architecture**: Modern `kdotool` generates a KWin script that uses KWin's built-in `callDBus(...)` function to communicate back to `kdotool` over a temporary D-Bus interface, avoiding log parsing entirely.

---

## 5. Recommended Solution: Dynamic KWin Script with Bi-Directional D-Bus

### 5.1 Architecture Overview

```
┌────────────────────────────────────────────────────────┐
│                   Rust Daemon                          │
│                                                        │
│  1. Connect to Session Bus (zbus)                      │
│  2. Request name "org.screentime.Daemon"               │
│  3. Serve interface "org.screentime.Tracker"           │
│     with method WindowChanged(app_class, title)        │
│  4. Write kwin_tracker.js to $XDG_RUNTIME_DIR          │
│  5. Call org.kde.KWin.Scripting.loadScript(...)        │
│  6. Call Script.run()                                  │
│                                                        │
│       ▲                                   │            │
│       │ D-Bus Method Call                 │ D-Bus Call │
│       │ "WindowChanged"                   │ loadScript │
│       │ (zero polling)                    │ unload     │
│       │                                   ▼            │
│  ┌────┴───────────────────────────────────────┐        │
│  │               KWin Compositor              │        │
│  │                                            │        │
│  │  ┌──────────────────────────────────────┐  │        │
│  │  │         screentime_tracker.js        │  │        │
│  │  │                                      │  │        │
│  │  │  - workspace.windowActivated         │  │        │
│  │  │  - window.captionChanged             │  │        │
│  │  │  - calls callDBus(...) on changes    │  │        │
│  │  └──────────────────────────────────────┘  │        │
│  └────────────────────────────────────────────┘        │
└────────────────────────────────────────────────────────┘
```

### 5.2 Plasma 5 vs Plasma 6 Unified Polyfill Script

Between KDE Plasma 5 (LTS) and Plasma 6 (current standard), KWin refactored its scripting API:
- `workspace.activeClient` → `workspace.activeWindow`
- `workspace.clientActivated` → `workspace.windowActivated`
- Window caption updates are observed via the `captionChanged` signal on the active window.

The JavaScript snippet below polyfills both versions dynamically and handles window focus, title updates, and desktop background focus:

```javascript
// screentime_kwin.js
// Polyfill and event listener for KDE Plasma 5 & 6

var isPlasma6 = (typeof workspace.windowActivated !== "undefined");

function getActiveWindow() {
    return isPlasma6 ? workspace.activeWindow : workspace.activeClient;
}

var currentTrackedWindow = null;

function notifyChange(win) {
    if (!win) {
        callDBus(
            "org.screentime.Daemon",
            "/org/screentime/Tracker",
            "org.screentime.Tracker",
            "WindowChanged",
            "",
            "",
            function() {}
        );
        return;
    }

    // resourceClass corresponds to app_id on Wayland and WM_CLASS on XWayland.
    // Fall back to desktopFileName or resourceName if resourceClass is empty.
    var appClass = win.resourceClass || win.desktopFileName || win.resourceName || "";
    var title = win.caption || "";

    callDBus(
        "org.screentime.Daemon",
        "/org/screentime/Tracker",
        "org.screentime.Tracker",
        "WindowChanged",
        String(appClass),
        String(title),
        function() {}
    );
}

function onCaptionChanged() {
    var win = getActiveWindow();
    notifyChange(win);
}

function onWindowActivated(win) {
    // Unhook previous caption listener if needed
    if (currentTrackedWindow && typeof currentTrackedWindow.captionChanged !== "undefined") {
        try {
            currentTrackedWindow.captionChanged.disconnect(onCaptionChanged);
        } catch (e) {
            // Safe to ignore if already disconnected
        }
    }

    currentTrackedWindow = win;

    if (win) {
        if (typeof win.captionChanged !== "undefined") {
            try {
                win.captionChanged.connect(onCaptionChanged);
            } catch (e) {
                // Ignore connection errors on exotic surfaces
            }
        }
        notifyChange(win);
    } else {
        notifyChange(null);
    }
}

// Register window activation listener
if (isPlasma6) {
    workspace.windowActivated.connect(onWindowActivated);
} else {
    workspace.clientActivated.connect(onWindowActivated);
}

// Initial notification for the currently focused window on startup
var initial = getActiveWindow();
if (initial) {
    onWindowActivated(initial);
} else {
    notifyChange(null);
}
```

### 5.3 Window Metadata Extraction Details
- **`win.resourceClass`**:
  - Under Wayland, KWin maps the client's `xdg_toplevel` `app_id` (e.g. `org.mozilla.firefox`, `google-chrome`, `kitty`, `org.kde.dolphin`) directly to `resourceClass`.
  - Under XWayland, KWin maps the X11 `WM_CLASS` class string directly to `resourceClass`.
- **`win.desktopFileName`**: The basename of the matching `.desktop` entry. Serves as a reliable fallback.
- **`win.caption`**: Full title string displayed on the window titlebar.
- **`win.captionChanged`**: Signal emitted when tab switching or document saving alters the title without changing focus.

---

## 6. Concrete Rust Implementation Architecture (`zbus`)

### 6.1 D-Bus Interface Implementation

Using `zbus` (pure Rust, async/await native, zero C dependencies):

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use zbus::{interface, connection::Connection};
use zbus::zvariant::OwnedObjectPath;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub app_class: String,
    pub title: String,
}

/// D-Bus object served by screentimed to receive notifications from KWin script.
pub struct KdeTrackerReceiver {
    tx: mpsc::Sender<Option<WindowInfo>>,
    current: Arc<Mutex<Option<WindowInfo>>>,
}

#[interface(name = "org.screentime.Tracker")]
impl KdeTrackerReceiver {
    async fn window_changed(&self, app_class: String, title: String) {
        let info = if app_class.is_empty() && title.is_empty() {
            None
        } else {
            Some(WindowInfo { app_class, title })
        };

        // Cache latest for one-shot queries
        let mut cur = self.current.lock().await;
        *cur = info.clone();

        // Forward to the daemon orchestrator channel
        let _ = self.tx.send(info).await;
    }
}
```

### 6.2 Script Deployment & Lifecycle Manager

```rust
pub struct KdeWindowTracker {
    connection: Connection,
    script_path: PathBuf,
    current_window: Arc<Mutex<Option<WindowInfo>>>,
    plugin_name: &'static str,
}

impl KdeWindowTracker {
    pub async fn new(
        tx: mpsc::Sender<Option<WindowInfo>>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let current_window = Arc::new(Mutex::new(None));
        let receiver = KdeTrackerReceiver {
            tx,
            current: Arc::clone(&current_window),
        };

        // 1. Establish session bus connection and register our service
        let connection = zbus::connection::Builder::session()?
            .name("org.screentime.Daemon")?
            .serve_at("/org/screentime/Tracker", receiver)?
            .build()
            .await?;

        // 2. Prepare script in $XDG_RUNTIME_DIR/screentime/
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"));
        let screentime_dir = runtime_dir.join("screentime");
        tokio::fs::create_dir_all(&screentime_dir).await?;
        let script_path = screentime_dir.join("kwin_window_tracker.js");

        // Write embedded script
        let script_content = include_str!("kwin_tracker.js");
        tokio::fs::write(&script_path, script_content).await?;

        let tracker = Self {
            connection,
            script_path,
            current_window,
            plugin_name: "screentime_kwin_tracker",
        };

        // 3. Load and execute script in KWin
        tracker.load_and_run_script().await?;

        Ok(tracker)
    }

    async fn load_and_run_script(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        )
        .await?;

        // Unload any existing instance first to prevent duplicates
        let _: Result<bool, _> = proxy
            .call("unloadScript", &(self.plugin_name))
            .await;

        // Load our script from disk
        let script_path_str = self.script_path.to_str().ok_or("Invalid path")?;
        let script_id: i32 = proxy
            .call("loadScript", &(script_path_str, self.plugin_name))
            .await?;

        if script_id < 0 {
            return Err(format!("KWin failed to load script: id={}", script_id).into());
        }

        // Plasma 6 script path: /Scripting/Script<id>
        // Plasma 5 script path: /<id>
        // Try Plasma 6 first, fallback to Plasma 5, or call start() on /Scripting
        let plasma6_path = format!("/Scripting/Script{}", script_id);
        let script_proxy_result = zbus::Proxy::new(
            &self.connection,
            "org.kde.KWin",
            &plasma6_path,
            "org.kde.kwin.Script",
        )
        .await;

        match script_proxy_result {
            Ok(script_proxy) => {
                let run_res: Result<(), _> = script_proxy.call("run", &()).await;
                if run_res.is_err() {
                    // Fallback to start()
                    let _: () = proxy.call("start", &()).await?;
                }
            }
            Err(_) => {
                let _: () = proxy.call("start", &()).await?;
            }
        }

        Ok(())
    }

    /// One-shot active window query (instant cache read).
    pub async fn get_active_window(&self) -> Option<WindowInfo> {
        self.current_window.lock().await.clone()
    }

    /// Graceful cleanup when daemon stops.
    pub async fn stop(&self) {
        if let Ok(proxy) = zbus::Proxy::new(
            &self.connection,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        )
        .await {
            let _: Result<bool, _> = proxy.call("unloadScript", &(self.plugin_name)).await;
        }
        let _ = tokio::fs::remove_file(&self.script_path).await;
    }
}

impl Drop for KdeWindowTracker {
    fn drop(&mut self) {
        // Synchronous non-blocking trigger if tokio runtime is tearing down
    }
}
```

### 6.3 Handling KWin Restarts / Crashes
If KWin restarts (e.g. `kwin_wayland` crashes or is updated), dynamically loaded scripts are lost. The daemon can handle this transparently by watching D-Bus `NameOwnerChanged` for service `org.kde.KWin`:

```rust
async fn monitor_kwin_lifecycle(
    tracker: Arc<KdeWindowTracker>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dbus_proxy = zbus::fdo::DBusProxy::new(&tracker.connection).await?;
    let mut owner_changes = dbus_proxy.receive_name_owner_changed().await?;

    tokio::spawn(async move {
        while let Some(signal) = owner_changes.next().await {
            let args = match signal.args() {
                Ok(a) => a,
                Err(_) => continue,
            };
            if args.name == "org.kde.KWin" && !args.new_owner.is_empty() {
                log::info!("KWin restarted detected (new owner: {}); re-registering tracker script", args.new_owner);
                // Give KWin 500ms to initialize its scripting subsystem
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                if let Err(e) = tracker.load_and_run_script().await {
                    log::error!("Failed to reload KWin script after restart: {}", e);
                }
            }
        }
    });

    Ok(())
}
```

---

## 7. Security and Permissions Assessment

1. **Privilege Requirements**:
   - **Zero root or sudo privileges**: Communicates exclusively via `$DBUS_SESSION_BUS_ADDRESS`.
   - **Zero Polkit prompts**: Polkit only mediates system bus authorization; session bus interactions within the logged-in user account execute immediately without user prompts.
2. **Sandbox / Packaging Compatibility**:
   - Native packages (deb, rpm, Arch, Nix, Cargo): Full direct access to session bus.
   - Flatpak (if distributed as Flatpak in future): Requires session bus talk permission:
     `--talk-name=org.kde.KWin` and `--own-name=org.screentime.Daemon`.
3. **Compositor Stability**:
   - `callDBus` in KWin fails silently without throwing fatal exceptions if the recipient D-Bus daemon is absent or terminates.
   - The script performs no invasive operations, memory poking, or geometry mutation; it only reads properties and emits notifications.

---

## 8. Integration with the Unified `WindowTracker` Trait

In issue `08-window-tracker-and-idle-detector-trait-design`, the unified platform abstraction is designed as an async event stream or callback. The KDE tracker maps directly to this model:

```rust
#[async_trait::async_trait]
pub trait WindowTracker: Send + Sync {
    /// Receive real-time window changes (None represents desktop or no focused window).
    async fn next_event(&mut self) -> Option<Option<WindowInfo>>;

    /// Query currently active window immediately.
    async fn active_window(&self) -> Option<WindowInfo>;
}
```

The `KdeWindowTracker` feeds directly into this interface via an internal `tokio::sync::mpsc::channel`, maintaining parity with the Hyprland IPC tracker and the X11 EWMH tracker.

---

## 9. Citations & References

1. **KDE KWin Scripting Documentation**:
   - KWin Scripting API reference: [develop.kde.org/docs/plasma/kwin](https://develop.kde.org/docs/plasma/kwin/)
   - Plasma 6 Porting Guide for KWin Scripts: [develop.kde.org/docs/plasma/kwin/porting-to-plasma-6](https://develop.kde.org/docs/plasma/kwin/)
2. **KDE Plasma Wayland Protocol Definitions**:
   - `plasma-window-management.xml`: [invent.kde.org/libraries/plasma-wayland-protocols](https://invent.kde.org/libraries/plasma-wayland-protocols/-/blob/master/src/protocols/plasma-window-management.xml)
   - Wayland Standard `ext-foreign-toplevel-list-v1`: [wayland.app/protocols/ext-foreign-toplevel-list-v1](https://wayland.app/protocols/ext-foreign-toplevel-list-v1)
3. **Reference Implementations**:
   - `2e3s/awatcher` (Rust ActivityWatch watcher with KWin scripting backend): [github.com/2e3s/awatcher](https://github.com/2e3s/awatcher)
   - `jinliu/kdotool` (KDE Wayland automation tool in Rust using D-Bus scripting): [github.com/jinliu/kdotool](https://github.com/jinliu/kdotool)
4. **Rust D-Bus Library**:
   - `zbus`: Pure Rust D-Bus implementation [crates.io/crates/zbus](https://crates.io/crates/zbus)
