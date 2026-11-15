use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::info;

pub const NATIVE_HOST_NAME: &str = "com.screentime.native";
pub const DEFAULT_FIREFOX_EXTENSION_ID: &str = "screentime@screentime.local";
pub const DEFAULT_EXTENSION_PLACEHOLDER: &str = "EXTENSION_ID_PLACEHOLDER";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChromeManifest {
    pub name: String,
    pub description: String,
    pub path: String,
    #[serde(rename = "type")]
    pub host_type: String,
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirefoxManifest {
    pub name: String,
    pub description: String,
    pub path: String,
    #[serde(rename = "type")]
    pub host_type: String,
    pub allowed_extensions: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestInstallOptions {
    pub host_path: Option<PathBuf>,
    pub extension_id: Option<String>,
    pub chrome_dir: Option<PathBuf>,
    pub firefox_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestInstallResult {
    pub chrome_manifest: PathBuf,
    pub firefox_manifest: PathBuf,
    pub host_path: PathBuf,
    pub extension_id: String,
}

pub fn default_home_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home);
    }
    PathBuf::from(".")
}

pub fn default_chrome_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local_app_data)
                .join("Google")
                .join("Chrome")
                .join("User Data")
                .join("NativeMessagingHosts");
        }
    }
    #[cfg(target_os = "macos")]
    {
        return default_home_dir()
            .join("Library")
            .join("Application Support")
            .join("Google")
            .join("Chrome")
            .join("NativeMessagingHosts");
    }
    default_home_dir()
        .join(".config")
        .join("google-chrome")
        .join("NativeMessagingHosts")
}

pub fn default_firefox_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(app_data) = std::env::var("APPDATA") {
            return PathBuf::from(app_data)
                .join("Mozilla")
                .join("NativeMessagingHosts");
        }
    }
    #[cfg(target_os = "macos")]
    {
        return default_home_dir()
            .join("Library")
            .join("Application Support")
            .join("Mozilla")
            .join("NativeMessagingHosts");
    }
    default_home_dir()
        .join(".mozilla")
        .join("native-messaging-hosts")
}

/// Locate or resolve the path to `screentime-browser-host` binary.
pub fn resolve_browser_host_binary(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }

    let binary_name = if cfg!(windows) {
        "screentime-browser-host.exe"
    } else {
        "screentime-browser-host"
    };

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let candidate = parent.join(binary_name);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    for sub in &["target/release", "target/debug"] {
        let candidate = PathBuf::from(sub).join(binary_name);
        if candidate.exists() {
            if let Ok(canonical) = candidate.canonicalize() {
                return canonical;
            }
            return candidate;
        }
    }

    if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\screentime").join(binary_name)
    } else {
        PathBuf::from("/usr/local/bin").join(binary_name)
    }
}

/// Extract existing Chrome extension ID from an existing manifest if present.
pub fn extract_existing_extension_id(manifest_path: &Path) -> Option<String> {
    if !manifest_path.exists() {
        return None;
    }
    let data = fs::read_to_string(manifest_path).ok()?;
    let manifest: ChromeManifest = serde_json::from_str(&data).ok()?;
    for origin in manifest.allowed_origins {
        if let Some(rest) = origin.strip_prefix("chrome-extension://") {
            let id = rest.trim_end_matches('/');
            if !id.is_empty() && id != DEFAULT_EXTENSION_PLACEHOLDER {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// Generate Chrome manifest struct.
pub fn generate_chrome_manifest(host_path: &Path, extension_id: &str) -> ChromeManifest {
    ChromeManifest {
        name: NATIVE_HOST_NAME.to_string(),
        description: "ScreenTime browser URL tracker".to_string(),
        path: host_path.to_string_lossy().to_string(),
        host_type: "stdio".to_string(),
        allowed_origins: vec![format!("chrome-extension://{extension_id}/")],
    }
}

/// Generate Firefox manifest struct.
pub fn generate_firefox_manifest(host_path: &Path) -> FirefoxManifest {
    FirefoxManifest {
        name: NATIVE_HOST_NAME.to_string(),
        description: "ScreenTime browser URL tracker".to_string(),
        path: host_path.to_string_lossy().to_string(),
        host_type: "stdio".to_string(),
        allowed_extensions: vec![DEFAULT_FIREFOX_EXTENSION_ID.to_string()],
    }
}

/// Install Chrome and Firefox native messaging host manifests.
pub fn install_manifests(options: &ManifestInstallOptions) -> io::Result<ManifestInstallResult> {
    let host_path = resolve_browser_host_binary(options.host_path.as_deref());

    let chrome_dir = options
        .chrome_dir
        .clone()
        .unwrap_or_else(default_chrome_dir);
    let firefox_dir = options
        .firefox_dir
        .clone()
        .unwrap_or_else(default_firefox_dir);

    fs::create_dir_all(&chrome_dir)?;
    fs::create_dir_all(&firefox_dir)?;

    let chrome_manifest_path = chrome_dir.join(format!("{NATIVE_HOST_NAME}.json"));
    let firefox_manifest_path = firefox_dir.join(format!("{NATIVE_HOST_NAME}.json"));

    // Determine extension ID (explicit, preserved from disk, or placeholder)
    let extension_id = if let Some(ref id) = options.extension_id {
        id.clone()
    } else if let Some(existing_id) = extract_existing_extension_id(&chrome_manifest_path) {
        info!("Preserving existing Chrome extension ID: {existing_id}");
        existing_id
    } else {
        DEFAULT_EXTENSION_PLACEHOLDER.to_string()
    };

    let chrome_manifest = generate_chrome_manifest(&host_path, &extension_id);
    let firefox_manifest = generate_firefox_manifest(&host_path);

    let chrome_json = serde_json::to_string_pretty(&chrome_manifest)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let firefox_json = serde_json::to_string_pretty(&firefox_manifest)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    fs::write(&chrome_manifest_path, format!("{chrome_json}\n"))?;
    fs::write(&firefox_manifest_path, format!("{firefox_json}\n"))?;

    info!(
        "Installed Chrome native messaging manifest: {:?}",
        chrome_manifest_path
    );
    info!(
        "Installed Firefox native messaging manifest: {:?}",
        firefox_manifest_path
    );

    Ok(ManifestInstallResult {
        chrome_manifest: chrome_manifest_path,
        firefox_manifest: firefox_manifest_path,
        host_path,
        extension_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_manifest_generation() {
        let host = Path::new("/usr/bin/screentime-browser-host");
        let chrome = generate_chrome_manifest(host, "my-extension-id");
        assert_eq!(chrome.name, NATIVE_HOST_NAME);
        assert_eq!(chrome.path, "/usr/bin/screentime-browser-host");
        assert_eq!(
            chrome.allowed_origins,
            vec!["chrome-extension://my-extension-id/"]
        );

        let firefox = generate_firefox_manifest(host);
        assert_eq!(firefox.name, NATIVE_HOST_NAME);
        assert_eq!(firefox.path, "/usr/bin/screentime-browser-host");
        assert_eq!(
            firefox.allowed_extensions,
            vec![DEFAULT_FIREFOX_EXTENSION_ID]
        );
    }

    #[test]
    fn test_install_manifests_lifecycle_and_preservation() {
        let temp = tempdir().unwrap();
        let chrome_dir = temp.path().join("chrome");
        let firefox_dir = temp.path().join("firefox");

        let mut opts = ManifestInstallOptions {
            host_path: Some(PathBuf::from("/custom/bin/screentime-browser-host")),
            extension_id: Some("initial-ext-id".to_string()),
            chrome_dir: Some(chrome_dir.clone()),
            firefox_dir: Some(firefox_dir.clone()),
        };

        // First install with explicit ID
        let res = install_manifests(&opts).unwrap();
        assert_eq!(res.extension_id, "initial-ext-id");
        assert!(res.chrome_manifest.exists());
        assert!(res.firefox_manifest.exists());

        // Second install without explicit ID should preserve the previously written ID
        opts.extension_id = None;
        let res2 = install_manifests(&opts).unwrap();
        assert_eq!(res2.extension_id, "initial-ext-id");

        // Verify JSON contents
        let chrome_data = fs::read_to_string(&res2.chrome_manifest).unwrap();
        let chrome: ChromeManifest = serde_json::from_str(&chrome_data).unwrap();
        assert_eq!(
            chrome.allowed_origins,
            vec!["chrome-extension://initial-ext-id/"]
        );

        let ff_data = fs::read_to_string(&res2.firefox_manifest).unwrap();
        let ff: FirefoxManifest = serde_json::from_str(&ff_data).unwrap();
        assert_eq!(ff.allowed_extensions, vec![DEFAULT_FIREFOX_EXTENSION_ID]);
    }
}
