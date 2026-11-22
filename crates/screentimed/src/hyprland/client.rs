use screentime_core::{IdleState, TrackerError, WindowInfo};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

pub fn resolve_socket_dir(custom_dir: Option<&Path>) -> Result<PathBuf, TrackerError> {
    if let Some(dir) = custom_dir {
        return Ok(dir.to_path_buf());
    }

    let his = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .map_err(|_| TrackerError::Tracker("HYPRLAND_INSTANCE_SIGNATURE not set".to_string()))?;

    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        let dir = PathBuf::from(xdg).join("hypr").join(&his);
        if dir.exists() || dir.join(".socket2.sock").exists() || dir.join(".socket.sock").exists() {
            return Ok(dir);
        }
    }

    let tmp_dir = PathBuf::from("/tmp").join("hypr").join(&his);
    if tmp_dir.exists()
        || tmp_dir.join(".socket2.sock").exists()
        || tmp_dir.join(".socket.sock").exists()
    {
        return Ok(tmp_dir);
    }

    Err(TrackerError::Tracker(format!(
        "Hyprland socket directory not found for instance signature '{his}'"
    )))
}

pub fn get_hyprland_socket_dir() -> Result<PathBuf, TrackerError> {
    resolve_socket_dir(None)
}

pub fn parse_active_window_event(payload: &str) -> Option<WindowInfo> {
    let trimmed = payload.trim();
    if trimmed.is_empty() || trimmed == "," {
        return None;
    }

    let (class, title) = match trimmed.split_once(',') {
        Some((c, t)) => (c.trim(), t.trim()),
        None => (trimmed, ""),
    };

    if class.is_empty() {
        return None;
    }

    Some(WindowInfo::new(class, title))
}

pub fn parse_active_window_line(line: &str) -> Option<Option<WindowInfo>> {
    let trimmed = line.trim();
    let payload = trimmed.strip_prefix("activewindow>>")?;
    Some(parse_active_window_event(payload))
}

pub fn parse_active_window_json(buf: &[u8]) -> Result<Option<WindowInfo>, TrackerError> {
    if buf.is_empty() {
        return Ok(None);
    }

    let val: serde_json::Value = serde_json::from_slice(buf)
        .map_err(|e| TrackerError::Tracker(format!("Failed to parse activewindow JSON: {e}")))?;

    let app_class = val
        .get("class")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if app_class.is_empty() {
        return Ok(None);
    }

    let title = val
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    Ok(Some(WindowInfo::new(app_class, title)))
}

/// Parses a Hyprland `dpms` event payload into `(optional_monitor_name, is_on)`.
pub fn parse_dpms_event(payload: &str) -> Option<(Option<String>, bool)> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (monitor, status_str) = match trimmed.split_once(',') {
        Some((mon, s)) => {
            let mon_trimmed = mon.trim();
            let mon_opt = if mon_trimmed.is_empty() {
                None
            } else {
                Some(mon_trimmed.to_string())
            };
            (mon_opt, s.trim())
        }
        None => (None, trimmed),
    };

    let is_on = match status_str.to_ascii_lowercase().as_str() {
        "0" | "off" | "false" => false,
        "1" | "on" | "true" => true,
        _ => return None,
    };

    Some((monitor, is_on))
}

pub fn parse_dpms_event_line(line: &str) -> Option<(Option<String>, bool)> {
    let trimmed = line.trim();
    let payload = trimmed.strip_prefix("dpms>>")?;
    parse_dpms_event(payload)
}

pub fn parse_dpms_payload(payload: &str) -> Option<IdleState> {
    let (_, is_on) = parse_dpms_event(payload)?;
    Some(if is_on {
        IdleState::Active
    } else {
        IdleState::Idle
    })
}

pub fn parse_dpms_line(line: &str) -> Option<IdleState> {
    let trimmed = line.trim();
    let payload = trimmed.strip_prefix("dpms>>")?;
    parse_dpms_payload(payload)
}

/// Parses `j/monitors` JSON into a per-monitor DPMS status map.
/// Returns `Ok(None)` when the response is empty or contains no valid monitor entries
/// (avoiding false idle transitions during monitor reconfiguration/reload).
pub fn parse_monitors_map(buf: &[u8]) -> Result<Option<HashMap<String, bool>>, TrackerError> {
    if buf.is_empty() {
        return Ok(None);
    }

    let val: serde_json::Value = serde_json::from_slice(buf)
        .map_err(|e| TrackerError::Tracker(format!("Failed to parse monitors JSON: {e}")))?;

    let monitors = val
        .as_array()
        .ok_or_else(|| TrackerError::Tracker("Expected monitors JSON array".to_string()))?;

    if monitors.is_empty() {
        return Ok(None);
    }

    let mut map = HashMap::new();
    for (idx, m) in monitors.iter().enumerate() {
        let Some(dpms) = m.get("dpmsStatus").and_then(|v| v.as_bool()) else {
            continue;
        };
        let name = m
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("monitor-{idx}"));
        map.insert(name, dpms);
    }

    if map.is_empty() {
        return Ok(None);
    }

    Ok(Some(map))
}

pub fn parse_monitors_json(buf: &[u8]) -> Result<Option<IdleState>, TrackerError> {
    let Some(map) = parse_monitors_map(buf)? else {
        return Ok(None);
    };

    let any_on = map.values().any(|&on| on);
    Ok(Some(if any_on {
        IdleState::Active
    } else {
        IdleState::Idle
    }))
}

pub async fn query_socket1(socket_dir: &Path, command: &[u8]) -> Result<Vec<u8>, TrackerError> {
    let socket_path = socket_dir.join(".socket.sock");
    let mut stream = UnixStream::connect(&socket_path)
        .await
        .map_err(TrackerError::Io)?;

    stream.write_all(command).await.map_err(TrackerError::Io)?;
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(TrackerError::Io)?;
    Ok(buf)
}

pub async fn query_active_window_at(socket_dir: &Path) -> Result<Option<WindowInfo>, TrackerError> {
    let buf = query_socket1(socket_dir, b"j/activewindow").await?;
    parse_active_window_json(&buf)
}

pub async fn query_monitors_map_at(
    socket_dir: &Path,
) -> Result<Option<HashMap<String, bool>>, TrackerError> {
    let buf = query_socket1(socket_dir, b"j/monitors").await?;
    parse_monitors_map(&buf)
}

pub async fn query_monitors_at(socket_dir: &Path) -> Result<Option<IdleState>, TrackerError> {
    let buf = query_socket1(socket_dir, b"j/monitors").await?;
    parse_monitors_json(&buf)
}
