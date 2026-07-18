# 工作区（Project / Worktree Context）

**Scope**：`agent/features/project/**`——worktree 工作区上下文管理（进入 / 退出 worktree、工作根切换、git 出站端口）。
**主触发**：改 `agent/features/project/**`。
**次触发**：改 worktree 进入 / 退出 / 持久化，或经 slash 命令操作 worktree。
**配套**：worktree 状态的会话落盘见 `storage.md`；经 slash 命令触发 worktree 操作的编排见 `runtime.md`。消费 `share::session_types::PersistedWorkspaceContext`（最小共享内核）。

## 状态唯一性

- `WorkspaceService`（`src/domain/service.rs`）是**唯一**可变 workspace 状态源，单锁 `Mutex<WorkspaceState>`。**NEVER** 在别处另建可变 workspace 状态或缓存 `workspace_root` 副本。
- `WorkspaceState`（`src/domain/state.rs`）持 `initial_cwd` / `workspace_root` / `path_base` / worktree 栈 `stack`；进入、退出 worktree 即栈 frame 的压入 / 弹出。
- 默认基线分支 `main`、worktree 目录 `.worktrees`（`DEFAULT_WORKTREE_BASE` / `DEFAULT_WORKTREE_DIR`）。

## Project 专用 Hexagonal 分层

- Project 使用私有 `domain/` 与 `adapters/`，不采用固定 `api/business/contract/gateway` 层。`lib.rs` 是唯一公开 façade，只能精确 re-export 已登记的 Project 能力；**NEVER** 暴露 `domain` / `adapters` 模块或使用通配 re-export。
- 领域状态、规则、类型与 `GitWorktreeOps` 出站端口归 `domain/`；`GitCli` 实现归 `adapters/git.rs`，默认生产装配由 adapter 层为 `WorkspaceService::new` 提供。`domain/` **NEVER** 依赖 `adapters/`。跨 crate 消费方 **MUST** 从 `project::<Symbol>` crate-root façade 导入，**NEVER** 穿透内部模块。
- 新增公开符号 **MUST** 同步登记到 `check-crate-api-boundary.sh` 的 Project root allowlist，并更新 [Architecture Guards](../docs/design/03-engineering/01-architecture-guards.md)；Current → Target 差距、责任与退出条件见 [Migration Governance](../docs/design/03-engineering/03-migration-governance.md)。这不是迁移例外。

- Project 发布彼此独立的 `WorkspaceRead` / `WorkspaceControl` / `WorkspacePersist`。不得用聚合 wrapper 向所有 Tool 广播三种能力：文件 Tool 只拿 Read；Bash、EnterWorktree、ExitWorktree 构造时才拿 Control；Runtime 自行持有 Persist。
- `WorkspaceViews` 是 composition wiring 形态，只能由 Runtime adapter 转成消费能力，**NEVER** 出现在 Tools domain。

## 架构边界：git 出站端口

- git 操作经 `GitWorktreeOps` 出站端口抽象，生产适配器 `GitCli` 位于 `src/adapters/git.rs` 并派生 `git` CLI。
- **边界规则**：`project` **MAY** 派生 `git` 子进程；`share`（最小共享内核）**NEVER** 派生子进程。需要 git 能力的内层逻辑 **MUST** 经此端口注入，以便测试替身。
- `WorkspaceError`（`src/domain/types.rs`）集中错误，用户可见消息为中文。
