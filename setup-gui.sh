#!/bin/bash
# CodeAtlas GUI Setup Wizard Launcher (Linux/macOS)
# Canonical POSIX entry point for the interactive setup wizard.
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

# Build TypeScript + copy assets
echo "Building Setup Wizard..."
cd "$WIZARD_DIR" && npm run build

if [ ! -f "$WIZARD_DIR/main/electron-main.js" ]; then
    echo "Error: Build failed. Check TypeScript compilation." >&2
    exit 1
fi

# Launch Electron (prefer the local binary to avoid npx prompts)
echo ""
echo "Starting CodeAtlas Setup Wizard..."
ELECTRON_BIN="$WIZARD_DIR/node_modules/.bin/electron"
if [ -x "$ELECTRON_BIN" ]; then
    cd "$WIZARD_DIR" && "$ELECTRON_BIN" .
else
    cd "$WIZARD_DIR" && npx electron .
fi
