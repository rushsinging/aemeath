#!/usr/bin/env bash
# Clean aemeath build artifacts
set -euo pipefail

cd "$(dirname "$0")"

# shellcheck source=.cargo/lib.sh
source ".cargo/lib.sh"

TARGET_ROOT="$HOME/.cache/aemeath-target"

usage() {
    cat <<EOF
Usage: clean.sh <branch> [--include-main]
       clean.sh --all [--include-main]

  <branch>          Clean the target dir for a specific branch.
  --all             Clean target dirs for all branches (excludes main by default).
  --include-main    When used with --all, also clean the main branch target dir.

Examples:
  clean.sh feature/my-branch
  clean.sh --all
  clean.sh --all --include-main
EOF
}

# --- parse arguments ---
ALL=false
INCLUDE_MAIN=false
BRANCH=""

for arg in "$@"; do
    case "$arg" in
        --all)
            ALL=true
            ;;
        --include-main)
            INCLUDE_MAIN=true
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            if [ -n "$BRANCH" ]; then
                echo ">>> error: unexpected argument '$arg'" >&2
                usage >&2
                exit 1
            fi
            BRANCH="$arg"
            ;;
    esac
done

if [ "$ALL" = true ]; then
    if [ -n "$BRANCH" ]; then
        echo ">>> error: cannot specify both a branch and --all" >&2
        usage >&2
        exit 1
    fi
    # Clean all per-branch target subdirectories under the cache root.
    if [ ! -d "$TARGET_ROOT" ]; then
        echo ">>> target root not found: $TARGET_ROOT"
    else
        cleaned=0
        for dir in "$TARGET_ROOT"/*/; do
            branch=$(basename "$dir")
            if [ "$branch" = "main" ] && [ "$INCLUDE_MAIN" = false ]; then
                echo ">>> skipped: main (use --include-main to also clean)"
                continue
            fi
            SIZE=$(du -sh "$dir" | cut -f1)
            rm -rf "$dir"
            echo ">>> cleaned: $branch ($SIZE freed)"
            cleaned=1
        done
        if [ "$cleaned" -eq 0 ]; then
            echo ">>> no branch target dirs to clean"
        fi
    fi
elif [ -n "$BRANCH" ]; then
    sanitized=$(sanitize_branch_name "$BRANCH")
    TARGET_DIR="$TARGET_ROOT/$sanitized"
    if [ -d "$TARGET_DIR" ]; then
        SIZE=$(du -sh "$TARGET_DIR" | cut -f1)
        rm -rf "$TARGET_DIR"
        echo ">>> cleaned: $BRANCH -> $TARGET_DIR ($SIZE freed)"
    else
        echo ">>> target dir not found: $TARGET_DIR"
    fi
else
    usage >&2
    exit 1
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
