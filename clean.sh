#!/usr/bin/env bash
# Clean aemeath build artifacts
set -euo pipefail

cd "$(dirname "$0")"

TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/aemeath-target}"

if [ -d "$TARGET_DIR" ]; then
    SIZE=$(du -sh "$TARGET_DIR" | cut -f1)
    rm -rf "$TARGET_DIR"
    echo ">>> cleaned: $TARGET_DIR ($SIZE freed)"
else
    echo ">>> target dir not found: $TARGET_DIR"
fi
