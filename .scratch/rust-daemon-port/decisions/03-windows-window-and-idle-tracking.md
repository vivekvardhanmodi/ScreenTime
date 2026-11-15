# Windows Window and Idle Tracking

Type: research
Status: resolved
Blocked by: None

## Question

What Win32 APIs (e.g. `SetWinEventHook` with `EVENT_SYSTEM_FOREGROUND` vs event loops vs `GetForegroundWindow` polling, plus `GetLastInputInfo` for idle detection) are most robust, low-CPU, and battery-efficient for Windows 10/11, and which Rust crate (`windows-sys` vs `windows`) should be used?

## Answer

Use `windows-sys` (compiles in ~1.2s vs ~28s for `windows`, zero COM/WinRT bloat). Run a single dedicated background OS thread hosting an invisible message-only window (`HWND_MESSAGE`). Install `SetWinEventHook` with `EVENT_SYSTEM_FOREGROUND` for push-based window focus switches with 0.0% CPU when idle; resolve process path via `QueryFullProcessImageNameW` (with `EnumChildWindows` handling for UWP/`ApplicationFrameHost.exe` targets). Detect idle and lock states via `GetLastInputInfo` (with 32-bit wrapping subtraction for the 49.7-day rollover), display power toggles via `RegisterPowerSettingNotification(GUID_CONSOLE_DISPLAY_STATE)` producing `WM_POWERBROADCAST`, and session lock/unlock via `WTSRegisterSessionNotification` producing `WM_WTSSESSION_CHANGE`. Events stream to Tokio over `tokio::sync::mpsc`. See full research in [.scratch/rust-daemon-port/research/03-windows-window-and-idle-tracking.md](file:///home/vivek/.gemini/antigravity/worktrees/screentime/port_daemon_rust_screentime/.scratch/rust-daemon-port/research/03-windows-window-and-idle-tracking.md).
