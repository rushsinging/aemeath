#!/usr/bin/env bash
# GitHub Repository Rulesets 一键同步脚本。
#
# Rulesets 是 GitHub 服务端规则（非仓库文件），push/merge 时由 GitHub 强制检查。
# 本脚本将 .github/rulesets/*.json 的定义同步到仓库，作为 branch protection 配置的
# 唯一真相源（single source of truth）。新仓库初始化或重置规则时执行一次即可。
#
# 用法：
#   bash .github/rulesets/setup.sh          # 同步所有 ruleset
#   bash .github/rulesets/setup.sh --dry    # 只打印 gh 命令，不执行
#
# 前置：gh 已登录且有仓库 admin 权限。
#
# 与旧 branch protection 的关系：Rulesets 优先级高于 branch protection。
# 迁移时建议先删除同名 branch protection，再创建 ruleset（本脚本不自动删除旧 protection）。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
REPO="${GITHUB_REPOSITORY:-rushsinging/aemeath}"
DRY_RUN=0

if [[ "${1:-}" == "--dry" ]]; then
  DRY_RUN=1
fi

echo "[ruleset] target repo: $REPO"
echo "[ruleset] definitions dir: $SCRIPT_DIR"

shopt -s nullglob
json_files=("$SCRIPT_DIR"/*.json)

if [ ${#json_files[@]} -eq 0 ]; then
  echo "[ruleset] no ruleset JSON found in $SCRIPT_DIR"
  exit 0
fi

for json_file in "${json_files[@]}"; do
  name=$(python3 -c "import json,sys; print(json.load(open('$json_file'))['name'])")
  echo
  echo "=== syncing ruleset: $name ==="
  echo "    source: ${json_file#$ROOT/}"

  # 查找已有 ruleset（按 name 匹配）
  existing_id=$(gh api "repos/$REPO/rulesets" 2>/dev/null \
    | python3 -c "
import json, sys
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(0)
for r in data:
    if r.get('name') == '$name':
        print(r['id'])
        break
" 2>/dev/null || true)

  if [ -n "$existing_id" ]; then
    echo "    existing ruleset id: $existing_id → PUT update"
    cmd=(gh api "repos/$REPO/rulesets/$existing_id" -X PUT --input "$json_file")
  else
    echo "    no existing ruleset → POST create"
    cmd=(gh api "repos/$REPO/rulesets" --input "$json_file")
  fi

  if [ "$DRY_RUN" -eq 1 ]; then
    echo "    [dry-run] ${cmd[*]}"
  else
    "${cmd[@]}" >/dev/null
    echo "    ✓ synced"
  fi
done

echo
echo "[ruleset] done. 查看: gh api repos/$REPO/rulesets"
