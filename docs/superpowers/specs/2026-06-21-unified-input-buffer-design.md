# 设计：统一输入缓冲区 + 占位按 id 清 + MessagesSync 退出 display

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/390 （A3 阶段，重塑自原「TUI 纯化」）
> 伞 Issue: #394　关联：#391（撤回/召回/clear/abort，本设计前置）
> 日期: 2026-06-21
> 状态: 设计待 review

## 1. 背景与根问题

#390 A1/A2 已合入 main：常驻会话 actor + `InputId` + 批量 `UserMessagesAdded` 归宿事件 + 移除内容去重。原计划 A3「TUI 纯化」执行中暴露**输入双路径**的根本问题：

- **事件通道**（`SendChatInputEvent`）：携 `InputId` → runtime 回带同 id 的 `UserMessagesAdded` → 可按 id 清占位。
- **文本队列**（`push_queue` → TUI `input_queue: VecDeque<String>` → `TuiQueueDrainPort` → runtime `drain_queued_input`）：**无 id**，runtime drain 后 `classify_text(text, Vec::new())` **生成新 id**，与占位 id 永不配对。

`submit_user_input_event` **同时**走两条（`push_queue` + `SendChatInputEvent`，两个 port 都进了 `ChatRequest`）。

**已实测确认**（run_loop_gate + 两路同携同一条提交）：`drain_sources` 两路都收 + A2 已移除去重 → `appended_user_messages == 2`、`messages.len() == 2`，即**双 append**。线上目前未暴露，唯一原因是 `MessagesSync` handler 的 `self.input.clear_queue()`（`ui_event.rs`）在回合内 gate drain 队列**之前**清空了 `input_queue`。一旦 A3 让 `MessagesSync` 退出 display（含移除 `clear_queue`），该掩盖消失 → 双 append 复活。

**结论**：文本队列作为 runtime 输入源必须废弃，输入统一为单一带 id 事件流。

## 2. 指导原则

待处理输入**单一真相 = runtime `PendingInputBuffer`**（由 `ChatInputEvent` 事件通道喂入）。TUI 只发带 id 事件、只靠归宿事件按 id 维护占位/回显。`MessagesSync` 退出 display，降级为镜像 + 落盘。

## 3. 目标架构

### 3.1 单一输入路径（废弃并行队列）
- `submit_user_input_event`：只 `SendChatInputEvent { UserMessage { id, text, images } }`；**删除** `self.input.push_queue(...)`。
- `ChatRequest.queue_drain` → `None`；**移除** `TuiQueueDrainPort`、`UiEvent::DrainQueuedInput`、runtime `drain_sources` 的队列分支（或在 `queue_drain: None` 下使其恒为 no-op，最终删除死路径）。
- 净效果：每条输入唯一经事件通道到达 runtime，携 `InputId`；双 append 根因消除。

### 3.2 占位生命周期（单一规则）
- **创建**：仅 `submit_user_input_event`（真实用户消息，携 `InputId`）。占位块 `QueuedUserMessage` 已携 `input_id`（A3 Task 1 已落地，commit `d193f3ed`）。
- **清除**：仅 `UserMessagesAdded` 按 `id`（`clear_queued_submission_by_id` + 顺序 `append_user_echo`）。
- **busy 期 slash/control**（`key.rs:174`）：**不再创建 `QueuedUserMessage` 占位**（决策 A）。ControlCommand 是命令、无 `UserMessagesAdded` 归宿；靠现有状态通知（"message event queued"）反馈即可，避免剥离 MessagesSync 全清后产生无法按 id 清除的孤儿占位。

### 3.3 MessagesSync 退出 display
- `UiEvent::MessagesSync(msgs)` handler **删除**：`old_len` diff、`new_user_texts` 收集、`clear_queued_submission_echo()` 全清、`append_user_echo` 循环、`input.clear_queue()`。
- **保留**：`self.chat.messages = msgs`（镜像）+ `Effect::SaveSession { notify: false }`（落盘）。
- resume 显示不受影响：resume 走独立 `resume_session_messages` → `render_history_message`（`effect/session/resume.rs`），与 MessagesSync display 无关。

### 3.4 Up 键（决策 B）
- 当前 `key.rs:224`「`input_queue` 非空 → 全部恢复到输入框 + 清占位」依赖被废弃的队列，**移除该分支**。Up 回归光标 / 输入历史导航（已有 history 能力）。
- 「召回未发送的 pending 输入」完整能力（按 id 跨异步边界从 `PendingInputBuffer` 撤回）归 **#391**。

## 4. 明确不做（归 #391）
- 按 `InputId` 撤回 / 召回 pending 输入（`UserMessageWithdrawn { id }` + 与 `UserMessagesAdded` 互斥竞态）。
- `/clear` reset vs abort 语义统一、reset/cancel 接口。

## 5. 验收
- **纯逻辑单测**：单一事件路径 append 一次（无双 append）；占位按 id 清 + 顺序回显；MessagesSync 不再产生用户回显块、仅更新镜像；busy-slash 不建占位。
- **回归测试**：删除文本队列后，原队列相关测试（DrainQueuedInput / push_queue 路径）移除或改写。
- **交互人工验收（需用户在场）**：首条 / busy 期插话回显、占位按 id 清除、**不重 / 不漏 / 不错位**、resume 后历史显示正常、busy 期连发多条、busy-slash 命令行为。

## 6. 风险
- 设计 §6：A1–A3 强交互、失败模式纯视觉，**MUST 与用户配合 TUI 验收**。
- 删除队列路径牵动 busy-submit、Up 键、DrainQueuedInput、spinner「排队中」状态——需确认「排队中」反馈在占位块驱动下仍正常。
- 短期 UX 代价：busy 期排队消息暂不能 Up 键拉回编辑（#391 补上）。

## 7. 非目标（YAGNI）
- 不动 provider / 持久化 JSON 格式。
- 不在本设计实现撤回/召回（仅保证不破坏其后续接入）。
