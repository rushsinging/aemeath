# #49 TUI 渲染管线与 Runtime Observation Context 重构设计（修订版）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/151
> Issue 补充评论: https://github.com/rushsinging/aemeath/issues/151#issuecomment-4697155019

## 修订记录

- **rev.2（本版）**：基于对源码的逐条 fact-check，整合架构 review 意见——
  1. 点名真正的 bug 机制：`BindRuntimeTurn` → `Observe*` 两步有状态协议（经 active 中转），并明确废弃它（§5.1、§7.1）。
  2. 新增「双真相源 + 死字段」问题（§5.5）与「`ConversationChange` 过度建模」问题（§5.6），均附源码实证。
  3. `OutputTimelineModel` 从「搬家复用 `ConversationBlock`」升级为「存有类型引用（id），payload 单一 owner」，从源头消除 tool 数据双写（§4.5、§7.3）。
  4. 规定 projector 双 patch 的原子性约束（§7.4）。
  5. 新增「最小止血方案」（§9-bis），满足宪法「止血与根因并陈」要求。
  6. 重排迁移相位：纠错相位（context）提前、作为可独立发布单元；结构去重与 timeline 拆分合并一次做（§11）。
  7. 补 history restore 合成 context 的显式设计；将「扩 SDK lifecycle context」升为去 active 反查的硬前置（§10.3、§12、§14）。
- **rev.1**：初版设计稿。

## 状态

- 状态：设计稿（rev.2）
- 范围：`apps/cli/src/tui/**` 的 runtime event → model → view model → render 链路
- 目标架构：DDD + 六边形架构 + Clean Architecture + VPA（View / Presenter / Application）
- 核心不变量：runtime streaming event 的归属必须来自事件自带的 `chat_id + turn_id`，永远不能从 UI active 状态反查；同一份显示数据必须单一 owner，不得跨 store 双写

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
4. 曾出现过 `UiEvent` 原本携带正确 runtime `turn_id`，但归属在进入 model 时丢失，model 再从 `active_chat_id / active_turn` 反查，导致 history restore、display replay、旧 block 或其他事件影响后拿到错误 turn 的问题。

### 1.1 第 4 点的真实机制（rev.2 补充）

第 4 点不是「context 没传进来」，而是「context 传进来了却被绕过」。当前实现是一个**两步、跨 intent、经 active 中转**的协议：

```text
map_agent_event 每个 runtime 分支：
  1) 先 push  ConversationIntent::BindRuntimeTurn { chat_id, turn_id }
  2) 再 push  ConversationIntent::Observe* { ...无 chat_id/turn_id... }

model.apply(BindRuntimeTurn) -> ensure_runtime_turn() -> 写 self.active_chat_id
model.apply(Observe*)        -> current_runtime_turn() -> 读 self.active_chat_id + active_turn()
```

实证：

- `adapter/agent_event.rs`：`Text/Thinking/BlockComplete/ToolCallStart/ToolCallUpdate/ToolResult` 等分支均「先 `conversation(BindRuntimeTurn{..})`、后 push `Observe*`」。
- `model/conversation/model.rs`：`ensure_runtime_turn()` 写 `self.active_chat_id = Some(chat_id)`；`observe_tool_call_start / observe_tool_call_update / append_assistant_text` 经 `current_runtime_turn()` 读 `active_chat_id` 与该 chat 的 `active_turn()`。
- `ConversationIntent::Observe*` 自身**不携带** `chat_id/turn_id`。

因此 context 实际上只活在 `BindRuntimeTurn` 与 `ensure_runtime_turn` 之间，随即被压进 active 状态并丢失强绑定。任何东西在 bind 与 observe 之间改了 active（history restore 合成 chat、display replay、其它事件插入、未来多 turn 并发），observe 就绑错 turn。这是 §1.4 串线的根因机制。

本设计目标是将这类问题从架构层面杜绝，而不是局部补丁修复。

## 2. 设计目标

1. 明确 TUI 渲染链路的分层边界：Adapter、Application、Domain Model、Presenter、ViewModel、Render。
2. 将 runtime observation 与 conversation domain language 隔离。
3. 建立强制 runtime turn context 不变量，防止 `turn_id` 在任何层丢失，并**废弃经 active 中转的 `BindRuntimeTurn` 协议**。
4. 拆分 `ConversationModel.blocks` 的职责，引入 `OutputTimelineModel` 作为输出区 read model，且**只存有类型引用（id），不复制 payload**——同一份显示数据单一 owner（DRY，宪法 #4）。
5. 将 tool call/result 的 id 绑定、乱序修复、orphan promote 逻辑收敛到 `ToolFlowProjector`，并保证其对多 model 的 patch 原子应用。
6. 清除上层对 render 模块的反向依赖，保持 Clean Architecture 依赖方向。
7. 保持现有 TUI 行为和视觉表现不变，只整理架构边界和内部数据流。
8. 为后续渲染优化、session replay、多会话/多 turn 显示打下稳定基础。

## 3. 非目标

1. 不重写 ratatui widget 或视觉样式。
2. 不改变 SDK 对外语义，除非为补全缺失的 runtime context 必须扩展 DTO（见 §10.3、§13 风险2）。
3. 不引入新的 UI 状态管理框架。
4. 不一次性拆 crate；本轮仍在 `apps/cli/src/tui/**` 内演进。
5. 不改变 provider / runtime chat loop 的执行逻辑。
6. 不实现新的多会话 UI，只保证未来可支持。
7. 不把所有历史 session 数据模型一次性迁移；history restore 只需遵守新 context 不变量（合成 context 的规则见 §10.3、§12.3）。

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

该 context **必须作为每个 observation 的内联字段**直达 model 写入点，不得拆成「先 bind、后读 active」的两步协议（见 §7.1）。

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

### 4.5 同一份显示数据单一 owner（rev.2 新增）

任一可显示数据（tool call 的 status/args/summary/result、文本/thinking/系统/错误正文等）**必须只有一个所有者**：

1. tool call / tool result 的内容真相只存于 `ConversationModel.chats`（`ChatTurn.tool_calls`）。
2. 输出区 timeline **只持有有类型的引用（id）+ 交织顺序**，不复制上述 payload。
3. Presenter（ViewAssembler）在 assemble 时按 id **join** 出 ViewModel。

禁止出现「同一字段在两个 model / 两个 struct 里各存一份、靠手工同步」的结构。当前 `ConversationBlock::ToolCall { name, summary, args_preview }` 即违反本条（见 §5.5），重构后必须消除。

## 5. 当前问题分析

### 5.1 `Observe*` 混合了外部观察和领域命令，且经 active 中转

`ConversationIntent::ObserveAssistantText`、`ObserveToolCallUpdate` 等名字来自 runtime 观察视角，不是 conversation domain 的统一语言。它们直接进入 `ConversationModel.apply()`，导致领域模型理解 provider id、arguments delta、orphan result、tool streaming 顺序等外部细节。

更关键的是归属传递方式：context 经独立的 `ConversationIntent::BindRuntimeTurn` 写入 active、再由 observe handler 从 active 读回（机制见 §1.1）。这是一个有状态、可被打断的两步协议，是串线 bug 的真正来源。

设计上应拆成两层，并让 context 内联随行、一步到位：

```text
RuntimeObservation { context, .. }            // context 内联，无独立 bind 步骤
  -> RuntimeObservationProjector / Application Service
  -> ConversationCommand + OutputTimelineCommand   // 命令均带 context
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

`view_assembler/output.rs` 不应依赖 render 层的 nesting / tool display 细节。实证：它 `use crate::tui::render::output::nesting::{allowed_child, MAX_BLOCK_DEPTH}` 与 `crate::tui::render::output::tool_display::lookup_display`。Presenter 可以决定展示语义，但不应依赖具体 renderer 的模块。

依赖方向应是：

```text
model -> view_assembler -> view_model -> render
```

而不是：

```text
view_assembler -> render
```

### 5.4 Adapter 反向依赖 render text utility

`adapter/agent_event.rs` 使用 render/display 下的 safe text utility（`use crate::tui::render::display::safe_text::safe_str_slice_by_char`）。字符串安全切片是通用文本处理能力，应移动到 TUI shared text utility，而不是放在 render 层。

### 5.5 双真相源与死字段（rev.2 新增）

`ConversationModel` 内并存两套同一对话的表示：

- `chats: Vec<Chat>` → `ChatTurn.tool_calls`（`ToolCall { status, args_preview, summary, result, activities }`）——工具生命周期的真实 owner，`observe_*` 正确改它。
- `blocks: Vec<ConversationBlock>`——扁平、按显示顺序排列的列表，由 `tool_order.rs` 命令式拼接（`insert_tool_call_block_before_active_text` / `insert_tool_result_after_tool_call` / `move_tool_results_after_tool_call` / `promote_orphan_tool_result`）。

两者对 tool 数据**双写**，且 `blocks` 侧是**死副本**：

- `ConversationBlock::ToolCall { id, name, summary, args_preview }` 的 `name/summary/args_preview` 三字段**只写不读**——所有 match 都是 `{ id, .. }`，只在 `tool_order.rs` 与 `observe_tool_call_update` 写入；渲染侧 `view_assembler/output.rs` 拿到 block 后丢弃它们、改用 `find_tool_view()` 回 `chats` 取真值。
- 渲染一行工具，数据劈在 `blocks`（顺序、result content）与 `chats`（status/args/summary/result）两处，靠手工顺序索引缝合。

这违反宪法 #4（DRY：数据访问逻辑 MUST 定义一次）。注意：`blocks` 并非纯冗余——跨异构条目（用户/assistant/thinking/工具/系统/错误/askuser）的**交织顺序只有 `blocks` 持有**，`chats` 无法重建。所以正解不是删 `blocks`，而是让它退化为「只存顺序 + 引用」（§4.5、§7.3）。

### 5.6 `ConversationChange` 过度建模（rev.2 新增）

`ConversationChange` 有约 20 个变体（`ChatStarted / ToolCallObserved / ToolCallBound / OrphanToolResultObserved / StyleBoundaryResetRequired / ...`），但唯一消费者 `update/root_reducer.rs::apply_conversation_changes` 把它们全部塌缩为两个脏位（`mark_output()` / `mark_status()`）。这是 write-only 仪式：要么让 change 真正驱动增量更新/订阅，要么收敛粒度到消费者实际需要的程度。本设计将其纳入 `ModelChange + DirtyPolicy`（§8.2）统一治理。

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

### 7.1 RuntimeTurnContext（替代 BindRuntimeTurn 两步协议）

定义 TUI 内部强类型 context：

```text
RuntimeTurnContext
  - chat_id: ChatId
  - turn_id: ChatTurnId
```

它**内联**进每个 observation 与其派生命令，作为单一载体随行：

- **废弃** `ConversationIntent::BindRuntimeTurn`：不再有「先 bind 写 active、后 observe 读 active」的两步协议。
- `RuntimeObservation` 的每个变体、以及 projector 产出的 `ConversationCommand / OutputTimelineCommand`，均带 `context: RuntimeTurnContext` 字段。
- model 写入点直接消费 context 参数定位 `(chat_id, turn_id)`，**不读 `active_*`**。

### 7.2 RuntimeObservation

新增 application 层事件语言，用于表达 runtime 到 TUI 的观察事件（context 一律内联）：

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

`adapter/agent_event.rs` 负责从 `UiEvent` 构造 `RuntimeObservation`。如果某个 SDK/TUI event 无法提供 context，则不允许进入该 observation 分支，应在 adapter 层记录 diagnostic 或扩展 SDK DTO（见 §13 风险2）。

### 7.3 OutputTimelineModel（只存引用，不存 payload）

新增输出区 read model：

```text
OutputTimelineModel
  - items: Vec<OutputTimelineItem>          // 只存交织顺序 + 有类型引用
  - active_text_block: Option<(RuntimeTurnContext, BlockId)>
  - active_thinking_block: Option<(RuntimeTurnContext, BlockId)>
  - orphan_tool_results: 受 context 限定的暂存状态（仅暂存，最终 owner 仍是 chats）

OutputTimelineItem  // 引用语义，禁止内嵌 tool payload 副本
  - ToolCall   { context, tool_call_id }    // status/args/summary/result 回 chats 取
  - ToolResult { context, tool_call_id }    // 同上；仅记录「结果应渲染在此调用之后」
  - AssistantText { block_id }
  - Thinking      { block_id }
  - UserMessage   { block_id }
  - System / Error / HookNotice / AgentProgress / AskUser { block_id }
```

要点：

1. tool 条目**只引用 `tool_call_id`**，不复制 `name/summary/args_preview/result`——直接消除 §5.5 的死字段与双写。
2. 文本/thinking/系统/错误等 payload 进各自单一 owner 的 keyed store（短期可沿用现有 `ConversationBlock` 变体承载这些非 tool 文本，但 tool 变体必须降为引用）。
3. `insert_*_before_active_text / move_tool_results_after_tool_call / promote_orphan_tool_result` 这套命令式拼接，要么因「顺序在 append 时记一次」而消失，要么作为**纯排序/join pass** 收敛进 ToolFlowProjector 或 Presenter。

`ConversationModel` 保留 conversation domain 状态：

```text
ConversationModel
  - chats                  // tool call/result 的唯一 owner
  - active_chat_id         // 仅表达 UI 焦点，不参与 runtime 归属
  - queued_submissions
```

长期可进一步把 AskUser、Diagnostic、AgentProgress 从 timeline payload 中剥离为各自 read model，但本轮优先完成 timeline 职责拆分与 tool 数据去重。

### 7.4 ToolFlowProjector（多 model patch 原子应用）

负责处理 tool call/result 的 observation 到 timeline/domain patch：

```text
ToolFlowProjector
  input: ToolCallStarted / ToolCallUpdated / ToolResultObserved（均带 context）
  output: ConversationCommand + OutputTimelineCommand（均带 context）
```

职责包括：

1. runtime id 与 provider id 候选匹配（限定在同一 `RuntimeTurnContext` 内）
2. update 早于 start 时创建 placeholder tool call
3. result 早于 call/update 时暂存 orphan result（owner 仍是 chats，timeline 只记引用）
4. tool call 出现后 promote orphan result
5. result 引用排到对应 tool call 引用之后
6. 所有匹配都限定在同一个 `RuntimeTurnContext`

**原子性约束（rev.2 新增）**：projector 对 `ConversationModel` 与 `OutputTimelineModel` 的两份 patch **必须在同一次 reduce 内原子应用**，不得出现「chats 已更新、timeline 未更新」的半态。由于 §7.3 让 timeline 只存引用、payload 单一 owner，二者天然不会内容不一致；本约束进一步保证「引用存在性」与「内容存在性」同帧一致（例如：append 了 ToolCall 引用，则该 `tool_call_id` 必已在 chats 中存在 placeholder）。

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
3. 不允许任何 runtime observation event 在 mapping 时丢弃 context，**也不允许退化为经 active 推导**。

### 8.2 Application 处理

```text
RuntimeObservation
  -> RuntimeObservationProjector
  -> ConversationCommand / OutputTimelineCommand / RuntimeIntent / DiagnosticIntent（均带 context）
  -> models.apply(...)   // ConversationModel 与 OutputTimelineModel 的 patch 原子应用
  -> ModelChange
  -> DirtyPolicy
  -> Effect::RequestRender
```

Projector 是 anti-corruption layer：它可以理解 SDK/runtime observation 的细节，但 domain model 不应该直接理解 SDK event 形态。`ModelChange + DirtyPolicy` 统一收敛原 `ConversationChange` 的脏位派生职责（§5.6）。

### 8.3 Presenter 与 Render

```text
ConversationModel + OutputTimelineModel
  -> OutputViewAssembler        // 按 timeline 顺序遍历引用，回 chats join 出 tool 视图
  -> OutputViewModel
  -> DocumentRenderer
  -> OutputArea widget
```

Presenter 负责展示语义：标题、状态文案、层级、tool display summary、diagnostic block 映射、orphan/非嵌入 result 的摘要化。

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

## 9-bis. 最小止血方案（与根因方案并陈）

> 依据宪法「同一问题既可临时止血也能彻底重构时，MUST 同时给出最小化补丁与根因级彻底方案」。本节是止血；§4–§8、§11 是根因方案，且为默认推荐路径。

**止血目标**：仅消除 §1.4 的 active drift（拿到错误 turn），不做结构重塑。

**最小改动**（属于根因方案相位 A/B 的真子集，可先单独发版）：

1. 给 `ConversationIntent::Observe*` 直接加上 `context: RuntimeTurnContext` 字段，由 `map_agent_event` 内联填充；observe handler 用该字段定位 `(chat_id, turn_id)`，不再调用 `current_runtime_turn()`。
2. `ensure_runtime_turn()` **停止写 `active_chat_id`**；active 的变更只由用户动作/session selection 触发。
3. `RecordAgentProgress` 与 `CompleteChat` 携带 context，不再用 `active_chat_mut()` 查找 turn/tool。

**优劣对比**：

| | 最小止血 | 根因方案 |
|---|---|---|
| 改动面 | 小（仅归属定位路径） | 大（多相位，含 timeline 拆分/去重/依赖治理） |
| 风险 | 低 | 中（需完整测试护栏） |
| 消除 active drift | ✅ | ✅ |
| 消除双真相源/死字段（§5.5） | ❌ | ✅ |
| 消除依赖反转（§5.3/5.4） | ❌ | ✅ |
| 收敛 tool flow / change 建模（§5.6/§7.4） | ❌ | ✅ |
| 复发风险 | 中（结构问题仍在，易再次滋生反查） | 低 |

**推荐**：以止血方案作为根因方案相位 A/B 的首个可交付增量，先拿到正确性收益；随后按 §11 推进结构改造，避免结构缺陷复发。

## 10. 关键集成点设计

### 10.1 依赖方向治理（对应 §5.3/§5.4）

1. 新增 `tui/text/safe_text.rs`，迁移 safe text utility，adapter 改依赖它。
2. 将 output nesting 规则（`allowed_child` / `MAX_BLOCK_DEPTH`）与 tool display lookup 的「展示语义」部分移到 `view_assembler` / `view_model`；render 侧只保留绘制。
3. 增加依赖方向检查清单/门禁：`adapter/**`、`view_assembler/**` 不得 `use crate::tui::render::*`。

### 10.2 active 与 runtime 归属解耦（对应 §4.2/§4.3）

1. 拆分 `ensure_runtime_turn()`（仅确保数据存在）与 `activate_chat()`（仅由用户/session 触发）。
2. runtime observation 路径中静态禁止 `active_chat_mut()` / `active_turn_mut()`。

### 10.3 History restore / lifecycle 的 context 合成（rev.2 新增）

history restore（`render/display/render.rs`）当前无真实 runtime context，捏造 `ChatId::new("history-chat")` / `ChatTurnId::new("turn-1")` 并经同一套 `Observe*` 灌入。新不变量下，restore 必须**显式合成确定性 context**：

1. restore 为每条历史 assistant message 生成稳定的 `(synthetic_chat_id, synthetic_turn_id)`，并将该 context 作为 `RuntimeObservation`/command 的内联字段——与 live 路径走同一写入入口，复用同一 context 校验。
2. restore 完成后必须清理（或以 context 校验隔离）遗留的 active streaming block，确保后续 live runtime event 不会因 active 漂移而追加到历史 block（§4.2 第 6 项）。
3. `Done/Cancelled` 等 lifecycle 事件的 context 缺失，是去 active 反查的**硬前置**：优先扩展 SDK lifecycle event 携带 `(chat_id, turn_id)`；在扩展落地前，adapter 必须把它们标为 legacy lifecycle，**禁止修改具体 turn**，只允许更新全局 spinner/status。

## 11. 迁移步骤（rev.2 重排：纠错优先，结构去重合并）

> 排序原则：先发有用户可见正确性收益的纠错相位（可独立发布），再做架构卫生与结构去重；把「最高风险、最低即时价值」的 timeline 拆分与 tool 去重合并为一次完成，避免两次改动同一批 block 路径。

### 相位 A：最小止血（§9-bis，可独立发布）

落地 §9-bis 三步：Observe 内联 context、`ensure_runtime_turn` 不写 active、progress/complete 带 context。
验收：active drift 测试（text/thinking/block complete/tool/agent progress/turn complete）全绿；行为与视觉不变。

### 相位 B：RuntimeObservation 单载体 + 废弃 BindRuntimeTurn（纠错核心）

1. 新增 `RuntimeTurnContext`、`RuntimeObservation`。
2. `map_agent_event()` 生成 `RuntimeObservation`（context 内联），经 projector 转命令；**删除 `ConversationIntent::BindRuntimeTurn`**。
3. 为所有 runtime streaming event 增加 context-preservation 测试（SDK→UiEvent→Observation→command 三段保真）。
4. 扩展 SDK lifecycle event context（§10.3 第 3 项），使 `TurnCompleted` 带 context。

验收：从 `UiEvent` 到命令的 context 不可丢失，且链路中无 `BindRuntimeTurn`、无经 active 推导归属。

### 相位 C：边界减污（低风险，可与 A/B 并行）

落地 §10.1：safe_text 迁移、nesting/tool display 展示语义上移、依赖方向门禁。
验收：`adapter/**`、`view_assembler/**` 不再 `use crate::tui::render::*`。

### 相位 D：OutputTimelineModel 拆分 + tool 数据去重（结构核心，一次做透）

1. 新增 `model/output_timeline/`。
2. 将顺序/active text-thinking/orphan 暂存迁入 `OutputTimelineModel`；**timeline 的 tool 条目降为 `tool_call_id` 引用**，删除 `ConversationBlock::ToolCall` 的 `name/summary/args_preview` 死字段（§5.5）。
3. `ConversationModel` 保留 chats/turns/active_chat/queued submissions，作为 tool 数据唯一 owner。
4. `ViewAssembler` 改为「遍历 timeline 引用 + 回 chats join」。

验收：输出区 block 顺序与现有行为一致；无任何字段在 chats 与 timeline 双写；conversation domain 不再负责渲染 timeline。

### 相位 E：抽 ToolFlowProjector（含原子性约束）

1. 将 tool call start/update/result 绑定逻辑从 model 抽到 projector。
2. projector 产出对 conversation turn 与 timeline 的 patch，**同帧原子应用**（§7.4）。
3. 所有 provider id/runtime id 匹配限定在 context 内；orphan promote / result 排序只操作同 context 引用。

验收：现有 tool flow 测试全部通过，新增跨 turn 同 id 不串线、跨 chat 同 provider id 不串线测试。

### 相位 F：Render 纯化 + ConversationChange 收敛

1. Render 只消费 `OutputViewModel`，不读 domain model、不做归属判断。
2. tool display lookup、nesting、block semantic title 收敛到 Presenter。
3. 将 `ConversationChange` 收敛/并入 `ModelChange + DirtyPolicy`（§5.6），消除 write-only 变体。

验收：render 层不读取 domain model；脏位派生集中于 DirtyPolicy；dirty flag 触发 render 行为不变。

## 12. 测试策略

### 12.1 Context preservation tests

覆盖每个 mapping：

1. SDK event → UiEvent 保留 context。
2. UiEvent → RuntimeObservation 保留 context。
3. RuntimeObservation → model command 保留 context。
4. 链路中不存在 `BindRuntimeTurn`、不存在经 active 推导归属的路径（结构性断言）。

### 12.2 Active drift tests

每类 runtime observation 都要有 active drift 测试：

1. 创建 live context。
2. 将 active chat/turn 切到 stale context。
3. 投递 live context 的 runtime event。
4. 断言数据写入 live context，不写入 stale context。

覆盖：assistant text、thinking text、block complete、tool start/update/result、agent progress、turn complete / cancelled。

### 12.3 History restore / display replay tests

覆盖：

1. restore 合成确定性 context，并与 live 路径走同一写入入口（§10.3）。
2. restore 后 active streaming block 被清理或受 context 校验。
3. replay 旧 blocks 后新 runtime event 不追加到旧 block。
4. replay 不会改变 runtime observation 的 context 来源。

### 12.4 Tool flow tests

覆盖：

1. update 早于 start。
2. result 早于 call。
3. provider id 与 runtime id 不同。
4. 不同 turn 中 tool id 重复。
5. 不同 chat 中 provider id 重复。
6. orphan promote 只在同 context 内发生。
7. projector 双 patch 原子性：断言 timeline 引用与 chats 内容同帧一致，无半态（§7.4）。

### 12.5 单一 owner / 去重 tests（rev.2 新增）

1. tool call 的 status/args/summary/result 仅存于 chats；timeline 不含这些 payload 副本。
2. 修改 tool 状态后，无需同步任何 timeline 字段即可正确渲染（防止死字段回归）。

### 12.6 Render regression tests

保持现有 snapshot/文档渲染测试语义：

1. tool result 仍嵌在对应 tool call 下。
2. assistant/thinking block 顺序不变。
3. status/input/dialog 不受 timeline 拆分影响。
4. dirty flag 触发 render 的行为不变。

## 13. 风险与缓解

### 风险 1：拆分 OutputTimelineModel 影响范围大

缓解：相位 D 一次做透「拆分 + 去重」，避免先复用后再去重导致两次改同一批 block 路径；非 tool 文本 payload 可短期沿用现有 `ConversationBlock` 变体承载，但 tool 变体必须降为引用。先迁移所有权与引用化，再考虑重命名为 `OutputTimelineBlock`/`OutputTimelineItem`。

### 风险 2：`Done/Cancelled` 当前缺少 context

缓解：将「扩展 SDK lifecycle event context」列为相位 B 的硬前置（§10.3 第 3 项）。在扩展落地前，adapter 层必须把 lifecycle event 标为 legacy 并禁止修改具体 turn，只允许更新全局 spinner/status。

### 风险 3：ToolFlowProjector 与现有 model 测试耦合

缓解：先将现有 tool flow 测试复制到 projector 层，再从 model 迁移逻辑；保留原 model 测试直到逻辑完全迁出。

### 风险 4：依赖方向整理引发模块循环

缓解：新增小型 shared/presentation utility module（`tui/text`、`view_model/output_nesting`），避免在 render 与 assembler 间互相引用。

### 风险 5：projector 双 model patch 一致性（rev.2 新增）

缓解：以 §7.3 的「timeline 只存引用、payload 单一 owner」从结构上消除内容不一致；再以 §7.4 的同帧原子应用保证引用与内容存在性一致；§12.4.7 增设半态断言测试。

## 14. 验收标准

1. `adapter/**` 不依赖 `render/**`。
2. `view_assembler/**` 不依赖 `render/**`。
3. runtime observation event 全链路携带 `RuntimeTurnContext`，且**不存在 `BindRuntimeTurn` 两步协议**。
4. runtime observation 路径中没有 active chat/turn 反查归属。
5. `ensure_runtime_turn()` 不修改 UI active chat。
6. `RecordAgentProgress` 与 turn completion 不再依赖 active turn。
7. `OutputTimelineModel` 成为输出 timeline（顺序 + 引用）的所有者；**tool 数据无双写、无死字段**（chats 为唯一 owner）。
8. Tool call/result 绑定逻辑集中在 `ToolFlowProjector`，对多 model 的 patch 同帧原子应用。
9. `ConversationChange` 收敛进 `ModelChange + DirtyPolicy`，无 write-only 变体。
10. 所有 active drift、history restore、tool id collision、单一 owner 测试通过。
11. TUI 视觉与现有行为保持一致。

## 15. 与既有设计的关系

本设计延续并细化：

1. `047-051-ui-domain-ddd.md`：确认 TUI 是 UI Domain，而不是薄 adapter。
2. `047-tui-sdk-dto-boundary-design.md`：保持 SDK DTO 边界，runtime 内部类型不泄漏到 TUI。
3. `047-050-cli-tui-directory-cleanup.md`：目录整理为本设计的物理落地提供基础。
4. `048-tui-resize-render-refresh.md`：resize 属于 View/ViewState 刷新问题，不改变本设计的 runtime observation 分层。

本设计聚焦 Display Pipeline 与 Runtime Observation 的边界治理，是 UI Domain DDD 设计在 TUI 渲染链路上的具体化。
