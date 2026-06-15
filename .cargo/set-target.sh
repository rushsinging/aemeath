#!/usr/bin/env bash
# 根据当前 git 分支设置 CARGO_TARGET_DIR，使不同分支/工作区拥有隔离的构建目录。
# 用法：source .cargo/set-target.sh

# Resolve script dir even when sourced ($0 is unreliable under source).
_self="${BASH_SOURCE[0]:-$0}"
_script_dir="$(cd "$(dirname "$_self")" && pwd)"

# shellcheck source=lib.sh
source "$_script_dir/lib.sh"

branch=$(git branch --show-current 2>/dev/null || echo "unknown")
sanitized=$(sanitize_branch_name "$branch")

export CARGO_TARGET_DIR="${HOME}/.cache/aemeath-target/${sanitized}"
