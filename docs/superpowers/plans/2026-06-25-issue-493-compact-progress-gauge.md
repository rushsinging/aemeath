# Issue #493: Compact 进度事件 + TUI Gauge 进度条

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** compact 执行期间 runtime 通过事件通道向 TUI 发送阶段性进度，TUI 用 ratatui `Gauge` widget 渲染真实进度条（阶段文字 + 百分比），替代静态 "Compacting..." spinner。同时将 `/compact` 命令从请求-响应模式改为走 runtime 主循环事件流（umbrella issue #497 的首个落地）。

**Architecture:**

核心变化：`/compact` 不再调 `ac.compact_messages()` trait 方法（请求-响应，TUI 阻塞）。改为通过 `ChatInputEvent::Compact` 发往 runtime 主循环。主循环 idle gate 收到后，在 loop_runner 层面执行 compact（此时有 client/system_prompt/context_size 全部参数），通过已有的 `sink` 发送 `CompactProgress` 事件 + `MessagesSync` + `SystemMessage`。`auto_compact`（token 超阈值自动压缩）复用同一套进度事件。

事件链路：
```
/compact → ChatInputEvent::Compact → idle gate (compact_requested=true)
  → loop_runner 执行 manual_compact (有 sink + client 全参数)
    → compact_messages_with_llm(progress callback)
      → sink.send_event(CompactProgress { stage, current, total })
  → sink.send_event(MessagesSync + SystemMessage)

auto_compact (token 超阈值)
  → compact_messages_with_llm(progress callback)
    → sink.send_event(CompactProgress { stage, current, total })

TUI 侧：
  RuntimeStreamEvent::CompactProgress → ChatEvent → UiEvent → RuntimeIntent → RuntimeModel → LiveStatusViewModel → OutputArea Gauge
```

**Tech Stack:** Rust 2021, Tokio, ratatui（`widgets::Gauge`），crates `runtime`、`sdk`、`cli`。

---

## 设计决策

| # | 问题 | 决策 | 理由 |
|---|---|---|---|
| 1 | 进度粒度 | 混合式：阶段（preparing/summarizing/finalizing）+ map-reduce chunk i/N | 阶段覆盖全场景，chunk 是天然连续进度 |
| 2 | 进度条 vs 文字 | **ratatui `Gauge` widget** | 用户明确要求进度条 |
| 3 | SpinnerPhase 扩展 | **不扩展**，新增独立 `compact_progress` model 字段 | Gauge 是独立 widget 需独立 Rect，与 spinner `Line` 机制不同 |
| 4 | LLM streaming 复用 | 不复用，只用离散阶段 + chunk 计数 | 侵入性大且非线性 |
| 5 | `/compact` 执行路径 | **走 `ChatInputEvent::Compact` → 主循环 idle gate** | 用户要求统一走 runtime 事件流（#497）；不再调 `compact_messages()` trait 方法 |
| 6 | compact 执行位置 | **loop_runner 层**（非 `apply_gate` 内） | compact 需 client/system_prompt/context_size 参数，`apply_gate` 签名不含这些；loop_runner 层有全部上下文 |
| 7 | busy 时 `/compact` | 排队等回合结束回 idle 再执行 | 与 `Reset` 的 busy 语义一致 |

### Gauge ratio 映射

| 阶段 | ratio | label |
|---|---|---|
| preparing | 0.05 | `Compacting — preparing...` |
| summarizing（单次） | 0.50 | `Compacting — summarizing...` |
| summarizing（chunk i/N） | `0.15 + 0.70*(i/N)` | `Compacting — summarizing (chunk i/N)` |
| finalizing | 0.90 | `Compacting — finalizing...` |

### Sub-agent（AC5）

`compact_if_needed`（`loop_helpers.rs`）用本地 `compact_messages`（纯文本、无 LLM、<1ms），无需进度事件，仅注释记录。

## File Structure

### Runtime — 事件类型层
- `business/chat/looping/events.rs` — 新增 `CompactStage` 枚举 + `RuntimeStreamEvent::CompactProgress`
- `business/compact.rs` — re-export `CompactStage`、`CompactProgressFn`

### Runtime — compact 核心逻辑
- `business/compact/summary.rs` — 新增 `CompactProgressFn` trait；`compact_messages_with_llm` + `compact_messages_map_reduce` 加 `progress` 参数

### Runtime — 主循环 compact 执行
- `packages/sdk/src/chat.rs` — `ChatInputEvent` 新增 `Compact` 变体
- `business/chat/looping/input_gate.rs` — `apply_gate` 处理 `Compact`（idle 时设 `compact_requested=true`；busy 时放回 buffer）；`GateOutcome` 新增 `compact_requested` 字段
- `business/chat/looping/loop_runner.rs` — 新增 `IdleResult::CompactRequested`；idle 循环收到后执行 `manual_compact`（有 sink+client+全参数）；`auto_compact` 调 `compact_messages_with_llm` 时传入 progress 闭包
- `business/chat/looping/compact.rs` — 新增 `manual_compact` 函数（供 loop_runner idle 调用），执行 compact + 发 CompactProgress 事件
- `business/agent/runner/loop_helpers.rs` — 注释说明 sub-agent 不发进度

### Runtime — SDK trait 清理
- `packages/sdk/src/client.rs` — `compact_messages` trait 方法**保留**（auto_compact 内部仍可能用到，或标记 deprecated）
- `agent/features/runtime/src/core/client/trait_compact.rs` — `compact_messages_impl` 保留但 `/compact` 不再走此路径

### Runtime — 事件映射
- `agent/features/runtime/src/core/client/event.rs` — `runtime_event_to_sdk_event` 新增 `CompactProgress` 映射

### SDK
- `packages/sdk/src/chat_event.rs` — 新增 `ChatEvent::CompactProgress`

### TUI — 事件处理
- `model/runtime/compact_progress.rs`（新增）— `CompactProgressModel` + ratio/label 计算
- `model/runtime/model.rs` — `RuntimeModel` 加 `compact_progress` 字段
- `model/runtime/intent.rs` — 新增 `SetCompactProgress`；`StopSpinner` 同步清 compact_progress
- `app/event.rs` — 新增 `UiEvent::CompactProgress`
- `effect/session/processing.rs` — `sdk_event_to_ui_event` + 日志映射
- `adapter/agent_event.rs` — `CompactProgress → SetCompactProgress`；`PostCompact → StopSpinner` 时清 compact_progress

### TUI — slash 命令改造
- `app/slash.rs` — `/compact` 两个入口（line 46 和 `CommandAction::Compact`）都改为 `push_input_event(ChatInputEvent::Compact)`，不再调 `ac.compact_messages()`

### TUI — 渲染
- `view_model/live_status.rs` — 新增 `CompactProgressView`；`LiveStatusViewModel` 加字段
- `view_assembler/live_status.rs` — 从 `runtime.compact_progress` 派生 view
- `render/output_area/render.rs` — 有 compact_progress 时在 spinner 行后用 Gauge 渲染 1 行
- `render/output_area/status_line.rs` — 为 Gauge 预留 screen_line_map entry

---

## Task 1: 定义 CompactStage + RuntimeStreamEvent::CompactProgress

**Files:** `agent/features/runtime/src/business/chat/looping/events.rs`

- [ ] **Step 1:** 在 `RuntimeStreamEvent` 定义前添加 `CompactStage` 枚举（`Preparing`/`Summarizing`/`Finalizing`，带 `as_str()` 方法）
- [ ] **Step 2:** 在 `RuntimeStreamEvent` 末尾添加 `CompactProgress { stage, current, total }` 变体
- [ ] **Step 3:** `cargo check -p aemeath-runtime`

## Task 2: compact_messages_with_llm 接受进度回调

**Files:** `agent/features/runtime/src/business/compact/summary.rs`、`summary_tests.rs`

- [ ] **Step 1:** 定义 `CompactProgressFn` trait（`Fn(CompactStage, Option<usize>, Option<usize>)` 的 blanket impl）
- [ ] **Step 2:** `compact_messages_with_llm` 新增 `progress: Option<&dyn CompactProgressFn>` 参数
- [ ] **Step 3:** 在各阶段发出回调（Preparing/Summarizing/Finalizing）
- [ ] **Step 4:** `compact_messages_map_reduce` 同样新增参数，for 循环中发 chunk 进度
- [ ] **Step 5:** 更新现有测试调用点补传 `None`
- [ ] **Step 6:** 新增进度回调单元测试（本地回退路径，验证 Preparing → Finalizing）
- [ ] **Step 7:** `cargo test -p aemeath-runtime --lib business::compact::summary`

## Task 3: ChatInputEvent::Compact + idle gate 处理

**Files:** `packages/sdk/src/chat.rs`、`agent/features/runtime/src/business/chat/looping/input_gate.rs`

- [ ] **Step 1:** `ChatInputEvent` 新增 `Compact` 变体
- [ ] **Step 2:** `GateOutcome` 新增 `compact_requested: bool` 字段（Default = false）
- [ ] **Step 3:** `apply_gate` 中处理 `ChatInputEvent::Compact`：idle 时设 `compact_requested=true`、`decision=Proceed`、break；busy 时放回 buffer（与 `Reset` 语义一致）
- [ ] **Step 4:** 更新 `GateOutcome` 所有构造点补 `compact_requested: false`（或用 `..Default::default()`）
- [ ] **Step 5:** 更新 input_gate 测试（`Reset` 测试旁新增 `Compact` idle/busy 测试）
- [ ] **Step 6:** `cargo test -p aemeath-runtime --lib business::chat::looping::input_gate`

## Task 4: IdleResult::CompactRequested + manual_compact + loop_runner 集成

**Files:** `agent/features/runtime/src/business/chat/looping/loop_runner.rs`、`business/chat/looping/compact.rs`

- [ ] **Step 1:** `IdleResult` 新增 `CompactRequested` 变体
- [ ] **Step 2:** `idle_until_resume_or_shutdown` 检测 gate 的 `compact_requested`，返回 `CompactRequested`
- [ ] **Step 3:** `compact.rs` 新增 `manual_compact` 函数：执行 `compact_messages_with_llm`（传入 progress 闭包发 `CompactProgress` 事件），返回 `Option<CompactOutcome>`。包含 PreCompact/PostCompact hook（与 `auto_compact` 一致）
- [ ] **Step 4:** loop_runner 主循环中 `idle_until_resume_or_shutdown` 返回 `CompactRequested` 时：调 `manual_compact`（有 sink+client+system_prompt+context_size 全参数），替换 messages，发 `MessagesSync`，然后 `continue`（回 idle）
- [ ] **Step 5:** `auto_compact` 调 `compact_messages_with_llm` 时传入 progress 闭包（经 sink 发 CompactProgress）
- [ ] **Step 6:** 更新 loop_runner 测试中 `IdleResult` 的穷尽 match
- [ ] **Step 7:** `cargo test -p aemeath-runtime --lib business::chat::looping`

## Task 5: SDK ChatEvent + runtime_event_to_sdk_event

**Files:** `packages/sdk/src/chat_event.rs`、`agent/features/runtime/src/core/client/event.rs`

- [ ] **Step 1:** `ChatEvent` 新增 `CompactProgress { stage: String, current: Option<u32>, total: Option<u32> }`
- [ ] **Step 2:** `runtime_event_to_sdk_event` 新增映射分支
- [ ] **Step 3:** 更新测试 sink 的穷尽 match
- [ ] **Step 4:** `cargo check -p aemeath-sdk && cargo check -p aemeath-runtime`

## Task 6: TUI CompactProgressModel + RuntimeModel + RuntimeIntent

**Files:** `apps/cli/src/tui/model/runtime/compact_progress.rs`（新）、`model.rs`、`intent.rs`、`mod.rs`

- [ ] **Step 1:** 新增 `CompactProgressModel`（stage/current/total + `ratio()` + `label()`）
- [ ] **Step 2:** `RuntimeModel` 加 `compact_progress: Option<CompactProgressModel>`
- [ ] **Step 3:** `RuntimeIntent` 加 `SetCompactProgress { stage, current, total }`
- [ ] **Step 4:** `apply`：`SetCompactProgress` 设值；`StopSpinner` 清 compact_progress
- [ ] **Step 5:** `cargo check -p cli`

## Task 7: UiEvent + sdk_event_to_ui_event

**Files:** `apps/cli/src/tui/app/event.rs`、`effect/session/processing.rs`

- [ ] **Step 1:** `AppAction` 新增 `CompactProgress { stage, current, total }`
- [ ] **Step 2:** `sdk_event_to_ui_event` 新增映射
- [ ] **Step 3:** `log_sdk_event` 新增日志分支
- [ ] **Step 4:** `cargo check -p cli`

## Task 8: map_agent_event + StopSpinner 清理

**Files:** `apps/cli/src/tui/adapter/agent_event.rs`

- [ ] **Step 1:** `map_agent_event` 处理 `UiEvent::CompactProgress` → `SetCompactProgress`
- [ ] **Step 2:** 确认 `PostCompact → StopSpinner` 路径同时清 compact_progress（Task 6 Step 4 已在 apply 中处理）
- [ ] **Step 3:** `cargo check -p cli`

## Task 9: LiveStatusViewModel + LiveStatusAssembler

**Files:** `apps/cli/src/tui/view_model/live_status.rs`、`view_assembler/live_status.rs`

- [ ] **Step 1:** `view_model` 新增 `CompactProgressView { ratio: f64, label: String }`
- [ ] **Step 2:** `LiveStatusViewModel` 加 `compact_progress: Option<CompactProgressView>`
- [ ] **Step 3:** `LiveStatusAssembler::assemble` 从 `runtime.compact_progress` 派生 view
- [ ] **Step 4:** 更新所有 `LiveStatusViewModel` 构造点（测试夹具等）补 `Default`
- [ ] **Step 5:** `cargo test -p cli --lib tui::view_assembler::live_status`

## Task 10: OutputArea 渲染 Gauge widget

**Files:** `apps/cli/src/tui/render/output_area/render.rs`、`status_line.rs`

- [ ] **Step 1:** `append_status_lines` 中 spinner 行后、task lines 前，如有 compact_progress，预留 1 行 screen_line_map entry
- [ ] **Step 2:** `render` 中 Paragraph 渲染后，计算 Gauge 的 1 行 Rect（spinner 行的下一行），用 `Gauge::default().ratio(ratio).label(label).render(rect, buf)` 渲染
- [ ] **Step 3:** 确保 `trim_to_area_height` 正确处理 Gauge 行
- [ ] **Step 4:** 新增渲染测试：验证 compact_progress 存在时 Gauge 区域有带色 cell
- [ ] **Step 5:** `cargo test -p cli --lib tui::render::output_area`

## Task 11: slash.rs /compact 改为走事件通道 + 全量验证

**Files:** `apps/cli/src/tui/app/slash.rs`、`agent/features/runtime/src/business/agent/runner/loop_helpers.rs`

- [ ] **Step 1:** `/compact` 第一个入口（`slash.rs:46`）：不再调 `ac.compact_messages()`，改为 `push_input_event(ChatInputEvent::Compact)` + 设 spinner `Compacting`
- [ ] **Step 2:** `CommandAction::Compact` 分支（`slash.rs:273`）：同样改为 `push_input_event(ChatInputEvent::Compact)`
- [ ] **Step 3:** `loop_helpers.rs` `compact_if_needed` 添加注释说明 sub-agent 不发进度
- [ ] **Step 4:** `cargo fmt --check`
- [ ] **Step 5:** `cargo clippy -p cli -p aemeath-runtime -p aemeath-sdk -- -D warnings`
- [ ] **Step 6:** `cargo test -p cli -p aemeath-runtime -p aemeath-sdk`
- [ ] **Step 7:** 终端冒烟测试
