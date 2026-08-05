# Child Run Activities Unified Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 #612 与 #946 合并为一个垂直交付：Main Agent ToolCall 启动的单层 Child Run 以结构化 identity 和事件贯穿 Runtime→SDK→TUI→Model，并继续投影到现有 Agent ToolCall activity 展示，只补齐 Text、Thinking、ToolOutput 可见性。

**Architecture:** Child Run 事件在 Runtime 侧以唯一事实携带 `agent_id`、`run_id`、`parent_run_id`、`spawned_by_tool_call_id` 和结构化 kind；SDK 与 TUI ACL 无损转换，ConversationModel 按 identity 归属到父 Agent ToolCall。ViewAssembler/Render 继续消费现有 activity 投影，不创建 Child Run 根块或第二套视觉管线。Sub-agent scope 保持禁止 Agent 工具，不实现递归 Sub-agent。

**Tech Stack:** Rust 2021、Tokio、serde/schemars、现有 SDK Published Language、TUI TEA/ACL、ConversationModel、Agent ToolCall activity renderer。

---

## 文件结构

- `docs/superpowers/specs/2026-08-04-child-run-activities-design.md`：已确认的统一设计与边界。
- `packages/sdk/src/chat_view.rs`：发布 Child Run identity、结构化 activity kind/event。
- `packages/sdk/src/chat_event.rs`：在 AgentProgress 事件上承载 identity，保持兼容字段策略。
- `agent/features/tools/src/domain/tool_types.rs`：定义 Runtime→Tools 的 Child Run activity 事实类型，禁止用展示字符串表达身份。
- `agent/features/runtime/src/application/loop_engine/chat/events.rs`：传递结构化 Child Run activity 事件。
- `agent/features/runtime/src/application/run/derived/setup.rs`、`derived/progress.rs`、`derived/loop_run.rs`、`application/loop_engine/chat/agent_calls.rs`：生成单层 Child Run identity，关联父 Agent ToolCall，发布 Text/Thinking/ToolCall/ToolOutput/terminal 事实。
- `agent/features/runtime/src/adapters/sdk_event_mapper.rs`：Runtime facts → SDK Published Language 的唯一转换。
- `apps/cli/src/tui/adapter/tui_runtime_event.rs`：TUI-owned Child Run DTO。
- `apps/cli/src/tui/adapter/event_mapping.rs`：SDK → TUI DTO 的无损 ACL。
- `apps/cli/src/tui/adapter/agent_event.rs`、`agent_event/progress.rs`：TUI DTO → Conversation Intent，保留现有摘要格式但不静默丢弃 ToolOutput/Text/Thinking。
- `apps/cli/src/tui/model/conversation/tool_call.rs`、`tool_observe.rs`、`intent.rs`、`intent_impls.rs`：按 Child Run identity 归属结构化活动并派生现有 activity lines。
- `apps/cli/src/tui/model/conversation/agent_progress.rs`、相关 model tests：收窄旧摘要存储为兼容投影，避免第二事实源。
- `agent/features/tools/src/adapters/registry.rs`：锁定 Sub-agent 不含 Agent 的 scope 证明。
- 对应 `*_tests.rs`、adapter contract tests 和 CLI scenario tests：覆盖每一相邻层与最终现有展示。

### Task 1: 锁定设计与工具权限约束

**Files:**
- Modify: `docs/superpowers/specs/2026-08-04-child-run-activities-design.md`（如实施中发现契约差异，保持与代码同步）
- Test: `agent/features/tools/src/adapters/registry.rs` 同级测试模块或现有 scope contract tests

- [ ] **Step 1: 写 Sub-agent scope 的失败契约测试**

在现有 `sub_agent_scope_characterization_is_exact` 附近补充明确断言：Sub-agent 工具集合不含 `Agent`，Main 工具集合包含 `Agent`，并断言其他普通工具仍按现有集合存在。

- [ ] **Step 2: 运行测试验证基线**

Run: `cargo test -p tools sub_agent_scope_characterization_is_exact -- --exact`
Expected: PASS，证明“不允许 Sub-agent 继续调 Agent”是当前生产约束；如果失败，先修复权限边界再继续事件改造。

- [ ] **Step 3: 记录设计与代码对齐结果**

更新设计文档的约束和验证章节，明确本交付只支持 Main→Child Run，不实现递归 Sub-agent。

### Task 2: 定义结构化 Child Run identity 与活动事件

**Files:**
- Modify: `agent/features/tools/src/domain/tool_types.rs`
- Modify: `packages/sdk/src/chat_view.rs`
- Modify: `packages/sdk/src/chat_event.rs`
- Test: `agent/features/tools/src/domain/tool_types_tests.rs` 或现有测试入口
- Test: `packages/sdk/src/chat_view.rs` 的外置测试文件（按仓库测试组织规范放置）
- Test: `packages/sdk/src/chat_event_tests.rs`

- [ ] **Step 1: 写 identity / event 失败测试**

测试必须构造两个固定 Child Run identity，断言 `agent_id`、`run_id`、`parent_run_id`、`spawned_by_tool_call_id` 和事件 kind/payload 全部保留；覆盖 Text、Thinking、普通 ToolCall、ToolOutput、ToolResult、Completed/Failed/Cancelled terminal。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p tools child_run_activity_preserves_identity -- --exact`
Run: `cargo test -p sdk child_run_activity_round_trips_without_field_loss -- --exact`
Expected: FAIL，正式类型或事件字段尚不存在。

- [ ] **Step 3: 实现最小领域类型与 SDK Published Language**

定义职责化类型（避免宽泛 `Projection` 命名）：

```rust
struct ChildRunIdentity {
    agent_id: String,
    run_id: String,
    parent_run_id: String,
    spawned_by_tool_call_id: ToolCallId,
}

enum ChildRunActivityKind {
    Text { text: String },
    Thinking { text: String },
    ToolCall { id: ToolCallId, name: String, input: Value },
    ToolOutput { tool_name: String, text: String },
    ToolResult { tool_call_id: ToolCallId, output: String, content: Value, is_error: bool },
    Terminal { outcome: ChildRunTerminalOutcome },
}

struct ChildRunActivityEvent {
    identity: ChildRunIdentity,
    sequence: u64,
    kind: ChildRunActivityKind,
}
```

SDK 类型使用现有 ID 类型和 serde/schemars；历史 wire 字段使用 `#[serde(default)]`，不得让展示字符串成为 Published Language。

- [ ] **Step 4: 运行类型测试**

Run: `cargo test -p tools child_run_activity_preserves_identity -- --exact`
Run: `cargo test -p sdk child_run_activity_round_trips_without_field_loss -- --exact`
Expected: PASS。

### Task 3: Runtime 生成并发布 Child Run identity

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/chat/agent_calls.rs`
- Modify: `agent/features/runtime/src/application/run/derived/setup.rs`
- Modify: `agent/features/runtime/src/application/run/derived/progress.rs`
- Modify: `agent/features/runtime/src/application/run/derived/loop_run.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/events.rs`
- Test: `agent/features/runtime/src/application/run/derived/tests/runtime_context_wiring.rs`
- Test: `agent/features/runtime/src/application/run/derived/tests.rs`
- Test: `agent/features/runtime/src/application/loop_engine/chat/agent_calls.rs` 外置测试或既有 agent call 场景测试

- [ ] **Step 1: 写 Runtime identity 和事件完整性失败测试**

构造一个父 Run、一个父 Agent ToolCall 和两个并发 child progress stream，断言每个事件携带对应 Child Run 的 `agent_id/run_id/parent_run_id/spawned_by_tool_call_id`；断言普通 ToolCall/ToolOutput 不混入另一个 Child Run；断言 Child terminal 与父 ToolResult 作为两个事件保留。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p runtime child_run_activity_preserves_parent_tool_identity`
Expected: FAIL，当前 `AgentProgressEvent` 只有 source context/sequence/kind，运行转发层未携带完整 Child Run identity 和 terminal 事件。

- [ ] **Step 3: 从派生 Run 与 Agent ToolCall 边界构造 identity**

在 Sub-agent 建立后使用派生 Run 的稳定 run id；使用父 `identity.run_id()` 作为 parent run；使用父执行边界的 `effective_call.id` 作为 spawned-by ToolCall；为每个 child stream 固定 agent id（由 Agent ToolCall identity 派生，不依赖到达顺序）。Sub-agent scope 不注入 Agent dispatch。

- [ ] **Step 4: 发布结构化 Text/Thinking/ToolCall/ToolOutput/terminal**

将现有 `AgentProgressKind` 的生产事件接入结构化 Child Run activity；普通 ToolCall 与 ToolOutput 由 child stream 事件发布；`AgentRunTerminal` 结束时发布独立 Child terminal，父 `send_tool_result` 仍照旧发布父 ToolResult，不复用 terminal 文本替代它。

- [ ] **Step 5: 运行 Runtime 测试**

Run: `cargo test -p runtime child_run_activity_preserves_parent_tool_identity`
Run: `cargo test -p runtime --lib`
Expected: PASS，且父 ToolResult/Child terminal 均存在。

### Task 4: Runtime → SDK 结构化映射

**Files:**
- Modify: `agent/features/runtime/src/adapters/sdk_event_mapper.rs`
- Test: `agent/features/runtime/src/adapters/sdk_event_mapper_tests.rs`
- Test: `agent/features/runtime/tests/sdk_event_mapper_contract.rs`

- [ ] **Step 1: 写 SDK mapper 字段无损失败测试**

表驱动覆盖两个 child identity 和全部 activity kind，断言 Runtime event 映射到 SDK event 后 identity、sequence、kind、payload 完整一致；禁止用 `format!` 或字符串摘要替代 kind。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p runtime sdk_event_mapper_child_run_activity_preserves_identity`
Expected: FAIL。

- [ ] **Step 3: 实现唯一 mapper**

在 `sdk_event_mapper.rs` 中集中完成 Runtime Child Run activity → SDK Child Run activity 的穷举转换；不在 Runtime loop、Tools 或 TUI 重复定义 SDK shape。

- [ ] **Step 4: 运行 mapper 契约测试**

Run: `cargo test -p runtime sdk_event_mapper_child_run_activity_preserves_identity`
Run: `cargo test -p sdk child_run_activity_round_trips_without_field_loss -- --exact`
Expected: PASS。

### Task 5: SDK → TUI ACL 无损转换

**Files:**
- Modify: `apps/cli/src/tui/adapter/tui_runtime_event.rs`
- Modify: `apps/cli/src/tui/adapter/event_mapping.rs`
- Test: `apps/cli/src/tui/adapter/event_mapping_tests.rs`
- Test: `apps/cli/src/tui/adapter/agent_event/tests.rs`

- [ ] **Step 1: 写 TUI ACL 失败测试**

构造两个 Child Run SDK event，断言 TUI DTO 保留全部 identity、sequence、kind/payload；覆盖 Text、Thinking、ToolCall、ToolOutput、ToolResult、terminal；明确 ToolOutput 不返回 `Noop`。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p cli child_run_activity_sdk_to_tui_preserves_identity`
Expected: FAIL。

- [ ] **Step 3: 实现 TUI-owned DTO 和映射**

在 `tui_runtime_event.rs` 定义独立 TUI-owned identity/event 类型；`event_mapping.rs` 只做字段转换。`agent_event.rs` 不接触 SDK 类型，不直接执行 I/O。

- [ ] **Step 4: 运行 ACL 测试**

Run: `cargo test -p cli child_run_activity_sdk_to_tui_preserves_identity`
Run: `cargo test -p cli agent_progress_tool_output_is_not_silently_dropped`
Expected: PASS。

### Task 6: ConversationModel 按 Child Run identity 归属并投影现有 activity

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/intent.rs`
- Modify: `apps/cli/src/tui/model/conversation/intent_impls.rs`
- Modify: `apps/cli/src/tui/model/conversation/tool_call.rs`
- Modify: `apps/cli/src/tui/model/conversation/tool_observe.rs`
- Modify: `apps/cli/src/tui/model/conversation/agent_progress.rs`
- Test: `apps/cli/src/tui/model/conversation/model_tests/progress_timeline.rs`
- Test: `apps/cli/src/tui/model/conversation/output_view_change_tests.rs`

- [ ] **Step 1: 写 Model 失败测试**

断言两个固定 Child Run 的 Text/Thinking/ToolCall/ToolOutput 分别进入对应父 Agent ToolCall；父 ToolResult 与 Child terminal 独立；重复事件幂等；未知父关联产生可诊断记录；现有 ToolCall activity 文本格式不变。

- [ ] **Step 2: 运行失败测试**

Run: `cargo test -p cli concurrent_child_run_activities_attach_to_matching_parent_agent_calls`
Expected: FAIL。

- [ ] **Step 3: 建立唯一 Child Run activity 事实存储**

在 ConversationModel 中按 Child Run identity 保存结构化事件或等价的 bounded child activity record；现有 `ToolCall.activities` 只由该事实派生/刷新，不再让独立字符串路径和结构化路径分别写入。

- [ ] **Step 4: 保持展示投影不变并补齐 Text/Thinking/ToolOutput**

Text 与 Thinking 映射到现有 Agent activity 行；ToolCall 继续复用 `format_agent_progress` 的 `→` 摘要；ToolOutput 使用现有 Agent streaming preview policy，不再返回空 mapping；Thinking 只增加语义字段/样式，不改变布局。

- [ ] **Step 5: 运行 Model 测试**

Run: `cargo test -p cli concurrent_child_run_activities_attach_to_matching_parent_agent_calls`
Run: `cargo test -p cli tui::model::conversation::model_tests::progress_timeline`
Expected: PASS，现有 Agent ToolCall / ToolResult 断言不回归。

### Task 7: 合并场景与展示验证

**Files:**
- Modify: `apps/cli/src/tui/app/scenario_tests/agent_progress.rs` 或同职责场景测试文件
- Test: `apps/cli/src/tui/app/scenario_tests/agent_progress.rs`
- Test: `apps/cli/src/tui/adapter/event_mapping_tests.rs`
- Test: `apps/cli/src/tui/model/conversation/model_tests/progress_timeline.rs`

- [ ] **Step 1: 写 Main + 并发 Sub-agent 场景失败测试**

场景输入包含两个 Main Agent ToolCall、两个 child identity，以及交错的 Text、Thinking、普通 ToolCall、ToolOutput、ToolResult、Child terminal 和父 ToolResult。断言最终现有 Agent ToolCall activity 展示中 A/B 内容不串流，Text/Thinking/ToolOutput 可见，父结果与 Child terminal 不覆盖。

- [ ] **Step 2: 运行失败场景**

Run: `cargo test -p cli main_with_concurrent_child_runs_preserves_existing_agent_activity_display`
Expected: FAIL。

- [ ] **Step 3: 接通最终场景路径**

让 scenario harness 经过真实 `sdk_event_to_tui_event` → `map_runtime_event` → reducer → existing OutputViewAssembler，禁止直接写 Model 私有字段或绕过 ACL。

- [ ] **Step 4: 运行场景测试**

Run: `cargo test -p cli main_with_concurrent_child_runs_preserves_existing_agent_activity_display`
Expected: PASS。

### Task 8: 守卫、文档回填与验证

**Files:**
- Modify: `docs/superpowers/specs/2026-08-04-child-run-activities-design.md`
- Modify: `docs/design/02-modules/tui/03-event-flow-and-acl.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`
- Modify: `docs/design/03-engineering/04-testing-and-coverage.md`
- Modify: `apps/cli/src/tui/architecture_tests.rs` 或对应 TUI guard 文件

- [ ] **Step 1: 添加结构化链路 Guard 测试**

禁止非 ACL 层直接依赖 SDK Child Run 类型；禁止 TUI event mapping 对 Child Run kind 使用 wildcard 或默认空 mapping；禁止 Sub-agent scope 注册 Agent；禁止结构化 Child Run facts 旁路写入 activity。

- [ ] **Step 2: 运行故意违规验证**

临时在测试 fixture 中制造每类违规，确认对应 guard 失败；恢复后运行 clean pass。故意违规不得提交。

- [ ] **Step 3: 回填文档与实施差异**

记录实际代码路径、已对齐项、仍保留的兼容字段及归属；同步明确 #612/#946 已合并为一个交付批次，现有展示边界没有改变。

- [ ] **Step 4: 执行格式、测试和 Guard**

Run: `cargo fmt --check`
Run: `cargo test -p tools`
Run: `cargo test -p sdk`
Run: `cargo test -p runtime`
Run: `cargo test -p cli`
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Run: `bash .agents/hooks/check-architecture-guards.sh`
Expected: 全部通过；失败项需保留首次失败证据并修复，不能用重跑成功覆盖。

- [ ] **Step 5: 最终状态核对**

Run: `git diff --check && git status --short --branch && git log --oneline origin/main..HEAD`
Expected: 变更仅限 #612/#946 合并交付相关路径；无未解释旧路径、无 whitespace error。
