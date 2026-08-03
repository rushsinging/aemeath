#!/bin/bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
export ROOT
# 扫描整个 CLI app（不止 tui）：工具调用 header / 摘要等字符串处理也可能落在非 tui 路径。
TARGET="$ROOT/apps/cli/src"
if [[ ! -d "$TARGET" ]]; then
  echo "ERROR: target directory not found: $TARGET" >&2
  echo "Run this script from the repository checkout; expected CLI sources under apps/cli/src." >&2
  exit 2
fi

FAILED=0
COUNT=0

while IFS=: read -r rel line_no line; do
      # guard-registry:false-positive.tui.unsafe-text-inline-allow
      if [[ "$line" == *"allow unsafe_text_op"* ]]; then
        continue
      fi
      if [[ "$line" =~ ^[[:space:]]*#\!?\[ ]]; then
        continue
      fi
      # guard-registry:scope.tui.safe-text-owners
      case "$rel" in
        apps/cli/src/tui/render/display/safe_text.rs|apps/cli/src/tui/display/safe_text.rs|apps/cli/src/tui/text.rs)
            continue
            ;;
      esac
      printf 'unsafe text op: %s:%s:%s\n' "$rel" "$line_no" "$line"
      FAILED=1
      COUNT=$((COUNT + 1))
    done < <(
      # perl 单进程批量处理全部文件（$ARGV 携带文件名），避免逐文件 fork perl——
      # 实测从 4.4s 降到 ~0.3s。
      find "$TARGET" -name '*.rs' -print0 | xargs -0 perl -ne '
              my $rel = $ARGV;
              $rel =~ s/^\Q$ENV{ROOT}\E\///;
              if (/\.chars\(\)\.nth\(/ || /&\s*[A-Za-z_][A-Za-z0-9_]*\s*\[[^\]]*\.\.[^\]]*\]/ || /[A-Za-z_][A-Za-z0-9_]*\s*\[\s*[A-Za-z_][A-Za-z0-9_]*\s*\.\.\s*[A-Za-z_][A-Za-z0-9_]*\s*\]/ || (/[A-Za-z_][A-Za-z0-9_]*\s*\[\s*[A-Za-z_][A-Za-z0-9_]*\s*\]/ && /allow unsafe_text_op/) || /\.split_at\(/) {
                print "$rel:$.:$_";
              }
if (eof) { close ARGV; }
                          '
    )

if [[ "$COUNT" -eq 0 ]]; then
  echo "Unsafe CLI text/index operations found (0)."
fi

if [[ "$FAILED" -ne 0 ]]; then
  echo "Unsafe CLI text/index operations found ($COUNT). Use safe helpers: strip_prefix/strip_suffix, get() with Option, floor_char_boundary(), split_at_ascii(). For Vec slice (not str slice), add explicit 'allow unsafe_text_op: Vec slice' comment. NEVER 用「输入是 ASCII」搪塞动态文本。"
  exit 1
fi
