#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
LIMIT="${AEMEATH_RS_LINE_LIMIT:-400}"
FAILED=0
COUNT=0
DETAILS=()

SOURCE_ROOTS=(
  "$ROOT/apps"
  "$ROOT/crates"
)

EXISTING_ROOTS=()
for source_root in "${SOURCE_ROOTS[@]}"; do
  if [[ -d "$source_root" ]]; then
    EXISTING_ROOTS+=("$source_root")
  fi
done

if [[ "${#EXISTING_ROOTS[@]}" -eq 0 ]]; then
  echo "ERROR: expected DDD source roots under apps/ or agent/." >&2
  exit 2
fi

while IFS= read -r -d '' file; do
  rel="${file#$ROOT/}"
  lines="$(wc -l < "$file" | tr -d ' ')"
  if (( lines > LIMIT )); then
    message="rust file too large: $rel has $lines lines (limit $LIMIT)"
    printf '%s\n' "$message" >&2
    DETAILS+=("$message")
    FAILED=1
    COUNT=$((COUNT + 1))
  fi
done < <(find "${EXISTING_ROOTS[@]}" -name '*.rs' -print0)
if [[ "$FAILED" -ne 0 ]]; then
  summary="Rust file line limit exceeded ($COUNT). Split files to keep each .rs <= $LIMIT lines."
  reason="$summary"
  for detail in "${DETAILS[@]}"; do
    reason+=$'\n'"$detail"
  done
  python3 -c 'import json, sys; print(json.dumps({"decision":"block","reason":sys.argv[1]}, ensure_ascii=False))' "$reason"
  exit 2
fi

echo "Rust file line limit OK (<= $LIMIT lines)."
