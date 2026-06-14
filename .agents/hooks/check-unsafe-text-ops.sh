#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
# 扫描整个 CLI app（不止 tui）：工具调用 header / 摘要等字符串处理也可能落在非 tui 路径。
TARGET="$ROOT/apps/cli/src"
if [[ ! -d "$TARGET" ]]; then
  echo "ERROR: target directory not found: $TARGET" >&2
  echo "Run this script from the repository checkout; expected CLI sources under apps/cli/src." >&2
  exit 2
fi

FAILED=0
COUNT=0

while IFS= read -r -d '' file; do
    rel="${file#$ROOT/}"
    case "$rel" in
      apps/cli/src/tui/render/display/safe_text.rs)
          continue
          ;;
      apps/cli/src/tui/display/safe_text.rs)
          continue
          ;;
    esac

    # 检测会因「字节偏移落在非 char 边界」而 panic 的文本操作：
    #   chars-nth（字符索引误当字节）、&var[a..b] / var[a..b]（&str/String 字节切片）、
    #   split_at（str::split_at 在非 char 边界 panic）。
    # 刻意不检测 get-range（返回 Option 不 panic，是 safe_text 推荐用法，flag 会误伤）
    # 与 truncate（本仓库内均为 Vec::truncate，flag 会产生误导性注解）。
    while IFS=: read -r line_no line; do
      if [[ "$line" == *"allow unsafe_text_op"* ]]; then
        continue
      fi
      if [[ "$line" =~ ^[[:space:]]*#\!?\[ ]]; then
        continue
      fi
      printf 'unsafe text op: %s:%s:%s\n' "$rel" "$line_no" "$line"
      FAILED=1
      COUNT=$((COUNT + 1))
    done < <(
      perl -ne '
              print "$.:$_" if /\.chars\(\)\.nth\(/;
              print "$.:$_" if /&\s*[A-Za-z_][A-Za-z0-9_]*\s*\[[^\]]*\.\.[^\]]*\]/;
              print "$.:$_" if /[A-Za-z_][A-Za-z0-9_]*\s*\[\s*[A-Za-z_][A-Za-z0-9_]*\s*\.\.\s*[A-Za-z_][A-Za-z0-9_]*\s*\]/;
              print "$.:$_" if /[A-Za-z_][A-Za-z0-9_]*\s*\[\s*[A-Za-z_][A-Za-z0-9_]*\s*\]/ && /allow unsafe_text_op/;
              print "$.:$_" if /\.split_at\(/;
            ' "$file"
    )
done < <(find "$TARGET" -name '*.rs' -print0)

if [[ "$COUNT" -eq 0 ]]; then
  echo "Unsafe CLI text/index operations found (0)."
fi

if [[ "$FAILED" -ne 0 ]]; then
  echo "Unsafe CLI text/index operations found ($COUNT). Use crate::tui::render::display::safe_text helpers (truncate_ellipsis / truncate_unicode_width / safe_byte_prefix / safe_str_slice_by_char), or add an explicit 'allow unsafe_text_op: <结构性边界理由>' comment when the offsets are provably on char boundaries (e.g. derived from an ASCII delimiter search). NEVER 用「输入是 ASCII」搪塞动态文本。"
  exit 1
fi
