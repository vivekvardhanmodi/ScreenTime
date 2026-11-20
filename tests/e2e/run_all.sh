#!/usr/bin/env bash
# Runner for all available end-to-end tests based on platform and installed display servers.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== Running E2E Test Suite ==="

RAN_ANY=0

if command -v sway &>/dev/null; then
    echo "Running Wayland (Sway) E2E harness..."
    "$SCRIPT_DIR/test_wayland_sway.sh"
    RAN_ANY=1
else
    echo "Skipping Wayland E2E: 'sway' not found on PATH."
fi

if command -v openbox &>/dev/null && (command -v xvfb-run &>/dev/null || [[ -n "${DISPLAY:-}" ]]); then
    echo "Running X11 (Openbox) E2E harness..."
    "$SCRIPT_DIR/test_x11_openbox.sh"
    RAN_ANY=1
else
    echo "Skipping X11 E2E: 'openbox' or 'xvfb-run' not found on PATH."
fi

if [[ $RAN_ANY -eq 0 ]]; then
    echo "Notice: Neither headless Sway nor Openbox/Xvfb found locally."
    echo "Both will run in the GitHub Actions Tier 1 CI pipeline."
fi

echo "=== All Available E2E Tests Complete ==="
