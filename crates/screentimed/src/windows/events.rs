use screentime_core::WindowInfo;

/// Platform events produced by the Windows OS event listener thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformEvent {
    /// Foreground window change with resolved application image and window title.
    /// `None` indicates desktop or unfocused foreground.
    WindowChanged(Option<WindowInfo>),

    /// Console display powered off (monitor sleep / DPMS equivalent).
    DisplayOff,

    /// Console display powered on (monitor awake).
    DisplayOn,

    /// Console display dimmed.
    DisplayDim,

    /// User session locked (e.g. Win+L).
    SessionLocked,

    /// User session unlocked after authentication.
    SessionUnlocked,

    /// User logged off.
    SessionLogoff,

    /// System entering power suspend / sleep.
    Suspend,

    /// System resuming from power suspend / sleep.
    Resume,
}

/// Win32 Power Broadcast event codes.
#[allow(dead_code)]
pub const PBT_APMSUSPEND: usize = 0x0004;
#[allow(dead_code)]
pub const PBT_APMRESUMESUSPEND: usize = 0x0007;
#[allow(dead_code)]
pub const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;

/// Win32 Session Change notification codes from `Wtsapi32.h`.
#[allow(dead_code)]
pub const WTS_CONSOLE_CONNECT: usize = 0x1;
#[allow(dead_code)]
pub const WTS_CONSOLE_DISCONNECT: usize = 0x2;
#[allow(dead_code)]
pub const WTS_REMOTE_CONNECT: usize = 0x3;
#[allow(dead_code)]
pub const WTS_REMOTE_DISCONNECT: usize = 0x4;
pub const WTS_SESSION_LOGON: usize = 0x5;
pub const WTS_SESSION_LOGOFF: usize = 0x6;
pub const WTS_SESSION_LOCK: usize = 0x7;
pub const WTS_SESSION_UNLOCK: usize = 0x8;
#[allow(dead_code)]
pub const WTS_SESSION_REMOTE_CONTROL: usize = 0x9;

/// Win32 Power Setting Notification wParam code.
#[allow(dead_code)]
pub const PBT_POWERSETTINGCHANGE: usize = 0x8013;

/// Windows message codes.
#[allow(dead_code)]
pub const WM_POWERBROADCAST: u32 = 0x0218;
#[allow(dead_code)]
pub const WM_WTSSESSION_CHANGE: u32 = 0x02B1;

/// Parses a `POWERBROADCAST_SETTING.Data` DWORD for `GUID_CONSOLE_DISPLAY_STATE`.
///
/// Values:
/// - `0`: Display is off.
/// - `1`: Display is on.
/// - `2`: Display is dimmed.
pub fn parse_display_power_state(data: u32) -> Option<PlatformEvent> {
    match data {
        0 => Some(PlatformEvent::DisplayOff),
        1 => Some(PlatformEvent::DisplayOn),
        2 => Some(PlatformEvent::DisplayDim),
        _ => None,
    }
}

/// Parses a `WM_WTSSESSION_CHANGE` message wParam code.
pub fn parse_session_change_event(wparam: usize) -> Option<PlatformEvent> {
    match wparam {
        WTS_SESSION_LOCK => Some(PlatformEvent::SessionLocked),
        WTS_SESSION_UNLOCK => Some(PlatformEvent::SessionUnlocked),
        WTS_SESSION_LOGOFF => Some(PlatformEvent::SessionLogoff),
        _ => None,
    }
}

/// Parses a `WM_POWERBROADCAST` message wParam code for system suspend/resume events.
pub fn parse_power_broadcast(wparam: usize) -> Option<PlatformEvent> {
    match wparam {
        PBT_APMSUSPEND => Some(PlatformEvent::Suspend),
        PBT_APMRESUMESUSPEND | PBT_APMRESUMEAUTOMATIC => Some(PlatformEvent::Resume),
        _ => None,
    }
}

/// Cleans and sanitizes a window title string.
///
/// Strips null characters, leading/trailing whitespace, and returns an empty string
/// if no title characters exist.
pub fn clean_window_title(raw: &str) -> String {
    raw.trim_matches('\0').trim().to_string()
}

/// Decodes a UTF-16 wide-character buffer (as returned by `GetWindowTextW`) into a Rust String.
///
/// Finds the first null terminator (`0x0000`) if present, converting with lossy UTF-16 replacement.
pub fn decode_wide_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..len]);
    clean_window_title(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_display_power_state() {
        assert_eq!(
            parse_display_power_state(0),
            Some(PlatformEvent::DisplayOff)
        );
        assert_eq!(parse_display_power_state(1), Some(PlatformEvent::DisplayOn));
        assert_eq!(
            parse_display_power_state(2),
            Some(PlatformEvent::DisplayDim)
        );
        assert_eq!(parse_display_power_state(3), None);
        assert_eq!(parse_display_power_state(999), None);
    }

    #[test]
    fn test_parse_session_change_event() {
        assert_eq!(
            parse_session_change_event(WTS_SESSION_LOCK),
            Some(PlatformEvent::SessionLocked)
        );
        assert_eq!(
            parse_session_change_event(WTS_SESSION_UNLOCK),
            Some(PlatformEvent::SessionUnlocked)
        );
        assert_eq!(
            parse_session_change_event(WTS_SESSION_LOGOFF),
            Some(PlatformEvent::SessionLogoff)
        );
        assert_eq!(parse_session_change_event(WTS_SESSION_LOGON), None);
        assert_eq!(parse_session_change_event(999), None);
    }

    #[test]
    fn test_parse_power_broadcast() {
        assert_eq!(
            parse_power_broadcast(PBT_APMSUSPEND),
            Some(PlatformEvent::Suspend)
        );
        assert_eq!(
            parse_power_broadcast(PBT_APMRESUMESUSPEND),
            Some(PlatformEvent::Resume)
        );
        assert_eq!(
            parse_power_broadcast(PBT_APMRESUMEAUTOMATIC),
            Some(PlatformEvent::Resume)
        );
        assert_eq!(parse_power_broadcast(0), None);
        assert_eq!(parse_power_broadcast(999), None);
    }

    #[test]
    fn test_clean_window_title() {
        assert_eq!(
            clean_window_title("   GitHub - Screentime   "),
            "GitHub - Screentime"
        );
        assert_eq!(
            clean_window_title("Title with \0 embedded null"),
            "Title with \0 embedded null"
        );
        assert_eq!(clean_window_title("\0Trailing null\0"), "Trailing null");
        assert_eq!(clean_window_title("   "), "");
    }

    #[test]
    fn test_decode_wide_string() {
        let text: Vec<u16> = "Terminal - fish\0unused extra".encode_utf16().collect();
        assert_eq!(decode_wide_string(&text), "Terminal - fish");

        let no_null: Vec<u16> = "No null terminator".encode_utf16().collect();
        assert_eq!(decode_wide_string(&no_null), "No null terminator");

        let empty: Vec<u16> = vec![0];
        assert_eq!(decode_wide_string(&empty), "");
    }
}
