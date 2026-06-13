# #49 TUI 渲染管线与 Runtime Observation Context 重构设计

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/151
> Issue 补充评论: https://github.com/rushsinging/aemeath/issues/151#issuecomment-4697155019

## 状态

- 状态：设计稿
- 范围：`apps/cli/src/tui/**` 的 runtime event → model → view model → render 链路
- 目标架构：DDD + 六边形架构 + Clean Architecture + VPA（View / Presenter / Application）
- 核心不变量：runtime streaming event 的归属必须来自事件自带的 `chat_id + turn_id`，永远不能从 UI active 状态反查

## 1. 背景

TUI 渲染逻辑已经从早期直接渲染状态，逐步演进为：

```text
UiEvent
  -> adapter/agent_event.rs
  -> ConversationIntent / RuntimeIntent / DiagnosticIntent
  -> TuiModel
  -> ViewAssembler
  -> ViewModel
  -> DocumentRenderer / Widget Render
```

这个方向是正确的：`ViewModel`、`ViewAssembler`、dirty flag、`DocumentRenderer` 都说明 TUI 已经在向 Clean/VPA 架构靠拢。

但 runtime Observe 事件引入后，职责边界变得不清晰：

1. runtime event 被直接映射为 `ConversationIntent::Observe*`，领域模型被迫理解 streaming 细节。
2. `ConversationModel` 同时承担对话状态、输出 timeline、tool call/result 乱序绑定、active streaming block、AskUser block 等多种职责。
3. 部分 presentation / adapter 代码依赖 render 层工具函数，出现依赖方向反转。
4. 曾出现过 `UiEvent` 原本携带正确 runtime `turn_id`，但转成 `Observe*` 时丢失，model 再从 `active_chat_id / active_turn` 反查，导致 history restore、display replay、旧 block 或其他事件影响后拿到错误 turn 的问题。

本设计目标是将这类问题从架构层面杜绝，而不是局部补丁修复。

## 2. 设计目标

1. 明确 TUI 渲染链路的分层边界：Adapter、Application、Domain Model、Presenter、ViewModel、Render。
2. 将 runtime observation 与 conversation domain language 隔离。
3. 建立强制 runtime turn context 不变量，防止 `turn_id` 在任何层丢失。
4. 拆分 `ConversationModel.blocks` 的职责，引入 `OutputTimelineModel` 作为输出区 read model。
5. 将 tool call/result 的 id 绑定、乱序修复、orphan promote 逻辑收敛到 `ToolFlowProjector`。
6. 清除上层对 render 模块的反向依赖，保持 Clean Architecture 依赖方向。
7. 保持现有 TUI 行为和视觉表现不变，只整理架构边界和内部数据流。
8. 为后续渲染优化、session replay、多会话/多 turn 显示打下稳定基础。

## 3. 非目标

1. 不重写 ratatui widget 或视觉样式。
2. 不改变 SDK 对外语义，除非为补全缺失的 runtime context 必须扩展 DTO。
3. 不引入新的 UI 状态管理框架。
4. 不一次性拆 crate；本轮仍在 `apps/cli/src/tui/**` 内演进。
5. 不改变 provider / runtime chat loop 的执行逻辑。
6. 不实现新的多会话 UI，只保证未来可支持。
7. 不把所有历史 session 数据模型一次性迁移；history restore 只需遵守新 context 不变量。

## 4. 核心架构不变量

### 4.1 Runtime context 是 streaming event 归属的唯一事实来源

所有来自 Agent Runtime 的 streaming / lifecycle observation event，进入 TUI 后必须携带显式 context：

```text
RuntimeTurnContext {
  chat_id: ChatId,
  turn_id: ChatTurnId,
}
```

适用事件包括：

1. assistant text delta
2. thinking text delta
3. block complete
4. tool call start
5. tool call update
6. tool result
7. agent progress
8. turn complete / done / cancelled
9. 后续新增的任何 runtime streaming observation

### 4.2 禁止 runtime observation 从 active 状态反查归属

以下模式在 runtime observation 路径中禁止使用：

1. `active_chat_id` 推导 chat
2. `active_chat_mut()` 推导 chat
3. `active_turn_mut()` 推导 turn
4. 最后一个 block 推导 turn
5. 当前 display replay 状态推导 turn
6. history restore 后遗留 active block 推导 turn
7. tool id 全局匹配但不限定 `(chat_id, turn_id)`

`active_*` 只能表达 UI 当前焦点或用户输入上下文，不能表达 runtime event 的归属。

### 4.3 Runtime observation 不应隐式切换 UI active chat

`ensure_runtime_turn(context)` 只负责确保对应 chat/turn 数据存在。它不应修改 `active_chat_id`。

UI active chat 的变化只能由用户动作、明确的 session selection 或 application 层显式命令触发。

### 4.4 Block 更新必须受 context 限定

所有输出 block 的写入与更新必须使用 context 限定：

```text
(chat_id, turn_id, block/tool id/provider id)
```

不能只按 tool id、provider id、active block id 或 block 顺序匹配。

## 5. 当前问题分析

### 5.1 `Observe*` 混合了外部观察和领域命令

`ConversationIntent::ObserveAssistantText`、`ObserveToolCallUpdate` 等名字来自 runtime 观察视角，不是 conversation domain 的统一语言。它们直接进入 `ConversationModel.apply()`，导致领域模型理解 provider id、arguments delta、orphan result、tool streaming 顺序等外部细节。

设计上应拆成两层：

```text
RuntimeObservation
  -> RuntimeObservationProjector / Application Service
  -> ConversationCommand + OutputTimelineCommand
```

### 5.2 `ConversationModel` 过载

当前 `ConversationModel` 同时管理：

1. chats / turns
2. active chat
3. output blocks
4. active assistant/thinking block
5. tool call/result 绑定
6. orphan tool result promote
7. queued submissions
8. agent progress
9. ask_user block

这些职责属于不同子域。尤其 `blocks` 更像输出区 timeline read model，而不是 conversation aggregate。

### 5.3 Presenter 反向依赖 render

`view_assembler/output.rs` 不应依赖 render 层的 nesting / tool display 细节。Presenter 可以决定展示语义，但不应依赖具体 renderer 的模块。

依赖方向应是：

```text
model -> view_assembler -> view_model -> render
```

而不是：

```text
view_assembler -> render
```

### 5.4 Adapter 反向依赖 render text utility

`adapter/agent_event.rs` 使用 render/display 下的 safe text utility。字符串安全切片是通用文本处理能力，应移动到 TUI shared text utility，而不是放在 render 层。

## 6. 目标分层

推荐结构：

```text
tui/
  app/或update/
    root_reducer.rs
    runtime_observation.rs
    dirty_policy.rs
    effects.rs

  adapter/
    agent_event.rs
    key_event.rs
    effect_result.rs

  model/
    conversation/
    output_timeline/
    input/
    runtime/
    diagnostic/
    session/

  view_assembler/
    output.rs
    output_nesting.rs
    status.rs
    dialog.rs

  view_model/
    output.rs
    status.rs
    dialog.rs
    input.rs

  render/
    output/
    input/
    status/
    dialog/
    theme/

  text/
    safe_text.rs
```

依赖规则：

1. `model` 不依赖 `view_model`、`view_assembler`、`render`、`ratatui`。
2. `adapter` 不依赖 `render`。
3. `update/application` 可以调用 model，并产出 effects / dirty changes。
4. `view_assembler` 只依赖 model 与 view_model，不依赖具体 widget。
5. `render` 只消费 view_model 与 view state，不读取 domain model。
6. `app.rs` 只做 composition、event loop、调度和 frame 组装。

## 7. 新增核心概念

### 7.1 RuntimeTurnContext

定义 TUI 内部强类型 context：

```text
RuntimeTurnContext
  - chat_id: ChatId
  - turn_id: ChatTurnId
```

它应替代散落在 Observe intent 中的独立 `chat_id, turn_id` 字段，减少遗漏概率。

### 7.2 RuntimeObservation

新增 application 层事件语言，用于表达 runtime 到 TUI 的观察事件：

```text
RuntimeObservation
  - AssistantText { context, text }
  - ThinkingText { context, text }
  - BlockCompleted { context }
  - ToolCallStarted { context, id, provider_id, name, index }
  - ToolCallUpdated { context, id, provider_id, name, index, arguments, summary, status }
  - ToolResultObserved { context, id, provider_id, tool_name, output, content, is_error, image_count }
  - AgentProgressObserved { context, tool_id, message }
  - TurnCompleted { context, reason }
```

`adapter/agent_event.rs` 负责从 `UiEvent` 构造 `RuntimeObservation`。如果某个 SDK/TUI event 无法提供 context，则不允许进入该 observation 分支，应在 adapter 层记录 diagnostic 或扩展 SDK DTO。

### 7.3 OutputTimelineModel

新增输出区 read model：

```text
OutputTimelineModel
  - blocks: Vec<ConversationBlock 或 OutputTimelineBlock>
  - active_text_block: Option<(RuntimeTurnContext, BlockId)>
  - active_thinking_block: Option<(RuntimeTurnContext, BlockId)>
  - orphan_tool_results: 受 context 限定的暂存状态
```

`ConversationModel` 保留 conversation domain 状态：

```text
ConversationModel
  - chats
  - active_chat_id
  - queued_submissions
```

长期可进一步把 AskUser、Diagnostic、AgentProgress 从 conversation blocks 中剥离，但本轮优先完成 timeline 职责拆分。

### 7.4 ToolFlowProjector

负责处理 tool call/result 的 observation 到 timeline/domain patch：

```text
ToolFlowProjector
  input: ToolCallStarted / ToolCallUpdated / ToolResultObserved
  output: ConversationCommand + OutputTimelineCommand
```

职责包括：

1. runtime id 与 provider id 候选匹配
2. update 早于 start 时创建 placeholder tool call
3. result 早于 call/update 时暂存 orphan result
4. tool call 出现后 promote orphan result
5. result block 移动到对应 tool call 后
6. 所有匹配都限定在同一个 `RuntimeTurnContext`

## 8. 数据流设计

### 8.1 Runtime event 入口

```text
sdk::ChatEvent
  -> sdk_event_to_ui_event()
  -> UiEvent::{..., context: UiTurnContext}
  -> map_agent_event()
  -> RuntimeObservation::{..., context: RuntimeTurnContext}
```

要求：

1. `sdk::ChatEventContext` 中的 `chat_id / turn_id` 必须完整映射到 `UiTurnContext`。
2. `UiTurnContext` 必须完整映射到 `RuntimeTurnContext`。
3. 不允许任何 runtime observation event 在 mapping 时丢弃 context。

### 8.2 Application 处理

```text
RuntimeObservation
  -> RuntimeObservationProjector
  -> ConversationCommand / OutputTimelineCommand / RuntimeIntent / DiagnosticIntent
  -> models.apply(...)
  -> ModelChange
  -> DirtyPolicy
  -> Effect::RequestRender
```

Projector 是 anti-corruption layer：它可以理解 SDK/runtme observation 的细节，但 domain model 不应该直接理解 SDK event 形态。

### 8.3 Presenter 与 Render

```text
ConversationModel + OutputTimelineModel
  -> OutputViewAssembler
  -> OutputViewModel
  -> DocumentRenderer
  -> OutputArea widget
```

Presenter 负责展示语义：标题、状态文案、层级、tool display summary、diagnostic block 映射。

Render 负责终端绘制：wrap、gutter、scroll、cache、style、ratatui spans。

## 9. VPA 定位

### View

View 是 ratatui widget 和 render module：

1. `OutputArea`
2. `InputArea`
3. `StatusBar`
4. dialog / popup
5. `DocumentRenderer`

View 只消费 ViewModel 与 ViewState，不读取 domain model。

### Presenter

Presenter 是 `view_assembler/**`：

1. 将 domain/read model 组合成 ViewModel。
2. 处理展示语义，如 block title、工具显示摘要、层级、状态标签。
3. 不持有长期状态。
4. 不执行 I/O。

### Application

Application 是 `update/**` 与 projector：

1. 接收 `TuiMsg` / `RuntimeObservation`。
2. 调用 model。
3. 维护 dirty policy。
4. 产生 effects。
5. 不直接渲染。

### Domain / Model

Model 是 UI Domain 的状态与规则：

1. conversation domain
2. output timeline read model
3. runtime model
4. input model
5. diagnostic/session model

Model 不依赖 ratatui 和 render。

## 10. 迁移步骤

### 阶段 1：边界减污，不改行为

1. 新增 `tui/text/safe_text.rs`，迁移 safe text utility。
2. 将 output nesting 规则从 `render/output` 移到 `view_assembler` 或 `view_model`。
3. 保持 `ToolDisplayEntry` 展示注册行为不变，但避免 domain/adapter 依赖 render。
4. 增加依赖方向检查用的代码审查清单。

验收：adapter/view_assembler 不再依赖 render module 的实现细节。

### 阶段 2：引入 RuntimeObservation 与 RuntimeTurnContext

1. 新增 `RuntimeTurnContext`。
2. 新增 `RuntimeObservation`。
3. `map_agent_event()` 先生成 runtime observation，再由 projector 转现有 `ConversationIntent`。
4. 将 `ConversationIntent::Observe*` 改为 `context: RuntimeTurnContext` 或等价强类型字段。
5. 为所有 runtime streaming event 增加 context-preservation 测试。

验收：从 `UiEvent` 到 `ConversationIntent/Command` 的 context 不可丢失。

### 阶段 3：移除 active 归属反查

1. 拆分 `ensure_runtime_turn()` 与 `activate_chat()`。
2. `ensure_runtime_turn()` 不再修改 `active_chat_id`。
3. `RecordAgentProgress` 补齐 `RuntimeTurnContext`，禁止使用 `active_chat_mut()` 查找 tool。
4. `CompleteChat` 替换为 `CompleteRuntimeTurn { context, reason }` 或 `CompleteChat { chat_id }`。
5. runtime observation 路径中禁止 `active_chat_mut()` / `active_turn_mut()`。

验收：active drift 测试覆盖 text、thinking、block complete、tool、agent progress、turn complete。

### 阶段 4：拆 OutputTimelineModel

1. 新增 `model/output_timeline/`。
2. 将 `ConversationModel.blocks`、active text/thinking block、orphan tool result 迁入 OutputTimelineModel。
3. `ConversationModel` 保留 chats/turns/active_chat/queued submissions。
4. `ViewAssembler` 改为读取 `OutputTimelineModel`。
5. 保持现有 `ConversationBlock` 名称可短期复用，后续再重命名为 `OutputTimelineBlock`。

验收：输出区 block 顺序与现有行为一致，conversation domain 不再负责渲染 timeline。

### 阶段 5：抽 ToolFlowProjector

1. 将 tool call start/update/result 的绑定逻辑从 model 中抽出。
2. Projector 产出对 conversation turn 与 output timeline 的 patch。
3. 所有 provider id/runtime id 匹配限定在 context 内。
4. orphan result promote 与 move result after call 只操作同 context blocks。

验收：现有 tool flow 测试全部通过，并新增跨 turn 同 id 不串线测试。

### 阶段 6：Render 纯化

1. Render 只消费 `OutputViewModel`。
2. tool display lookup、nesting、block semantic title 等逻辑收敛到 Presenter。
3. Render 保留 wrap、style、gutter、cache、scroll、selection、animation frame。

验收：render 层不读取 domain model，不执行业务归属判断。

## 11. 测试策略

### 11.1 Context preservation tests

覆盖每个 mapping：

1. SDK event → UiEvent 保留 context。
2. UiEvent → RuntimeObservation 保留 context。
3. RuntimeObservation → model command 保留 context。

### 11.2 Active drift tests

每类 runtime observation 都要有 active drift 测试：

1. 创建 live context。
2. 将 active chat/turn 切到 stale context。
3. 投递 live context 的 runtime event。
4. 断言数据写入 live context，不写入 stale context。

覆盖：

1. assistant text
2. thinking text
3. block complete
4. tool start/update/result
5. agent progress
6. turn complete / cancelled

### 11.3 History restore / display replay tests

覆盖：

1. restore 后 active streaming block 被清理或受 context 校验。
2. replay 旧 blocks 后新 runtime event 不追加到旧 block。
3. replay 不会改变 runtime observation 的 context 来源。

### 11.4 Tool flow tests

覆盖：

1. update 早于 start。
2. result 早于 call。
3. provider id 与 runtime id 不同。
4. 不同 turn 中 tool id 重复。
5. 不同 chat 中 provider id 重复。
6. orphan promote 只在同 context 内发生。

### 11.5 Render regression tests

保持现有 snapshot/文档渲染测试语义：

1. tool result 仍嵌在对应 tool call 下。
2. assistant/thinking block 顺序不变。
3. status/input/dialog 不受 timeline 拆分影响。
4. dirty flag 触发 render 的行为不变。

## 12. 风险与缓解

### 风险 1：拆分 OutputTimelineModel 影响范围大

缓解：先复用现有 `ConversationBlock`，只移动所有权，不立即重命名和重塑 block enum。

### 风险 2：`Done/Cancelled` 当前缺少 context

缓解：优先扩展 SDK lifecycle event。如果短期无法扩展，则 adapter 层必须明确标记为 legacy lifecycle event，并禁止它修改具体 turn；只允许更新全局 spinner/status。

### 风险 3：ToolFlowProjector 与现有 model 测试耦合

缓解：先将现有 tool flow 测试复制到 projector 层，再从 model 迁移逻辑。

### 风险 4：依赖方向整理引发模块循环

缓解：新增小型 shared/presentation utility module，例如 `tui/text`、`view_model/output_nesting`，避免在 render 与 assembler 间互相引用。

## 13. 验收标准

1. `adapter/**` 不依赖 `render/**`。
2. `view_assembler/**` 不依赖 `render/**`。
3. runtime observation event 全链路携带 `RuntimeTurnContext`。
4. runtime observation 路径中没有 active chat/turn 反查归属。
5. `ensure_runtime_turn()` 不修改 UI active chat。
6. `RecordAgentProgress` 与 turn completion 不再依赖 active turn。
7. `OutputTimelineModel` 成为输出 blocks 的所有者。
8. Tool call/result 绑定逻辑集中在 `ToolFlowProjector`。
9. 所有 active drift、history restore、tool id collision 测试通过。
10. TUI 视觉与现有行为保持一致。

## 14. 与既有设计的关系

本设计延续并细化：

1. `047-051-ui-domain-ddd.md`：确认 TUI 是 UI Domain，而不是薄 adapter。
2. `047-tui-sdk-dto-boundary-design.md`：保持 SDK DTO 边界，runtime 内部类型不泄漏到 TUI。
3. `047-050-cli-tui-directory-cleanup.md`：目录整理为本设计的物理落地提供基础。
4. `048-tui-resize-render-refresh.md`：resize 属于 View/ViewState 刷新问题，不改变本设计的 runtime observation 分层。

本设计聚焦 Display Pipeline 与 Runtime Observation 的边界治理，是 UI Domain DDD 设计在 TUI 渲染链路上的具体化。
