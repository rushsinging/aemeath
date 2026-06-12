# 工作区（Project / Worktree Context）

**Scope**：`agent/features/project/**`——worktree 工作区上下文管理（进入 / 退出 worktree、工作根切换、git 出站端口）。
**主触发**：改 `agent/features/project/**`。
**次触发**：改 worktree 进入 / 退出 / 持久化，或经 slash 命令操作 worktree。
**配套**：worktree 状态的会话落盘见 `storage.md`；经 slash 命令触发 worktree 操作的编排见 `runtime.md`。消费 `share::session_types::PersistedWorkspaceContext`（最小共享内核）。

## 状态唯一性

- `WorkspaceService`（`src/business/workspace_service.rs`）是**唯一**可变 workspace 状态源，单锁 `Mutex<WorkspaceState>`。**NEVER** 在别处另建可变 workspace 状态或缓存 `working_root` 副本。
- `WorkspaceState`（`src/business/workspace_state.rs`）持 `initial_cwd` / `working_root` / `path_base` / worktree 栈 `stack`；进入、退出 worktree 即栈 frame 的压入 / 弹出。
- 默认基线分支 `main`、worktree 目录 `.worktrees`（`DEFAULT_WORKTREE_BASE` / `DEFAULT_WORKTREE_DIR`）。

## COLA 分层（内层不依赖外层）

- 领域类型与能力端口定义在最内层 `business/`（`workspace_types.rs` 为所有者）；`contract.rs` / `gateway.rs` 仅向外 re-export。**NEVER** 让 business `use crate::contract`。
- `api.rs` 只引用 `crate::contract` / `crate::gateway`；`lib.rs` 仅 `business` 私有，其余 `pub`。
- 新增领域类型 **MUST** 落在 `business/`，再经 contract / gateway 暴露——与其他 feature 一致。

## 架构边界：git 出站端口

- git 操作经 `GitWorktreeOps` 出站端口（`src/business/git_ops.rs`）抽象，生产适配器 `GitCli` 派生 `git` CLI。
- **边界规则**：`project` **MAY** 派生 `git` 子进程；`share`（最小共享内核）**NEVER** 派生子进程。需要 git 能力的内层逻辑 **MUST** 经此端口注入，以便测试替身。
- `WorkspaceError`（`workspace_types.rs`）集中错误，用户可见消息为中文。
