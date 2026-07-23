#!/bin/bash
# 拒绝在 main 工作区直接使用 Edit/Write 工具修改项目内代码。
# 仅在 agent 处于 git worktree 中时允许 Edit/Write。
# 项目外文件（如 ~/.agents/*.json）不受此 hook 约束，直接放行。

set -euo pipefail

# 读取 PreToolUse stdin（兼容 Claude Code 顶层 envelope 与 Aemeath 的嵌套 envelope）。
hook_input="$(cat || true)"
tool_name="$(printf '%s' "$hook_input" | jq -r '.tool_name // .PreToolUse.tool_name // empty' 2>/dev/null || true)"
file_path="$(printf '%s' "$hook_input" | jq -r '.tool_input.file_path // .PreToolUse.tool_input.file_path // empty' 2>/dev/null || true)"

# 仅对 Edit/Write 生效；其他工具（Read/Bash/...）放行
case "$tool_name" in
    Edit|Write) ;;
    *) exit 0 ;;
esac

# 解析项目根
project_root="$(git rev-parse --show-toplevel 2>/dev/null || echo "${AEMEATH_PROJECT_DIR:-}")"

# 项目根解析失败 → fail-open（让上层逻辑兜底）
if [ -z "$project_root" ]; then
    exit 0
fi

# file_path 解析失败 → fail-open
if [ -z "$file_path" ]; then
    exit 0
fi

# 规范化文件绝对路径（允许文件不存在，使用 -m）
abs_file="$(realpath -m -- "$file_path" 2>/dev/null || echo "$file_path")"

# 项目外文件 → 放行（仅约束项目内代码）
case "$abs_file" in
    "$project_root"/*) ;;  # 项目内：继续 worktree 校验
    *) exit 0 ;;           # 项目外：直接放行
esac

# 已在 linked worktree 中 → 放行
# 用 git 原生检测：linked worktree 的 absolute-git-dir（.git/worktrees/<name>）
# 与 git-common-dir（主仓库 .git）必定不同；main 工作区两者相同。
# 注意：必须基于 file_path 所在目录判断，而非 hook 进程 cwd——
# Claude Code 主进程可能在主工作区启动并 fork hook，hook cwd 与 Edit/Write
# 的 file_path（可能在 worktree 内）解耦。
#
# 新文件的父目录可能尚不存在。此时必须向上查找最近存在的祖先目录，
# 再基于该目录判断 git 上下文；否则 `cd "$target_dir"` 会泄漏 shell 噪音，
# 并把 worktree 内的缺失父目录误报成 main workspace。
nearest_existing_dir() {
    local dir="$1"
    while [ ! -d "$dir" ] && [ "$dir" != "/" ]; do
        dir="$(dirname "$dir")"
    done
    printf '%s' "$dir"
}

target_dir="$(dirname "$abs_file")"
probe_dir="$(nearest_existing_dir "$target_dir")"
abs_git_dir="$(git -C "$probe_dir" rev-parse --absolute-git-dir 2>/dev/null || true)"
abs_common_dir="$(git -C "$probe_dir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"

if [ -n "$abs_git_dir" ] && [ -n "$abs_common_dir" ] \
   && [ "$abs_git_dir" != "$abs_common_dir" ]; then
    exit 0
fi

current_branch=$(git -C "$probe_dir" branch --show-current 2>/dev/null || echo "unknown")
project_dir="$project_root"

if [ -z "$abs_git_dir" ] || [ -z "$abs_common_dir" ]; then
    cat >&2 <<ERR
[Hook blocked] Edit/Write rejected.

Reason:
  Unable to resolve git context for the target path. The nearest existing
  ancestor is not a git working tree, so this hook cannot prove the target is
  inside an isolated linked worktree.

How to fix:
  1. Create or enter a valid git worktree for this change.
  2. Re-run the Edit/Write tool from that worktree.

Project directory: ${project_dir}
Target file:       ${abs_file}
Nearest ancestor:  ${probe_dir}
ERR
    exit 2
fi

cat >&2 <<ERR
[Hook blocked] Edit/Write rejected.

Reason:
  Target path resolves to the main git workspace (branch: ${current_branch}),
  not an isolated linked worktree. According to AGENTS.md, all project file
  modifications MUST be done in a linked worktree, NEVER directly in the main
  workspace.

How to fix:
  1. Create a worktree for your change:
       git worktree add .worktrees/<branch-name> -b <branch-name>
  2. Switch to the worktree (cd .worktrees/<branch-name>).
  3. Re-run the Edit/Write tool from the worktree.
  4. After finishing, commit, push, and create a PR from the worktree.

Project directory: ${project_dir}
Current branch:    ${current_branch}
Target file:       ${abs_file}
Nearest ancestor:  ${probe_dir}
ERR

exit 2
