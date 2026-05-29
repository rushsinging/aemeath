#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
FAILED=0
COUNT=0

TUI_PURE_DIRS=(
  "apps/cli/src/tui/app"
  "apps/cli/src/tui/model"
  "apps/cli/src/tui/view_assembler"
  "apps/cli/src/tui/view_model"
)

# ---------------------------------------------------------------------------
# Exemption list: files in tui/app/ that are part of the runtime / command-
# execution layer and are expected to contain side effects (async ops,
# block_on, spawns, etc.).
#
# The strict TEA purity check applies to update/ and state/ subdirectories
# as well as pure-data modules (event.rs, msg.rs, resize.rs).
# ---------------------------------------------------------------------------
EXEMPT_FILES=(
  "apps/cli/src/tui/app/mod.rs"
  "apps/cli/src/tui/app/run_loop.rs"
  "apps/cli/src/tui/app/runtime.rs"
  "apps/cli/src/tui/app/session/processing.rs"
  "apps/cli/src/tui/app/session/session_lifecycle.rs"
  "apps/cli/src/tui/app/slash.rs"
  "apps/cli/src/tui/app/slash/dialog.rs"
  "apps/cli/src/tui/app/slash/help.rs"
  "apps/cli/src/tui/app/slash/help_display.rs"
  "apps/cli/src/tui/app/slash/memory.rs"
  "apps/cli/src/tui/app/slash/save.rs"
  "apps/cli/src/tui/app/slash/suggestions.rs"
  "apps/cli/src/tui/app/slash_tests.rs"
)

is_exempt() {
  local rel="$1"
  local f
  for f in "${EXEMPT_FILES[@]}"; do
    if [[ "$rel" == "$f" ]]; then
      return 0
    fi
  done
  return 1
}

for dir in "${TUI_PURE_DIRS[@]}"; do
  TARGET="$ROOT/$dir"
  if [[ ! -d "$TARGET" ]]; then
    continue
  fi

  while IFS= read -r -d '' file; do
    rel="${file#$ROOT/}"

    # Skip files in the exemption list (runtime / command-execution layer)
    if is_exempt "$rel"; then
      continue
    fi

    while IFS=: read -r line_no line; do
      if [[ "$line" == *"allow tea_side_effect"* ]]; then
        continue
      fi
      printf 'TUI update side effect: %s:%s:%s\n' "$rel" "$line_no" "$line"
      FAILED=1
      COUNT=$((COUNT + 1))
    done < <(
      perl -ne '
        print "$.:$_" if /tokio::spawn\s*\(/;
        print "$.:$_" if /std::thread::spawn\s*\(/;
        print "$.:$_" if /Command::new\s*\(/;
        print "$.:$_" if /HookRunner::run|\.run_hook\s*\(/;
        print "$.:$_" if /clipboard::|arboard::|copypasta::/;
        print "$.:$_" if /read_clipboard_image\s*\(/;
        print "$.:$_" if /process_image_file\s*\(/;
        # ── New patterns ──────────────────────────────────────────────
        print "$.:$_" if /\bHandle::block_on\s*\(|\bRuntime::block_on\s*\(/;
        print "$.:$_" if /block_in_place\b/;
        print "$.:$_" if /\.await\b/;
      ' "$file"
    )
  done < <(find "$TARGET" -name '*.rs' -print0)
done

if [[ "$FAILED" -ne 0 ]]; then
  echo "TUI update side effects found ($COUNT). Return Cmd variants from update() and execute side effects in app runtime/cmd_exec instead."
  exit 1
fi

echo "TUI update TEA purity OK."
