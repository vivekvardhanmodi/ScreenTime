use std::path::{Path, PathBuf};

pub const DEFAULT_KWIN_SCRIPT: &str = include_str!("kwin_tracker.js");

pub fn generate_kwin_script(service_name: &str, object_path: &str, interface_name: &str) -> String {
    format!(
        r#"// screentime_kwin_tracker.js
// Polyfill and event listener for KDE Plasma 5 & 6

var isPlasma6 = (typeof workspace.windowActivated !== "undefined");

function getActiveWindow() {{
    return isPlasma6 ? workspace.activeWindow : workspace.activeClient;
}}

var currentTrackedWindow = null;

function notifyChange(win) {{
    if (!win) {{
        callDBus(
            "{service_name}",
            "{object_path}",
            "{interface_name}",
            "WindowChanged",
            "",
            "",
            function() {{}}
        );
        return;
    }}

    // resourceClass corresponds to app_id on Wayland and WM_CLASS on XWayland.
    // Fall back to desktopFileName or resourceName if resourceClass is empty.
    var appClass = win.resourceClass || win.desktopFileName || win.resourceName || "";
    var title = win.caption || "";

    callDBus(
        "{service_name}",
        "{object_path}",
        "{interface_name}",
        "WindowChanged",
        String(appClass),
        String(title),
        function() {{}}
    );
}}

function onCaptionChanged() {{
    var win = getActiveWindow();
    notifyChange(win);
}}

function onWindowActivated(win) {{
    // Unhook previous caption listener if needed
    if (currentTrackedWindow && typeof currentTrackedWindow.captionChanged !== "undefined") {{
        try {{
            currentTrackedWindow.captionChanged.disconnect(onCaptionChanged);
        }} catch (e) {{
            // Safe to ignore if already disconnected
        }}
    }}

    currentTrackedWindow = win;

    if (win) {{
        if (typeof win.captionChanged !== "undefined") {{
            try {{
                win.captionChanged.connect(onCaptionChanged);
            }} catch (e) {{
                // Ignore connection errors on exotic surfaces
            }}
        }}
        notifyChange(win);
    }} else {{
        notifyChange(null);
    }}
}}

// Register window activation listener
if (isPlasma6) {{
    workspace.windowActivated.connect(onWindowActivated);
}} else {{
    workspace.clientActivated.connect(onWindowActivated);
}}

// Initial notification for the currently focused window on startup
var initial = getActiveWindow();
if (initial) {{
    onWindowActivated(initial);
}} else {{
    notifyChange(null);
}}
"#
    )
}

pub fn resolve_kwin_script_path(custom_dir: Option<&Path>, filename: Option<&str>) -> PathBuf {
    let script_filename = filename.unwrap_or("kwin_tracker.js");
    match custom_dir {
        Some(dir) => dir.join(script_filename),
        None => {
            let base = std::env::var("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir());
            base.join("screentime").join(script_filename)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_script_matches_generated() {
        let generated = generate_kwin_script(
            "org.screentime.Daemon",
            "/org/screentime/Tracker",
            "org.screentime.Tracker",
        );
        assert_eq!(DEFAULT_KWIN_SCRIPT, generated);
    }

    #[test]
    fn test_script_contains_plasma5_and_6_hooks() {
        assert!(DEFAULT_KWIN_SCRIPT.contains("workspace.windowActivated"));
        assert!(DEFAULT_KWIN_SCRIPT.contains("workspace.clientActivated"));
        assert!(DEFAULT_KWIN_SCRIPT.contains("workspace.activeWindow"));
        assert!(DEFAULT_KWIN_SCRIPT.contains("workspace.activeClient"));
        assert!(DEFAULT_KWIN_SCRIPT.contains("captionChanged"));
        assert!(DEFAULT_KWIN_SCRIPT.contains("callDBus"));
    }

    #[test]
    fn test_resolve_script_path() {
        let custom = Path::new("/tmp/custom_test_dir");
        let p = resolve_kwin_script_path(Some(custom), Some("my_script.js"));
        assert_eq!(p, custom.join("my_script.js"));

        let p_default_name = resolve_kwin_script_path(Some(custom), None);
        assert_eq!(p_default_name, custom.join("kwin_tracker.js"));
    }
}
