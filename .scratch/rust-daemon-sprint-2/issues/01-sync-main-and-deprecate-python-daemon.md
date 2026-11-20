# 01: Sync origin/main and Gracefully Deprecate Python Daemon

**What to build:**
1. Merge the 7 commits from `origin/main` into `port_daemon_rust_screentime` (tightened CORS in `screentime/api.py`, async browser URL file read in `screentime/daemon.py`, and code health cleanup in `screentime/tui/app.py`).
2. Update `pyproject.toml` to rename the Python daemon entry point from `screentimed` to `screentimed-legacy`.
3. Add a runtime deprecation warning in `screentime/daemon.py` instructing users to use the high-performance native `screentimed` Rust binary instead.
4. Remove `pywayland` and C-header build dependencies from Python manifests to streamline Python environment setup.

**Blocked by:** None

**Status:** open

- [ ] Merge `origin/main` with 0 conflicts
- [ ] Rename Python entrypoint in `pyproject.toml` to `screentimed-legacy`
- [ ] Add deprecation warning to `screentime/daemon.py`
- [ ] Verify Python venv installs and runs TUI and Web server without `pywayland`
