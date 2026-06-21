# 设计：输入/回合撤销语义统一（/clear 重置 + 批量撤回 pending）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/391
> 伞 Issue: #394　前置: #390 A1–A4 已合入
> 关联设计:
> - `2026-06-20-persistent-session-actor-design.md` §6/§8（预留 reset/cancel 接口）
> - `2026-06-21-unified-input-buffer-design.md` §3.4（Up 召回归本 issue）

## 1. 背景与根问题（代码佐证）

`/clear` 语义**分散、重载、随忙/闲而异**，跨 3 层；#390 actor 模型落地后还产生新的同步债：

1. **idle `/clear`（`slash.rs:42-48`）**：TUI 本地 `chat.messages.clear()`（**仅镜像**）+ `reset_runtime_state()`（**暴力 drop `input_event_tx` 通道**，靠 `run_loop.rs:234` 自愈重建常驻 loop）。问题：runtime 的真实 `messages` 不在 TUI 手里，drop+重建是「重启 loop」而非「语义级 reset」；重建竞态窗口期提交可能丢失。
2. **busy `/clear`（`input_gate.rs:208`）**：`/clear`→`ControlCommandKind::Abort`→`AbortCurrentLoop`→`messages.pop()` 回滚本回合已 append 输入 + `cancel_to_idle` 回空闲。语义是「**中止回合**」≠「清空会话」，与 idle 完全不同。
3. **runtime 注册 `/clear`（`misc.rs:41-51`）**：`/clear`→`Clear` action（走 idle 路径）；`/clear history`→**只返回字符串、不实际清理**；`/clear all`→`Confirm(ClearAllHistory)`（**TUI 端 `handle_command_action` 无对应 arm**）。后两者基本是死代码。
4. **Esc 键（`key.rs:150-160`）**：busy 态 `agent_client.cancel()` + "Interrupted"——**中止回合能力已存在**；idle 态 `Esc→InputIntent::Clear`（清输入框文档）。但「中止回合」目前**只能靠 Esc**，`/clear` 在 busy 态被挪用作 abort。
5. **ThinkingQueued 孤儿状态（`spinner.rs:15`）**：A3 删除了唯一写入点（`DrainQueuedInput` handler），变体+渲染臂+测试残留为死状态。「排队中」视觉反馈现由 `QueuedUserMessage` 占位块承载。
6. **撤回接口缺失**：`PendingInputBuffer` 只有 `push/extend/drain`，无 `withdraw`；占位生命周期只有 create(`enqueue_submission_echo`)+clear_by_id(`UserMessagesAdded`)，无法跨异步边界撤回未处理输入；Up 键召回能力（A3 §3.4）被搁置待本 issue 补。

## 2. 指导原则

- **单一命令单一语义**：`/clear` 恒为「整段会话重置」（**不打断当前回合**，等 idle 后执行）；强制中止回合由 **Esc/Ctrl+C（busy 态，现状已有）** 承担，与 `/clear` 彻底解耦。`/stop` 不新增（YAGNI）。
- **runtime 拥有真相**：reset 由 runtime 语义级执行（清 messages + 重置状态 + 通知 TUI），TUI 只发意图、靠通知同步，**NEVER** 靠 drop 通道实现 reset。
- **撤回是批量操作**：`clear_all()` 一次性清空整个 pending buffer，把全部被撤回文本用换行符拼接还原到输入框。

## 3. 目标架构

### 3.1 命令语义重映射

| 入口 | idle 态 | busy 态 |
|---|---|---|
| `/clear` | 整段重置（调 `reset_session()`，立即执行） | 整段重置（调 `reset_session()`，**Reset 事件排队，等回合结束回 idle 后执行**） |
| `Esc` | 清输入框文档（不变） | **强制中止当前回合**（现状：`agent_client.cancel()`，**不变**） |
| `Ctrl+C` | 清输入框/退出（不变） | **强制中止当前回合/强退**（不变） |

- `/clear` **不打断当前回合**：busy 态调 `reset_session()` 只经通道发 `Reset` 事件，该事件排在 pending 队列中，等当前 LLM 回合**自然结束**回到 idle gate 时才被 drain 执行。强制中止回合 = **Esc/Ctrl+C**（现有 cancel 机制，不变）。
- `/clear` 语义统一 = 「整段重置」。idle 立即生效；busy 排队等 idle 生效。两种情况最终都清空 messages + pending buffer。
- `/clear history`、`/clear all` 死路径删除（`misc.rs`）；`/clear` 统一映射到 reset。

### 3.2 runtime `reset()` 接口（取代暴力 drop 通道，纯事件驱动）

**设计要点**：reset **不走 CancellationToken、不强制打断**。`reset_session()` 只做一件事——经 input_events 通道发 `ChatInputEvent::Reset` 事件。该事件在 loop 的 **idle gate** 被 drain 时执行清理。

**`AgentClient::reset_session()` 设计**：

```
reset_session() = input_events.send(ChatInputEvent::Reset)
```

- **idle 态**：loop 在 idle gate 立即 drain 到 `Reset` → 执行清理。
- **busy 态**：loop 正在 `await` LLM stream，`Reset` 事件排在通道里等待。当前回合**自然结束**（Done/Cancelled）回到 idle gate 后，drain 到 `Reset` → 执行清理。**不打断 LLM 调用。**

**loop 在 idle gate drain 到 `Reset` 时执行**：
  1. `messages.clear()`（清空 runtime 真相源）
  2. `pending_input.clear()`（丢弃所有未处理输入，含排队的 `Reset` 之后的输入）
  3. `send_event(SessionReset)`（通知 TUI）
  4. 留在 idle 继续常驻 loop（**不退出**，等下一条输入）

> 与现有「drop `input_event_tx` 触发自愈重建」的区别：reset 是 loop 内部状态变更，**无通道销毁/重建竞态**，loop 身份连续，历史/配置/agent_runner/worktree 全保留。
>
> 与 CancellationToken 方案的区别：**不碰 cancel 机制**。reset 纯粹是 idle gate 的事件处理，不干预 LLM 调用，不触发现有 `cancel_to_idle` 路径。实现更简单、风险更低。

**竞态安全**：`Reset` 事件与 gate drain 在同一 loop 任务内串行执行（loop 是 messages/buffer 的单一 owner），无并发写入。busy 态下 `Reset` 排队等 idle，不会与正在进行的回合产生竞争。

### 3.3 批量撤回 pending 输入（一次性清空 + 文本还原）

撤回是**一次性清空整个 pending buffer** 的操作，并把全部被撤回文本用换行符拼接还原到输入框：

- runtime 侧：`PendingInputBuffer::drain_all() -> Vec<ChatInputEvent>`——取出并清空整个 `VecDeque`，返回所有被撤回的事件（用于提取文本）。
- 触发点：TUI 经 `input_events` 通道发 `ChatInputEvent::WithdrawAll`；loop 在 **idle gate** 和 **回合 gate drain 前** 收到时调用 `buffer.drain_all()`。
- **执行逻辑**：
  - 若 buffer 非空 → `drain_all()` 取出全部事件 → 从中提取所有 `UserMessage` 的 text → `send_event(UserMessagesWithdrawn { texts })`（携带被撤回的文本列表）。
  - 若 buffer 为空 → no-op（不发事件）。
  - 关键：drain_all 与 gate drain 串行（同一 loop 任务内，无并发），无竞态窗口。
- 新事件：`RuntimeStreamEvent::UserMessagesWithdrawn { texts: Vec<String> }` → `ChatEvent::UserMessagesWithdrawn { texts }` 透传到 `UiEvent`。
- TUI handler：收到 `UserMessagesWithdrawn{texts}` → `clear_all_queued_submission_echos()`（清全部占位）+ `apply(InputIntent::RestoreText { text: texts.join("\n") })`（换行拼接还原，光标置尾）。

**Up 键撤回**（`key.rs:218-225`）：busy 态且 `queued_submissions` 非空 → 发 `ChatInputEvent::WithdrawAll`（不直接本地清占位，等 runtime 确认后批量清 + 还原文本）。idle 态 Up 保持光标上移 / 历史导航不变。

### 3.4 ThinkingQueued 删除

- 移除 `SpinnerPhase::ThinkingQueued` 变体（`spinner.rs:15`）。
- 移除渲染臂（`live_status.rs:66`）、动画臂（`spinner_anim.rs:105`）、`phase_text` 测试引用（`live_status.rs:162`）。
- 排队视觉反馈唯一由 `QueuedUserMessage` 占位块承载。

### 3.5 死代码删除

- `misc.rs`：删 `/clear history`（返回字符串的死分支）、`/clear all`（`Confirm(ClearAllHistory)`，TUI 无 handler）；`/clear` 统一为 `CommandAction::Clear`（映射到 reset）。
- `ConfirmAction::ClearAllHistory`：若无其他引用则一并删（需 grep 确认 TUI `handle_command_action` 确实无 arm）。

## 4. 分阶段实施（每阶段独立 PR，TDD）

| 阶段 | 内容 | 行为变化 | 验收 |
|---|---|---|---|
| **S1** | runtime `reset()` 接口：`ChatInputEvent::Reset` + `reset_session()`（**纯发 Reset 事件，不碰 cancel**）+ loop idle gate drain 清理（messages/buffer 清空）+ `SessionReset` 事件 | ✅ 新增 | 单测：idle Reset→清理+SessionReset；busy Reset 排队等 idle 后清理 |
| **S2** | idle/busy `/clear` 改调 `reset_session()`，**移除暴力 drop `input_event_tx`**（`slash.rs`/`run_loop.rs:234` 自愈重建分支）；`SessionReset` handler 清镜像+output_area+占位 | ✅ reset 路径变 | **TUI 人工验**：idle 立即清、busy 等 idle 后清 |
| **S3** | `PendingInputBuffer::drain_all()`（批量）+ `ChatInputEvent::WithdrawAll` + `UserMessagesWithdrawn{texts}` 事件链；TUI handler 清全部占位+换行拼接 `RestoreText`；Up 键 busy 撤回全部 | ✅ 新增撤回 | 单测：drain_all 命中/空 no-op；**TUI 验** Up 撤回+还原 |
| **S4** | 删 `SpinnerPhase::ThinkingQueued` + 渲染/动画/测试臂 | ❌ 纯清理 | 编译通过、相关测试移除 |
| **S5** | 删 `/clear history`、`/clear all` 死分支 + `ConfirmAction::ClearAllHistory`（若无引用） | ❌ 纯清理 | 命令补全不含死项 |

S2/S3 强交互（失败模式纯视觉），**MUST 与用户配合 TUI 验收**；S4/S5 行为等价/纯清理。

## 5. 验收手段

- **纯逻辑单测（主力）**：reset 清理 + SessionReset 事件、drain_all 命中/空 no-op、loop 跨 reset 状态、busy 态 Reset 排队等 idle。
- **交互人工/截图验**（idle/busy /clear、Up 撤回+还原、占位清）——TUI 难自动化区，需用户在场。
- `-qv` 不覆盖 TUI（保留）。

## 6. 风险

- **S2 强交互**：reset 替换 drop 重建，是 reset 机制核心改造，**MUST TUI 验收**（改一段→跑→看）。
- **S1 busy 排队语义**：必须确保 busy 态 `Reset` 事件在回合自然结束回到 idle gate 时被正确 drain，不丢失（通道容量足够）、不重复清理。
- **S3 文本拼接**：多条 pending 文本换行拼接还原，需确认输入框对多行文本的处理（是否支持多行编辑）。
- 并发：reset/withdraw 与常驻 loop 可变状态（messages、buffer）单一 owner（loop 任务），避免锁竞争。

## 7. 非目标（YAGNI）

- 不新增 `/stop` 命令（Esc/Ctrl+C busy 已覆盖强制中止回合语义）。
- `/clear` **不强制打断**当前回合（不碰 CancellationToken）；强制中止 = Esc/Ctrl+C。
- 不动 provider / 持久化 JSON 格式。
- 不改 `Cancel`/`Abort` 现有中止语义（Esc/Ctrl+C busy 复用之）。
- 不做「逐条按 id 撤回」（只做一次性批量撤回全部 pending）。
- 不做「撤回已开始处理的输入」（只撤 pending 缓冲区内未处理的）。

## 8. 已敲定的 open decisions

- **决策 1**：`/clear` = 「整段会话重置」，**不打断当前回合**（idle 立即执行，busy 排队等回合自然结束回 idle 后执行）；强制中止回合 = **Esc/Ctrl+C（busy）**（现状已有，不变）。`/stop` 不新增。
- **决策 2**：runtime 暴露 `reset_session()` 接口（**纯发 `Reset` 事件，不碰 CancellationToken**），loop idle gate drain 时清理 + 发 `SessionReset`，**取代暴力 drop 通道**。
- **决策 3**：撤回 = **一次性批量清空** `PendingInputBuffer::drain_all()`——非空则清空+发 `UserMessagesWithdrawn{texts}`，空则 no-op；TUI 收到后清全部占位 + 把 `texts` 用换行符拼接还原到输入框。触发键 = **busy 态 Up**。
- **决策 4**：**删除** `SpinnerPhase::ThinkingQueued` 孤儿状态。
- **决策 5**：删 `/clear history`、`/clear all` 死分支。