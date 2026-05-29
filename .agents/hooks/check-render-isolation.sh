#!/usr/bin/env bash
set -euo pipefail

# Render isolation guard（#58 输出区单一真相管线）
# 扫描 apps/cli/src/tui/render/output/，确保渲染层保持纯函数边界：
#   1) 不引用 Model 可变类型（use crate::tui::model::...）；引用 view_model:: 允许。
#   2) 不做 IO（std::fs::/std::process::/tokio::）——排除注释与 #[cfg(test)] 测试代码。
#   3) 选区上色唯一路径：除 selection_overlay.rs 外，render/output/ 下不得出现 SELECTION_BG
#      （防 #61/#62 旁路上色回归）。测试断言行（assert*）豁免。

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

TARGET="$ROOT/apps/cli/src/tui/render/output"
FAILED=0
COUNT=0

if [[ ! -d "$TARGET" ]]; then
  echo "render isolation: 目标目录不存在 $TARGET" >&2
  exit 1
fi

while IFS= read -r -d '' file; do
  rel="${file#$ROOT/}"
  base="$(basename "$file")"
  while IFS=: read -r line_no line; do
    printf 'render isolation violation: %s:%s:%s\n' "$rel" "$line_no" "$line"
    FAILED=1
    COUNT=$((COUNT + 1))
  done < <(
    SELECTION_FILE="$base" perl -ne '
      # 进入 #[cfg(test)] 测试模块后停止扫描（测试代码豁免 IO/选区断言）。
      if (/^\s*#\[cfg\(test\)\]/) { $in_test = 1; }
      next if $in_test;

      # 跳过纯注释行（// 与 //!）。
      next if /^\s*\/\//;

      # 1) 禁止引用 Model 可变类型；view_model:: 允许。
      print "$.:$_" if /use\s+crate::tui::model::/;

      # 2) 禁止 IO。
      print "$.:$_" if /\bstd::fs::/;
      print "$.:$_" if /\bstd::process::/;
      print "$.:$_" if /\btokio::/;

      # 3) 选区上色唯一路径：仅 selection_overlay.rs 可用 SELECTION_BG；
      #    其它文件出现即违规（断言行 assert 豁免）。
      if ($ENV{SELECTION_FILE} ne "selection_overlay.rs") {
        print "$.:$_" if /SELECTION_BG/ && !/assert/;
      }
    ' "$file"
  )
done < <(find "$TARGET" -name '*.rs' -print0)

if [[ "$FAILED" -ne 0 ]]; then
  echo "[architecture] render/output 必须保持纯渲染：禁止 Model 可变依赖 / IO / 旁路选区上色（$COUNT 处）。" >&2
  exit 1
fi

echo "render isolation OK."
