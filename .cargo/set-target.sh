#!/usr/bin/env bash
# 根据当前 git 分支设置 CARGO_TARGET_DIR，使不同分支/工作区拥有隔离的构建目录。
# 用法：source .cargo/set-target.sh

branch=$(git branch --show-current 2>/dev/null || echo "unknown")
sanitized=$(printf '%s' "$branch" | tr '/\\ ' '_' | tr -cd 'A-Za-z0-9_.-')

export CARGO_TARGET_DIR="${HOME}/.cache/aemeath-target/${sanitized}"
