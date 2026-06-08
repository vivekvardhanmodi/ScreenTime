#!/usr/bin/env bash
#
# Helper script to set the Chrome extension ID in the native messaging host manifest.
# Usage: ./set-extension-id.sh YOUR_EXTENSION_ID
#

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

MANIFEST="$HOME/.config/google-chrome/NativeMessagingHosts/com.screentime.native.json"

if [[ $# -ne 1 ]]; then
    echo -e "${RED}Usage:${NC} $0 <CHROME_EXTENSION_ID>"
    echo ""
    echo "To find your extension ID:"
    echo "  1. Open chrome://extensions/"
    echo "  2. Find 'ScreenTime Tracker'"
    echo "  3. Copy the ID shown (e.g., ooaoiipnihlenheiciokajmcpallfmcg)"
    exit 1
fi

EXT_ID="$1"

if [[ ! -f "$MANIFEST" ]]; then
    echo -e "${RED}Error:${NC} Native messaging host manifest not found at:"
    echo "  $MANIFEST"
    echo ""
    echo "Run ./install.sh first."
    exit 1
fi

# Replace the extension ID (works for both placeholder and existing ID)
sed -i "s|chrome-extension://[^/]*/|chrome-extension://${EXT_ID}/|" "$MANIFEST"

echo -e "${GREEN}✅ Extension ID set to:${NC} $EXT_ID"
echo ""
echo "Manifest updated at:"
echo "  $MANIFEST"
echo ""
echo "Now restart Chrome for the changes to take effect."
