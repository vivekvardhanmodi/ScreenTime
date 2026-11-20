# 06: Two-Tier GitHub Actions CI Pipeline

**What to build:**
Deploy a complete CI pipeline in `.github/workflows/ci.yml`:
1. **Tier 1 (Protocol E2E Testing):**
   - *Headless Wayland:* Run headless Sway (`WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 sway`), spawn test apps, switch focus with `swaymsg`, trigger DPMS, and assert SQLite rows.
   - *Headless X11:* Run `xvfb-run` + Openbox, switch focus with `xdotool`, and assert SQLite rows.
   - *Windows:* Run on `windows-latest`, launch real Win32 test processes, and assert event hook capture in SQLite.
2. **Tier 2 (Multi-Distro Packaging Smoke Tests):**
   - Run `install.sh` in containerized Arch Linux, Fedora, and Ubuntu images to verify package manager resolution and clean builds.

**Blocked by:** 01, 02, 03, 04, 05

**Status:** open

- [ ] Create headless Wayland E2E test script and CI job
- [ ] Create headless X11 E2E test script and CI job
- [ ] Create Windows desktop CI test job
- [ ] Create multi-distro Docker smoke test workflow
- [ ] Verify workflow passes cleanly on GitHub Actions
