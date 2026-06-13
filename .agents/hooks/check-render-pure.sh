#!/usr/bin/env bash
set -euo pipefail

tmp="${TMPDIR:-/tmp}/aemeath-render-domain-guard.txt"
: > "$tmp"

if grep -R "model\.conversation\|model\.runtime" apps/cli/src/tui/render --include='*.rs' | grep -v "src/tui/render/.*/.*_tests.rs" | grep -v "src/tui/render/.*_tests.rs" | grep -v "src/tui/render/display/render.rs" >> "$tmp"; then
  echo "render layer must not read conversation/runtime domain model directly; use view models/view_state projections" >&2
  cat "$tmp" >&2
  exit 1
fi
