# 07: Windows 10/11 Window Tracking and Idle Detection

**What to build:** Build the native Windows window tracker and idle detector in `screentimed` using `windows-sys`. Uses an invisible message-only window (`HWND_MESSAGE`) to pump `SetWinEventHook` (`EVENT_SYSTEM_FOREGROUND`), resolves process names (traversing UWP `ApplicationFrameHost.exe`), detects user inactivity via `GetLastInputInfo` (with 32-bit rollover handling), and monitors display power / session lock events.

**Blocked by:** 02 (Core Domain Traits and SessionManager State Machine)

**Status:** ready-for-agent

- [ ] Runs a dedicated background OS worker thread registering an invisible message-only window (`HWND_MESSAGE`)
- [ ] Installs `SetWinEventHook` with `EVENT_SYSTEM_FOREGROUND` for push-based focus changes with 0.0% idle CPU
- [ ] Reads window titles safely using `GetWindowTextW` (kernel cache read, hang-proof)
- [ ] Resolves executable process image via `QueryFullProcessImageNameW`
- [ ] Handles UWP/Store apps by traversing child windows (`Windows.UI.Core.CoreWindow`) inside `ApplicationFrameHost.exe`
- [ ] Reads hardware idle time using `GetLastInputInfo` with 32-bit wrapping subtraction for the 49.7-day rollover
- [ ] Registers for `GUID_CONSOLE_DISPLAY_STATE` via `RegisterPowerSettingNotification` to receive `WM_POWERBROADCAST` on monitor sleep/wake
- [ ] Registers for session lock/unlock via `WTSRegisterSessionNotification` (`WM_WTSSESSION_CHANGE`)
