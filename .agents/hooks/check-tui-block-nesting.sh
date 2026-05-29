#!/usr/bin/env bash
set -euo pipefail

# gutter-ownership 不变量守卫（Task 4.2 建立）：
# gutter（marker/indent）只由渲染器 document_renderer.rs 经 apply_gutter 注入，
# block 组件的 render_self 绝不自写 gutter/marker/indent。
#
# 本守卫扫描 apps/cli/src/tui/render/output/blocks/*.rs，若任一 block 组件
# 调用 apply_gutter，则失败。这是高价值、无歧义的检查。
#
# 注意：marker 前缀检测（如硬编码 "● "/"> " 首 span）有意从简——
# thinking.rs(💭)、queued_submission.rs(⏳) 合法保留内容字形，
# ask_user/edit_diff 含内容内前缀，强行用正则检测易误报，故此处不做。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fi

TARGET_DIR="apps/cli/src/tui/render/output/blocks"
FAILED=0
COUNT=0

target="$ROOT/$TARGET_DIR"
if [[ -d "$target" ]]; then
  while IFS= read -r -d '' file; do
    rel="${file#$ROOT/}"
    while IFS=: read -r line_no line; do
      printf 'TUI block nesting violation: %s:%s:%s\n' "$rel" "$line_no" "$line"
      FAILED=1
      COUNT=$((COUNT + 1))
    done < <(
      perl -ne 'print "$.:$_" if /\bapply_gutter\s*\(/;' "$file"
    )
  done < <(find "$target" -name '*.rs' -print0)
fi

if [[ "$FAILED" -ne 0 ]]; then
  echo "block 组件禁止自写 gutter：apply_gutter 只能由 document_renderer.rs 注入 ($COUNT)。" >&2
  exit 1
fi

echo "TUI block nesting (gutter ownership) OK."
