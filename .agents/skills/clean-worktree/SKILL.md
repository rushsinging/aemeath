---
name: clean-worktree
description: Use when auditing or cleaning Git worktrees, local branches left after pull requests, or Cargo build caches in the aemeath repository.
---

# Clean Worktree

## Overview

安全盘查并清理 aemeath 的 Git worktree、本地分支和构建缓存。核心原则：**先证明工作已进入 `origin/main`，再删除；任何 dirty、未合入或证据不足的对象只报告。**

## Safety Rules

- MUST 从主 checkout 执行；先记录 `git worktree list --porcelain`。
- MUST `git fetch --all --prune`，以最新 `origin/main` 为唯一合入基线。
- NEVER 删除当前 worktree、主 checkout、dirty/untracked worktree 或 `.claude/.agents` 配置目录。
- NEVER 仅因远端分支消失、Issue 已关闭、PR 已关闭或 `git branch -d` 成功就认定可清理。
- NEVER 仅靠 ancestry 判断 squash merge；也不得把相同标题当作充分证据。
- NEVER 对证据不足的分支执行 `git branch -D` 或 `git worktree remove --force`。
- MUST 先成功移除 worktree，再删除其本地分支；不得手工 `rm -rf` worktree。
- MUST 使用仓库脚本清理构建缓存，NEVER 直接删除活跃 worktree 的缓存。

## Workflow

### 1. 回到主仓并冻结盘点

```bash
root=$(git rev-parse --show-toplevel)
common_dir=$(git rev-parse --path-format=absolute --git-common-dir)
main_root=$(git -C "$(dirname "$common_dir")" rev-parse --show-toplevel)
cd "$main_root"
git fetch --all --prune
git status --short --branch
git worktree list --porcelain
```

主仓 dirty 时停止自动清理并报告。

### 2. 审计每个 worktree

对每个非主 worktree 收集：路径、分支、HEAD、`git status --porcelain`、upstream、关联 PR。只要有 modified、staged、untracked、rebase/merge 状态，分类为 **保留**。

关联 PR 优先按 head branch 查询；临时 review 分支再查询 tip commit association：

```bash
gh api --paginate "repos/rushsinging/aemeath/pulls?state=all&head=rushsinging:<branch>&per_page=100"
gh api "repos/rushsinging/aemeath/commits/<sha>/pulls" \
  -H 'Accept: application/vnd.github+json'
```

### 3. 证明已进入 origin/main

仅当存在完整证据链时标记 **可清理**：

| 场景 | 必需证据 |
|---|---|
| 普通 merge | PR `mergedAt` 非空，merge commit 是 `origin/main` ancestor，且本地 head 被 PR head 覆盖 |
| Squash merge | PR `mergedAt` 非空，squash commit 在 `origin/main`；本地 head 等于/被 PR head 覆盖，或全部本地 patch 与 PR commits 对应 |
| Release 集成 | 子 PR 已合入 release，且后续 release→main PR 的 merge commit在 `origin/main`，并包含该子 PR merge commit |
| 替代 PR | 后续 merged PR 明确关联同一 Issue/工作，且逐项 patch、文件或最终行为证明确实覆盖旧分支 |

PR `closed` 但 `mergedAt == null` 一律视为 **未合入**，除非另有替代 PR 的完整证据。

常用核验：

```bash
git merge-base --is-ancestor <merge_sha> origin/main
git log --oneline --no-merges origin/main..<branch>
git patch-id --stable
gh pr view <pr> --json state,mergedAt,headRefOid,mergeCommit,baseRefName,headRefName
```

### 4. 先报告，再删除

输出三组：

1. 可清理：clean 且已证明进入 `origin/main`；
2. 必须保留：dirty、open PR、closed-unmerged PR、无 PR 或未提交工作；
3. 证据不足：说明缺少什么。

仅删除第一组：

```bash
git worktree remove <path>
git worktree prune
git branch -D -- <branch>
```

删除前重新检查该 worktree 的 status，避免盘查后状态变化（TOCTOU）。

### 5. 清理 Cargo 构建缓存

```bash
scripts/clean-worktree-targets.sh --dry-run --max-size-gb 20
scripts/clean-worktree-targets.sh --yes --max-size-gb 20
```

脚本标记为 active 的缓存必须保留。若仍超预算，报告对应活跃 worktree，不手工删除。

### 6. 最终验证

```bash
git fetch --all --prune
git status --short --branch
git rev-parse HEAD
git rev-parse origin/main
git worktree list --porcelain
git for-each-ref --format='%(refname:short)|%(objectname)|%(upstream:track)' refs/heads
gh pr list --repo rushsinging/aemeath --state open --limit 100
```

报告删除的 worktree、分支、缓存体积，以及所有保留项和关联 PR。不能声称清理完成，除非主仓状态和剩余对象已重新核验。

## Common Mistakes

| 错误 | 正确做法 |
|---|---|
| squash 后 ancestry 不成立就认为未合入 | 查询 PR head/merge commit，并比较 patch/最终覆盖 |
| PR closed 就删除 | `mergedAt == null` 默认保留 |
| 直接 `rm -rf .worktrees/x` | 使用 `git worktree remove` |
| dirty worktree 也用 `--force` | 停止并报告未提交内容 |
| 手工删 `~/.cache/aemeath-target/*` | 使用 `scripts/clean-worktree-targets.sh` |
| 把 `.claude/worktrees` 当 Git worktree 根 | 以 `git worktree list --porcelain` 为唯一 worktree 清单 |
