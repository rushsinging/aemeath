#!/usr/bin/env bash
# 单个 .rs 文件不超过 400 行（含测试），扫描 apps/ 与 agent/，排除 target/.git/.worktrees
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LIMIT=400
violations=0

while IFS= read -r f; do
  lines=$(wc -l < "$f")
  if [ "$lines" -gt "$LIMIT" ]; then
    echo "  $f: $lines 行 (> $LIMIT)"
    violations=$((violations+1))
  fi
done < <(find "$ROOT/apps" "$ROOT/agent" -type f -name '*.rs' \
          -not -path '*/target/*' -not -path '*/.git/*' -not -path '*/.worktrees/*')

if [ "$violations" -gt 0 ]; then
  echo "Rust file-lines guard FAILED: $violations 个文件超过 $LIMIT 行。"
  exit 1
fi

echo "Rust file-lines guard OK."
