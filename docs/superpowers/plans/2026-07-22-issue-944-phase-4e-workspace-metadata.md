# Issue #944 阶段 4E 实施计划：Workspace metadata 异步化与同步 Git 退役

> 对应 Issue：[ #944](https://github.com/rushsinging/aemeath/issues/944)。
> 前置：4C 已建立 `WorkspaceProvider { root, revision }`；4D 已建立 Effect/result Intent 消费边界。
> 本阶段只将 branch/worktree kind 派生移出 SDK→UiEvent / ACL 同步路径；不改 #943 的整体 DTO ACL，不触及 legacy AskUser。

## Goal

`WorkingDirectoryChanged` 只传纯 workspace snapshot。snapshot 经 reducer 写入 `WorkspaceProvider` 并递增 revision；Coordinator 依据 Change 派生异步 `ResolveWorkspaceMetadata { root, revision }` Effect。executor 查询 Git metadata 后以 Workspace result Intent 回灌，只有 root 与 revision 同时匹配时才写入 branch/kind。

## Architecture

```text
SDK WorkingDirectoryChanged { path_base, workspace_root, stack }
  → UiEvent::WorkingDirectoryChanged { raw/display paths, workspace }
  → WorkspaceIntent::ApplySnapshot { path_base, workspace_root }
  → WorkspaceChange::SnapshotApplied { root, revision }
  → Effect::ResolveWorkspaceMetadata { root, revision }
  → executor async git metadata query
  → WorkspaceIntent::ApplyMetadata { root, revision, branch, kind }
  → WorkspaceProvider: root + revision guard
  → MetadataApplied | MetadataDiscarded
```

`ApplyMetadata` 仅更新 `branch/kind`，不得递增 snapshot revision；root 或 revision 不匹配时不得写入任何字段。Git command 失败时返回 `branch=None`、`kind=Unknown`，仍受同一 guard 约束。

## Scope

### 建立

- `WorkspaceMetadata { root, revision, branch, kind }` 纯值类型。
- `WorkspaceIntent::ApplyMetadata` 与 `WorkspaceChange::{MetadataRequested, MetadataApplied, MetadataDiscarded}`。
- `Effect::ResolveWorkspaceMetadata { root, revision }`。
- executor 内唯一 Git 查询实现与 Workspace result Intent 回灌。
- root + revision stale guard。
- L1 Workspace state、L2 reducer/coordinator、L3 executor seam、L0 同步 git denylist 测试。

### 退役

- `StatusContextUpdate.branch`、`StatusContextUpdate.kind`。
- `git_branch_for()`、`worktree_kind_for()`。
- processing `event_mapping.rs` 的同步 Git command。
- `status_context_for_workspace()` 的同步 Git command。
- `WorkspaceIntent::ApplySnapshot` 的 branch/kind 参数。
- `WorkingDirectoryChanged` 的 branch/kind 负载。

### 不做

- 不重写 #943 的 SDK `ChatEvent` → TUI-owned DTO 总体映射。
- 不实现 Workspace 以外的异步 metadata。
- 不改 legacy AskUser sender / `update_ui`。
- 不改变 `cwd` / `path_base` / `workspace_root` 的真相归属。

## Files

- Modify: `apps/cli/src/tui/model/workspace_provider.rs`
- Modify: `apps/cli/src/tui/model/workspace_provider_tests.rs`
- Modify: `apps/cli/src/tui/update/intent.rs`
- Modify: `apps/cli/src/tui/update/root_reducer.rs`
- Modify: `apps/cli/src/tui/update/root_reducer_intent_tests.rs`
- Modify: `apps/cli/src/tui/update/coordinator.rs`
- Modify: `apps/cli/src/tui/effect/effect.rs`
- Modify: `apps/cli/src/tui/effect/executor.rs`
- Create: `apps/cli/src/tui/effect/executor_workspace_tests.rs`
- Modify: `apps/cli/src/tui/app/event.rs`
- Modify: `apps/cli/src/tui/app.rs`
- Modify: `apps/cli/src/tui/effect/session/processing/event_mapping.rs`
- Modify: `apps/cli/src/tui/app/update/ui_event.rs`
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
- Modify: `apps/cli/src/tui/architecture_tests.rs`
- Modify: `docs/design/03-engineering/03-migration-governance.md`

## TDD Tasks

### Task 1：Workspace metadata state 与 stale guard

- [ ] 在 `workspace_provider_tests.rs` 先写失败测试：
  - snapshot 初始写入 root 并从 revision 0 变为 1；
  - 同 root + revision 的 metadata 更新 branch/kind，revision 保持不变；
  - root 不同或 revision 过期的 metadata 被拒绝，branch/kind 保持原值；
  - 新 snapshot 写入后，先前 revision 的 metadata 必须被拒绝；
  - Git 失败对应 `None/Unknown` metadata 也只能在匹配时应用。
- [ ] 运行 `cargo test -p cli tui::model::workspace_provider -- --nocapture`，确认因类型/variant 缺失失败。
- [ ] 在 `workspace_provider.rs` 定义：

```rust
WorkspaceIntent::ApplySnapshot { path_base, workspace_root }
WorkspaceIntent::ApplyMetadata { root, revision, branch, kind }
WorkspaceChange::SnapshotApplied { root, revision }
WorkspaceChange::MetadataApplied { revision }
WorkspaceChange::MetadataDiscarded { root, revision }
```

- [ ] 将 `ApplySnapshot` 设为 branch=None、kind=Unknown，并递增 revision；`ApplyMetadata` 严格比较当前 `workspace_root == root && self.revision == revision`。
- [ ] 重跑定向测试，确认通过。

### Task 2：Reducer / Coordinator 的 Change → Effect 链

- [ ] 在 `root_reducer_intent_tests.rs` 写失败测试：`ApplySnapshot` 标记 output/status dirty，并得到一条 `ResolveWorkspaceMetadata { root, revision }` Effect；metadata applied 仅标 status dirty；discarded 不产生渲染请求。
- [ ] 在 `coordinator.rs` 写失败测试：仅 `SnapshotApplied` 产生 metadata Effect；`MetadataApplied` / `MetadataDiscarded` 不产生 metadata Effect。
- [ ] 运行 root reducer 和 coordinator 测试，确认失败。
- [ ] 让 `WorkspaceChange::SnapshotApplied` 经 reducer 转为 ModelChange/Coordinator 输入；Coordinator 生成：

```rust
Effect::ResolveWorkspaceMetadata { root: String, revision: u64 }
```

- [ ] 运行：

```bash
cargo test -p cli tui::update::root_reducer -- --nocapture
cargo test -p cli tui::update::coordinator -- --nocapture
```

确认通过。

### Task 3：executor 异步 Git seam 与 result Intent

- [ ] 在 `executor_workspace_tests.rs` 写失败测试，使用 injected/test-only metadata resolver，覆盖：
  - resolver 收到 Effect 的 root；
  - 成功结果回灌 `WorkspaceIntent::ApplyMetadata`；
  - Git error 回灌 `branch=None/kind=Unknown`；
  - resolver 结果到达前切换 root，旧 result 被 WorkspaceProvider 拒绝。
- [ ] 运行 `cargo test -p cli tui::effect::executor::workspace -- --nocapture`，确认失败。
- [ ] executor 是唯一 `std::process::Command("git")` 调用点；实现 branch 与 worktree kind 查询，运行于 Effect 的异步任务中，result 回灌 `AgentIntent::Workspace(ApplyMetadata)`。
- [ ] 不允许 executor 直接写 `WorkspaceProvider`；必须经 `App::apply_agent_intent`。
- [ ] 重跑定向 executor 测试，确认通过。

### Task 4：退役同步 Git 与清理 event 负载

- [ ] 先在 `architecture_tests.rs` 写失败 denylist：
  - `app.rs` 不含 `git_branch_for`、`worktree_kind_for`、`Command::new("git")`；
  - `effect/session/processing/event_mapping.rs` 不含这些 helper 或 `Command::new("git")`；
  - `StatusContextUpdate` 不含 branch/kind；
  - `WorkspaceIntent::ApplySnapshot` 不含 branch/kind。
- [ ] 删除 app helper；`status_context_for_workspace` 仅构造纯 snapshot。
- [ ] processing mapper 和 `UiEvent::WorkingDirectoryChanged` 仅传 path/root/workspace；adapter、update UI 和 reducer 只派发 Snapshot。
- [ ] 状态栏在 metadata 未回填时显示无 branch、Main/Unknown 的安全默认投影。
- [ ] 运行 architecture 测试，确认同步 git 已不可达。

### Task 5：文档与验收

- [ ] 更新 Migration Governance O6/TUI-4：标记 Workspace snapshot 已无同步 Git，metadata 通过 root+revision async Effect 回填；写明 #943 后续负责纯 DTO ACL 的整体收敛。
- [ ] 更新本计划实施结果与剩余 #943/#1246/legacy 责任。
- [ ] 运行：

```bash
cargo test -p cli tui::model::workspace_provider -- --nocapture
cargo test -p cli tui::update::root_reducer -- --nocapture
cargo test -p cli tui::update::coordinator -- --nocapture
cargo test -p cli tui::effect::executor::workspace -- --nocapture
cargo test -p cli tui::architecture_tests -- --nocapture
cargo fmt --all -- --check
cargo check -p cli
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-tea-purity.sh
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-effect-boundary.sh
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-model-view-boundaries.sh
git diff --check
```

## 实施结果（2026-07-22）

- `WorkspaceIntent::ApplySnapshot` 已收缩为纯 path/root snapshot；写入时清空 branch 并将 kind 设为 Unknown，同时递增 revision。
- `ApplyMetadata { root, revision, branch, kind }` 仅在双键匹配时写入；root 或 revision 失配返回 `MetadataDiscarded`，不触发 render。
- Coordinator 从 `SnapshotApplied` 生成 `ResolveWorkspaceMetadata`；executor 以 `spawn_blocking` 异步执行唯一 Git 查询，并回灌 `WorkspaceMetadataResolved`，再由 adapter 映射为 Workspace Intent。
- `StatusContextUpdate`、processing mapper、App helper 已删除 branch/kind 和同步 Git；状态栏在 metadata 回填前使用 Unknown/无 branch 安全默认值。
- 验证通过：workspace/reducer/coordinator/executor/adapter/architecture 定向测试、`cargo check -p cli`、fmt、TUI TEA/effect/model-view guards、`git diff --check`。`cargo clippy -p cli --all-targets -- -D warnings` 仍受 4D 已记录的既有 lint 阻断。

## Exit Criteria

- 同步 SDK event / ACL / UI update 路径零 Git command。
- metadata 只可由 executor 回灌，只有 root + revision 双重命中才能写入。
- stale metadata 结果有 L1/L3 测试，不覆盖新 workspace。
- metadata failure 不破坏 workspace snapshot，只产生 Unknown projection。
- #943 可在此 consumer seam 上替换为最终 TUI-owned Workspace DTO；legacy AskUser 不在本阶段扩张。

## Plan Self-Review

- 覆盖目标：异步 metadata、双键陈旧拒绝、同步 Git 退役、状态栏安全降级。
- 边界清楚：Coordinator 只产 Effect；executor 唯一执行 Git；WorkspaceProvider 唯一应用 metadata。
- 未越界到 #943：不重写 SDK DTO，只移除当前事件映射的同步 Git。
- 未越界到 #1246/AskUser：不接 Runtime suspension，不触及 sender 链。
