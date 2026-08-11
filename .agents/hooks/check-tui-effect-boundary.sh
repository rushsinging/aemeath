#!/bin/bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
export ROOT
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

  while IFS=: read -r rel line_no line; do
      printf 'TUI effect boundary violation: %s:%s:%s\n' "$rel" "$line_no" "$line"
      FAILED=1
      COUNT=$((COUNT + 1))
    done < <(
      # perl 单进程批量处理全部文件（$ARGV 携带文件名），避免逐文件 fork perl
      find "$target" -name '*.rs' -print0 | xargs -0 perl -ne '
        my $rel = $ARGV;
        $rel =~ s/^\Q$ENV{ROOT}\E\///;
        if (/tokio::spawn\s*\(/ || /std::thread::spawn\s*\(/ || /Command::new\s*\(/ || /HookRunner::run|\.run_hook\s*\(/ || /clipboard::|arboard::|copypasta::/ || /read_clipboard_image\s*\(/ || /process_image_file\s*\(/ || /\bHandle::block_on\s*\(|\bRuntime::block_on\s*\(/ || /block_in_place\b/ || /\.await\b/ || /mpsc::Sender/) {
          print "$rel:$.:$_";
        }
if (eof) { close ARGV; }
                    '
    )
done

if [[ "$FAILED" -ne 0 ]]; then
  echo "TUI model/update must describe side effects as Effect values instead of executing them directly ($COUNT)." >&2
  exit 1
fi

echo "TUI effect boundary OK."
