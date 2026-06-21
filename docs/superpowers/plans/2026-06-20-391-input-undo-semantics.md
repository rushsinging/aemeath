# #391 输入/回合撤销语义统一 Implementation Plan

> 配套设计：`docs/superpowers/specs/2026-06-20-input-undo-semantics-design.md`
> 伞 Issue: #394　前置: #390 A1–A4 已合入
> For agentic workers: 用 superpowers:subagent-driven-development 或 superpowers:executing-plans 执行，checkbox 跟踪进度。

**Goal:** 统一 `/clear` 为「整段会话重置」（**不打断当前回合**，idle 立即执行 / busy 排队等回合自然结束回 idle 后执行，取代暴力 drop 通道）；新增**批量撤回** pending 输入（一次性清空 buffer + 换行拼接还原输入框 + busy Up 触发）；删除 `SpinnerPhase::ThinkingQueued` 孤儿状态与 `/clear history`、`/clear all` 死代码。

**Architecture:** runtime 新增 `reset_session()`（**纯经 input_events 发 `Reset` 事件，不碰 CancellationToken**；loop idle gate drain 到 Reset → 清 messages/buffer + 发 `SessionReset`）；`PendingInputBuffer::drain_all()` 批量清空 + `UserMessagesWithdrawn{texts}` 事件链；TUI 只发意图、靠通知同步镜像。强制中止回合 = Esc/Ctrl+C（现有 cancel 机制，**不动**）。

**Tech Stack:** Rust workspace（sdk / runtime / cli），tokio mpsc，ratatui TUI。

## 全局约束

- **MUST** 持久化 JSON 格式不变（新增事件仅运行时，不落盘）。
- **MUST** runtime 拥有真相：TUI **NEVER** 靠 drop 通道实现 reset，只发意图（`reset_session()`）+ 靠 `SessionReset` 通知同步。
- **MUST** `/clear` **不打断当前回合**：`reset_session()` 只发 `Reset` 事件，**NEVER** 调 `cancel_token.cancel()`；busy 态 Reset 排队等回合自然结束回 idle gate 执行。强制中止 = Esc/Ctrl+C（**NEVER** 改现有 cancel 语义）。
- **MUST** 撤回为批量操作：`drain_all()` 非空则清空+发 `UserMessagesWithdrawn{texts}`，空则 no-op；TUI 收到后清全部占位 + `texts.join("\n")` 还原输入框。
- **NEVER** 新增 `/stop`（Esc/Ctrl+C busy 已覆盖强制中止回合）。
- 验证门禁：`cargo clippy --all-targets --all-features`（0/0）、`cargo test --workspace`、`bash .agents/hooks/check-architecture-guards.sh`。worktree 内先 `source .cargo/set-target.sh`。
- TDD：每个带逻辑的 Task **MUST** 先写失败测试再实现。

---

# 阶段 S1：runtime `reset()` 接口（独立 PR）

> 行为变化：✅ 新增。runtime 暴露 `reset_session()`，loop 收到 Reset 事件后清 messages/buffer + 发 SessionReset。本阶段 **不接线 `/clear`**（S2 才接），保持零回归。

### Task S1-1：事件类型链（sdk/runtime，additive 无人 emit）

**Files:**
- Modify: `packages/sdk/src/chat.rs:38`（`ChatInputEvent` 加 `Reset` 变体）
- Modify: `agent/features/runtime/src/business/chat/looping/events.rs:57`（`RuntimeStreamEvent` 加 `SessionReset`）
- Modify: `packages/sdk/src/chat_event.rs`（`ChatEvent` 加 `SessionReset`，透传）
- Modify: `apps/cli/src/tui/effect/...` → `UiEvent` 透传点（grep `ChatEvent::` 找转换处，加 `SessionReset` no-op arm 占位；S1-4 才实现）

**Interfaces:**
- `ChatInputEvent::Reset`（无字段）
- `RuntimeStreamEvent::SessionReset`（无字段）
- `ChatEvent::SessionReset`（无字段）

- [ ] **Step 1: 加 `ChatInputEvent::Reset`**（chat.rs，接 `Cancel` 后），doc 注释「整段会话重置：清 messages+pending，通知 TUI」
- [ ] **Step 2: 加 `RuntimeStreamEvent::SessionReset`**（events.rs，接 `UserMessagesAdded` 附近），doc「loop 执行 reset 清理后发出」
- [ ] **Step 3: 加 `ChatEvent::SessionReset`**（chat_event.rs）+ 转换处透传（`RuntimeStreamEvent → ChatEvent` 映射，grep 定位）
- [ ] **Step 4: `UiEvent`/转换链补 `SessionReset` no-op arm**（防 non-exhaustive 编译错误；TUI handler 留 S1-4 实现）
- [ ] **Step 5: 编译通过** — `cargo build --workspace`

### Task S1-2：`AgentClient::reset_session()` trait + impl

**Files:**
- Modify: `packages/sdk/src/client.rs:73`（trait 加 `reset_session()`，紧邻 `cancel()`）
- Modify: runtime `AgentClient` impl（grep `impl AgentClient` 定位，紧邻 `cancel()` impl）
- Modify: runtime 现有测试 mock `AgentClient`（grep `impl.*AgentClient` 找 mock，补 `reset_session` stub）

**Interfaces:**
- `trait AgentClient { fn reset_session(&self); }`（与 `cancel()` 同签名：同步、无返回）

> **关键**：`reset_session()` **只发 `Reset` 事件，NEVER 调 `cancel_token.cancel()`**。reset 不打断当前回合。

- [ ] **Step 1: 写失败测试** — runtime impl 测试：调用 `reset_session()` 后，`input_events` 收到一条 `ChatInputEvent::Reset`，且 `cancel_token` **未被触发**（`is_cancelled()==false`）
- [ ] **Step 2: 确认失败** — 编译失败（方法未定义）
- [ ] **Step 3: trait 加方法**（client.rs:73，`fn reset_session(&self);`，doc「整段重置：经通道发 Reset 事件，loop idle gate drain 时清空。不打断当前回合」）
- [ ] **Step 4: runtime impl**（紧邻 `cancel()` impl）：`self.input_events.send(ChatInputEvent::Reset)`（send 失败=loop 已停，忽略）。**NEVER** 调 `cancel_token.cancel()`
- [ ] **Step 5: 补所有 mock** 的 `reset_session` stub（no-op 即可）
- [ ] **Step 6: 测试通过** — `cargo test --workspace`

### Task S1-3：loop idle gate 内 Reset 事件处理（核心）

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/input_gate.rs`（idle gate drain `Reset`）
- Modify: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`（新增 helper）
- Test: `agent/features/runtime/src/business/chat/looping/loop_runner_tests.rs`

**Interfaces:**
- 新增 helper（loop_runner.rs）：`async fn reset_session_state(ctx, sink) -> ()`：`ctx.messages.clear(); ctx.pending_input.clear(); sink.send(SessionReset);`
- idle gate：drain 到 `Reset` → 调 `reset_session_state`（直接清，无在途回合）

> **关键设计**：reset **只在 idle gate 处理**。busy 态时 `Reset` 事件排在通道里，等当前回合**自然结束**（Done/Cancelled）回到 idle gate 后才被 drain。**NEVER** 在 busy 态干预 LLM 调用、**NEVER** 碰 CancellationToken、**NEVER** 在 `cancel_to_idle` 收口处做特殊处理。

- [ ] **Step 1: 写失败测试**（loop_runner_tests.rs）：
  - 测 A（idle）：idle 态，pending buffer 有 2 条，发 `Reset` → loop 处理后 `messages.is_empty() && pending.is_empty() && 收到 SessionReset`
  - 测 B（busy 排队）：busy 态（mock LLM 会正常返回），发 `Reset` → **不立即清理**；回合 Done 回 idle 后 → drain 到 Reset → 清空 + SessionReset
- [ ] **Step 2: 确认失败** — `cargo test reset` → FAIL（Reset 未处理）
- [ ] **Step 3: 实现 `reset_session_state` helper**（loop_runner.rs：`messages.clear(); pending_input.clear(); sink.send(SessionReset);`）
- [ ] **Step 4: idle gate drain Reset**（input_gate.rs idle_until 路径，drain 事件循环里 match `Reset` → 调 helper）。回合 gate drain 时**跳过** `Reset`（留给 idle gate 处理）
- [ ] **Step 5: 测试通过** — `cargo test reset`
- [ ] **Step 6: clippy + 门禁** — `cargo clippy --all-targets --all-features` + `check-architecture-guards.sh`
- [ ] **Step 7: 提交** — `feat(runtime): reset_session() 接口 + idle gate Reset 清理 (#391 S1)`

### Task S1-4：TUI SessionReset handler（清镜像 + 占位 + output_area）

**Files:**
- Modify: `apps/cli/src/tui/...`（grep `ChatEvent::` 找 UiEvent→handler 转换处，`SessionReset` arm 实现）
- Modify: `apps/cli/src/tui/app/state/chat.rs:70`（`reset_runtime_state` 复用，或新增 reset 专用方法）

**Interfaces:**
- 收到 `SessionReset` → `chat.messages.clear()` + `output_area.clear()` + `clear_queued_submission_echos()`（grep 占位清理）+ `reset_runtime_state()` + `StatusNotice::info("Session cleared")`

- [ ] **Step 1: 实现 handler**（把 S1-1 Step 4 的 no-op arm 改为实际清理）
- [ ] **Step 2: 编译 + 单测**（若有 mock handler 测试）
- [ ] **Step 3: clippy + 门禁**

---

# 阶段 S2：接线 `/clear` → `reset_session()` + 移除 drop 通道（独立 PR，TUI 人工验）

> 行为变化：✅ reset 路径变。idle/busy `/clear` 统一调 `reset_session()`，移除暴力 drop `input_event_tx` + run_loop 自愈重建。**强交互，MUST 与用户配合 TUI 验收。**
> 依赖：S1 合入。

### Task S2-1：idle `/clear` 改调 `reset_session()`

**Files:**
- Modify: `apps/cli/src/tui/app/slash.rs:42-48`（当前 `chat.messages.clear()` + `reset_runtime_state()`）

**改动：** idle `/clear` 不再本地清镜像/drop 通道，改为调 `agent_client.reset_session()`（由 S1 的 SessionReset 通知驱动 TUI 清镜像）。镜像清理移到 SessionReset handler（S1-4 已实现）。

- [ ] **Step 1: 改 slash.rs** — `/clear` arm：`spawn_refs.agent_client.reset_session()` + `StatusNotice::info("Clearing session...")`；**移除** `chat.messages.clear()` / `reset_runtime_state()`（后者含 drop 通道，现由 reset_session 取代）
- [ ] **Step 2: 确认 S1-4 handler 覆盖清理**（messages/output_area/占位/reset_runtime_state 里的非通道字段）——注意 `reset_runtime_state` 原本 drop 了 `input_event_tx`，S1-4 handler **不可再 drop 通道**，改为保留通道（loop 身份连续）。若 S1-4 复用了 `reset_runtime_state`，需拆分：通道 drop 逻辑移除，其余状态重置保留
- [ ] **Step 3: TUI 人工验** — 启动 TUI，输入几轮对话后 `/clear`：确认 messages 清空、output_area 清空、输入框保留、可继续对话（loop 未退出）
- [ ] **Step 4: clippy + 门禁**

### Task S2-2：busy `/clear` 改调 `reset_session()`（排队等 idle，不 Abort）

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/input_gate.rs:208`（当前 `/clear`→`ControlCommandKind::Abort`）

**改动：** busy 态 `/clear` 经事件通道到达 runtime 后，不再映射为 Abort，改为发 `Reset` 事件（**排队等回合自然结束回 idle 后执行**）。**不中止当前回合。**

> 细节：busy `/clear` 经 `ControlCommand{raw:"/clear"}` 到达 loop gate。gate drain 时识别该 raw → 发 `ChatInputEvent::Reset`（排队等 idle gate 处理）。或 TUI 端 busy `/clear` 直接调 `agent_client.reset_session()`（不经 ControlCommand 通道），更直接——推荐后者，语义更清晰。

- [ ] **Step 1: 写失败测试**（loop_runner_tests.rs）— busy 态发 `/clear` → **不立即清理**（messages 保留），回合 Done 回 idle 后 → drain 到 Reset → 清空 + SessionReset
- [ ] **Step 2: 改 input_gate.rs** — `/clear` 识别分支改为发 `Reset`（不再 Abort）
- [ ] **Step 3: 移除 `ControlCommandKind::Abort` 中 `/clear` 的映射**（Abort 保留给其他控制命令或废弃，需 grep 确认 Abort 是否还有其他用途；若 Abort 仅服务 `/clear` 则一并清理）
- [ ] **Step 4: 测试通过** — `cargo test clear`
- [ ] **Step 5: TUI 人工验** — busy 态（LLM 响应中）输入 `/clear`：确认**不打断 LLM**，回合自然结束后 messages 全部清空 + 回 idle + 可继续
- [ ] **Step 6: clippy + 门禁**
- [ ] **Step 7: 提交** — `refactor(tui,runtime): /clear 统一调 reset_session()，移除 drop 通道重建 (#391 S2)`

### Task S2-3：移除 run_loop 自愈重建分支

**Files:**
- Modify: `apps/cli/src/tui/app/run_loop.rs:234-239`（当前 `if input_event_tx.is_none() { ensure_persistent_processing() }`）

**改动：** reset 不再 drop 通道（S2-1/S2-2），loop 身份连续，自愈重建分支成为死代码，移除。

- [ ] **Step 1: 确认无其他路径会 drop `input_event_tx`**（grep `clear_input_event_buffer` / `input_event_tx = None` 全部调用点，确认仅 run_loop 收尾退出时保留）
- [ ] **Step 2: 移除 run_loop.rs:234-239 自愈分支** + 相关注释
- [ ] **Step 3: TUI 人工验** — `/clear` 多次 + 正常对话交替，确认 loop 稳定不重建
- [ ] **Step 4: clippy + 门禁**

---

# 阶段 S3：批量撤回 pending 输入 + Up 触发（独立 PR，TUI 人工验）

> 行为变化：✅ 新增撤回能力。`PendingInputBuffer::drain_all()` 批量清空 + `UserMessagesWithdrawn{texts}` 事件链 + busy Up 触发 + 换行拼接还原输入框。**强交互，MUST TUI 验收。**
> 依赖：S1 合入（复用 loop idle gate drain 模式）。

### Task S3-1：`PendingInputBuffer::drain_all()`（批量清空）

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/loop_helpers.rs`（或 buffer 定义处，grep `PendingInputBuffer`）
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- `pub fn drain_all(&mut self) -> Vec<ChatInputEvent>`：取出并清空整个 `VecDeque`，返回所有事件（空则返回空 Vec）。

- [ ] **Step 1: 写失败测试**（2 例）：
  - 非空：push UserMessage(A) + UserMessage(B) → drain_all() 返回 vec![A, B] → buffer 空
  - 空：buffer 空 → drain_all() 返回 vec![] → buffer 仍空
- [ ] **Step 2: 确认失败** — `cargo test drain_all` → FAIL
- [ ] **Step 3: 实现 `drain_all`**（`std::mem::take` 或 `VecDeque::drain` 收集）
- [ ] **Step 4: 测试通过**
- [ ] **Step 5: 提交** — `feat(runtime): PendingInputBuffer::drain_all 批量清空 (#391 S3)`

### Task S3-2：事件类型链（WithdrawAll 输入 + Withdrawn 批量输出）

**Files:**
- Modify: `packages/sdk/src/chat.rs:38`（`ChatInputEvent::WithdrawAll`，无字段）
- Modify: `agent/features/runtime/src/business/chat/looping/events.rs:57`（`RuntimeStreamEvent::UserMessagesWithdrawn { texts: Vec<String> }`）
- Modify: `packages/sdk/src/chat_event.rs`（`ChatEvent::UserMessagesWithdrawn { texts }` 透传）+ UiEvent 链

- [ ] **Step 1: 加三个变体**（additive）
- [ ] **Step 2: 转换链透传** + no-op TUI handler 占位（S3-4 实现）
- [ ] **Step 3: 编译通过**

### Task S3-3：loop 内 WithdrawAll 处理

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/input_gate.rs`（idle gate + 回合 gate drain `WithdrawAll`）
- Test: `loop_runner_tests.rs`

**改动：** drain 到 `WithdrawAll` → `buffer.drain_all()`；若返回非空 Vec → 从中提取所有 `UserMessage` 的 text → `send_event(UserMessagesWithdrawn{texts})`；返回空 Vec → no-op。

- [ ] **Step 1: 写失败测试**：
  - 非空：buffer 有 2 条 → 发 WithdrawAll → 收到 UserMessagesWithdrawn{texts:[a,b]} + buffer 空
  - 空：buffer 空 → 发 WithdrawAll → 无事件
- [ ] **Step 2: 确认失败**
- [ ] **Step 3: 实现 gate drain WithdrawAll**（idle + 回合 gate）
- [ ] **Step 4: 测试通过** + clippy + 门禁

### Task S3-4：TUI handler（清全部占位 + 换行拼接 RestoreText）

**Files:**
- Modify: TUI `ChatEvent::UserMessagesWithdrawn` handler（S3-2 占位处）
- Modify: `apps/cli/src/tui/model/conversation/model.rs`（`clear_all_queued_submission_echos`，grep 现有占位清理，或新增批量清理）
- Modify: `apps/cli/src/tui/model/input/...`（新增 `InputIntent::RestoreText { text }`：替换文档内容 + 光标置尾）

**改动：** 收到 `UserMessagesWithdrawn{texts}` → `clear_all_queued_submission_echos()` + `apply(InputIntent::RestoreText { text: texts.join("\n") })`。

- [ ] **Step 1: 加 `InputIntent::RestoreText`** + input model 处理（set document = text, cursor end）
- [ ] **Step 2: 实现 handler**（占位→实际：清全部占位 + texts.join("\n") 还原）
- [ ] **Step 3: 编译 + 单测**

### Task S3-5：Up 键撤回全部（busy 态）

**Files:**
- Modify: `apps/cli/src/tui/app/update/key.rs:218-225`（当前 Up 只 MoveCursorUp）

**改动：** busy 态 && `queued_submissions` 非空 → 发 `ChatInputEvent::WithdrawAll`（等 S3-4 handler 回调批量清占位 + 换行拼接还原文本，**不本地预清**）。idle 态保持 MoveCursorUp（历史导航）不变。

- [ ] **Step 1: 写 key 处理逻辑**（busy + queued 非空分支 → 发 WithdrawAll）
- [ ] **Step 2: TUI 人工验** — busy 态排队 2 条消息（文本 "aaa"、"bbb"）→ Up → 输入框显示 "aaa\nbbb" + 全部占位消失；无 pending 时 Up 无反应（drain_all 空返回 no-op）
- [ ] **Step 3: clippy + 门禁**
- [ ] **Step 4: 提交** — `feat(tui,runtime): 批量撤回 pending 输入 + Up 触发 (#391 S3)`

---

# 阶段 S4：删除 `SpinnerPhase::ThinkingQueued` 孤儿状态（独立 PR）

> 行为变化：❌ 纯清理（死状态，无写入点）。可与 S5 合并为一个 PR。
> 依赖：无（独立）。

### Task S4-1：移除变体 + 渲染/动画/测试臂

**Files:**
- Modify: `apps/cli/src/tui/spinner.rs:15`（移除 `ThinkingQueued` 变体）
- Modify: `apps/cli/src/tui/render/display/live_status.rs:66`（移除渲染 arm）
- Modify: `apps/cli/src/tui/render/display/spinner_anim.rs:105`（移除动画 arm）
- Modify: `apps/cli/src/tui/render/display/live_status.rs:162`（移除 `phase_text` 测试引用）

- [ ] **Step 1: grep `ThinkingQueued` 全部引用** — `rg "ThinkingQueued"` 确认无写入点（A3 已删 DrainQueuedInput）
- [ ] **Step 2: 移除变体 + 全部 match arm + 测试引用**
- [ ] **Step 3: 编译 + 测试通过** — `cargo build -p cli && cargo test -p cli spinner`
- [ ] **Step 4: clippy + 门禁**
- [ ] **Step 5: 提交** — `refactor(tui): 删除 ThinkingQueued 孤儿状态 (#391 S4)`

---

# 阶段 S5：删除 `/clear history`、`/clear all` 死代码（独立 PR）

> 行为变化：❌ 纯清理（死路径，TUI 无 handler）。可与 S4 合并。
> 依赖：无（独立）。

### Task S5-1：移除 misc.rs 死分支 + ClearAllHistory

**Files:**
- Modify: `agent/features/runtime/src/core/command/commands/misc.rs:41-51`（删 `/clear history`、`/clear all` 分支）
- Modify: `packages/sdk/src/commands.rs`（删 `ConfirmAction::ClearAllHistory`，需 grep 确认无其他引用）

- [ ] **Step 1: grep `ClearAllHistory` + `clear history` + `clear all`** 确认 TUI `handle_command_action` 无 arm（已是死代码）
- [ ] **Step 2: 移除 misc.rs 死分支**（保留 `/clear`→`Clear`，S2 改为映射 reset）
- [ ] **Step 3: 移除 `ConfirmAction::ClearAllHistory`**（若无其他引用）
- [ ] **Step 4: 编译 + 测试** — `cargo build --workspace && cargo test --workspace`
- [ ] **Step 5: clippy + 门禁**
- [ ] **Step 6: 提交** — `refactor(runtime): 删除 /clear history、/clear all 死代码 (#391 S5)`

---

## 阶段依赖与并行性

```
S1 (reset 接口) ─┬─→ S2 (/clear 接线，依赖 S1)
                 └─→ S3 (撤回，依赖 S1 的 loop drain 模式)
S4 (删 ThinkingQueued) ─┐ 可并行，无依赖
S5 (删死代码)          ─┘ 可并行，无依赖
```

- **S4/S5 可随时并行**（纯清理，无依赖）。
- **S1 是 S2/S3 的前置**（提供 reset_session + loop drain）。
- **S2/S3 可并行**（S2 改 /clear 路径，S3 加撤回，改动文件不重叠；但都依赖 S1）。
- 每阶段 = 独立 PR，合并顺序：S4/S5 先行 → S1 → S2/S3。

## TUI 人工验收清单（S2/S3）

执行 S2/S3 时，**MUST** 与用户逐项确认：
1. **idle /clear**：对话几轮后 `/clear` → messages + output_area 全清，输入框保留，可继续对话。
2. **busy /clear**：LLM 响应中 `/clear` → **不打断 LLM**，回合自然结束后 messages 全清 + 回 idle + 可继续。
3. **/clear 后 loop 稳定**：连续 `/clear` + 对话交替，无卡死/重建。
4. **Up 撤回全部**：busy 排队 2 条（"aaa"、"bbb"）→ Up → 输入框显示 "aaa\nbbb" + 全部占位消失。
5. **Up no-op**：无 pending 时 Up 无反应（drain_all 空返回 no-op，不干预光标）。

> 失败模式纯视觉（镜像不同步、占位残留、文本未还原），`-qv` 无法覆盖，**MUST 人工验**。