# #1382 策略 trait 注入实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 MainRunPort 和 SubAgentRun 的 RunLoopPort 实现中约 600-700 行重复代码通过 4 个策略 trait 注入消除，统一 LLM 日志格式。

**Architecture:** 不合并 struct（Main 有 58 个字段，4 个类型不兼容），而是提取 InputStrategy / EventStrategy / LlmStrategy / ToolStrategy 四个策略 trait，将通用流程骨架提取为共享函数，差异逻辑通过策略注入。

**Tech Stack:** Rust, async_trait, runtime crate

---

## 文件结构

| 文件 | 职责 | 操作 |
|---|---|---|
| `application/loop_engine/input_strategy.rs` | InputStrategy trait + ChannelInput + FixedPromptInput | 新建 |
| `application/loop_engine/event_strategy.rs` | EventStrategy trait + TuiEventProjector + TerminalExtractor | 新建 |
| `application/loop_engine/llm_strategy.rs` | LlmStrategy trait + TuiLlmStrategy + ProgressLlmStrategy | 新建 |
| `application/loop_engine/tool_strategy.rs` | ToolStrategy trait + FullToolStrategy + SubToolStrategy | 新建 |
| `application/loop_engine/shared.rs` | 共享函数：needs_compaction, build_context_window, compact_with_reflection | 新建 |
| `application/loop_engine/llm_log.rs` | 统一 LLM 日志 builder（合并 llm_log.rs + logging.rs） | 新建 |
| `application/main_loop/looping/main_run_port.rs` | MainRunPort 瘦身，委托到策略 | 修改 |
| `application/subagent/runner/loop_run.rs` | SubAgentRun 瘦身，委托到策略 | 修改 |
| `application/main_loop/looping/llm_log.rs` | 删除（合并到 loop_engine/llm_log.rs） | 删除 |
| `application/subagent/runner/logging.rs` | 删除（合并到 loop_engine/llm_log.rs） | 删除 |
| `application/loop_engine/mod.rs` | 注册新模块 | 修改 |
| `application/main_loop/looping/mod.rs` | 删除 llm_log 模块声明 | 修改 |
| `application/subagent/runner/mod.rs` | 删除 logging 模块声明 | 修改 |
| `docs/design/02-modules/runtime/03-loop-and-state-machine.md` | 补充策略 trait 设计 | 修改 |

---

## Task 1: 提取 `needs_compaction` 共享函数

**Files:**
- Create: `agent/features/runtime/src/application/loop_engine/shared.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/mod.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs:1234-1251`
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs:536-553`

**目标：** `needs_compaction` 在 Main 和 Sub 中字符级一致（均从 `self.context_request` 取 `ContextRequest`、调 `self.context.build_window(...).await`、调 `window.needs_compaction()`、写 `self.context_window = Some(window)`），提取为共享函数消除重复。

- [ ] **Step 1: 写失败测试**

创建 `agent/features/runtime/src/application/loop_engine/shared_tests.rs`：

```rust
#![cfg(test)]

use super::shared::check_needs_compaction;

// 纯逻辑测试：验证 check_needs_compaction 在 None context_request 时返回错误
#[tokio::test]
async fn check_needs_compaction_returns_err_when_not_frozen() {
    // context_request = None 模拟未冻结
    let result = check_needs_compaction(None).await;
    assert!(result.is_err());
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p runtime --lib shared_tests -- --nocapture`
Expected: FAIL — `shared` 模块不存在

- [ ] **Step 3: 创建 shared.rs**

创建 `agent/features/runtime/src/application/loop_engine/shared.rs`：

```rust
//! RunLoopPort 共享逻辑——Main 和 Sub 完全一致的方法提取到此。

use super::{
    DrainEpoch, LoopEngineError,
};
use crate::ports::{ContextRequest, ContextWindow};

/// 检查是否需要 compact。
///
/// 从冻结的 ContextRequest 构建 window，判断是否需要压缩。
/// 调用方需在调用后将返回的 window 存入 `self.context_window`。
pub(super) async fn needs_compaction_with_window(
    context_request: Option<&ContextRequest>,
    context: &crate::application::context_coordination::ContextCoordinator,
    context_size: usize,
) -> Result<(bool, ContextWindow), LoopEngineError> {
    let request = context_request
        .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
    let window = context
        .build_window(request.clone(), context_size)
        .await
        .map_err(|e| LoopEngineError::Adapter(format!("build_window 失败: {e}")))?;
    let needed = window.needs_compaction();
    Ok((needed, window))
}
```

在 `agent/features/runtime/src/application/loop_engine/mod.rs`（或 `engine.rs` 的 `mod` 声明处）添加：
```rust
mod shared;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p runtime --lib shared_tests`
Expected: PASS

- [ ] **Step 5: MainRunPort 委托到共享函数**

修改 `main_run_port.rs:1234-1251`（`needs_compaction` 方法体）替换为：

```rust
async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
    let (needed, window) = crate::application::loop_engine::shared::needs_compaction_with_window(
        self.context_request.as_ref(),
        self.context,
        self.context_size,
    )
    .await?;
    self.context_window = Some(window);
    Ok(needed)
}
```

- [ ] **Step 6: SubAgentRun 委托到共享函数**

修改 `loop_run.rs:536-553`（`needs_compaction` 方法体）替换为：

```rust
async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
    let (needed, window) = crate::application::loop_engine::shared::needs_compaction_with_window(
        self.context_request.as_ref(),
        &self.context,
        self.ctx_context_size,
    )
    .await?;
    self.context_window = Some(window);
    Ok(needed)
}
```

- [ ] **Step 7: 编译验证**

Run: `cargo check -p runtime`
Expected: exit 0

- [ ] **Step 8: 测试验证**

Run: `cargo test -p runtime --lib`
Expected: 全量通过（506+ tests）

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "refactor(runtime): #1382 提取 needs_compaction 共享函数"
```

---

## Task 2: 统一 LLM 日志

**Files:**
- Create: `agent/features/runtime/src/application/loop_engine/llm_log.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/mod.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs` (use + invoke_model_impl 内部调用)
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs` (use + log_input/log_output 调用)
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_helpers.rs` (log_tool_results 调用)
- Delete: `agent/features/runtime/src/application/main_loop/looping/llm_log.rs`
- Delete: `agent/features/runtime/src/application/subagent/runner/logging.rs`

**目标：** 合并两套 LLM 日志函数为统一 builder，统一 JSON schema（补齐 system_blocks 到 Sub、补齐 tool_result 到 Main、统一 messages 取法）。

统一后 schema：
- input: `{event_type, role, messages, system_blocks, system_blocks_count, tool_schemas_count, tool_schemas_names}`
- output: `{event_type, role, stop_reason, input_tokens, output_tokens, elapsed_secs, provider, content_blocks}`
- tool_call: `{event_type, role, tool_use_id, tool_name, input}`
- tool_result: `{event_type, role, tool_use_id, tool_name, is_error, output}`

- [ ] **Step 1: 创建统一 llm_log.rs**

创建 `agent/features/runtime/src/application/loop_engine/llm_log.rs`，包含 4 个 builder 函数：

```rust
//! 统一 LLM 日志 builder——Main 和 Sub 共用。
//!
//! 统一 schema：caller 注入 event_type + role，builder 构造消息体。

use crate::application::main_loop::looping::InvocationResponse;
use crate::application::subagent::ToolCall;
use provider::RequestSystemBlock;
use share::message::Message;
use sdk::ids::ToolCallId;
use std::collections::HashMap;

type JsonObject = serde_json::Map<String, serde_json::Value>;

/// 构造 LLM input 日志。
///
/// `persisted_message_count`：Main 传已持久化消息数，用于标注；Sub 传 0。
/// `role`：caller 注入的日志角色名（如 "main"、"subagent:coder"）。
pub(crate) fn build_input_log(
    messages: &[Message],
    persisted_message_count: usize,
    system_blocks: &[RequestSystemBlock],
    tool_schemas: &[serde_json::Value],
    role: &str,
) -> serde_json::Value {
    let mut obj = JsonObject::new();
    obj.insert("event_type".into(), "llm_input".into());
    obj.insert("role".into(), role.into());

    let logged: Vec<serde_json::Value> = messages
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let mut m = JsonObject::new();
            m.insert("role".into(), format!("{:?}", msg.role).into());
            m.insert("content".into(), msg.content_text().into());
            m.insert("len".into(), msg.content_text().len().into());
            if i < persisted_message_count {
                m.insert("persisted".into(), true.into());
            }
            m.into()
        })
        .collect();
    obj.insert("messages".into(), logged.into());

    let sys_blocks: Vec<serde_json::Value> = system_blocks
        .iter()
        .map(|b| {
            let mut m = JsonObject::new();
            m.insert("type".into(), format!("{:?}", b.block_type).into());
            m.insert("len".into(), b.text.len().into());
            m.into()
        })
        .collect();
    obj.insert("system_blocks".into(), sys_blocks.clone().into());
    obj.insert("system_blocks_count".into(), sys_blocks.len().into());

    let names: Vec<&str> = tool_schemas
        .iter()
        .filter_map(|s| s.get("name").and_then(|n| n.as_str()))
        .collect();
    obj.insert("tool_schemas_count".into(), names.len().into());
    obj.insert("tool_schemas_names".into(), names.into());

    serde_json::Value::Object(obj)
}

/// 构造 LLM output 日志。
pub(crate) fn build_output_log(
    resp: &InvocationResponse,
    elapsed_secs: f64,
    provider: &str,
    role: &str,
) -> serde_json::Value {
    let mut obj = JsonObject::new();
    obj.insert("event_type".into(), "llm_output".into());
    obj.insert("role".into(), role.into());
    obj.insert("stop_reason".into(), format!("{:?}", resp.stop_reason).into());
    obj.insert("input_tokens".into(), resp.input_tokens.unwrap_or(0).into());
    obj.insert("output_tokens".into(), resp.output_tokens.unwrap_or(0).into());
    obj.insert("elapsed_secs".into(), elapsed_secs.into());
    obj.insert("provider".into(), provider.into());

    let blocks: Vec<serde_json::Value> = resp
        .content
        .iter()
        .map(|b| {
            let mut m = JsonObject::new();
            m.insert("type".into(), format!("{:?}", b.block_type).into());
            if let Some(t) = b.as_text() {
                m.insert("text_len".into(), t.len().into());
            }
            m.into()
        })
        .collect();
    obj.insert("content_blocks".into(), blocks.into());

    serde_json::Value::Object(obj)
}

/// 构造 tool_call 日志。
pub(crate) fn build_tool_call_log(tool_call: &ToolCall, role: &str) -> serde_json::Value {
    let mut obj = JsonObject::new();
    obj.insert("event_type".into(), "tool_call".into());
    obj.insert("role".into(), role.into());
    obj.insert("tool_use_id".into(), tool_call.id.to_string().into());
    obj.insert("tool_name".into(), tool_call.name.clone().into());
    obj.insert("input".into(), tool_call.input.clone());
    serde_json::Value::Object(obj)
}

/// 构造 tool_result 日志。
pub(crate) fn build_tool_result_log(
    id: &ToolCallId,
    output: &str,
    is_error: bool,
    call_info: &HashMap<ToolCallId, (String, String)>,
    role: &str,
) -> serde_json::Value {
    let mut obj = JsonObject::new();
    obj.insert("event_type".into(), "tool_result".into());
    obj.insert("role".into(), role.into());
    obj.insert("tool_use_id".into(), id.to_string().into());
    let (name, _) = call_info.get(id).cloned().unwrap_or_default();
    obj.insert("tool_name".into(), name.into());
    obj.insert("is_error".into(), is_error.into());
    obj.insert("output".into(), output.into());
    serde_json::Value::Object(obj)
}
```

> **注意**：上述代码中的 `block_type`、`as_text()`、`content_text()` 等字段访问需与实际 `RequestSystemBlock` / `InvocationResponse` / `Message` 的 API 对齐。实施时按实际 struct 字段名调整。

在 `engine.rs` 或 `mod.rs` 中注册：
```rust
pub(crate) mod llm_log;
```

- [ ] **Step 2: 修改 MainRunPort 调用点**

`main_run_port.rs` 的 `invoke_model_impl`（行 549-847）中：
- 替换 `use` 中 `crate::application::main_loop::looping::llm_log::{log_llm_input, log_llm_output_and_tool_calls}` 为 `crate::application::loop_engine::llm_log::{build_input_log, build_output_log}`
- 在日志调用处改为：
  ```rust
  let log_data = build_input_log(&messages_for_api, persisted_count, &system_blocks, &tool_schemas, "main");
  log::debug!(target: crate::LOG_TARGET, "{}", log_data);
  // output 类似
  ```
- 同时为 tool_call/tool_result 添加日志调用（补齐 Main 侧缺失的 tool_result 日志）

- [ ] **Step 3: 修改 SubAgentRun 调用点**

`loop_run.rs` 的 `invoke_model`（行 581-836）中：
- 替换 `use super::logging::{...}` 为 `use crate::application::loop_engine::llm_log::{build_input_log, build_output_log, build_tool_call_log}`
- 删除 `self.log_input()`（行 597）、`self.log_output()`（行 760）方法调用
- 改为直接 builder 调用，`role` 参数传 `&self.role_name_for_log`
- `log_tool_calls`（行 369-378）也替换为 builder

- [ ] **Step 4: 修改 loop_helpers.rs 调用点**

`loop_helpers.rs` 的 `log_tool_results`（行 49-69）：
- 替换 `build_json_logger_tool_result_data` 为 `crate::application::loop_engine::llm_log::build_tool_result_log`
- `role` 参数传 `&self.role_name_for_log`

- [ ] **Step 5: 删除旧文件**

```bash
rm agent/features/runtime/src/application/main_loop/looping/llm_log.rs
rm agent/features/runtime/src/application/subagent/runner/logging.rs
```

- [ ] **Step 6: 更新模块声明**

- `main_loop/looping/mod.rs`：删除 `mod llm_log;` 或 `#[path = "llm_log.rs"] mod llm_log;`
- `subagent/runner/mod.rs`：删除 `mod logging;`

- [ ] **Step 7: 编译验证**

Run: `cargo check -p runtime`
Expected: exit 0

- [ ] **Step 8: 测试验证**

Run: `cargo test -p runtime --lib`
Expected: 全量通过

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "refactor(runtime): #1382 统一 LLM 日志 builder"
```

---

## Task 3: 提取 `InputStrategy` trait

**Files:**
- Create: `agent/features/runtime/src/application/loop_engine/input_strategy.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/mod.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs`

**目标：** 将 drain_input / await_user_input / freeze_step / accept_step_input 的差异逻辑提取到 InputStrategy trait。

- [ ] **Step 1: 定义 InputStrategy trait**

创建 `agent/features/runtime/src/application/loop_engine/input_strategy.rs`：

```rust
//! InputStrategy — Main vs Sub 输入源差异策略。

use async_trait::async_trait;
use super::{DrainEpoch, DrainOutcome, LoopEngineError, LoopInput};
use share::message::Message;

#[async_trait]
pub(crate) trait InputStrategy: Send {
    /// 从输入源 drain 一批输入（Main: channel+buffer; Sub: 固定 prompt）。
    async fn drain(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError>;

    /// 等待用户输入（Main: channel park; Sub: unreachable）。
    async fn await_user(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError>;

    /// 冻结 step 输入为 ContextRequest 的 source messages。
    fn freeze(&mut self, inputs: &[LoopInput]) -> Vec<Message>;

    /// 返回本 step 采纳的用户消息（用于 finalize_step 持久化）。
    fn accepted_messages(&self) -> Vec<Message>;

    /// 是否有 pending tool results（Sub 内部续跑标记）。
    fn pending_tool_results(&mut self) -> bool { false }

    /// 取出 stop hook feedback（Main 独有）。
    fn stop_hook_feedback(&mut self) -> Option<Message> { None }
}
```

在 `mod.rs` / `engine.rs` 注册：
```rust
pub(crate) mod input_strategy;
```

- [ ] **Step 2: 实现 ChannelInput（Main 侧）**

在 `input_strategy.rs` 中实现 Main 的 `ChannelInput`：

```rust
/// Main 侧输入策略：RunInputBuffer + input_events channel + stop hook feedback。
pub(crate) struct ChannelInput<'a> {
    pub(crate) run_input_buffer: RunInputBuffer,
    pub(crate) queue: &'a dyn QueueDrainPort,
    pub(crate) input_events: &'a dyn InputEventDrainPort,
    pub(crate) pending_input: &'a mut PendingInputBuffer,
    pub(crate) prompt: &'a str,
    // epoch, stop_hook_feedback, pending_tool_results, per_turn_adopted 等
    // 按 MainRunPort drain_input/freeze_step/accept_step_input 实际引用的字段对齐
}
```

> **注意**：此 struct 的字段需要与 `main_run_port.rs:1085-1152`（drain_input）、`988-1034`（freeze_step）、`1036-1083`（accept_step_input）中使用的 self 字段精确对齐。实施时逐行提取这些方法中访问的 self 字段。

- [ ] **Step 3: 实现 FixedPromptInput（Sub 侧）**

```rust
/// Sub 侧输入策略：固定 prompt，drain 一次即 seal。
pub(crate) struct FixedPromptInput {
    pub(crate) prompt: &'a str,
    pub(crate) prompt_drained: bool,
    pub(crate) next_epoch: DrainEpoch,
    pub(crate) has_tool_results_pending: bool,
    pub(crate) accepted_input: Vec<Message>,
}
```

- [ ] **Step 4: MainRunPort 委托**

在 `MainRunPort` struct 中添加字段：
```rust
input: ChannelInput<'a>,
```

将 `drain_input` / `await_user_input` / `freeze_step` / `accept_step_input` 方法体替换为委托 `self.input.drain(...)` 等。

- [ ] **Step 5: SubAgentRun 委托**

在 `SubAgentRun` struct 中添加字段：
```rust
input: FixedPromptInput,
```

同样委托。

- [ ] **Step 6: 编译验证**

Run: `cargo check -p runtime`
Expected: exit 0

- [ ] **Step 7: 测试验证**

Run: `cargo test -p runtime --lib`
Expected: 全量通过，特别是 input_gate_tests / run_input_buffer_tests

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "refactor(runtime): #1382 提取 InputStrategy trait"
```

---

## Task 4: 提取 `EventStrategy` trait

**Files:**
- Create: `agent/features/runtime/src/application/loop_engine/event_strategy.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/mod.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs`

**目标：** emit / claim_terminal / claim_cancellation 的差异逻辑提取到 EventStrategy trait。

- [ ] **Step 1: 定义 EventStrategy trait**

创建 `agent/features/runtime/src/application/loop_engine/event_strategy.rs`：

```rust
//! EventStrategy — Main vs Sub 事件投影差异策略。

use async_trait::async_trait;
use crate::domain::agent_run::RunDomainEvent;

#[async_trait]
pub(crate) trait EventStrategy: Send {
    /// 投影 domain events（Main: RuntimeStreamEvent; Sub: terminal 提取）。
    async fn project(&mut self, events: Vec<RunDomainEvent>);
}
```

> claim_terminal / claim_cancellation 在 Main 和 Sub 中完全相同（都返回 `true`），无需策略化——它们是 RunLoopPort 的默认实现。

- [ ] **Step 2: 实现 TuiEventProjector（Main）**

封装 `main_run_port.rs:1517-1582`（emit 方法体）的逻辑，投影到 RuntimeStreamEvent。

- [ ] **Step 3: 实现 TerminalExtractor（Sub）**

封装 `loop_run.rs:984-1002`（emit 方法体）的逻辑，提取 AgentRunTerminal。

- [ ] **Step 4: 委托和编译测试**

同 Task 3 Step 4-8。

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor(runtime): #1382 提取 EventStrategy trait"
```

---

## Task 5: 提取 `LlmStrategy` trait

**Files:**
- Create: `agent/features/runtime/src/application/loop_engine/llm_strategy.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/mod.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs`

**目标：** invoke_model 共享流程提取，差异通过 LlmStrategy 钩子注入。

- [ ] **Step 1: 定义 LlmStrategy trait**

创建 `agent/features/runtime/src/application/loop_engine/llm_strategy.rs`：

```rust
//! LlmStrategy — Main vs Sub LLM 调用差异策略。

use async_trait::async_trait;

#[async_trait]
pub(crate) trait LlmStrategy: Send {
    /// 返回当前 reasoning level（Main: 动态; Sub: 固定）。
    fn reasoning_level(&self) -> workflow::api::ReasoningLevel;

    /// 是否需要 tool_identity 映射（Main 独有）。
    fn needs_tool_identity(&self) -> bool { false }

    /// invoke 前钩子（Main: ModelStreamWaiting 心跳; Sub: 无）。
    async fn pre_invoke(&mut self) {}

    /// response 后钩子（Main: Usage/TurnStarted 事件; Sub: progress 上报）。
    async fn post_response(&mut self, elapsed: f64) {}

    /// MaxOutputTokens 截断处理（Sub 独有）。
    /// 返回 true 表示已处理，调用方应提前返回。
    fn handle_max_tokens(
        &self,
        _messages: &mut Vec<share::message::Message>,
    ) -> bool { false }
}
```

- [ ] **Step 2: 实现 TuiLlmStrategy 和 ProgressLlmStrategy**

按 `main_run_port.rs:549-847`（invoke_model_impl）和 `loop_run.rs:581-836`（invoke_model）的实际差异点提取。

- [ ] **Step 3: 提取 invoke_model 共享流程**

将 `invoke_model_impl` / `invoke_model` 的共享部分（build window → log → provider.invoke → retry → log → build step）提取为 `shared.rs` 中的函数，差异通过 LlmStrategy 钩子调用。

- [ ] **Step 4-6: 委托、编译、测试、Commit**

同上模式。

---

## Task 6: 提取 `ToolStrategy` trait + 统一 `compact`

**Files:**
- Create: `agent/features/runtime/src/application/loop_engine/tool_strategy.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/shared.rs`
- Modify: `main_run_port.rs` / `loop_run.rs`

**目标：** execute_tools 差异提取 + compact PreCompact reflection 统一。

- [ ] **Step 1: 定义 ToolStrategy trait**

- [ ] **Step 2: 实现 FullToolStrategy 和 SubToolStrategy**

- [ ] **Step 3: 统一 compact — PreCompact reflection hook**

在 `shared.rs` 中添加：
```rust
/// PreCompact reflection trigger（从 MainRunPort.compact 提取）。
///
/// Sub 侧通过参数控制是否启用。
pub(super) fn maybe_trigger_pre_compact_reflection(
    outcome: &compact::CompactOutcome,
    context_window: Option<&ContextWindow>,
    // reflection 相关参数...
)
```

- [ ] **Step 4-6: 委托、编译、测试、Commit**

---

## Task 7: 更新设计文档

**Files:**
- Modify: `docs/design/02-modules/runtime/03-loop-and-state-machine.md`
- Modify: `docs/design/02-modules/runtime/06-ports-and-adapters.md`

- [ ] **Step 1: 03-loop-and-state-machine.md 补充策略 trait 设计**

在 §2.5（RunLoopPort trait 描述后）添加策略 trait 注入的设计描述。

- [ ] **Step 2: 06-ports-and-adapters.md 补充 adapter 策略注入**

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "docs(runtime): #1382 补充策略 trait 设计文档"
```

---

## Task 8: 最终验证

- [ ] `cargo test -p runtime --lib` 全量通过
- [ ] `cargo test -p composition`
- [ ] `cargo clippy -p runtime -p composition --all-targets --all-features -- -D warnings`
- [ ] `bash .agents/hooks/check-architecture-guards.sh`
- [ ] 确认 RunLoopPort impl 块总行数从 ~1183 降至 ≤ 700
- [ ] 手动验证 Sub LLM 日志输出含 `event_type` + `role` 字段
