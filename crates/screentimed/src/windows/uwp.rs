/// Window class name used by UWP application child windows hosted in ApplicationFrameHost.
pub const UWP_CORE_WINDOW_CLASS: &str = "Windows.UI.Core.CoreWindow";

/// Executable image name of the Windows UWP container host.
pub const UWP_FRAME_HOST_EXE: &str = "ApplicationFrameHost.exe";

/// Extracts the executable filename from a full path or image path.
///
/// Handles both Windows backslashes (`\`) and Unix forward slashes (`/`),
/// trimming surrounding whitespace.
pub fn extract_process_name(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

/// Returns whether an application executable name matches the UWP container host (`ApplicationFrameHost.exe`).
pub fn is_uwp_container(app_name: &str) -> bool {
    app_name.eq_ignore_ascii_case(UWP_FRAME_HOST_EXE)
}

/// A lightweight representation of a child window candidate for UWP unwrapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildWindowCandidate {
    pub hwnd: isize,
    pub pid: u32,
    pub class_name: String,
}

impl ChildWindowCandidate {
    pub fn new(hwnd: isize, pid: u32, class_name: impl Into<String>) -> Self {
        Self {
            hwnd,
            pid,
            class_name: class_name.into(),
        }
    }
}

/// Identifies the real hosted child process inside an `ApplicationFrameHost.exe` top-level container.
///
/// UWP / Windows Store apps run inside a parent frame host window (`ApplicationFrameHost.exe`).
/// The true application logic (e.g. `CalculatorApp.exe`, `WindowsNotepad.exe`) runs in a separate
/// child process attached to a child window whose window class is `"Windows.UI.Core.CoreWindow"`.
///
/// This function filters out the host PID, searches for candidate windows matching
/// `"Windows.UI.Core.CoreWindow"`, and returns the child's `(HWND, PID)` if found.
pub fn find_uwp_child_process(
    host_pid: u32,
    candidates: &[ChildWindowCandidate],
) -> Option<(isize, u32)> {
    for child in candidates {
        if child.pid != 0 && child.pid != host_pid && child.class_name == UWP_CORE_WINDOW_CLASS {
            return Some((child.hwnd, child.pid));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_process_name_windows_paths() {
        assert_eq!(
            extract_process_name(
                r"C:\Program Files\WindowsApps\Microsoft.WindowsCalculator_8wekyb3d8bbwe\CalculatorApp.exe"
            ),
            "CalculatorApp.exe"
        );
        assert_eq!(
            extract_process_name(r"C:\Windows\System32\notepad.exe"),
            "notepad.exe"
        );
        assert_eq!(
            extract_process_name(
                r"\Device\HarddiskVolume3\Program Files\Google\Chrome\Application\chrome.exe"
            ),
            "chrome.exe"
        );
        assert_eq!(extract_process_name("code.exe"), "code.exe");
        assert_eq!(extract_process_name(""), "");
        assert_eq!(extract_process_name("   "), "");
    }

    #[test]
    fn test_extract_process_name_mixed_and_unix_paths() {
        assert_eq!(
            extract_process_name("C:/Program Files/Sublime Text/sublime_text.exe"),
            "sublime_text.exe"
        );
        assert_eq!(extract_process_name("/usr/bin/alacritty"), "alacritty");
    }

    #[test]
    fn test_is_uwp_container() {
        assert!(is_uwp_container("ApplicationFrameHost.exe"));
        assert!(is_uwp_container("applicationframehost.exe"));
        assert!(is_uwp_container("APPLICATIONFRAMEHOST.EXE"));
        assert!(!is_uwp_container("CalculatorApp.exe"));
        assert!(!is_uwp_container("explorer.exe"));
        assert!(!is_uwp_container(""));
    }

    #[test]
    fn test_find_uwp_child_process() {
        let host_pid = 1000;

        let candidates = vec![
            ChildWindowCandidate::new(0x101, host_pid, "Windows.UI.Core.CoreWindow"), // same PID, skip
            ChildWindowCandidate::new(0x102, 2000, "IntermediateD3DWindow"), // wrong class, skip
            ChildWindowCandidate::new(0x103, 0, "Windows.UI.Core.CoreWindow"), // PID 0, skip
            ChildWindowCandidate::new(0x104, 3000, "Windows.UI.Core.CoreWindow"), // Match!
            ChildWindowCandidate::new(0x105, 4000, "Windows.UI.Core.CoreWindow"), // Second match
        ];

        let result = find_uwp_child_process(host_pid, &candidates);
        assert_eq!(result, Some((0x104, 3000)));
    }

    #[test]
    fn test_find_uwp_child_process_none_found() {
        let host_pid = 1000;

        let candidates = vec![
            ChildWindowCandidate::new(0x101, host_pid, "Windows.UI.Core.CoreWindow"),
            ChildWindowCandidate::new(0x102, 2000, "WorkerW"),
        ];

        let result = find_uwp_child_process(host_pid, &candidates);
        assert_eq!(result, None);
    }
}
