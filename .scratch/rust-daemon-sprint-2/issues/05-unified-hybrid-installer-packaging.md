# 05: Unified Hybrid Installer and Native Packaging

**What to build:**
Refactor `install.sh` to package and wire the hybrid Rust daemon and Python UI stack:
1. Verify system prerequisites (`cargo`, `python3`, `node/npm`).
2. Build Rust release binaries (`cargo build --release`) and install `screentimed` and `screentime-browser-host` to `~/.local/bin/`.
3. Invoke `screentimed install-manifests` to configure Chrome and Firefox native messaging manifests directly.
4. Set up Python virtual environment strictly for the Textual TUI (`screentime`) and Web API (`screentime-web`), symlinking them into `~/.local/bin/`.
5. Remove obsolete C headers and `pywayland` build dependencies from system dependency checks.

**Blocked by:** 01, 02, 03, 04

**Status:** open

- [ ] Add `cargo build --release` step and binary installation to `install.sh`
- [ ] Replace hand-rolled manifest creation with `screentimed install-manifests`
- [ ] Update symlinks in `~/.local/bin/` so `screentimed` points to the Rust release binary
- [ ] Drop `pywayland` and Wayland header requirements from `install.sh`
- [ ] Test end-to-end installation on local system
