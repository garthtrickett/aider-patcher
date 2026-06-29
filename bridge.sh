#!/usr/bin/env bash
# File: bridge.sh
# ==============================================================================
# Compatibility wrapper for the native Rust watcher built into aider-patcher.

set -euo pipefail

WATCH_DIR="${WATCH_DIR:-$HOME/Downloads}"
PROJECT_DIR="${PROJECT_DIR:-$(pwd)}"

cd "$PROJECT_DIR"

if command -v aider-patcher >/dev/null 2>&1; then
    PATCHER_BIN="aider-patcher"
elif [ -x "./aider-patcher" ]; then
    PATCHER_BIN="./aider-patcher"
elif [ -x "./target/release/aider-patcher" ]; then
    PATCHER_BIN="./target/release/aider-patcher"
elif [ -x "./target/debug/aider-patcher" ]; then
    PATCHER_BIN="./target/debug/aider-patcher"
else
    echo "❌ ERROR: 'aider-patcher' binary not found."
    echo "Build it with 'cargo build --release' or install it on PATH."
    exit 1
fi

exec "$PATCHER_BIN" --watch --watch-dir "$WATCH_DIR" --cwd "$PROJECT_DIR" "$@"
