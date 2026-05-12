#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIMIT="${AEMEATH_RS_LINE_LIMIT:-400}"
FAILED=0
COUNT=0

while IFS= read -r -d '' file; do
  rel="${file#$ROOT/}"
  lines="$(wc -l < "$file" | tr -d ' ')"
  if (( lines > LIMIT )); then
    printf 'rust file too large: %s has %s lines (limit %s)\n' "$rel" "$lines" "$LIMIT"
    FAILED=1
    COUNT=$((COUNT + 1))
  fi
done < <(find "$ROOT" -path "$ROOT/target" -prune -o -path "$ROOT/.git" -prune -o -name '*.rs' -print0)

if [[ "$FAILED" -ne 0 ]]; then
  echo "Rust file line limit exceeded ($COUNT). Split files to keep each .rs <= $LIMIT lines."
  exit 1
fi

echo "Rust file line limit OK (<= $LIMIT lines)."
