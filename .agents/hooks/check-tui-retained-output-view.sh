#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

fail=0
app_update="$ROOT/apps/cli/src/tui/app/update.rs"
assembler="$ROOT/apps/cli/src/tui/view_assembler/output.rs"
retained="$ROOT/apps/cli/src/tui/view_assembler/retained_output_view.rs"
journal="$ROOT/apps/cli/src/tui/model/conversation/output_view_change.rs"

if grep -nE 'assemble_from_conversation\(' "$app_update"; then
  echo "[architecture] TUI production refresh must consume RetainedOutputView, not full conversation assembly" >&2
  fail=1
fi

if ! grep -q 'RetainedOutputView' "$retained" || ! grep -q '\.sync(&self\.model\.conversation' "$app_update"; then
  echo "[architecture] RetainedOutputView must remain the production output view owner" >&2
  fail=1
fi

if ! grep -q '#\[cfg(test)\]' "$assembler" || ! grep -q 'pub fn assemble_from_conversation' "$assembler"; then
  echo "[architecture] full output assembly must remain a test-only semantic reference" >&2
  fail=1
fi

if ! grep -q 'OUTPUT_VIEW_JOURNAL_CAPACITY' "$journal" || ! grep -q 'pop_front' "$journal"; then
  echo "[architecture] output view journal must remain explicitly bounded" >&2
  fail=1
fi

if grep -RInE 'OutputProjection|output_projection|OutputViewCache' \
  "$ROOT/apps/cli/src/tui" --include='*.rs'; then
  echo "[architecture] retired output projection/cache naming must not return" >&2
  fail=1
fi

exit "$fail"
