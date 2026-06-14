#!/bin/bash
# 拒绝在 main 工作区直接使用 Edit/Write 工具修改文件。
# 仅在 agent 处于 git worktree 中时允许 Edit/Write。

set -euo pipefail

if [ "${AEMEATH_IN_WORKTREE:-0}" = "1" ]; then
    exit 0
fi

current_branch=$(git branch --show-current 2>/dev/null || echo "unknown")
project_dir=$(git rev-parse --show-toplevel 2>/dev/null || echo "${AEMEATH_PROJECT_DIR:-$(pwd)}")

cat >&2 <<ERR
[Hook blocked] Edit/Write rejected.

Reason:
  You are currently on the main workspace (branch: ${current_branch}).
  According to AGENTS.md, all file modifications MUST be done in an isolated
git worktree, NEVER directly on the main workspace.

How to fix:
  1. Create a worktree for your change:
       git worktree add .worktrees/<branch-name> -b <branch-name>
  2. Switch to the worktree (cd .worktrees/<branch-name>).
  3. Re-run the Edit/Write tool from the worktree.
  4. After finishing, commit, push, and create a PR from the worktree.

Project directory: ${project_dir}
Current branch:    ${current_branch}
ERR

exit 2
