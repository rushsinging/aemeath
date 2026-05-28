#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
FAILED=0
COUNT=0

TARGET_DIRS=(
  "apps/cli/src/tui/model"
  "apps/cli/src/tui/update"
)

for dir in "${TARGET_DIRS[@]}"; do
  target="$ROOT/$dir"
  if [[ ! -d "$target" ]]; then
    continue
  fi

  while IFS= read -r -d '' file; do
    rel="${file#$ROOT/}"
    while IFS=: read -r line_no line; do
      printf 'TUI effect boundary violation: %s:%s:%s\n' "$rel" "$line_no" "$line"
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
        print "$.:$_" if /\bHandle::block_on\s*\(|\bRuntime::block_on\s*\(/;
        print "$.:$_" if /block_in_place\b/;
        print "$.:$_" if /\.await\b/;
        print "$.:$_" if /mpsc::Sender/;
      ' "$file"
    )
  done < <(find "$target" -name '*.rs' -print0)
done

if [[ "$FAILED" -ne 0 ]]; then
  echo "TUI model/update must describe side effects as Effect values instead of executing them directly ($COUNT)." >&2
  exit 1
fi

echo "TUI effect boundary OK."
