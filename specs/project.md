# 工作区（Project / Worktree Context）

**Scope**：`agent/features/project/**`——worktree 工作区上下文管理（进入 / 退出 worktree、工作根切换、git 出站端口）。
**主触发**：改 `agent/features/project/**`。
**次触发**：改 worktree 进入 / 退出 / 持久化，或经 slash 命令操作 worktree。
**配套**：worktree 状态的会话落盘见 `storage.md`；经 slash 命令触发 worktree 操作的编排见 `runtime.md`。消费 `share::session_types::PersistedWorkspaceContext`（最小共享内核）。

## 状态唯一性

- `WorkspaceService`（`src/business/workspace_service.rs`）是**唯一**可变 workspace 状态源，单锁 `Mutex<WorkspaceState>`。**NEVER** 在别处另建可变 workspace 状态或缓存 `workspace_root` 副本。
- `WorkspaceState`（`src/business/workspace_state.rs`）持 `initial_cwd` / `workspace_root` / `path_base` / worktree 栈 `stack`；进入、退出 worktree 即栈 frame 的压入 / 弹出。
- 默认基线分支 `main`、worktree 目录 `.worktrees`（`DEFAULT_WORKTREE_BASE` / `DEFAULT_WORKTREE_DIR`）。

## 迁移期实现约束

本节只承载开发者当前 **MUST** 遵守的 Project 操作约束。守卫的实际检查行为、常量与白名单见 [Architecture Guards](../docs/design/03-engineering/01-architecture-guards.md)；Current → Target 差距、责任、进度与退出条件见 [Migration Governance](../docs/design/03-engineering/03-migration-governance.md)；目标结构判据见 [代码组织规范](../docs/design/01-system/06-code-organization.md)。

- 守卫替换前，Project 代码 **MUST** 继续使用现行 `business` / `contract` / `gateway` / `api` 边界：领域类型与能力端口归 `business/` 所有，`contract.rs` / `gateway.rs` 只做受控 re-export，`api.rs` 只经 `contract` / `gateway` 发布，`lib.rs` 保持 `business` 私有。
- `business` **NEVER** 依赖 `contract`。
- 守卫替换前，新增 Project 领域类型 **MUST** 落在 `business/`，再经 `contract` / `gateway` / `api` 受控暴露。

## 架构边界：git 出站端口

- git 操作经 `GitWorktreeOps` 出站端口（`src/business/git_ops.rs`）抽象，生产适配器 `GitCli` 派生 `git` CLI。
- **边界规则**：`project` **MAY** 派生 `git` 子进程；`share`（最小共享内核）**NEVER** 派生子进程。需要 git 能力的内层逻辑 **MUST** 经此端口注入，以便测试替身。
- `WorkspaceError`（`workspace_types.rs`）集中错误，用户可见消息为中文。
