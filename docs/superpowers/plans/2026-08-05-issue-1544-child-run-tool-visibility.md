# Child Run 工具结果可见性修复 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Child Run 工具活动复用现有 `ToolRenderPolicy`，确保 Skill 等隐藏结果只向 LLM 交付而不泄漏到 TUI，同时保留可见工具结果与完整跨层身份。

**Architecture:** Tools Published Language 的 Child Run ToolResult 增加 canonical `tool_name`，Runtime、SDK 和 TUI ACL 逐层无损透传。TUI model 继续保存结构化 Child Run 事实，但只将 ToolCall header、允许展示的 ToolOutput/ToolResult 与 terminal 投影到父 Agent activity；结果可见性唯一读取现有 `tool_display::result_policy`。兼容 `AgentProgress` 不再把 ToolResult 原始正文降级成 Message，从源头消除第二条正文展示路径。

**Tech Stack:** Rust、Tokio、serde/schemars、ratatui、inventory、Cargo tests

---

## 文件结构

- `agent/features/tools/src/domain/tool_types.rs`：Child Run 与兼容 AgentProgress 的内部 Published Language；ToolResult 增加 `tool_name`。
- `agent/features/runtime/src/application/run/derived/loop_run.rs`：Sub Run 工具执行结束时从 `ToolExecution.tool_name` 生成结构化进度事实。
- `agent/features/runtime/src/application/loop_engine/chat/agent_calls.rs`：将 AgentProgress ToolResult 投影为 Child Run ToolResult，保留 `tool_name`。
- `agent/features/runtime/src/adapters/sdk_event_mapper.rs`：Runtime → SDK 映射；Child Run 保留 `tool_name`，legacy AgentProgress 丢弃 ToolResult 正文。
- `packages/sdk/src/chat_view.rs`：SDK Published Language 的 Child Run ToolResult 增加 `tool_name` 并覆盖 serde round-trip。
- `apps/cli/src/tui/adapter/{event_mapping.rs,tui_runtime_event.rs,child_run_activity_mapping_tests.rs}`：SDK → TUI ACL 无损透传 `tool_name`。
- `apps/cli/src/tui/model/conversation/tool_observe.rs`：结构化保存 Child Run 事实，并按现有 ToolRenderPolicy 生成可见 activity。
- `apps/cli/src/tui/model/conversation/model_tests/progress_timeline.rs`：model 层隐藏/可见/去重契约。
- `apps/cli/src/tui/app/scenario_tests/agent_progress.rs`：TestBackend 最终屏幕回归场景。

### Task 1: Tools 与 Runtime ToolResult 契约

**Files:**
- Modify: `agent/features/tools/src/domain/tool_types.rs:244-420`
- Modify: `agent/features/runtime/src/application/run/derived/loop_run.rs:379-405`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/agent_calls.rs:423-457`
- Test: `agent/features/runtime/src/application/loop_engine/chat/agent_calls.rs:460-560`

- [ ] **Step 1: 写失败测试，要求 Child Run ToolResult 保留 canonical tool_name**

在 `child_run_activity_projection_preserves_parent_tool_identity_and_kinds` 附近新增独立测试，构造 `AgentProgressKind::ToolResult`，断言投影结果为：

```rust
assert!(matches!(
    projected[0].kind,
    tools::ChildRunActivityKind::ToolResult {
        ref tool_name,
        ref output,
        ..
    } if tool_name == "Skill" && output == "SKILL_BODY_SENTINEL"
));
```

- [ ] **Step 2: 运行测试并确认 RED**

Run: `cargo test -p runtime child_run_tool_result_preserves_canonical_tool_name --lib`

Expected: 编译失败或断言失败，因为 `AgentProgressKind::ToolResult` / `ChildRunActivityKind::ToolResult` 尚无 `tool_name`。

- [ ] **Step 3: 最小实现内部 Published Language 与事件构造**

给两种 ToolResult 变体增加同名字段：

```rust
tool_name: String,
```

`ProgressToolRoundObserver::execution_finished` 从每个 `ToolExecution.tool_name` 填入；`child_run_activity_kinds` 原样透传。

- [ ] **Step 4: 运行定向测试并确认 GREEN**

Run: `cargo test -p runtime child_run_tool_result_preserves_canonical_tool_name --lib`

Expected: PASS。

### Task 2: SDK Published Language 与 legacy 收口

**Files:**
- Modify: `packages/sdk/src/chat_view.rs:63-114,213-260`
- Modify: `agent/features/runtime/src/adapters/sdk_event_mapper.rs:630-718`
- Test: `agent/features/runtime/src/adapters/sdk_event_mapper_tests.rs:115-199`

- [ ] **Step 1: 写失败测试，要求 Runtime → SDK 保留 tool_name**

扩展 mapper 测试，构造 Child Run Skill ToolResult，并断言：

```rust
assert!(matches!(
    event.kind,
    sdk::ChildRunActivityKindView::ToolResult {
        ref tool_name,
        ref output,
        ..
    } if tool_name == "Skill" && output == "SKILL_BODY_SENTINEL"
));
```

- [ ] **Step 2: 写失败测试，要求 legacy AgentProgress 不发布 ToolResult 正文**

为 `project_agent_progress_event` 增加行为测试：ToolResult 输入不能得到包含 `SKILL_BODY_SENTINEL` 的 `AgentProgressKindView::Message`。将 mapper 改为可选投影：

```rust
pub(crate) fn project_agent_progress_event(
    event: tools::AgentProgressEvent,
) -> Option<AgentProgressEventView>
```

测试期望 ToolResult 返回 `None`。

- [ ] **Step 3: 运行测试并确认 RED**

Run: `cargo test -p runtime sdk_child_run_tool_result_preserves_tool_name legacy_agent_progress_drops_tool_result_body --lib`

Expected: FAIL；SDK ToolResult 无字段，legacy mapper 仍产生 Message。

- [ ] **Step 4: 最小实现 SDK 字段与可选 legacy 映射**

SDK `ChildRunActivityKindView::ToolResult` 增加：

```rust
tool_name: String,
```

`child_run_activity_to_sdk` 原样映射。`project_agent_progress_event` 对 `ToolResult` 与 `Terminal` 返回 `None`；调用处只在 `Some` 时发布兼容 `ChatEvent::AgentProgress`，结构化 `ChildRunActivity` 仍完整发布。

- [ ] **Step 5: 补 SDK serde round-trip 测试**

在 `packages/sdk/src/chat_view.rs` 的 round-trip 测试中使用 ToolResult fixture，并在反序列化后断言 `tool_name == "Skill"`。

- [ ] **Step 6: 运行 Runtime 与 SDK 测试**

Run: `cargo test -p runtime sdk_child_run_tool_result_preserves_tool_name --lib && cargo test -p runtime legacy_agent_progress_drops_tool_result_body --lib && cargo test -p sdk child_run_activity_round_trips_without_field_loss --lib`

Expected: 全部 PASS。

### Task 3: SDK → TUI ACL 字段完整性

**Files:**
- Modify: `apps/cli/src/tui/adapter/tui_runtime_event.rs:460-486`
- Modify: `apps/cli/src/tui/adapter/event_mapping.rs:888-930`
- Test: `apps/cli/src/tui/adapter/child_run_activity_mapping_tests.rs`

- [ ] **Step 1: 写失败测试，要求 SDK → TUI 保留 tool_name**

新增 Skill ToolResult 映射测试：

```rust
assert!(matches!(
    event.kind,
    TuiChildRunActivityKind::ToolResult {
        ref tool_name,
        ref output,
        ..
    } if tool_name == "Skill" && output == "SKILL_BODY_SENTINEL"
));
```

- [ ] **Step 2: 运行测试并确认 RED**

Run: `cargo test -p cli child_run_tool_result_sdk_to_tui_preserves_tool_name`

Expected: 编译失败，因为 TUI ToolResult 还没有 `tool_name`。

- [ ] **Step 3: 最小实现 TUI DTO 与 adapter 透传**

给 `TuiChildRunActivityKind::ToolResult` 增加：

```rust
tool_name: String,
```

`child_run_activity` mapper 从 SDK 同名字段原样赋值，不做名称推断或别名转换。

- [ ] **Step 4: 运行 adapter 测试并确认 GREEN**

Run: `cargo test -p cli child_run_tool_result_sdk_to_tui_preserves_tool_name`

Expected: PASS。

### Task 4: TUI model 复用 ToolRenderPolicy

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/tool_observe.rs:35-117`
- Test: `apps/cli/src/tui/model/conversation/model_tests/progress_timeline.rs:1-135`

- [ ] **Step 1: 写失败测试，要求隐藏 Skill ToolResult 且保留结构化事实**

创建父 Agent ToolCall，依次应用 Skill ToolCall 和带 `SKILL_BODY_SENTINEL\n<system-reminder>LLM_ONLY</system-reminder>` 的 Skill ToolResult。断言：

```rust
assert!(model.child_run_activities.iter().any(|entry| {
    matches!(entry.kind, TuiChildRunActivityKind::ToolResult { .. })
}));
assert!(!parent_call.activities.iter().any(|line| {
    line.contains("SKILL_BODY_SENTINEL") || line.contains("system-reminder")
}));
```

同时断言 ToolCall header `Skill superpowers:using-superpowers` 可见一次。

- [ ] **Step 2: 写失败测试，要求可见工具结果保持展示**

构造 `Grep` ToolCall/ToolResult，断言父 Agent activity 包含 `VISIBLE_GREP_RESULT`，证明不是全局丢弃 Child Run ToolResult。

- [ ] **Step 3: 运行测试并确认 RED**

Run: `cargo test -p cli child_run_hidden_tool_result_is_not_attached_to_parent_activity child_run_visible_tool_result_remains_attached`

Expected: Skill 测试 FAIL，因为当前直接 `output.clone()`。

- [ ] **Step 4: 实现单一可见性决策**

在 `tool_observe.rs` 中引入现有：

```rust
use crate::tui::render::output::tool_display::{
    format_subagent_tool_header, result_policy, ResultPolicy,
};
```

投影规则：

- `ToolCall` 使用 `format_subagent_tool_header(name, input, None)`；
- `ToolResult { tool_name, output, .. }` 查询 `result_policy(tool_name)`；
- `ResultPolicy::Hidden` 返回空可见消息，但仍记录 `child_run_activities` 与 sequence；
- `ResultPolicy::Visible` 返回 output；
- 空可见消息不调用 `record_agent_progress`，只返回 `OutputDirty` 之外无需新增变化。

不得匹配 `tool_name == "Skill"`，也不得扫描 XML 标签。

- [ ] **Step 5: 运行 model 测试并确认 GREEN**

Run: `cargo test -p cli child_run_hidden_tool_result_is_not_attached_to_parent_activity child_run_visible_tool_result_remains_attached`

Expected: 两项 PASS。

### Task 5: TestBackend 最终屏幕回归

**Files:**
- Modify: `apps/cli/src/tui/app/scenario_tests/agent_progress.rs:92-169`

- [ ] **Step 1: 写失败场景测试**

在父 Agent ToolCall 下发送：

1. Child Run Skill ToolCall，input 为 `{"skill":"superpowers:using-superpowers"}`；
2. Child Run Skill ToolResult，正文同时包含 `SKILL_BODY_SENTINEL`、`<skill-request>` 与 `<system-reminder>`；
3. Child Run Grep ToolCall/ToolResult，正文为 `VISIBLE_GREP_RESULT`。

屏幕断言：

```rust
assert!(screen.contains("Skill superpowers:using-superpowers"));
assert!(screen.contains("VISIBLE_GREP_RESULT"));
for hidden in ["SKILL_BODY_SENTINEL", "<skill-request>", "system-reminder"] {
    assert!(!screen.contains(hidden), "screen leaked {hidden}:\n{screen}");
}
```

- [ ] **Step 2: 运行场景测试并确认 RED 或由 Task 4 直接 GREEN**

Run: `cargo test -p cli child_run_skill_result_is_hidden_while_visible_tool_result_renders`

Expected: 若 Task 4 已完整修复则 PASS；若 FAIL，只修正组合链路，不弱化断言。

- [ ] **Step 3: 修正必要的组合投影并重跑**

Run: `cargo test -p cli child_run_skill_result_is_hidden_while_visible_tool_result_renders`

Expected: PASS。

### Task 6: 回归验证与收尾

**Files:**
- Verify only unless failures expose scoped defects.

- [ ] **Step 1: 格式化并检查 diff**

Run: `cargo fmt --all -- --check && git diff --check`

Expected: PASS；若 fmt check 失败，仅运行 `cargo fmt --all`，不手工调格式。

- [ ] **Step 2: 运行相关 crate 测试**

Run: `cargo test -p tools --lib && cargo test -p sdk --lib && cargo test -p runtime --lib && cargo test -p cli`

Expected: 全部 PASS。

- [ ] **Step 3: 运行 clippy**

Run: `cargo clippy -p tools -p sdk -p runtime -p cli --all-targets -- -D warnings`

Expected: PASS，无 warning。

- [ ] **Step 4: 运行相关架构守卫**

Run: `bash .agents/hooks/check-architecture-guards.sh --fast`

Expected: PASS。

- [ ] **Step 5: 检查兼容路径与死代码**

Run: `rg 'AgentProgressKind::ToolResult|ChildRunActivityKind.*ToolResult|SKILL_BODY_SENTINEL' agent packages apps/cli/src/tui`

Expected: ToolResult 正文只有结构化 Child Run 路径消费；legacy AgentProgress 不再降级展示；无 Skill 名称特判和 XML 标签过滤生产逻辑。

- [ ] **Step 6: 核对 Issue checklist 并更新状态**

将 Issue #1544 的每项验证更新为完成并附命令证据，状态改为“待确认”；不自行关闭 Issue。

### Task 7: 收敛 adopted 输入双轨并保留 Skill metadata

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/engine/contracts.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/input_gate.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/idle_lifecycle.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/run_input_buffer.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/input_strategy.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/session_driver/run_launch.rs`
- Modify: `agent/features/runtime/src/application/run/execution_state.rs`
- Test: `agent/features/runtime/src/application/loop_engine/chat/input_gate_tests.rs`
- Test: `agent/features/runtime/src/application/loop_engine/chat/run_input_buffer_tests.rs`
- Test: `agent/features/runtime/src/application/loop_engine/chat/session_driver_input_adoption_tests.rs`

- [ ] **Step 1: 建立唯一 accepted input 契约**

新增 `AcceptedUserInput`，直接保存 `input_id` 与 canonical `Message`，集中提供排队展示和撤回文本。`LoopInput` 仅保留 engine-driven fixed prompt 场景，不再承载 Main 用户输入。

- [ ] **Step 2: 写失败测试覆盖 Skill metadata**

分别验证 InputGate、RunInputBuffer drain 和首 Step freeze 后仍保留 `MessageSource::SkillRequest` 以及完整 `SkillRequestMetadata`。测试还必须覆盖普通文本、图片、InputId、withdraw 和 queued snapshot。

- [ ] **Step 3: 运行定向测试并确认 RED**

Run: `cargo test -p runtime skill_request --lib`

Expected: 新契约测试编译失败或断言失败，因为现有 `ChatInputEvent → LoopInput → Message::user` 会擦除 Skill metadata。

- [ ] **Step 4: 收敛 InputGate 与 RunInputBuffer**

`GateOutcome` 只返回 `accepted_inputs: Vec<AcceptedUserInput>`，删除 `adopted_messages`、`adopted_events`。`RunInputBuffer` 只存 `AcceptedUserInput`，Session adapter 在 admission 边界把 `ChatInputEvent` 转成 typed input；控制事件继续路由到 `PendingInputBuffer`。

- [ ] **Step 5: 收敛 idle 与 busy Run 链路**

`IdleResult::Resumed` 只携带 `accepted_inputs`。idle new Run 使用相同输入同时初始化模型增量消息并 seed 首次 drain；busy next-step 直接 drain同一类型。禁止从 Message 或字符串反向重建 typed input。

- [ ] **Step 6: 收敛 freeze/adoption**

Main Run 的 `DrainOutcome` 直接携带 `AcceptedUserInput`；freeze 从 canonical Message 构建模型输入，并用同一对象生成 `(InputId, Message)` adoption 事实。Sub fixed prompt 继续使用无 InputId 的 `LoopInput`，不参与用户 adoption。

- [ ] **Step 7: 验证全链路与退役路径**

运行 Runtime、SDK、CLI 分层测试与 framebuffer 场景，确认 slash `raw_input` 恰好显示一次，模型仍收到内部 `<skill-request>`，并确认仓库不再存在 `adopted_messages` / `adopted_events` 双轨字段。