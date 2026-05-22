#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
TARGET="$ROOT/cli/src/tui/app/update"
FAILED=0
COUNT=0

if [[ ! -d "$TARGET" ]]; then
  echo "ERROR: target directory not found: $TARGET" >&2
  exit 2
fi

while IFS= read -r -d '' file; do
  rel="${file#$ROOT/}"
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
    ' "$file"
  )
done < <(find "$TARGET" -path "$ROOT/.worktrees" -prune -o -name '*.rs' -print0)
if [[ "$FAILED" -ne 0 ]]; then
  echo "TUI update side effects found ($COUNT). Return Cmd variants from update() and execute side effects in app runtime/cmd_exec instead."
  exit 1
fi

echo "TUI update TEA purity OK."
