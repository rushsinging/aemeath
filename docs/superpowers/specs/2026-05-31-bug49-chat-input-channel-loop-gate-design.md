# Bug #49 Chat Input Event Channel + Loop Gate — 设计文档

**日期**：2026-05-31
**Bug**：#49
**状态**：已实施，待用户确认

## 概述

Bug #49 的根因不再视为某一个 hook 或退出分支漏 drain，而是 runtime 缺少统一的用户输入事件通道和安全消费门。旧实现依赖 `append_queued_input` 在 `process_chat_loop` 的多个分支主动拉取 TUI queue；只要用户输入发生在最后一次 drain 之后、下一次 LLM 请求或 Done 之前，就可能留在 input queue 中。

本设计已落地 **Chat Input Event Channel + Loop Gate**：TUI 在 agent 忙碌期间提交的输入通过 Cmd/Effect 进入 runtime 级事件通道；runtime 在安全边界通过 Loop Gate 统一消费 pending input。普通用户消息延展当前 Chat 为新的 Turn；control command 不进入 LLM，而是走命令执行语义。

## 当前问题

### 1. drain 调用散落在具体分支

`process_chat_loop` 当前有 6 个主循环调用点直接调用 `append_queued_input`（不含 `queue.rs` 内部测试；行号以当前实现为准）：

- `agent/features/runtime/src/business/chat/looping/loop_runner.rs:160`：interrupted 取消前
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs:277`：stall break 前
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs:297`：EndTurn / 无 tool call 完成前
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs:337`：Stop hook 通过后、Done 前（`f26e26d` 的 #49 点修复）
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs:365`：tool result sync 后
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs:378`：API error finalize 前

这些补丁修复了部分窗口，但没有形成架构不变量。新增 hook、耗时边界或退出路径时，仍然容易遗漏。

### 2. runtime 只能被动 drain TUI queue

TUI 忙碌时提交输入主要被放入本地 queue。runtime 只有在主动调用 queue port 时才知道这些输入存在。若某个耗时边界后直接进入下一步，runtime 无法及时响应用户追加指令或控制命令。

### 3. slash command 与普通消息缺少分类

当前 queue payload 是字符串集合。若忙碌期间用户输入 `/clear`、`/save`、`/model` 等 slash command，缺少明确规则决定它们是否应进入 LLM messages、是否应中断当前 loop、或是否旁路执行。

## 设计目标

1. 忙碌期间用户输入 MUST 进入 runtime 可见的输入事件通道，不只停留在 TUI 本地展示队列。
2. 普通用户消息 MUST 在安全边界进入 `messages`，并触发当前 Chat 内的新 Turn，而不是等当前 agent 自然 Done。
3. Control command MUST NOT 作为普通 user message 发送给 LLM。
4. 所有继续调用 LLM、准备结束、长耗时边界返回的路径 MUST 经过统一 Loop Gate。
5. 新增 hook/tool/compact/reflection 边界时 SHOULD 复用 Loop Gate，避免再次散落 drain。
6. TUI `update()` MUST 只产出纯 Cmd/Effect，NEVER 直接执行 channel send 等副作用。
7. 本修复聚焦单个 chat loop，不重写 provider streaming、tool 执行并发模型或完整 slash 命令系统。

## 命名与现有 SDK 契约

`packages/sdk/src/chat.rs` 已存在：

```text
pub struct ChatInput {
    text: String,
    image_paths: Vec<String>,
}
```

该类型表示“启动一次 chat 的初始输入”，不是忙碌期间追加输入事件。本设计新增类型 MUST 避免同名冲突，建议命名为：

```text
ChatInputEvent
- UserMessage { text, image_paths }
- ControlCommand { raw }
- Cancel
```

`ChatInput` 继续用于一次 chat 的初始入参；`ChatInputEvent` 用于 chat 已运行时从 TUI 发送给 runtime 的追加事件。

## Chat 边界语义

忙碌期间收到 `ChatInputEvent::UserMessage` 时，runtime 不新建一个独立 Chat，而是**延展当前 Chat 为新的 Turn**：

- append 到当前 chat loop 的 `messages`。
- 触发当前 loop 的下一次 LLM 请求。
- 当前 `session_id`、chat 事件流、cost 追踪和 audit/log 归账仍归属于当前 Chat。
- TUI 上应呈现为当前对话中的新用户 turn，而不是一个新的 processing job。

该决策与“Chat = 一次用户输入触发的完整处理单元”的定义存在扩展关系：忙碌期追加输入被视为用户对当前处理单元的交互式续写，而不是另一次独立入口请求。实施时 MUST 在相关代码注释或类型命名中体现该语义，避免未来误改为多 chat 并发。

## 核心设计

### 1. ChatInputEvent 事件

在 SDK/runtime 边界引入追加输入事件类型，替代“只有字符串 queue”的隐式语义。

```text
ChatInputEvent
- UserMessage { text, image_paths }
- ControlCommand { raw }
- Cancel
```

首期实现范围：

- `UserMessage`：普通文本和图片附件；aemeath 已支持图片输入，事件类型 MUST 保留 `image_paths` 或等价图片载荷。
- `ControlCommand`：以 `/` 开头的 slash command。
- `Cancel`：可映射现有 interrupted/cancel token。`ChatInputEvent::Cancel` 与现有 cancel token / interrupted flag 任一触发时，取消处理 MUST 幂等；重复 Cancel 不得导致双重 finalize 或重复清理。

### 2. PendingInputBuffer

runtime 主 loop 维护 pending buffer，作为 input channel 与 messages/command 执行之间的缓冲层。

职责：

- 按提交顺序保存 pending `ChatInputEvent`。
- 将普通消息与 control command 分类，但处理时 MUST 保持提交顺序。
- 同一批输入中混合 command 和普通文本时，命令执行与消息 append 按提交顺序交错处理。
- 避免 slash command 被 append 到 LLM messages。

建议接口：

```text
PendingInputBuffer
- push(input)
- drain_channel(input_port)
- drain_for_gate() -> Vec<ChatInputEvent>
- is_empty()
```

### 3. Loop Gate

Loop Gate 是主循环唯一的安全消费入口。它先从 input channel drain 新事件到 pending buffer，再按提交顺序处理 pending 内容。

```text
GateDecision
- Proceed
- ContinueNextTurn
- AbortCurrentLoop
- CancelCurrentLoop
```

Gate 行为：

1. drain input channel。
2. 将事件追加到 `PendingInputBuffer`。
3. 按提交顺序逐条处理 pending 事件。
4. `Cancel` 具有最高优先级；同一批出现 Cancel 时，不再 append 后续 user message，不再启动续轮。
5. Abort command（如 `/clear`）优先级高于 reconfigure/user message；命中 abort 后，同批后续事件全部丢弃，返回 `AbortCurrentLoop`。
6. Reconfigure/side-effect command 通过命令路径执行，不进入 LLM messages。
7. UserMessage append 到 `messages`，发送 `MessagesSync`。
8. 若 append 了普通用户消息且未被 Cancel/Abort 覆盖，返回 `ContinueNextTurn`。
9. 否则返回 `Proceed`。

### 4. Gate 调用点与决策语义

主循环至少包含三个 gate。同一个 `GateDecision` 在不同 gate 的含义不同，实施时 MUST 按 gate 明确处理。

#### BeforeLlmGate

在构造 `messages_for_api` / 调用 LLM 前执行。

覆盖：

- `PostToolBatch` hook 期间提交的输入
- tool hook / permission hook 返回后的输入
- compact hook 返回后的输入
- reflection 等上一轮尾部逻辑期间提交的输入

决策语义：

- `Proceed`：继续构造本次 LLM 请求。
- `ContinueNextTurn`：语义上表示 messages 已追加新 user turn；由于当前位置本来就即将调用 LLM，控制流可等价于 `Proceed`，但 MUST 使用追加后的 messages。
- `AbortCurrentLoop` / `CancelCurrentLoop`：不得调用 LLM，按 abort/cancel finalize。

#### BeforeFinishGate

在发送 `DoneWithDuration` / `Done` / `Cancelled` 或最终 break 前执行。

覆盖：

- Stop hook 期间提交的输入
- StopFailure hook 期间提交的输入
- API error、stall、cancel 等退出前窗口

决策语义：

- `Proceed`：允许 finish。
- `ContinueNextTurn`：阻止 Done/Cancelled/Error finalize，回到主 loop，强制下一轮 LLM。
- `AbortCurrentLoop`：执行 abort finalize，不再续轮。
- `CancelCurrentLoop`：执行 cancel finalize，不再续轮。

#### AfterBlockingBoundaryGate

在长耗时边界返回后执行，用于提升响应速度。

首期建议覆盖：

- tool batch 完成后
- post tool batch hook 后
- auto compact 后
- stop/stop failure hook 后

决策语义：

- `Proceed`：继续原流程。
- `AbortCurrentLoop` / `CancelCurrentLoop`：在安全边界终止当前 loop。
- AfterBlockingBoundaryGate 不直接裁决 finish，也不单独产出 `ContinueNextTurn` 作为控制流跳转；若它提前 append 了 UserMessage，只记录“messages 已更新 / pending_continue”，后续由紧随其后的 BeforeLlmGate 或 BeforeFinishGate 统一决定 Proceed 还是 ContinueNextTurn。

即使某个 AfterBlockingBoundaryGate 漏掉，BeforeLlmGate / BeforeFinishGate 仍作为兜底不变量。

## 事件优先级与保序

同一批 pending input 的处理规则：

1. `Cancel` 优先级最高。只要 gate drain 到 Cancel，当前 loop MUST 取消；Cancel 之后的 user message 不得 append，也不得触发续轮。Cancel 事件与现有 cancel token / interrupted flag 合流，取消 finalize MUST 幂等。
2. Abort command（如 `/clear`）优先级次之。它在 gate 处生效，MUST 不在 tool batch / hook / LLM streaming 中途生效。命中 abort 后，同批后续所有事件 MUST 丢弃（含其后的 UserMessage / command），并 SHOULD 向 TUI 发送一条 notice 说明后续排队输入已因 `/clear` 被丢弃。
3. Reconfigure command、side-effect command 与 UserMessage 按提交顺序逐条处理。
4. 示例 `[text1, /save, text2]` MUST 处理为：append `text1`，执行 `/save`，append `text2`。
5. 示例 `[/model x, text]` 在 BeforeLlmGate 中 MUST 先更新模型选择，再让紧接着的本次 LLM 请求使用新模型；若发生在 LLM streaming 中，则在最近安全边界生效。
6. 示例 `[text1, /clear, text2]` 在 gate 处命中 `/clear` 后 MUST 中断当前 loop 并清空 `/clear` 覆盖的状态；`text2` 及后续事件丢弃。若 `text1` 已按提交顺序 append，`/clear` 的清空语义 MUST 覆盖它，最终不得把 `text1` 或 `text2` 发给后续 LLM。

## Control Command 语义

Control command 分三类。

### 1. Abort commands

示例：

- `/clear`

行为：

- 不进入 LLM messages。
- 只在 Loop Gate 安全边界生效，NEVER 在 tool batch、hook 或 LLM streaming 中途清空状态。
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
- 若在 BeforeLlmGate 中执行，MUST 影响紧接着的本次 LLM 请求。
- 若在 LLM streaming / tool batch / hook 期间提交，MUST 在最近安全边界生效；已经发出的 LLM 请求不强制中途替换。
- 若实现上无法安全切换，命令结果 MUST 明确提示“将在下一次尚未发出的 LLM 请求生效”或要求当前轮结束后执行。

## TUI 行为

### 非忙碌状态

保持现有行为：Enter 后直接启动 chat 或执行 slash command。

### 忙碌状态

Enter 后不只写入本地 input queue，还要通过 Cmd/Effect 发送 `ChatInputEvent` 到 runtime input channel。

- `apps/cli/src/tui/app/update/enter.rs` 等 update 层 MUST 只更新纯状态并产出 Effect/Cmd。
- channel send MUST 在 effect 层执行，例如 `apps/cli/src/tui/effect/session/processing.rs` 或对应 session effect executor。
- 普通文本：发送 `UserMessage { text, image_paths }`，并继续显示 queued submission echo，直到 runtime `MessagesSync` 后清除。
- slash command：发送 `ControlCommand { raw }`，可显示为 queued command echo，执行完成后清除。
- 发送失败：保留本地 queue，并在 Done 兜底路径继续尝试，避免丢输入。

## Busy 状态与单一真相

TUI 与 runtime 都会观察“忙碌”，但真相边界必须明确：

- runtime 的 `PendingInputBuffer` 和 chat loop 状态是 runtime 侧输入处理真相。
- TUI 的 busy/runtime_state MUST 从 ChatEvent 流和本地 processing job 状态推导，用于展示与决定 Enter 产生何种 Effect。
- TUI MUST NOT 通过猜测 runtime pending buffer 来决定消息是否已消费。
- runtime MUST NOT 依赖 TUI 展示队列作为唯一输入真相；TUI 本地 queue 只是兼容兜底和 UI echo。

## Runtime 行为

主 loop 不在任意时刻直接修改 messages。所有来自 channel 的输入都进入 pending buffer，并只在 gate 处消费。

这保证：

- LLM streaming 中途不会被直接插入 message。
- tool batch 正在执行时不会破坏 tool_use/tool_result 配对。
- hook 正在运行时不会引入难以回滚的状态修改。
- `/clear` 等 abort command 不会撕裂 tool_use/tool_result 配对，因为只在安全边界生效。
- 用户输入会在最近的安全边界生效。

## 与 #72 现行 queue drain 通道的关系

#72 已引入当前过渡通道：`sdk::ChatRequest.queue_drain` + `sdk::QueueDrainPort` + runtime `RuntimeQueueDrainPort` + CLI `TuiQueueDrainPort`。这是一条 pull 模型通道：runtime 在若干 `append_queued_input` 调用点主动拉取 TUI queue。

这条思路已不再作为最终方向。#72 解决的是“runtime 完全读不到 TUI queue”的直接断点，但仍保留了 pull-drain 的根本限制：runtime 只有在散落调用点主动拉取时才知道输入存在，无法建立 #49 所需的统一输入不变量。#49 的 ChatInputEvent push channel + PendingInputBuffer + Loop Gate 是 #72 的最终收口方案。

本设计的新 input event channel 是 push 模型通道。实施时 MUST 明确二者关系：

1. 以 `ChatInputEvent` push channel 作为唯一长期目标；`RuntimeQueueDrainPort` / `TuiQueueDrainPort` 只作为迁移期兼容 adapter，不再继续扩展 pull-drain 方案。
2. 迁移期 gate 可同时从 push channel 与 #72 pull port 收集输入，但 MUST 为每批输入建立消费确认或 drain source 标记，避免同一 TUI queue 先被 push、后又被 pull 双消费。
3. 一旦 TUI 忙碌期 Enter 稳定发送 `ChatInputEvent`，对应路径 SHOULD 停止写入会被 `TuiQueueDrainPort` 再次 drain 的同一队列；若仍保留 UI echo，echo 必须与可消费 queue 分离。
4. #72 的 `RuntimeQueueDrainPort` 测试应保留到迁移完成，作为兼容 adapter 回归；新测试必须覆盖 push+pull 并存不双消费。

## 与现有 `append_queued_input` 的关系

`append_queued_input` 不应继续作为散落补丁存在。迁移方向：

1. 将其能力并入 Loop Gate 的 UserMessage append 分支。
2. 旧 `QueueDrainPort` 可临时作为 input channel 的兼容 adapter，具体即 #72 的 `RuntimeQueueDrainPort` / `TuiQueueDrainPort`。
3. `loop_runner.rs:337` 的 Stop-hook 后二次 drain 是 `f26e26d` 为 #49 添加的点修复；Loop Gate 落地时 MUST 删除这段临时补丁，并由 BeforeFinishGate 覆盖 Stop hook / StopFailure hook 后窗口。
4. 任一旧 `append_queued_input` 调用点的移除 MUST 与覆盖同一边界的 gate 落地在同一改动内，避免“新 gate + 旧 drain”对同一批输入双消费。
5. 迁移完成后，`process_chat_loop` 中不再直接调用 `append_queued_input`，而是调用命名清晰的 gate 函数。

## 错误处理

- input channel 关闭：视为无新输入，Loop Gate 返回 `Proceed`。
- control command 执行失败：发送错误事件/notice，但不把 command 文本发给 LLM。
- user message append 后 `MessagesSync` 失败：记录日志；若 sink 无法发送，chat loop 可继续按现有错误策略处理；同时 MUST 给 TUI 一个 echo 清除或降级信号（例如 `QueuedInputAccepted` / `QueuedInputFailed` / system notice），避免 queued submission echo 因等不到 `MessagesSync` 永久残留。
- `/clear` 执行中失败：必须避免半清空；至少保证当前 loop 不继续把已清空前的旧 messages 发给 LLM。

## 日志与观测

新增 debug 日志，便于确认 #49 是否彻底消失：

- input event 入 runtime channel
- gate drain 数量
- classified user/control/cancel 数量
- append user message 后的 messages 长度
- control command 执行结果
- gate decision 与 gate kind
- Done 前 pending buffer 是否为空

现有 `[bug49_input_queue_at_done]` 可保留到本 bug 确认归档后再删除或降级。

## 测试计划

### Runtime 单元测试

1. Stop hook 后收到 `UserMessage`，BeforeFinishGate 返回 `ContinueNextTurn`，不发送 Done。
2. PostToolBatch hook 期间收到 `UserMessage`，下一次 LLM 前消息已 append。
3. auto compact 或 compact hook 期间收到 `UserMessage`，BeforeLlmGate 消费并续轮。
4. API error 退出前收到 `UserMessage`，优先续轮，不直接 finalize。
5. interrupted/cancel 退出前收到 `Cancel`，Cancel 优先级高于 UserMessage，不续轮，且与现有 cancel token / interrupted flag 幂等合流。
6. queue 中只有 `/clear`，不 append 到 LLM messages，并返回 `AbortCurrentLoop`。
7. queue 中 `[text1, /save, text2]`，按提交顺序 append/执行/append，并续轮。
8. queue 中 `[text1, /clear, text2]`，`/clear` 在安全边界 abort，后续 `text2` 丢弃，最终不向 LLM 发送 `text1`/`text2`。
9. queue 中 `[/model xxx, text]` 在 BeforeLlmGate 中处理时，配置变更影响紧接着的 LLM 请求，普通文本进入同一次请求前的 messages。
10. input channel 关闭时 gate 返回 `Proceed`。
11. 旧 `QueueDrainPort` adapter 与新 input channel 不会双消费同一批输入。

### TUI/adapter 测试

1. 忙碌期间普通 Enter 只在 update 层产出发送 `ChatInputEvent::UserMessage` 的 Effect/Cmd，不直接执行 send。
2. 忙碌期间 slash Enter 只在 update 层产出发送 `ChatInputEvent::ControlCommand` 的 Effect/Cmd，不直接执行 send。
3. effect/session 层实际执行 channel send。
4. runtime `MessagesSync` 后 queued submission echo 被清除。
5. `MessagesSync` sink 失败时，TUI 仍能收到 echo 清除或降级信号，不留下幽灵 queued bubble。
6. control command 执行完成后 queued command echo 被清除。
7. input channel 发送失败时，本地 queue 不丢失。

### 回归验证

- `cargo test -p runtime`。
- `cargo test -p cli` 中覆盖 queue/enter/update/effect 的相关测试。
- 手动复现：tool batch / Stop hook / compact 期间提交普通文本和 `/clear`，确认不会留在 input queue，也不会把 slash command 发给 LLM。

## 涉及文件

预计涉及：

| 路径 | 改动 |
|------|------|
| `packages/sdk/src/chat.rs` / `packages/sdk/src/tui.rs` | 保留现有 `ChatInput`；新增 `ChatInputEvent` / input event port 契约 |
| `apps/cli/src/tui/app/update/enter.rs` | 忙碌时产出发送 input event 的 Effect/Cmd，而非直接发送或只保留本地 queue |
| `apps/cli/src/tui/effect/effect.rs` | 增加发送 chat input event 的纯 Effect 类型 |
| `apps/cli/src/tui/effect/session/processing.rs` | 创建 chat input channel，并由 effect executor 执行 send，把 receiver/port 注入 `ChatRequest` |
| `agent/features/runtime/src/core/client/trait_chat.rs` / `trait_impl.rs` | 将 SDK input event port 转接给 runtime chat loop |
| `agent/features/runtime/src/business/chat/looping/queue.rs` | 引入 PendingInputBuffer / GateDecision / gate helper |
| `agent/features/runtime/src/business/chat/looping/loop_runner.rs` | 用 Loop Gate 替换散落 `append_queued_input` 调用 |
| `agent/features/runtime/src/business/chat/looping/finalize.rs` | finish 前接入 BeforeFinishGate 或暴露可组合 finalize 步骤 |
| `docs/bug/active.md` | 更新 #49 根因与修复方案 |

## 实施状态（2026-05-31）

已落地首期实现：

1. SDK 新增 `ChatInputEvent`、`ChatInputEventPort`、`InputEventFuture`，并在 `ChatRequest` 增加 `input_events`。
2. Runtime 新增 `PendingInputBuffer`、`InputEventDrainPort`、Loop Gate 与 push/pull 合并 drain，`process_chat_loop` 的主循环消费点已统一为 `drain_and_apply_gate`。
3. TUI 忙碌期 Enter 产出 `Effect::SendChatInputEvent`，effect 层写入 `TuiInputEventPort` buffer，`SpawnContext` 将 port 注入 `ChatRequest.input_events`。
4. `MessagesSync` 会清理旧 input queue 与 queued submission echo；#72 的 pull queue adapter 保留为迁移期兜底。
5. 已验证：`cargo test -p sdk chat_input_event`、`cargo test -p runtime input_gate`、`cargo test -p runtime test_process_chat_loop_drains_input_after_stop_hook_before_done`、`cargo check -p runtime -p cli`。

## 明确不做

1. 不在 LLM streaming 正中直接修改 messages。
2. 不让 slash command 作为 user message 进入 LLM。
3. 不重写所有 slash command 实现；首期通过现有命令路径或最小 adapter 执行。
4. 不改变 tool_use/tool_result 配对规则。
5. 不把本修复扩大为完整多会话事件总线。
6. 不复用现有 `ChatInput` 名称表达忙碌期追加事件。

## 验收标准

1. 忙碌期间普通输入在最近安全边界进入当前 Chat 的下一 Turn。
2. 忙碌期间 slash command 不会出现在发送给 LLM 的 messages 中。
3. `/clear` 忙碌期间可在安全边界中断当前 loop，并清空相关 UI/runtime 状态；同批 `/clear` 后续输入被丢弃并有用户可见反馈。
4. `PostToolBatch`、Stop hook、compact、API error、cancel/stall 等路径不再依赖各自手写 drain。
5. `f26e26d` 的 Stop-hook 后二次 drain 临时补丁已被 BeforeFinishGate 取代，不与新 gate 并存。
6. Done 前 pending input 为空；若不为空，必须有日志说明原因和处理决策。
7. `ChatInputEvent` 与现有 `ChatInput` 不冲突。
8. update 层保持纯函数式边界，channel send 只在 effect 层执行。
9. push channel 与 #72 pull queue adapter 不双消费。
10. 相关 runtime 与 CLI 测试通过。

## 追踪归属

本工作以 Bug #49 为主线，因为它直接修复“last turn 用户输入留在 input queue”的用户可见错误。由于实现会动 SDK 契约和 runtime/TUI 边界，实际规模接近 feature；实施前 MAY 在 `docs/feature/active.md` 另登记一个配套 feature，但 commit message MUST 至少引用 `refs #49`。

## 关联

- Bug #49：last turn 用户输入留在 input queue。
- Feature #55：TUI 架构收口 — render / adapter / app 三层落地 + 清理 legacy core。
- Bug #72：旧 pull-drain 过渡方案；最终由 #49 ChatInputEvent + Loop Gate 统一解决。
- Feature #30：agent loop finalize 统一化。
- Feature #47：runtime 作为核心编排者，TUI 通过 SDK 契约接入。
