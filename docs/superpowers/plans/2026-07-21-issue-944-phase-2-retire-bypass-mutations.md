# Issue #944 阶段二实施计划：旁路 Mutation 退役

> 对应 Issue：[ #944](https://github.com/rushsinging/aemeath/issues/944)，父 Issue：#860。
> 前置阶段：阶段一已提供 `AgentIntent → reduce_intent` 骨架。

## 目标

将 spinner phase/stop、runtime queued 快照同步、compact runtime 清理收口为 `ConversationIntent → ConversationChange → root_reducer`。阶段完成后删除这三类旧旁路 API 与生产调用点。

## 退役清单

| 旧路径 | 替代 | 完成条件 |
|---|---|---|
| `App::spinner_phase` 直接写 spinner 字段 | `ConversationIntent::SetSpinnerPhase` | `spinner.rs` 不再赋值 spinner 字段 |
| `App::spinner_stop` 直接写 spinner 字段 | `ConversationIntent::StopSpinner` | `spinner.rs` 不再赋值 spinner 字段 |
| `ConversationModel::sync_queued_from_runtime` | `ConversationIntent::SyncQueuedSubmissions` | 旧方法删除，UiEvent 只发 Intent |
| `RuntimeState::clear_compact_runtime` 的 UI 调用 | `ConversationIntent::ClearCompactRuntime` | UI 不再直接调用 runtime 清理 |
| `RuntimeState::spinner_mut` | 无替代公开 API | 方法删除且生产代码零引用 |

## 任务

1. 先在 `root_reducer_intent_tests.rs` 写四项失败测试：phase、stop、queued snapshot、compact clear 的状态/dirty/revision 行为。
2. 在 `conversation/intent.rs` 增加四个 payload；在 `intent_impls.rs` 实现 `ConversationUpdate`；在 `change.rs` 增加必要 Change，并在 enum 转发中穷尽分发。
3. 在 `RuntimeState` 增加语义方法 `set_spinner_phase`、`stop_spinner`；删除 `spinner_mut`。不允许 UI 直写内部字段。
4. 将 `app/update/spinner.rs` 与 `app/update/ui_event.rs` 改为 `reduce_intent(AgentIntent::Conversation(...))`，并将 reducer 的 dirty 合并到 App；删除对应手工 `mark_output_dirty()`。
5. 在 `architecture_tests.rs` 加 L0 扫描：目标旧方法及 UI 直接 spinner 字段赋值在生产源中为零；排除领域 `runtime_state.rs` 的合法内部实现。

## 验收

```text
cargo test -p cli tui::update::root_reducer
cargo test -p cli tui::model::conversation
cargo test -p cli tui::architecture_tests
cargo fmt --all -- --check
cargo check -p cli
git diff --check
```

## 阶段退出

只在以下符号从生产代码消失后进入阶段三：`spinner_mut`、`sync_queued_from_runtime`，以及 App/UI 对 `clear_compact_runtime` 的调用和直接 spinner 字段赋值。
