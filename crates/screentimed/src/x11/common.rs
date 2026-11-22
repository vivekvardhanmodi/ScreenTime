use super::atoms::Atoms;
use screentime_core::{IdleState, WindowInfo};
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::properties::WmClass;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, Window,
};
use x11rb::protocol::ErrorKind;
use x11rb::rust_connection::RustConnection;

/// Check if an X11 error is an expected transient error (e.g. window closed mid-query).
pub fn is_ignorable_x11_error(err: &ReplyError) -> bool {
    match err {
        ReplyError::X11Error(e) => matches!(
            e.error_kind,
            ErrorKind::Window | ErrorKind::Drawable | ErrorKind::Match | ErrorKind::Value
        ),
        _ => false,
    }
}

/// Parse raw ICCCM WM_CLASS bytes (<instance>\0<class>\0).
/// Prefers `class` if non-empty, otherwise falls back to `instance`.
pub fn parse_wm_class_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }

    let first_null = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let instance = String::from_utf8_lossy(&bytes[0..first_null])
        .trim_matches('\0')
        .trim()
        .to_string();

    let class = if first_null + 1 < bytes.len() {
        let rem = &bytes[first_null + 1..];
        let second_null = rem.iter().position(|&b| b == 0).unwrap_or(rem.len());
        String::from_utf8_lossy(&rem[0..second_null])
            .trim_matches('\0')
            .trim()
            .to_string()
    } else {
        String::new()
    };

    if !class.is_empty() {
        Some(class)
    } else if !instance.is_empty() {
        Some(instance)
    } else {
        None
    }
}

/// Clean a text property byte slice, trimming null bytes and whitespace.
pub fn clean_text_property(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(bytes)
        .trim_matches('\0')
        .trim()
        .to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Fetch WM_CLASS for a window via x11rb `WmClass`.
pub fn fetch_wm_class(conn: &RustConnection, window: Window) -> Option<String> {
    match WmClass::get(conn, window) {
        Ok(cookie) => match cookie.reply() {
            Ok(Some(wm_class)) => {
                let cls = String::from_utf8_lossy(wm_class.class())
                    .trim_matches('\0')
                    .trim()
                    .to_string();
                if !cls.is_empty() {
                    return Some(cls);
                }
                let inst = String::from_utf8_lossy(wm_class.instance())
                    .trim_matches('\0')
                    .trim()
                    .to_string();
                if !inst.is_empty() {
                    return Some(inst);
                }
                None
            }
            Ok(None) => None,
            Err(ref e) if is_ignorable_x11_error(e) => None,
            Err(e) => {
                tracing::debug!("Error fetching WM_CLASS for window {window}: {e}");
                None
            }
        },
        Err(ref e) => {
            tracing::debug!("Connection error sending WM_CLASS query for window {window}: {e}");
            None
        }
    }
}

/// Fetch a UTF-8 or Latin-1 text property from a window.
pub fn fetch_text_property(
    conn: &RustConnection,
    window: Window,
    property: Atom,
    property_type: Atom,
) -> Option<String> {
    let reply = match conn.get_property(false, window, property, property_type, 0, 1024) {
        Ok(cookie) => match cookie.reply() {
            Ok(reply) => reply,
            Err(ref e) if is_ignorable_x11_error(e) => return None,
            Err(e) => {
                tracing::debug!("Error fetching property {property} on window {window}: {e}");
                return None;
            }
        },
        Err(_) => return None,
    };

    if reply.value_len > 0 {
        clean_text_property(&reply.value)
    } else {
        None
    }
}

/// Fetch window title from `_NET_WM_NAME` (UTF-8), falling back to `WM_NAME` (STRING).
pub fn fetch_window_title(conn: &RustConnection, window: Window, atoms: &Atoms) -> Option<String> {
    if let Some(title) = fetch_text_property(conn, window, atoms._NET_WM_NAME, atoms.UTF8_STRING) {
        return Some(title);
    }

    if let Some(title) = fetch_text_property(
        conn,
        window,
        u32::from(AtomEnum::WM_NAME),
        u32::from(AtomEnum::STRING),
    ) {
        return Some(title);
    }

    None
}

/// Query `_NET_ACTIVE_WINDOW` on the root window.
pub fn query_root_active_window(
    conn: &RustConnection,
    root: Window,
    atoms: &Atoms,
) -> Option<Window> {
    let cookie = conn
        .get_property(
            false,
            root,
            atoms._NET_ACTIVE_WINDOW,
            AtomEnum::WINDOW,
            0,
            1,
        )
        .ok()?;
    let reply = cookie.reply().ok()?;
    if let Some(window_id) = reply.value32().and_then(|mut it| it.next()) {
        if window_id != 0 {
            return Some(window_id);
        }
    }
    None
}

/// Fetch complete `WindowInfo` for a window (resolving class and title).
pub fn fetch_window_info(
    conn: &RustConnection,
    window: Window,
    atoms: &Atoms,
) -> Option<WindowInfo> {
    let app_class = fetch_wm_class(conn, window)?;
    let title = fetch_window_title(conn, window, atoms).unwrap_or_else(|| app_class.clone());
    Some(WindowInfo::new(app_class, title))
}

/// Subscribe to property change events on a window.
pub fn watch_window(conn: &RustConnection, window: Window) {
    let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE);
    let _ = conn.change_window_attributes(window, &aux);
    let _ = conn.flush();
}

/// Unsubscribe from property change events on a previously focused window.
pub fn unwatch_window(conn: &RustConnection, window: Window) {
    let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT);
    let _ = conn.change_window_attributes(window, &aux);
    let _ = conn.flush();
}

/// Evaluate user idle state based on milliseconds since user input, screensaver state, and threshold.
pub fn evaluate_idle_state(
    ms_since_user_input: u32,
    screensaver_state: u8,
    threshold: Duration,
) -> IdleState {
    let idle_duration = Duration::from_millis(ms_since_user_input as u64);
    let is_screensaver_on = screensaver_state == u8::from(x11rb::protocol::screensaver::State::ON);
    if idle_duration >= threshold || is_screensaver_on {
        IdleState::Idle
    } else {
        IdleState::Active
    }
}
