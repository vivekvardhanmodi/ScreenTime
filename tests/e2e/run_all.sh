#!/usr/bin/env bash
# Runner for all available end-to-end tests based on platform and installed display servers.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== Running E2E Test Suite ==="

STRICT=0
if [[ "${1:-}" == "--strict" ]]; then
    STRICT=1
fi

RAN_ANY=0

if command -v sway &>/dev/null; then
    echo "Running Wayland (Sway) E2E harness..."
    "$SCRIPT_DIR/test_wayland_sway.sh"
    RAN_ANY=1
else
    echo "Skipping Wayland E2E: 'sway' not found on PATH."
    if [[ $STRICT -eq 1 ]]; then
        echo "ERROR: --strict specified and 'sway' is not installed." >&2
        exit 1
    fi
fi

if command -v kwin_wayland &>/dev/null; then
    echo "Running Wayland (KDE KWin) E2E harness..."
    "$SCRIPT_DIR/test_wayland_kde.sh"
    RAN_ANY=1
else
    echo "Skipping KDE KWin E2E: 'kwin_wayland' not found on PATH."
fi

if command -v openbox &>/dev/null && (command -v xvfb-run &>/dev/null || [[ -n "${DISPLAY:-}" ]]); then
    echo "Running X11 (Openbox) E2E harness..."
    "$SCRIPT_DIR/test_x11_openbox.sh"
    RAN_ANY=1
else
    echo "Skipping X11 E2E: 'openbox' or 'xvfb-run' not found on PATH."
    if [[ $STRICT -eq 1 ]]; then
        echo "ERROR: --strict specified and 'openbox' or 'xvfb-run' is not installed." >&2
        exit 1
    fi
fi

if [[ $RAN_ANY -eq 0 ]]; then
    if [[ $STRICT -eq 1 ]]; then
        echo "ERROR: No E2E test environments available in --strict mode." >&2
        exit 1
    fi
    echo "Notice: Neither headless Sway nor Openbox/Xvfb found locally."
    echo "Use Docker or a VM with display servers installed to run E2E suites."
fi

echo "=== All Available E2E Tests Complete ==="
