#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
TARGET="$ROOT/apps/cli/src/tui"
if [[ ! -d "$TARGET" ]]; then
  echo "ERROR: target directory not found: $TARGET" >&2
  echo "Run this script from the repository checkout; expected TUI sources under apps/cli/src/tui." >&2
  exit 2
fi

FAILED=0
COUNT=0

while IFS= read -r -d '' file; do
    rel="${file#$ROOT/}"
    case "$rel" in
      apps/cli/src/tui/render/display/safe_text.rs)
          continue
          ;;
      apps/cli/src/tui/display/safe_text.rs)
          continue
          ;;
    esac

    while IFS=: read -r line_no line; do
      if [[ "$line" == *"allow unsafe_text_op"* ]]; then
        continue
      fi
      if [[ "$line" =~ ^[[:space:]]*#\!?\[ ]]; then
        continue
      fi
      printf 'unsafe text op: %s:%s:%s\n' "$rel" "$line_no" "$line"
      FAILED=1
      COUNT=$((COUNT + 1))
    done < <(
      perl -ne '
              print "$.:$_" if /\.chars\(\)\.nth\(/;
              print "$.:$_" if /&\s*[A-Za-z_][A-Za-z0-9_]*\s*\[[^\]]*\.\.[^\]]*\]/;
              print "$.:$_" if /[A-Za-z_][A-Za-z0-9_]*\s*\[\s*[A-Za-z_][A-Za-z0-9_]*\s*\.\.\s*[A-Za-z_][A-Za-z0-9_]*\s*\]/;
              print "$.:$_" if /[A-Za-z_][A-Za-z0-9_]*\s*\[\s*[A-Za-z_][A-Za-z0-9_]*\s*\]/ && /allow unsafe_text_op/;
            ' "$file"
    )
done < <(find "$TARGET" -name '*.rs' -print0)

if [[ "$COUNT" -eq 0 ]]; then
  echo "Unsafe TUI text/index operations found (0)."
fi

if [[ "$FAILED" -ne 0 ]]; then
  echo "Unsafe TUI text/index operations found ($COUNT). Use crate::tui::safe_text helpers or add an explicit allow unsafe_text_op comment for ASCII-only cases."
  exit 1
fi
