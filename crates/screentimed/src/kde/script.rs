use std::path::{Path, PathBuf};

pub const DEFAULT_SERVICE_NAME: &str = "org.screentime.Daemon";
pub const DEFAULT_OBJECT_PATH: &str = "/org/screentime/Tracker";
pub const DEFAULT_INTERFACE_NAME: &str = "org.screentime.Tracker";

pub const DEFAULT_KWIN_SCRIPT: &str = include_str!("kwin_tracker.js");

pub fn generate_kwin_script(service_name: &str, object_path: &str, interface_name: &str) -> String {
    DEFAULT_KWIN_SCRIPT
        .replace("\"org.screentime.Daemon\"", "__SCREENTIME_SERVICE__")
        .replace("\"/org/screentime/Tracker\"", "__SCREENTIME_OBJECT__")
        .replace("\"org.screentime.Tracker\"", "__SCREENTIME_INTERFACE__")
        .replace("__SCREENTIME_SERVICE__", &format!("\"{service_name}\""))
        .replace("__SCREENTIME_OBJECT__", &format!("\"{object_path}\""))
        .replace("__SCREENTIME_INTERFACE__", &format!("\"{interface_name}\""))
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
            DEFAULT_SERVICE_NAME,
            DEFAULT_OBJECT_PATH,
            DEFAULT_INTERFACE_NAME,
        );
        assert_eq!(DEFAULT_KWIN_SCRIPT, generated);
    }

    #[test]
    fn test_script_generation_avoids_overlapping_substitutions() {
        let generated = generate_kwin_script(
            DEFAULT_INTERFACE_NAME,
            "/org/custom/Path",
            "org.custom.Interface",
        );
        assert!(generated.contains("\"org.screentime.Tracker\""));
        assert!(generated.contains("\"/org/custom/Path\""));
        assert!(generated.contains("\"org.custom.Interface\""));
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
