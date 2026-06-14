#!/bin/bash
# 拒绝在 main 工作区直接使用 Edit/Write 工具修改文件。
# 仅在 agent 处于 git worktree 中时允许 Edit/Write。

set -euo pipefail

if [ "${AEMEATH_IN_WORKTREE:-0}" = "1" ]; then
    exit 0
fi

cat >&2 <<'ERR'
Error: Edit/Write is forbidden on the main workspace.
All file modifications must be done in a git worktree.
Please create a worktree and retry.
ERR

exit 2
