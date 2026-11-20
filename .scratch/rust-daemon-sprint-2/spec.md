# Specification: ScreenTime Rust Daemon Port Sprint 2 (Hardening, Packaging & Real E2E Testing)

Status: ready-for-agent

## Problem Statement

The initial ScreenTime Rust daemon port successfully ported window tracking backends (Hyprland, wlroots, KDE, X11, Windows) and browser IPC, but it remains incomplete for production daily driving. Specifically:

1. **Lid & Sleep Inaccuracies:** The daemon relies on a coarse 30-second heartbeat jump check to detect system sleep, leaking tracked time during short suspends. Furthermore, closing a laptop lid without immediate suspend does not pause tracking, while copying the legacy Python daemon's `/proc/acpi` poller would break clamshell mode (docked laptop with external display).
2. **Configuration Disconnect:** The Rust daemon only accepts CLI arguments and environment variables, completely ignoring the user's existing `~/.config/screentime/config.ini` file shared with the Web API and TUI.
3. **Unbounded Log Growth:** The daemon appends log output indefinitely to a single log file without size limits or rotation, risking disk bloat on long-running systems.
4. **Outdated Installer & Python Daemon Conflict:** The repository installation script (`install.sh`) still compiles and wires up the legacy Python daemon and browser host, overwriting the Rust daemon symlink and requiring heavy C headers and `pywayland` packages.
5. **False Confidence from Mock Tests:** In-memory unit tests passed 100% while masking real-world compositor protocol panics (such as Wayland opcode 0 dispatch failures). The project lacks real automated end-to-end verification across Wayland, X11, Windows, and popular Linux distributions.

## Solution

Complete the production hardening and deployment pipeline for the ScreenTime Rust daemon:

1. Provide zero-polling, event-driven system sleep and clamshell-aware laptop lid handling via D-Bus (`systemd-logind`) on Linux and power suspend notifications on Windows.
2. Establish hierarchical configuration precedence (CLI Flags > Environment Variables > `config.ini` > Defaults) to seamlessly read user settings from `~/.config/screentime/config.ini`.
3. Implement size-capped rolling file logging with configurable retention and CLI logging controls.
4. Update `install.sh` to compile and install the Rust daemon and native messaging host binaries, gracefully deprecate the Python daemon, and eliminate unnecessary Python build dependencies.
5. Deploy a two-tier GitHub Actions CI pipeline executing real black-box E2E testing against headless Wayland (Sway), headless X11 (Xvfb), and Windows runners, backed by multi-distro container build smoke tests.

## User Stories

1. As a laptop user closing my laptop lid while plugged into an external monitor, I want the daemon to keep tracking my work in clamshell mode, so that my active desktop screen time is not lost.
2. As a laptop user closing my laptop lid on battery power, I want the daemon to immediately pause tracking when the internal screen turns off, so that inactive transit time is not recorded.
3. As a user putting my machine to sleep or suspend, I want the daemon to catch the sleep event immediately via system signals before userspace freezes, so that zero seconds of sleep time leak into my active session.
4. As a user resuming my machine from sleep or hibernation, I want the daemon to cleanly start a fresh session upon wake without creating corrupted durations or duplicate entries.
5. As a Windows user locking or putting my computer to sleep, I want the daemon to capture the OS suspend notification, so that locked or asleep time is excluded from my statistics.
6. As a user with custom paths or intervals in `~/.config/screentime/config.ini`, I want `screentimed` to automatically load those settings, so that I do not need to repeat myself with command line flags.
7. As a user running `screentimed` with explicit CLI arguments, I want my CLI flags to override `config.ini` and environment variables, so that ad-hoc debugging takes immediate precedence.
8. As a user running `screentimed` with custom browser classes in `config.ini`, I want the daemon to treat those custom application names as web browsers, so that URL updates attach to their sessions.
9. As a user running the daemon continuously 24/7, I want `screentimed.log` to automatically roll over when it reaches the configured maximum size, so that the log file never consumes unbounded disk space.
10. As a user troubleshooting a daemon issue, I want to retain a configurable number of historical log backups, so that previous crash or error logs are preserved.
11. As a developer debugging startup behavior, I want to pass `--log-level debug` and `--log-console` directly on the command line, so that verbose output streams immediately to my terminal.
12. As a Linux user running `install.sh`, I want the script to build and install the fast Rust daemon (`screentimed`) and browser host (`screentime-browser-host`), so that I get the high-performance native tracking experience out of the box.
13. As a Linux user installing ScreenTime, I want `install.sh` to invoke the native manifest generator directly, so that Chrome and Firefox extensions are immediately wired to the Rust browser host without manual JSON editing.
14. As an existing ScreenTime user upgrading, I want the legacy Python daemon to be cleanly deprecated without breaking my existing database or Python TUI commands.
15. As a user on Arch Linux, Fedora, or openSUSE, I want `install.sh` to succeed without demanding obsolete Python Wayland C development headers, so that installation is fast and minimal.
16. As a contributor opening a pull request, I want automated CI to run real headless Wayland compositor tests, so that protocol regressions or child proxy panics are caught before merging.
17. As a contributor opening a pull request, I want automated CI to test real X11 window focus and property changes under a virtual X11 server, so that X11 tracking remains rock-solid.
18. As a Windows user, I want automated CI on `windows-latest` to verify foreground window hooks and single-instance named mutex locking, so that Windows releases are guaranteed to function.
19. As a Linux user on Fedora or Arch, I want containerized smoke tests in CI to verify that `install.sh` properly detects my package manager and compiles cleanly on my distribution.
20. As an operator inspecting daemon health, I want `screentimed status` to accurately report whether the process is active, its PID, and the listening socket path across all supported environments.

## Implementation Decisions

### 1. System Sleep and Clamshell-Aware Lid Handling
- **Linux D-Bus Architecture:** The daemon will subscribe to `org.freedesktop.login1.Manager` signals over the system D-Bus.
- **Sleep/Wake Transitions:** Intercept `PrepareForSleep(true)` to immediately close the active session in SQLite with an exact timestamp before system freeze. Intercept `PrepareForSleep(false)` upon system resumption to trigger an immediate active window query and resume tracking.
- **Clamshell Safety:** Monitor property changes for `LidClosed`. When `LidClosed` transitions to `true`, inspect connected monitor state. If external outputs are active, do not pause tracking (clamshell mode). If no external outputs exist, pause tracking as idle until the lid opens or a display powers on.
- **Windows Power Events:** On Windows, register for suspend/resume notifications via OS power broadcast events, committing the active session before system suspension.

### 2. Configuration Hierarchy and `config.ini` Compatibility
- **Precedence Order:**
  1. Command-Line Arguments (`--db`, `--socket`, `--platform`, `--device-id`, `--log-level`, etc.)
  2. Environment Variables (`SCREENTIME_DB_PATH`, `SCREENTIME_SOCKET`, etc.)
  3. Configuration File (`~/.config/screentime/config.ini` or Windows `%LOCALAPPDATA%\screentime\config.ini`)
  4. Built-in Static Defaults
- **Parsed Settings:** Support standard sections:
  - `[Paths]`: `data_dir`, `log_dir`
  - `[Daemon]`: `heartbeat_interval`, `idle_timeout`, `chrome_window_class`, `firefox_window_class`
  - `[Logging]`: `level`, `max_size_mb`, `backup_count`, `console`

### 3. Log Rotation and Observability
- **Size-Based Rolling Appender:** Introduce file rolling that monitors `screentimed.log` size, rotating to `.1`, `.2`, etc., up to `backup_count` when exceeding `max_size_mb`.
- **CLI Options:** Add `--log-level <LEVEL>` and `--log-console` to the daemon CLI to allow runtime control without editing configuration files or setting `RUST_LOG`.

### 4. Hybrid Installer and Python Daemon Deprecation
- **Rust Build Integration:** Update `install.sh` to run `cargo build --release` in the workspace root and install release binaries to the user's local binary path (`~/.local/bin/`).
- **Native Manifest Wiring:** Update `install.sh` to execute `screentimed install-manifests` with the installed browser host path, removing hand-rolled manifest generation.
- **Python Deprecation:** In `pyproject.toml`, rename the Python daemon entry point to `screentimed-legacy` and emit a runtime warning directing users to the Rust daemon. Remove `pywayland` and Wayland C-header prerequisites from Python package manifests and install scripts.
- **Main Branch Sync:** Fast-forward / merge the 7 commits from `origin/main` into the port branch.

### 5. Two-Tier GitHub Actions CI Pipeline
- **Tier 1: Desktop Protocol E2E Testing:**
  - *Wayland Runner:* Launch headless Sway with virtual outputs in an Ubuntu runner, spawning applications, driving focus with IPC, and asserting SQLite rows.
  - *X11 Runner:* Launch virtual X11 (`xvfb-run` + EWMH window manager) in an Ubuntu runner, switching focus with input tools, and asserting SQLite rows.
  - *Windows Runner:* Launch on `windows-latest`, spawning real Win32 test processes and asserting event hook capture in SQLite.
- **Tier 2: Multi-Distro Smoke Verification:**
  - Execute containerized runs of `install.sh` inside Arch Linux, Fedora, and Ubuntu Docker containers to verify prerequisite package resolution, compilation, and manifest installation.

## Testing Decisions

### What Makes a Good Test
- Tests must verify external observable behavior (process exit codes, IPC socket responses, and SQLite database records) rather than internal mock state.
- Pure-logic unit tests remain for configuration parsing and mathematical algorithms, but all desktop interactions must be proven against live compositors or display servers.

### Modules Tested
- **Configuration & Logging:** Unit tests verifying fallback hierarchy, malformed INI resilience, and rolling log file rotations.
- **Sleep & Lid Lifecycle:** Integration tests verifying signal handlers closing sessions at exact timestamps.
- **End-to-End Black Box:** Process-level E2E tests executing `screentimed` with `--db <tmp>` and `--socket <tmp>` against virtual compositors (Sway headless, Xvfb, Windows).

### Prior Art
- High-level CLI testing in `crates/screentimed/tests/cli_tests.rs`.
- Session state machine tests in `crates/screentimed/tests/session_manager_tests.rs`.

## Out of Scope
- Rewriting the Textual TUI (`screentime.tui`) or FastAPI server (`screentime.api`) in Rust (deferred to a future UI port phase).
- Support for macOS.
- Native GNOME Wayland window tracking without extensions.

## Further Notes
- All changes must follow repo commit conventions (`type(scope): <message>`).
- Database migrations and schema must preserve 100% backward and forward compatibility with existing user databases.
