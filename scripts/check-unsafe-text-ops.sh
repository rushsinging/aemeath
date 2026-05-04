#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$ROOT/aemeath-cli/src/tui"
FAILED=0

while IFS= read -r -d '' file; do
  rel="${file#$ROOT/}"
  case "$rel" in
    aemeath-cli/src/tui/safe_text.rs)
      continue
      ;;
  esac

  while IFS=: read -r line_no line; do
    if [[ "$line" == *"allow unsafe_text_op"* ]]; then
      continue
    fi
    printf 'unsafe text op: %s:%s:%s\n' "$rel" "$line_no" "$line"
    FAILED=1
  done < <(
    perl -ne '
      print "$.:$_" if /\.chars\(\)\.nth\(/;
      print "$.:$_" if /chars\s*\[[^\]]*\.\.[^\]]*\]/;
      print "$.:$_" if /\.split_off\s*\(/;
      print "$.:$_" if /&\s*[A-Za-z_][A-Za-z0-9_]*\s*\[[^\]]*\.\.[^\]]*\]/;
    ' "$file"
  )
done < <(find "$TARGET" -name '*.rs' -print0)

if [[ "$FAILED" -ne 0 ]]; then
  echo "Unsafe TUI text/index operations found. Use crate::tui::safe_text helpers or add an explicit allow unsafe_text_op comment for ASCII-only cases."
  exit 1
fi
