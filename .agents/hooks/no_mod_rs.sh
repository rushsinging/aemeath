#!/usr/bin/env bash
# no_mod_rs.sh — 架构 guard：检测项目中新增的 mod.rs 文件
# 用法：
#   ./no_mod_rs.sh          # 检查所有 .rs 源文件
#   ./no_mod_rs.sh --diff   # 仅检查 git 暂存区新增的文件
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$script_dir/../.." && pwd)}"
# 守卫：如果 AEMEATH_PROJECT_DIR 不含 .agents/hooks，回退到 BASH_SOURCE 推导。
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$script_dir/../.." && pwd)"
fi

mode="${1:-all}"

found=0

if [ "$mode" = "--diff" ]; then
  # 仅检查 git 暂存区中新增的 mod.rs
  while IFS= read -r f; do
    basename_f="$(basename "$f")"
    if [ "$basename_f" = "mod.rs" ]; then
      echo "ERROR: 新增 mod.rs 文件: $f" >&2
      found=1
    fi
  done < <(git -C "$ROOT" diff --cached --name-only --diff-filter=A -- '*.rs')
else
  # 检查所有 mod.rs
  while IFS= read -r f; do
    echo "ERROR: 发现 mod.rs 文件: $f" >&2
    found=1
  done < <(find "$ROOT" -type d -name '.worktrees' -prune -o -path '*/src/*' -name 'mod.rs' -not -path '*/.claude/*' -not -path '*/target/*' -print)
fi

if [ "$found" -ne 0 ]; then
  echo "" >&2
  echo "Rust 2018+ 推荐使用与目录同名的文件代替 mod.rs：" >&2
  echo "  foo/mod.rs → foo.rs（foo/ 子目录保留）" >&2
  echo "详见 AGENTS.md 架构约定。" >&2
  exit 1
fi

echo "OK: 未发现 mod.rs 文件"
