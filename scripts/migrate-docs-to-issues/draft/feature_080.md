<!-- Migrated from: docs/feature/active.md#80 -->
### #80 Agent context 所有权重构（project 拥有 WorkspaceState）

**状态**：✅ 已完成（合并 commit `26dee4c5`），待确认

**设计 / 计划文档**：
- 设计：[docs/superpowers/specs/2026-06-07-agent-context-ownership-redesign.md](../superpowers/specs/2026-06-07-agent-context-ownership-redesign.md)
- 实施计划：[docs/superpowers/plans/2026-06-07-agent-context-ownership-redesign.md](../superpowers/plans/2026-06-07-agent-context-ownership-redesign.md)

**背景**：`agent/` 用 5 套类型表达同一组 workspace 事实（`ToolContext` 三字段、`ToolContextParts`、`WorktreeWorkingContext`、`share::tool::WorkingContext`、`WorkspaceContext`），导致：所有权弥散；`working_root`/`path_base` 两把独立 `Arc<Mutex>` 存在撕裂读；子 agent 经 `Arc::clone` 共享父 workspace，子 EnterWorktree 会改到父工作目录；worktree 业务直接内联 `Command::new("git")`（domain 直捅 infra）。

**目标 / 决策**：抛开旧设计「runtime 拥有 context」，改为 **project 切片拥有 workspace 类型与转换规则，runtime 仅持有实例生命周期**（依据实测依赖图：无 feature 能依赖 runtime）。

**已完成的实现**：
1. **唯一可变 owner**：`project` 内 `WorkspaceState { initial_cwd, working_root, path_base, stack }`，由 `WorkspaceService` 包一把锁，enter/exit/set_cwd 原子切换（修撕裂读）。
2. **三能力 trait（inbound port，定义在 project，被 tools/runtime 消费）**：`WorkspaceRead`（current_root/current_path_base/resolve）、`WorkspaceControl`（set_cwd/switch_to/enter/exit）、`WorkspacePersist`（snapshot/restore）。`switch_to` 为带存在性+同源校验的跳转（供 `ExitWorktree{path}`），保留原安全边界。
3. **git outbound port**：`GitWorktreeOps` + `GitCli`（git_common_dir/show_toplevel/in_worktree/worktree_add/current_branch），可注入 `FakeGit` 做纯单测；项目内 git spawn 收敛至 `git_ops.rs`。
4. **子 agent 隔离**：`WorkspaceService::seed_isolated()` 从父当前快照派生独立实例（继承 root/base、空栈、独立锁），修复共享父 workspace 的 bug；`agent_semaphore` 仍共享。
5. **`ToolContext` → `ToolExecutionContext`**：删除 `working_root`/`path_base`/`context_stack` 三字段，改持 `Arc<WorkspaceService>` + `workspace_read()`/`workspace_control()` 访问器。
6. **runtime client 跨 chat 轮次持有 `WorkspaceService`**，取代 `inner.workspace_context` 与 per-loop seed；session 保存/恢复走 `snapshot()`/`restore()`（serde 字段不变，旧 session 兼容）。
7. **退役**：`WorktreeContextExt`、`ToolContextParts`/`build_tool_context`、`ProjectGateway`/`DefaultProjectGateway`、`share::tool::WorkingContext`、旧 `worktree.rs`/`working_paths.rs`。
8. **防回归 guard**：新增 `.agents/hooks/check-context-architecture.sh`（R1–R6：ToolExecutionContext 无三字段、tools 不引用持久化 DTO、WorkspaceState 仅 project、`workspace_control()` 仅 bash/worktree 工具、project git 仅 git_ops.rs、WorkspacePersist 仅 project+runtime），接入 `check-architecture-guards.sh`。

**关键修正（实施期发现）**：bash `cd` 也是 workspace mutator（纳入 `set_cwd`）；git spawn 禁入 share（GitCli 全在 project）；`ExitWorktree{path}` 经 `switch_to` 恢复存在性+同源校验（避免在 LLM 可控输入上削弱安全边界）。

**验证**（合并入 main 后在 main 复验）：`cargo test --workspace` 1387 通过；`cargo clippy --workspace --all-targets -- -D warnings` 无问题；`check-architecture-guards.sh` 全部通过（含新 context guard）。

**涉及路径**：
- `agent/features/project/src/business/{workspace_state,workspace_service,workspace_types,git_ops}.rs`、`contract.rs`、`api.rs`
- `agent/features/tools/src/contract/context.rs`、`contract.rs`、`business/{worktree,bash,file_read,file_edit,file_write,glob_tool,grep,lsp,agent_tool}.rs`
- `agent/features/runtime/src/core/client/{accessors,from_args,trait_chat,trait_session,trait_accessor,event,mapping}.rs`、`business/chat/looping/{loop_runner,agent_calls,non_agent,post_batch}.rs`、`business/agent/runner/setup.rs`
- `agent/composition/src/{app,lib}.rs`、`agent/shared/src/{session_types,tool}.rs`
- `.agents/hooks/check-context-architecture.sh`、`.agents/hooks/check-architecture-guards.sh`、`.agents/hooks/check-crate-api-boundary.sh`
