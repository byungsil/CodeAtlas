#!/bin/bash
# CodeAtlas GUI Setup Wizard Launcher (Linux/macOS)
# Usage: ./setup-gui.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WIZARD_DIR="$SCRIPT_DIR/setup-wizard"

if [ ! -d "$WIZARD_DIR" ]; then
    echo "Error: setup-wizard directory not found at $WIZARD_DIR" >&2
    exit 1
fi

# Install dependencies if needed
if [ ! -d "$WIZARD_DIR/node_modules" ]; then
    echo "Installing setup wizard dependencies..."
    cd "$WIZARD_DIR" && npm install
fi

# Build TypeScript
echo "Building Setup Wizard..."
cd "$WIZARD_DIR" && npx tsc

if [ ! -f "$WIZARD_DIR/electron-main.js" ]; then
    echo "Error: Build failed. Check TypeScript compilation." >&2
    exit 1
fi

# Launch Electron
echo ""
echo "Starting CodeAtlas Setup Wizard..."
cd "$WIZARD_DIR" && npx electron .
