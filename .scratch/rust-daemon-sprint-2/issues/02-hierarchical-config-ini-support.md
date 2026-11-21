# 02: Hierarchical Configuration and config.ini Support

**What to build:**
Implement hierarchical configuration resolution in `screentimed`:
CLI Arguments > Environment Variables > `~/.config/screentime/config.ini` > Built-in Defaults.

Supported sections and keys:
- `[Paths]`: `data_dir`, `log_dir`
- `[Daemon]`: `heartbeat_interval`, `idle_timeout`, `chrome_window_class`, `firefox_window_class`
- `[Logging]`: `level`, `max_size_mb`, `backup_count`, `console`

**Blocked by:** 01

**Status:** closed

- [x] Add INI parsing support (or pure-Rust INI reader) to `screentimed`
- [x] Implement `Config` loader resolving `~/.config/screentime/config.ini` (or `%LOCALAPPDATA%\screentime\config.ini`)
- [x] Wire resolved configuration into `SessionManager` (custom browser classes, heartbeat interval) and database paths
- [x] Unit tests verifying hierarchical precedence (CLI > Env > File > Defaults) and fallback behavior when `config.ini` is missing or malformed
