#!/usr/bin/env bash
# Clean aemeath build artifacts
set -euo pipefail

cd "$(dirname "$0")"

TARGET_ROOT="$HOME/.cache/aemeath-target"

# Clean all per-branch target subdirectories under the cache root.
if [ -d "$TARGET_ROOT" ]; then
    SIZE=$(du -sh "$TARGET_ROOT" | cut -f1)
    rm -rf "$TARGET_ROOT"
    echo ">>> cleaned: $TARGET_ROOT ($SIZE freed)"
else
    echo ">>> target root not found: $TARGET_ROOT"
fi

# Clean rotated log backups (e.g. aemeath.log.1, tui.log.12). Active *.log files
# are kept so running aemeath processes keep writing to their open handles.
AGENTS_DIR="${AEMEATH_AGENTS_DIR:-$HOME/.agents}"
AGENTS_DIR="${AGENTS_DIR/#\~/$HOME}" # expand a leading ~ if env var used one
LOGS_DIR="$AGENTS_DIR/logs"

if [ -d "$LOGS_DIR" ]; then
    # base ends in .log, suffix is a rotation index — mirrors logging::is_rotated_log_path.
    shopt -s nullglob
    rotated=("$LOGS_DIR"/*.log.[0-9]*)
    shopt -u nullglob
    if [ ${#rotated[@]} -gt 0 ]; then
        FREED=$(du -ch "${rotated[@]}" | tail -1 | cut -f1)
        rm -f "${rotated[@]}"
        echo ">>> cleaned: ${#rotated[@]} rotated log backups in $LOGS_DIR ($FREED freed)"
    else
        echo ">>> no rotated log backups in $LOGS_DIR"
    fi
else
    echo ">>> logs dir not found: $LOGS_DIR"
fi
