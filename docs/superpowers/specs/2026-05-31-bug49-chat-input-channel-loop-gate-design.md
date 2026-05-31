# Bug #49 Chat Input Channel + Loop Gate — 设计文档

**日期**：2026-05-31
**Bug**：#49
**状态**：设计完成，待实施

## 概述

Bug #49 的根因不再视为某一个 hook 或退出分支漏 drain，而是 runtime 缺少统一的用户输入事件通道和安全消费门。当前实现依赖 `append_queued_input` 在若干分支上主动拉取 TUI queue；只要用户输入发生在最后一次 drain 之后、下一次 LLM 请求或 Done 之前，就可能留在 input queue 中。

本设计采用 **Chat Input Channel + Loop Gate**：TUI 在 agent 忙碌期间提交的输入进入 runtime 级事件通道；runtime 在安全边界通过 Loop Gate 统一消费 pending input。普通用户消息进入下一轮 LLM；control command 不进入 LLM，而是走命令执行语义。

## 当前问题

### 1. drain 调用散落在具体分支

`process_chat_loop` 当前在以下分支调用 `append_queued_input`：

- interrupted 取消前
- stall break 前
- EndTurn / 无 tool call 完成前
- Stop hook 通过后、Done 前
- tool result sync 后
- API error finalize 前

这些补丁修复了部分窗口，但没有形成架构不变量。新增 hook、耗时边界或退出路径时，仍然容易遗漏。

### 2. runtime 只能被动 drain TUI queue

TUI 忙碌时提交输入主要被放入本地 queue。runtime 只有在主动调用 queue port 时才知道这些输入存在。若某个耗时边界后直接进入下一步，runtime 无法及时响应用户追加指令或控制命令。

### 3. slash command 与普通消息缺少分类

当前 queue payload 是字符串集合。若忙碌期间用户输入 `/clear`、`/save`、`/model` 等 slash command，缺少明确规则决定它们是否应进入 LLM messages、是否应中断当前 loop、或是否旁路执行。

## 设计目标

1. 忙碌期间用户输入 MUST 进入 runtime 可见的输入通道，不只停留在 TUI 本地展示队列。
2. 普通用户消息 MUST 在安全边界进入 `messages`，并触发下一轮 LLM，而不是等当前 agent 自然 Done。
3. Control command MUST NOT 作为普通 user message 发送给 LLM。
4. 所有继续调用 LLM、准备结束、长耗时边界返回的路径 MUST 经过统一 Loop Gate。
5. 新增 hook/tool/compact/reflection 边界时 SHOULD 复用 Loop Gate，避免再次散落 drain。
6. 本修复聚焦单个 chat loop，不重写 provider streaming、tool 执行并发模型或完整 slash 命令系统。

## 核心设计

### 1. ChatInput 事件

在 SDK/runtime 边界引入 chat input 事件类型，替代“只有字符串 queue”的隐式语义。

```text
ChatInput
- UserMessage { text, images }
- ControlCommand { raw }
- Cancel
```

首期实现范围：

- `UserMessage`：普通文本；若 TUI 支持 pending images，则保留图片字段或转换为现有消息结构。
- `ControlCommand`：以 `/` 开头的 slash command。
- `Cancel`：可先映射现有 interrupted/cancel token，不要求一次性替换全部取消路径。

### 2. PendingInputBuffer

runtime 主 loop 维护 pending buffer，作为 input channel 与 messages/command 执行之间的缓冲层。

职责：

- 按提交顺序保存 pending `ChatInput`。
- 将普通消息与 control command 分类。
- 同一批输入中混合 command 和普通文本时，保持相对顺序处理。
- 避免 slash command 被 append 到 LLM messages。

建议接口：

```text
PendingInputBuffer
- push(input)
- drain_channel(input_port)
- drain_for_gate() -> Vec<ChatInput>
- is_empty()
```

### 3. Loop Gate

Loop Gate 是主循环唯一的安全消费入口。它先从 input channel drain 新事件到 pending buffer，再处理 pending 内容。

```text
GateDecision
- Proceed
- ContinueNextTurn
- AbortCurrentLoop
```

Gate 行为：

1. drain input channel。
2. 执行 pending control command。
3. 将 pending user message append 到 `messages`。
4. 发送 `MessagesSync`。
5. 若 append 了普通用户消息，返回 `ContinueNextTurn`。
6. 若 control command 要中断当前 loop，返回 `AbortCurrentLoop`。
7. 否则返回 `Proceed`。

### 4. Gate 调用点

主循环至少包含三个 gate：

#### BeforeLlmGate

在构造 `messages_for_api` / 调用 LLM 前执行。

覆盖：

- `PostToolBatch` hook 期间提交的输入
- tool hook / permission hook 返回后的输入
- compact hook 返回后的输入
- reflection 等上一轮尾部逻辑期间提交的输入

#### BeforeFinishGate

在发送 `DoneWithDuration` / `Done` / `Cancelled` 或最终 break 前执行。

覆盖：

- Stop hook 期间提交的输入
- StopFailure hook 期间提交的输入
- API error、stall、cancel 等退出前窗口

#### AfterBlockingBoundaryGate

在长耗时边界返回后执行，用于提升响应速度。

首期建议覆盖：

- tool batch 完成后
- post tool batch hook 后
- auto compact 后
- stop/stop failure hook 后

即使某个 AfterBlockingBoundaryGate 漏掉，BeforeLlmGate / BeforeFinishGate 仍作为兜底不变量。

## Control Command 语义

Control command 分三类。

### 1. Abort commands

示例：

- `/clear`

行为：

- 不进入 LLM messages。
- 取消当前 chat loop。
- 清空会话消息、TUI queue、queued submission echo、task window 等现有 `/clear` 语义覆盖的状态。
- 返回 `AbortCurrentLoop`。

### 2. Side-effect commands

示例：

- `/save`
- `/todo`
- `/memory`
- `/reflect`
- `/help`

行为：

- 不进入 LLM messages。
- 通过现有命令执行路径旁路执行。
- 默认不打断当前 agent loop。
- 若命令产生 UI 输出，作为 system notice 或命令结果事件发回 TUI。

### 3. Reconfigure commands

示例：

- `/model`
- `/provider`

行为：

- 不进入 LLM messages。
- 更新配置或选择状态。
- 对当前正在进行的 LLM 请求不强制中途替换。
- 对下一轮 LLM 生效；若实现上无法安全切换，命令结果 MUST 明确提示“将在下一轮生效”或要求当前轮结束后执行。

## TUI 行为

### 非忙碌状态

保持现有行为：Enter 后直接启动 chat 或执行 slash command。

### 忙碌状态

Enter 后不只写入本地 input queue，还要发送 `ChatInput` 到 runtime input channel。

- 普通文本：发送 `UserMessage`，并继续显示 queued submission echo，直到 runtime `MessagesSync` 后清除。
- slash command：发送 `ControlCommand`，可显示为 queued command echo，执行完成后清除。
- 发送失败：保留本地 queue，并在 Done 兜底路径继续尝试，避免丢输入。

## Runtime 行为

主 loop 不在任意时刻直接修改 messages。所有来自 channel 的输入都进入 pending buffer，并只在 gate 处消费。

这保证：

- LLM streaming 中途不会被直接插入 message。
- tool batch 正在执行时不会破坏 tool_use/tool_result 配对。
- hook 正在运行时不会引入难以回滚的状态修改。
- 用户输入会在最近的安全边界生效。

## 与现有 `append_queued_input` 的关系

`append_queued_input` 不应继续作为散落补丁存在。迁移方向：

1. 将其能力并入 Loop Gate 的 UserMessage append 分支。
2. 旧 `QueueDrainPort` 可临时作为 input channel 的兼容 adapter。
3. 迁移完成后，`process_chat_loop` 中不再直接调用 `append_queued_input`，而是调用命名清晰的 gate 函数。

## 错误处理

- input channel 关闭：视为无新输入，Loop Gate 返回 `Proceed`。
- control command 执行失败：发送错误事件/notice，但不把 command 文本发给 LLM。
- user message append 后 `MessagesSync` 失败：记录日志；若 sink 无法发送，chat loop 可继续按现有错误策略处理。
- `/clear` 执行中失败：必须避免半清空；至少保证当前 loop 不继续把已清空前的旧 messages 发给 LLM。

## 日志与观测

新增 debug 日志，便于确认 #49 是否彻底消失：

- input event 入 runtime channel
- gate drain 数量
- classified user/control 数量
- append user message 后的 messages 长度
- control command 执行结果
- gate decision
- Done 前 pending buffer 是否为空

现有 `[bug49_input_queue_at_done]` 可保留到本 bug 确认归档后再删除或降级。

## 测试计划

### Runtime 单元测试

1. Stop hook 后收到 `UserMessage`，BeforeFinishGate 返回 `ContinueNextTurn`，不发送 Done。
2. PostToolBatch hook 期间收到 `UserMessage`，下一次 LLM 前消息已 append。
3. auto compact 或 compact hook 期间收到 `UserMessage`，BeforeLlmGate 消费并续轮。
4. API error 退出前收到 `UserMessage`，优先续轮，不直接 finalize。
5. interrupted/cancel 退出前收到 `UserMessage`，优先续轮或按明确 cancel 优先级处理。
6. queue 中只有 `/clear`，不 append 到 LLM messages，并返回 `AbortCurrentLoop`。
7. queue 中 `/save` + 普通文本，先执行 `/save`，再 append 普通文本并续轮。
8. queue 中 `/model xxx` + 普通文本，配置变更对下一轮生效，普通文本进入下一轮。
9. input channel 关闭时 gate 返回 `Proceed`。

### TUI/adapter 测试

1. 忙碌期间普通 Enter 会发送 `ChatInput::UserMessage`。
2. 忙碌期间 slash Enter 会发送 `ChatInput::ControlCommand`。
3. runtime `MessagesSync` 后 queued submission echo 被清除。
4. control command 执行完成后 queued command echo 被清除。
5. input channel 发送失败时，本地 queue 不丢失。

### 回归验证

- `cargo test -p runtime`。
- `cargo test -p cli` 中覆盖 queue/enter/update 的相关测试。
- 手动复现：tool batch / Stop hook / compact 期间提交普通文本和 `/clear`，确认不会留在 input queue，也不会把 slash command 发给 LLM。

## 涉及文件

预计涉及：

| 路径 | 改动 |
|------|------|
| `packages/sdk/src/chat.rs` / `packages/sdk/src/tui.rs` | 定义或扩展 `ChatInput` / input port 契约 |
| `apps/cli/src/tui/app/update/enter.rs` | 忙碌时发送 input event，而非只保留本地 queue |
| `apps/cli/src/tui/effect/session/processing.rs` | 创建 chat input channel，并把 sender/port 注入 `ChatRequest` |
| `agent/features/runtime/src/core/client/chat.rs` | 将 SDK input port 转接给 runtime chat loop |
| `agent/features/runtime/src/business/chat/looping/queue.rs` | 引入 PendingInputBuffer / GateDecision / gate helper |
| `agent/features/runtime/src/business/chat/looping/loop_runner.rs` | 用 Loop Gate 替换散落 `append_queued_input` 调用 |
| `agent/features/runtime/src/business/chat/looping/finalize.rs` | finish 前接入 BeforeFinishGate 或暴露可组合 finalize 步骤 |
| `docs/bug/active.md` | 更新 #49 根因与修复方案 |

## 明确不做

1. 不在 LLM streaming 正中直接修改 messages。
2. 不让 slash command 作为 user message 进入 LLM。
3. 不重写所有 slash command 实现；首期通过现有命令路径或最小 adapter 执行。
4. 不改变 tool_use/tool_result 配对规则。
5. 不把本修复扩大为完整多会话事件总线。

## 验收标准

1. 忙碌期间普通输入在最近安全边界进入下一轮 LLM。
2. 忙碌期间 slash command 不会出现在发送给 LLM 的 messages 中。
3. `/clear` 忙碌期间可中断当前 loop，并清空相关 UI/runtime 状态。
4. `PostToolBatch`、Stop hook、compact、API error、cancel/stall 等路径不再依赖各自手写 drain。
5. Done 前 pending input 为空；若不为空，必须有日志说明原因和处理决策。
6. 相关 runtime 与 CLI 测试通过。

## 关联

- Bug #49：last turn 用户输入留在 input queue。
- Bug #72：SDK 解耦后 runtime queue drain 端口曾为空实现。
- Feature #30：agent loop finalize 统一化。
- Feature #47：runtime 作为核心编排者，TUI 通过 SDK 契约接入。
