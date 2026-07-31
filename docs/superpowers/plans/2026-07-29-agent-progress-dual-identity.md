# AgentProgress 双身份修复实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 保留派生 Run 自己的 chat/turn 来源身份，同时把 Agent 进度稳定挂载到父 Run 的 Agent ToolCall block。

**Architecture:** `AgentProgress` 出站事件显式携带 `source_context` 与 `attachment_context`。派生 Runtime 创建来源身份，父 Agent ToolCall 转发器只补充挂载身份；SDK 与 TUI ACL 逐字段透传，Conversation Model 仅按 `attachment_context + tool_id` 更新父 ToolCall，不使用 active turn 或全局 tool-id 回退。

**Tech Stack:** Rust 2024 workspace、tokio mpsc、Runtime/SDK/TUI typed events、ratatui Conversation Model、cargo test/clippy。

---

## 文件结构

- Modify: `agent/features/tools/src/domain/tool_types.rs`
  - 为 `AgentProgressEvent` 增加可选的来源 chat/turn identity，并提供带来源身份的构造入口。
- Modify: `agent/features/runtime/src/application/run/derived/progress.rs`
  - 统一构造派生 Run 的结构化进度事件，避免各发射点漏填来源身份。
- Modify: `agent/features/runtime/src/application/run/derived/setup.rs`
  - 在派生 Run 创建时生成一次稳定 `source_context`，并传给所有 progress observer/finalizer。
- Modify: `agent/features/runtime/src/application/run/derived/loop_run.rs`
  - Started、Message、ToolCalls 事件保留同一派生来源身份。
- Modify: `agent/features/runtime/src/application/run/derived/finalize.rs`
  - Hook progress 保留派生来源身份。
- Modify: `agent/features/runtime/src/application/loop_engine/chat/events.rs`
  - 将 Runtime `AgentProgress` 的单一 `context` 拆成来源与挂载 context。
- Modify: `agent/features/runtime/src/application/loop_engine/chat/agent_calls.rs`
  - 父 Agent ToolCall 转发时只写 `attachment_context`，不覆盖 event 的 `source_context`。
- Modify: `agent/features/runtime/src/application/loop_engine/chat/non_agent.rs`
  - Bash streaming 使用相同事件形状；普通工具的来源与挂载身份相同。
- Modify: `agent/features/runtime/src/adapters/sdk_event_mapper.rs`
  - Runtime → SDK 双身份逐字段映射。
- Modify: `packages/sdk/src/chat_event.rs`
  - SDK `ChatEvent::AgentProgress` 暴露 `source_context` 与 `attachment_context`。
- Modify: `apps/cli/src/tui/adapter/tui_runtime_event.rs`
  - TUI-owned event language 保留两套 context。
- Modify: `apps/cli/src/tui/adapter/event_mapping.rs`
  - SDK → TUI 映射不折叠身份；TUI intent 只使用挂载身份。
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
  - legacy UiEvent 分支同步使用挂载身份，避免双路径语义不一致。
- Modify: `apps/cli/src/tui/app/event.rs`
  - legacy typed event 同步保留双身份。
- Modify: `apps/cli/src/tui/effect/session/processing/event_mapping.rs`
  - SDK → legacy UiEvent 双身份映射。
- Modify: `apps/cli/src/tui/effect/session/processing/logging.rs`
  - 日志同时输出 source/attachment identity，保留故障诊断证据。
- Test: 相邻模块现有 `*_tests.rs` 与 Runtime derived tests。
  - Runtime、SDK、TUI ACL、Conversation Model、并发 Agent block 和渲染各层分别覆盖。

---

### Task 1: Runtime 来源身份

**Files:**
- Modify: `agent/features/tools/src/domain/tool_types.rs`
- Modify: `agent/features/runtime/src/application/run/derived/progress.rs`
- Modify: `agent/features/runtime/src/application/run/derived/setup.rs`
- Modify: `agent/features/runtime/src/application/run/derived/loop_run.rs`
- Modify: `agent/features/runtime/src/application/run/derived/finalize.rs`
- Test: `agent/features/runtime/src/application/run/derived/tests.rs`

- [ ] **Step 1: 编写派生进度来源身份失败测试**

在 derived tests 中构造一个固定派生 chat/turn，收集 Started、Message 与 ToolCalls，断言三类事件都携带相同来源身份，且不等于父 chat/turn。

- [ ] **Step 2: 运行测试确认 RED**

Run:

```bash
cargo test -p runtime derived_progress_preserves_child_source_context -- --nocapture
```

Expected: FAIL，因为当前 `AgentProgressEvent` 没有来源身份。

- [ ] **Step 3: 实现最小来源身份模型**

为 `AgentProgressEvent` 增加 `source_context`；在派生 Run 创建时生成一次稳定来源 context，并通过统一 builder 传给 Started、Message、ToolCalls 与 Hook progress。普通 Bash progress 可以不提供派生来源，由父转发边界补成与挂载身份相同。

- [ ] **Step 4: 运行 Runtime 定向测试确认 GREEN**

Run:

```bash
cargo test -p runtime derived_progress_preserves_child_source_context -- --nocapture
```

Expected: PASS。

---

### Task 2: Runtime 出站双身份

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/chat/events.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/agent_calls.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/non_agent.rs`
- Test: `agent/features/runtime/src/application/loop_engine/chat/loop_runner_tests.rs`

- [ ] **Step 1: 编写父挂载不覆写来源身份失败测试**

构造 child source context、parent attachment context 与 Agent tool id，经 progress forward 后断言 Runtime 事件同时保留两者。

- [ ] **Step 2: 运行测试确认 RED**

Run:

```bash
cargo test -p runtime agent_progress_keeps_child_source_and_parent_attachment -- --nocapture
```

Expected: FAIL，因为 Runtime 事件当前只有单一 `context`。

- [ ] **Step 3: 实现 Runtime 双身份事件**

将 `RuntimeStreamEvent::AgentProgress` 改为 `source_context`、`attachment_context`、`tool_id`、`event`。Agent 转发器使用 child event 的来源身份和父 turn 的挂载身份；Bash streaming 将当前 turn 同时作为来源和挂载身份。

- [ ] **Step 4: 运行 Runtime 定向测试确认 GREEN**

Run:

```bash
cargo test -p runtime agent_progress_keeps_child_source_and_parent_attachment -- --nocapture
```

Expected: PASS。

---

### Task 3: SDK 契约透传

**Files:**
- Modify: `agent/features/runtime/src/adapters/sdk_event_mapper.rs`
- Modify: `packages/sdk/src/chat_event.rs`
- Test: `agent/features/runtime/src/adapters/sdk_event_mapper_tests.rs`

- [ ] **Step 1: 编写 Runtime → SDK 字段完整性失败测试**

断言 source chat/turn、attachment chat/turn、tool id、sequence 和 kind 全部保持，且 source 不被 attachment 覆盖。

- [ ] **Step 2: 运行测试确认 RED**

Run:

```bash
cargo test -p runtime sdk_agent_progress_preserves_source_and_attachment_contexts -- --nocapture
```

Expected: FAIL，因为 SDK `ChatEvent::AgentProgress` 当前只有一个 `context`。

- [ ] **Step 3: 实现 SDK 双身份映射**

修改 SDK 事件与 mapper，逐字段映射两套 `ChatEventContext`。不增加按 tool id 推断 context 的兼容逻辑。

- [ ] **Step 4: 运行 Runtime 与 SDK 测试确认 GREEN**

Run:

```bash
cargo test -p runtime sdk_agent_progress_preserves_source_and_attachment_contexts -- --nocapture
cargo test -p sdk agent_progress -- --nocapture
```

Expected: PASS。

---

### Task 4: TUI Consumer Adapter 挂载语义

**Files:**
- Modify: `apps/cli/src/tui/adapter/tui_runtime_event.rs`
- Modify: `apps/cli/src/tui/adapter/event_mapping.rs`
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
- Modify: `apps/cli/src/tui/app/event.rs`
- Modify: `apps/cli/src/tui/effect/session/processing/event_mapping.rs`
- Modify: `apps/cli/src/tui/effect/session/processing/logging.rs`
- Test: `apps/cli/src/tui/effect/session/processing.rs`
- Test: `apps/cli/src/tui/adapter/agent_event/tests.rs`

- [ ] **Step 1: 编写 SDK → TUI 双身份失败测试**

断言 TUI-owned event 同时保留 child source 与 parent attachment context。

- [ ] **Step 2: 编写 TUI adapter 挂载选择失败测试**

构造 source=`child-chat/child-turn`、attachment=`parent-chat/parent-turn`，断言 `UpdateAgentMeta` 和 `RecordAgentProgress` 都使用 parent attachment context。

- [ ] **Step 3: 运行测试确认 RED**

Run:

```bash
cargo test -p cli sdk_event_to_tui_runtime_event_preserves_agent_progress_identity -- --nocapture
cargo test -p cli agent_progress_uses_parent_attachment_context -- --nocapture
```

Expected: FAIL，因为 TUI 事件和 mapper 当前只有单一 context。

- [ ] **Step 4: 实现 Consumer Adapter 映射**

TUI typed event 保留双身份；SDK mapping 和 legacy UiEvent mapping 逐字段传递；生成 Conversation intent 时只读取 `attachment_context`。诊断日志同时记录 source 与 attachment。

- [ ] **Step 5: 运行 TUI adapter 测试确认 GREEN**

Run:

```bash
cargo test -p cli sdk_event_to_tui_runtime_event_preserves_agent_progress_identity -- --nocapture
cargo test -p cli agent_progress_uses_parent_attachment_context -- --nocapture
```

Expected: PASS。

---

### Task 5: Conversation Model 与并发 Agent block

**Files:**
- Test: `apps/cli/src/tui/model/conversation/model_tests/context.rs`
- Test: `apps/cli/src/tui/model/conversation/model_tests/progress_timeline.rs`
- Test: `apps/cli/src/tui/view_assembler/output_tests/tool_result_tests.rs`

- [ ] **Step 1: 增加并发父 ToolCall 场景测试**

在同一 parent turn 创建两个 Agent ToolCall，分别应用不同 child source、不同 attachment tool id 的进度，断言活动只进入各自 ToolCall。

- [ ] **Step 2: 增加根 timeline 排除断言**

断言两组进度均不产生根级 `AgentProgress` timeline item。

- [ ] **Step 3: 增加 render 断言**

断言两个 Agent block 各自显示对应 `activity_lines`，完成后的 ToolResult 行为保持不变。

- [ ] **Step 4: 运行 Conversation 与渲染测试**

Run:

```bash
cargo test -p cli concurrent_agent_progress_attaches_to_matching_parent_tool_blocks -- --nocapture
cargo test -p cli timeline_mirrors_blocks_no_agent_progress -- --nocapture
cargo test -p cli agent_progress -- --nocapture
```

Expected: PASS。

---

### Task 6: 全链路验证与清理

**Files:**
- Review all modified files.

- [ ] **Step 1: 检查旧单一 context 构造已退役**

Run:

```bash
rg -n 'AgentProgress \{[[:space:][:print:]]*context:' agent packages apps
```

Expected: 无旧单一 `context` 构造；所有 Runtime/SDK/TUI AgentProgress 使用显式 source/attachment 命名。

- [ ] **Step 2: 运行格式化**

Run:

```bash
cargo fmt --all -- --check
```

若失败，运行 `cargo fmt --all` 后重新执行 check。Expected: PASS。

- [ ] **Step 3: 运行分层测试**

Run:

```bash
cargo test -p runtime --lib
cargo test -p sdk
cargo test -p cli
```

Expected: 全部 PASS。

- [ ] **Step 4: 运行编译与 Clippy**

Run:

```bash
cargo check -p runtime -p sdk -p cli
cargo clippy -p runtime -p sdk -p cli --all-targets -- -D warnings
```

Expected: 全部 PASS，无 warning。

- [ ] **Step 5: 运行架构与 diff 门禁**

Run:

```bash
bash .agents/hooks/check-architecture-guards.sh
git diff --check
```

Expected: 全部 PASS。

- [ ] **Step 6: 更新 Task 状态并记录验证证据**

Task #5 仅在实现及分层测试通过后完成；Task #6 仅在编译、Clippy、架构 Guard 与 diff check 全部通过后完成。
