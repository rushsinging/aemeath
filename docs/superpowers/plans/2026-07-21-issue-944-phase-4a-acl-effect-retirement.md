# Issue #944 阶段 4A 实施计划：ACL Effect 收口

> 对应 Issue：[ #944](https://github.com/rushsinging/aemeath/issues/944)。
> 范围：只移除 `AgentEventMapping.effects`，让 Error 的 hook 副作用经 Model Change → Coordinator 推导。

## 目标

`adapter/agent_event.rs` 只产生 Context Intent，`root_reducer` 不再透传 mapper Effect。`UiEvent::Error` 仍产生错误展示、Diagnostic notice 和 `Effect::RunHook`，但 hook Effect 的来源改为 `ConversationChange::ErrorAppended`。

## 步骤

1. Red：扩展 reducer 测试，构造 `AgentEventMapping` 的错误 Intent，断言 reducer 返回 `RunHook { name: "error" }`；测试须先因 Coordinator 尚未映射而失败。
2. 在 `update/coordinator.rs` 新增 `effects_for_conversation_change`，仅将 `ErrorAppended` 映射为 `RunHook`，其余 Change 返回空；写单元测试。
3. 修改 `root_reducer::apply_conversation_changes`，在遍历 Change 时调用 Coordinator 并累积 Effect，再去重 Render。
4. 从 `AgentEventMapping` 删除 `effects` 字段及 `Effect` import；`UiEvent::Error` 仅生成 Conversation / Diagnostic Intent；更新 mapper 测试和 reducer fixture。
5. L0：ACL 生产源不得 import `Effect` 或出现 `effects:`；`root_reducer` 不得访问 `mapping.effects`。

## 退役与验收

- 退役：`AgentEventMapping.effects` 与 `mapping.effects.push(Effect::RunHook { ... })`。
- 验收：Error 映射和 reducer 行为测试、coordinator 单测、架构测试、`cargo check -p cli`、`git diff --check`。
