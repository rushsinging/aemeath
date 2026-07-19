#!/usr/bin/env bash
# 清理 worktree 开发模式下的 cargo target 产物堆积（#1226）。
#
# 三类清理：
#   1. 各 worktree 内的 target/（cargo 默认产物，可随时重建）
#   2. 僵尸 worktree（gitdir 失效或分支已合并进 main）
#   3. ~/.cache/aemeath-target/ 中不活跃分支的按分支缓存
#
# 用法：
#   ./scripts/clean-worktree-targets.sh            # 交互确认后清理
#   ./scripts/clean-worktree-targets.sh --dry-run  # 只列出待删项，不执行
#   ./scripts/clean-worktree-targets.sh --yes      # 跳过确认直接清理
#   ./scripts/clean-worktree-targets.sh --keep-current  # 保留当前 worktree 的 target
#
# --keep-current 同时保留 main 与当前所在 worktree 的 target，便于继续开发。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly SHARED_CACHE="${HOME}/.cache/aemeath-target"

DRY_RUN=0
ASSUME_YES=0
KEEP_CURRENT=0

usage() {
  sed -n '2,16p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --yes|-y) ASSUME_YES=1; shift ;;
    --keep-current) KEEP_CURRENT=1; shift ;;
    -h|--help) usage 0 ;;
    *) echo "unknown arg: $1" >&2; usage 1 ;;
  esac
done

# 进入主工作区以执行 git worktree 命令。
cd "$ROOT"

human_size() {
  if [[ ! -d "$1" ]]; then return; fi
  # du -sh 在 macOS/BSD 与 GNU 行为一致 enough。
  du -sh "$1" 2>/dev/null | awk '{print $1}'
}

run_rm() {
  local target="$1"
  if [[ $DRY_RUN -eq 1 ]]; then
    echo "  [dry-run] would remove: $target"
  else
    # best-effort：macOS APFS 偶发 "Directory not empty"（并发写/FS 延迟），重试一次后仍失败则跳过。
    if rm -rf "$target" 2>/dev/null; then
      echo "  removed: $target"
    elif rm -rf "$target" 2>/dev/null; then
      echo "  removed (retry): $target"
    else
      echo "  WARN: failed to remove (in use?): $target" >&2
    fi
  fi
}

current_worktree=""
if [[ $KEEP_CURRENT -eq 1 ]]; then
  current_worktree="$(git rev-parse --show-toplevel 2>/dev/null || true)"
fi

echo "==> 1/3 清理 worktree 内 target/"
reclaimed_worktree=0
while IFS= read -r line; do
  # git worktree list 输出：<path> <sha> [<branch>]
  wt_path="$(printf '%s' "$line" | awk '{print $1}')"
  [[ -n "$wt_path" ]] || continue
  target="$wt_path/target"
  [[ -d "$target" ]] || continue
  if [[ $KEEP_CURRENT -eq 1 && "$wt_path" == "$current_worktree" ]]; then
    echo "  keep (current): $target ($(human_size "$target"))"
    continue
  fi
  echo "  target: $target ($(human_size "$target"))"
  run_rm "$target"
  reclaimed_worktree=1
done < <(git worktree list --porcelain | awk '/^worktree / {print $2}')

echo
echo "==> 2/3 清理僵尸 worktree（gitdir 失效）"
git worktree prune --dry-run 2>/dev/null | while IFS= read -r stale; do
  echo "  stale: $stale"
done
if [[ $DRY_RUN -eq 1 ]]; then
  git worktree prune --dry-run >/dev/null 2>&1 || true
else
  git worktree prune
  echo "  pruned stale worktrees"
fi

echo
echo "==> 3/3 清理共享缓存 $SHARED_CACHE 中不活跃分支"
if [[ -d "$SHARED_CACHE" ]]; then
  if [[ $DRY_RUN -eq 1 ]]; then
    du -sh "$SHARED_CACHE"/* 2>/dev/null | sort -rh || true
  else
    if [[ $ASSUME_YES -ne 1 ]]; then
      echo "  共享缓存内容："
      du -sh "$SHARED_CACHE"/* 2>/dev/null | sort -rh || true
      printf "  清空整个共享缓存？(y/N) "
      read -r answer
      [[ "$answer" == "y" || "$answer" == "Y" ]] || { echo "  跳过共享缓存清理"; exit 0; }
    fi
    rm -rf "${SHARED_CACHE:?}"/*
    echo "  cleared $SHARED_CACHE"
  fi
else
  echo "  (共享缓存目录不存在，跳过)"
fi

if [[ $DRY_RUN -eq 1 ]]; then
  echo
  echo "[dry-run] 未删除任何文件。去掉 --dry-run 执行实际清理。"
fi
