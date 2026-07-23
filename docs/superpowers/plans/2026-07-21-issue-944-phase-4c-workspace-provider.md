# Issue #944 阶段 4C 实施计划：WorkspaceProvider 一次性迁移与旧路径退役

> 对应 Issue：[ #944](https://github.com/rushsinging/aemeath/issues/944)。
> 本计划一次完成 WorkspaceProvider、纯 snapshot revision、Status/Output cache 消费迁移和 Conversation 旧路径退役；不拆分为多个实施阶段。

## 目标

将当前落在 `ConversationModel::RuntimeState.workspace` 的 TUI 工作区投影完整迁移到 `TuiModel::workspace_provider`。所有当前 snapshot 值通过 reducer 写入独立 Context；`workspace_root` 变更必须使 OutputViewCache 失效，即使 Conversation revision 未变。

```text
WorkspaceProvider {
  cwd: Option<String>
  worktree: Option<String>
  path_base: Option<String>
  workspace_root: Option<String>
  branch: Option<String>
  kind: WorktreeKind
  revision: u64
}
```

`revision` 每次 snapshot apply 递增。当前 branch/kind 仅透传，不在本 Issue 引入 git 或异步 metadata 查询；#943 DTO 形状稳定后，后续在同一 revision 上接 root+revision 防陈旧回填。

## 文件与责任

- Create: `apps/cli/src/tui/model/workspace_provider.rs`、`workspace_provider_tests.rs`
- Modify: `model.rs`、`model/root.rs`、`update/intent.rs`、`update/root_reducer.rs`
- Modify: `app.rs`、`app/runtime.rs`、`app/update.rs`、`app/update/ui_event.rs`
- Modify: `view_assembler/status.rs`、`adapter/status_widget.rs`
- Delete/retire from Conversation: `WorkspaceState`、`RuntimeState.workspace`、`UpdateWorkspace`、`WorkspaceSnapshotReceived`、`WorkspaceChanged`、`WorkspaceSnapshotChanged` 及相关实现；`WorktreeKind` 作为共享枚举保留在原模块。
- Modify: `architecture_tests.rs` 与现有 Workspace/cache 回归测试。

## TDD 实施步骤

1. **Red：WorkspaceProvider 单元测试**
   - `SetCurrent` 更新 cwd/worktree，不改变 snapshot revision。
   - `ApplySnapshot` 更新 path/root/branch/kind 并将 revision 从 0 增至 1。
   - 重复 ApplySnapshot 仍单调递增 revision。

2. **Red：Reducer 与 cache 场景测试**
   - `AgentIntent::Workspace(ApplySnapshot)` 标记 status 和 output dirty。
   - Conversation revision 不变、Workspace root 变化时，`refresh_output_document_from_model()` 仍重建 output document。

3. **实现独立 Context**
   - 定义 `WorkspaceIntent`、`WorkspaceChange`、`WorkspaceProvider`。
   - `TuiModel` 加 `workspace_provider`；`AgentIntent` 加 Workspace；root reducer 处理 Change，ApplySnapshot 标记 output/status dirty。

4. **迁移消费者**
   - `App::new`、`update_project_context` 发 WorkspaceIntent。
   - `WorkingDirectoryChanged` 同时更新 App 的操作 cwd 与 WorkspaceProvider snapshot；不做 git 查询。
   - Status assembler 显式接收 WorkspaceProvider。
   - `update_agent_event` 的 tool header、Output cache key、OutputViewAssembler 使用 WorkspaceProvider root。

5. **退役 Conversation 路径**
   - 删除 Conversation workspace state / intent / change / reducer实现。
   - 删除 RuntimeState workspace 字段和相关 accessor/mutator。
   - 所有读取改为 WorkspaceProvider。

6. **L0 边界验证**
   - Conversation runtime/intent/change 不得出现 Workspace provider 字段、Intent 或 Change。
   - Status assembler 必须显式接收 WorkspaceProvider。
   - App output cache 必须由 WorkspaceProvider root 参与 key。

## 验收

```text
cargo test -p cli tui::model::workspace_provider
cargo test -p cli tui::update::root_reducer
cargo test -p cli tui::view_assembler::status
cargo test -p cli tui::app::update
cargo test -p cli tui::architecture_tests
cargo fmt --all -- --check
cargo check -p cli
git diff --check
```

## 明确不做

- 不调用 git 或执行异步 metadata Effect。
- 不定义 SDK/TUI-owned Workspace DTO。
- 不实现 root+revision metadata result 的陈旧拒绝；该实现等待 #943 的纯 DTO snapshot 契约后接入。
