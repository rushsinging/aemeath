#!/usr/bin/env bash
# 清理 worktree 开发模式下的 Cargo target 产物（#1226）。
#
# 默认策略：
#   1. 删除各 checkout/worktree 内遗留的 target/
#   2. prune Git 已失效的 worktree 元数据
#   3. 删除 ~/.cache/aemeath-target 中不属于活跃 worktree 的新格式缓存
#      （旧版分支名缓存无法可靠反查来源，只报告为 legacy/unmanaged）
#   4. 报告共享缓存是否超过预算；绝不为满足预算删除活跃 worktree 缓存
#
# 用法：
#   ./scripts/clean-worktree-targets.sh
#   ./scripts/clean-worktree-targets.sh --dry-run
#   ./scripts/clean-worktree-targets.sh --yes
#   ./scripts/clean-worktree-targets.sh --keep-current
#   ./scripts/clean-worktree-targets.sh --current --yes
#   ./scripts/clean-worktree-targets.sh --max-size-gb 50

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly SHARED_CACHE="${HOME}/.cache/aemeath-target"

# shellcheck source=../.cargo/lib.sh
source "$ROOT/.cargo/lib.sh"

DRY_RUN=0
ASSUME_YES=0
KEEP_CURRENT=0
CURRENT_ONLY=0
MAX_SIZE_GB=50

usage() {
  sed -n '2,16p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --yes|-y) ASSUME_YES=1; shift ;;
    --keep-current) KEEP_CURRENT=1; shift ;;
    --current) CURRENT_ONLY=1; shift ;;
    --max-size-gb)
      [[ $# -ge 2 && "$2" =~ ^[0-9]+$ ]] || { echo "--max-size-gb 需要非负整数" >&2; exit 1; }
      MAX_SIZE_GB="$2"; shift 2 ;;
    -h|--help) usage 0 ;;
    *) echo "unknown arg: $1" >&2; usage 1 ;;
  esac
done

cd "$ROOT"

human_size() {
  [[ -e "$1" ]] || return 0
  du -sh "$1" 2>/dev/null | awk '{print $1}'
}

dir_size_kb() {
  [[ -d "$1" ]] || { echo 0; return; }
  du -sk "$1" 2>/dev/null | awk '{print $1}'
}

run_rm() {
  local target="$1"
  if [[ $DRY_RUN -eq 1 ]]; then
    echo "  [dry-run] would remove: $target"
  elif rm -rf "$target" 2>/dev/null || rm -rf "$target" 2>/dev/null; then
    echo "  removed: $target"
  else
    echo "  WARN: failed to remove (in use?): $target" >&2
  fi
}

mapfile_compat() {
  local line
  while IFS= read -r line; do ACTIVE_WORKTREES+=("$line"); done
}

ACTIVE_WORKTREES=()
mapfile_compat < <(git worktree list --porcelain | awk '/^worktree / {sub(/^worktree /, ""); print}')
current_worktree="$(git rev-parse --show-toplevel 2>/dev/null || true)"

if [[ $CURRENT_ONLY -eq 1 ]]; then
  [[ $ASSUME_YES -eq 1 ]] || {
    echo "--current 只能与 --yes 一起使用，避免误删当前构建缓存" >&2
    exit 1
  }
  current_cache="$SHARED_CACHE/$(worktree_cache_key "$current_worktree")"
  echo "==> 清理当前 worktree 构建缓存"
  if [[ -d "$current_cache" ]]; then
    echo "  current: $current_cache ($(human_size "$current_cache"))"
    run_rm "$current_cache"
  else
    echo "  no current cache: $current_cache"
  fi
  exit 0
fi

echo "==> 1/4 清理 checkout/worktree 内遗留 target/"
for wt_path in "${ACTIVE_WORKTREES[@]}"; do
  target="$wt_path/target"
  [[ -d "$target" ]] || continue
  if [[ $KEEP_CURRENT -eq 1 && "$wt_path" == "$current_worktree" ]]; then
    echo "  keep (current): $target ($(human_size "$target"))"
    continue
  fi
  echo "  legacy target: $target ($(human_size "$target"))"
  run_rm "$target"
done

echo
echo "==> 2/4 清理失效 worktree 元数据"
if [[ $DRY_RUN -eq 1 ]]; then
  git worktree prune --dry-run 2>/dev/null || true
else
  git worktree prune
  echo "  pruned stale worktrees"
fi

echo
echo "==> 3/4 清理非活跃 worktree 缓存"
ACTIVE_KEYS=()
for wt_path in "${ACTIVE_WORKTREES[@]}"; do
  ACTIVE_KEYS+=("$(worktree_cache_key "$wt_path")")
done

is_active_key() {
  local candidate="$1" key
  for key in "${ACTIVE_KEYS[@]}"; do
    [[ "$candidate" == "$key" ]] && return 0
  done
  return 1
}

ORPHANS=()
if [[ -d "$SHARED_CACHE" ]]; then
  shopt -s nullglob
  for cache_dir in "$SHARED_CACHE"/*; do
    [[ -d "$cache_dir" ]] || continue
    cache_key="$(basename "$cache_dir")"
    if is_active_key "$cache_key"; then
      echo "  keep (active): $cache_key ($(human_size "$cache_dir"))"
    elif [[ "$cache_key" =~ -[0-9a-f]{16}$ ]]; then
      ORPHANS+=("$cache_dir")
      echo "  orphan: $cache_key ($(human_size "$cache_dir"))"
    else
      echo "  keep (legacy/unmanaged): $cache_key ($(human_size "$cache_dir"))"
    fi
  done
  shopt -u nullglob
else
  echo "  (共享缓存目录不存在，跳过)"
fi

if [[ ${#ORPHANS[@]} -gt 0 ]]; then
  if [[ $DRY_RUN -eq 0 && $ASSUME_YES -ne 1 ]]; then
    printf "  删除以上 %d 个孤儿缓存？(y/N) " "${#ORPHANS[@]}"
    read -r answer
    if [[ "$answer" != "y" && "$answer" != "Y" ]]; then
      echo "  跳过孤儿缓存清理"
      ORPHANS=()
    fi
  fi
  for cache_dir in "${ORPHANS[@]}"; do run_rm "$cache_dir"; done
else
  echo "  no orphan caches"
fi

echo
echo "==> 4/4 检查共享缓存预算"
size_kb="$(dir_size_kb "$SHARED_CACHE")"
limit_kb=$((MAX_SIZE_GB * 1024 * 1024))
if (( size_kb > limit_kb )); then
  echo "  WARN: 共享缓存约 $((size_kb / 1024 / 1024)) GiB，超过预算 ${MAX_SIZE_GB} GiB。" >&2
  echo "  活跃 worktree 缓存不会自动删除；请移除不用的 worktree 后重新运行清理。" >&2
else
  echo "  within budget: $(human_size "$SHARED_CACHE") / ${MAX_SIZE_GB}G"
fi

if [[ $DRY_RUN -eq 1 ]]; then
  echo
  echo "[dry-run] 未删除任何文件。去掉 --dry-run 执行实际清理。"
fi
