use std::time::Duration;
use tempfile::tempdir;

pub struct TestDbus {
    _temp_dir: tempfile::TempDir,
    address: String,
    process: std::process::Child,
}

impl TestDbus {
    pub fn spawn() -> Self {
        let temp_dir = tempdir().expect("create tempdir");
        let sock_path = temp_dir.path().join("bus.sock");
        let address = format!("unix:path={}", sock_path.display());

        let process = std::process::Command::new("dbus-daemon")
            .arg("--session")
            .arg(format!("--address={address}"))
            .arg("--nofork")
            .arg("--nopidfile")
            .spawn()
            .expect("spawn dbus-daemon");

        for _ in 0..100 {
            if sock_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(sock_path.exists(), "dbus socket must exist");

        Self {
            _temp_dir: temp_dir,
            address,
            process,
        }
    }

    pub fn address(&self) -> &str {
        &self.address
    }
}

impl Drop for TestDbus {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
