#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

TUI_DIR="$ROOT/apps/cli/src/tui"
allowed='^(adapter|app|effect|model|render|update|view_assembler|view_model|view_state)$'
fail=0

while IFS= read -r -d '' dir; do
  name="$(basename "$dir")"
  if [[ ! "$name" =~ $allowed ]]; then
    echo "[architecture] forbidden tui top-level directory: apps/cli/src/tui/$name" >&2
    echo "[architecture] place it under the spec layers: adapter/app/effect/model/render/update/view_assembler/view_model/view_state" >&2
    fail=1
  fi
done < <(find "$TUI_DIR" -mindepth 1 -maxdepth 1 -type d -print0)

# Guard old physical namespaces removed by feature #57.
if grep -RInE 'crate::tui::(core|output_area|input|display|completion|session)|tui::(core|output_area|input|display|completion|session)' \
  "$TUI_DIR" --include='*.rs' >/tmp/aemeath-tui-layout-imports.$$ 2>/dev/null; then
  if [ -s /tmp/aemeath-tui-layout-imports.$$ ]; then
    cat /tmp/aemeath-tui-layout-imports.$$ >&2
    echo "[architecture] old tui top-level module paths are forbidden after feature #57" >&2
    fail=1
  fi
fi
rm -f /tmp/aemeath-tui-layout-imports.$$

exit "$fail"
