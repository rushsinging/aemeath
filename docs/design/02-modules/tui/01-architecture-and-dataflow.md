# TUI · 架构与数据流

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#795（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 TUI 的八层 TEA 管线、三条信息流、Model Context 分层、枚举定义、ViewAssembler/ViewModel/ViewState、SDK DTO 边界与架构门禁。TUI 是入站适配器，不承载业务。

## 1. 定位

TUI 是**入站适配器**（Hexagonal Primary Adapter）：

- 通过 Runtime-owned `AgentClient` 入站 OHS（由 SDK 发布）与 Runtime 通信
- **不承载业务逻辑**——所有业务决策在 Runtime，TUI 只负责状态投影和用户输入翻译
- **纯展示层**——Model 不执行 IO、不调 AgentClient、不发 channel；reducer 只产出 Change
- 基于 The Elm Architecture（TEA）变体：event → update → model → view → effect

> 原始方案保留在 `snapshot/design/04-tui-design.md` 作为历史归档；本文是目标架构的唯一战术真相。

## 2. 六边形边界

```
    用户终端
       │
       ▼
┌──────────────────────────────────────────────────┐
│  TUI（入站适配器）                                  │
│                                                    │
│  Terminal Event → Msg → update → Model → View     │
│                                    │               │
│  Effect ← ──────────────────────── ┘               │
│                                                    │
│  依赖契约：Runtime-owned AgentClient OHS（SDK 发布）   │
└──────────────────┬─────────────────────────────────┘
                   │ AgentClient trait
                   ▼
┌──────────────────────────────────────────────────┐
│  Runtime（Application Core）                        │
│  AgentClientImpl → ChatLoop → ToolExecutor → ...  │
└──────────────────────────────────────────────────┘
```

- TUI 通过 `AgentClient` trait 调用 Runtime（发送用户输入、启动 chat stream、执行 slash 命令）
- Runtime 通过 `ChatStream`（异步流）返回事件给 TUI
- **TUI 不直接依赖 Runtime 内部类型**——只通过 SDK DTO 通信（见 §8）

## 3. 八层 TEA 管线

```
① Terminal Event    crossterm Event（Key/Mouse/Paste/Resize）
       │
       ▼
② Msg               TuiMsg（统一输入信号）
       │
       ▼
③ Coordinator       App::update(msg)
   │                ├─ map_agent_event（TUI-owned UiEvent → Intent）
   │                ├─ root_reducer（Intent → Model change）
   │                └─ effects_for（Change → Effect request）
       │
       ▼
④ Model             TuiModel { conversation, input, diagnostic, session, config, workspace }
   │                apply(Intent) → Change
       │
       ▼
⑤ ViewAssembler     OutputViewAssembler / StatusViewAssembler / InputViewAssembler / DialogViewAssembler
   │                读 Model + ViewState → 产出 ViewModel
       │
       ▼
⑥ ViewModel         OutputViewModel / StatusLineViewModel / InputAreaViewModel / DialogViewModel
   │                纯数据，无 ratatui 依赖
       │
       ▼
⑦ Render            ratatui Buffer 写入
   │                读 ViewModel + ViewState + BlockCache → 写 Buffer
       │
       ▼
⑧ Effect            Effect enum（StartRun / RequestRunCancellation / SendInteractionReply / ...）
                    通过 EffectExecutor 异步执行
```

### 3.1 数据流方向

```
① → ② → ③ → ④ → ⑤ → ⑥ → ⑦
                │
                └→ ⑧ →（副作用执行后产生新 Msg）→ ②
```

- ①→⑦ 是**正向管线**：事件 → 更新 → 渲染
- ⑧ 是**反馈环**：Effect 执行后产生新 Msg（如 SDK 事件回传），回到 ②
- ③ Coordinator **MUST** 只从 Change 生成 Effect；只有独立 Effect runner **MAY** 执行副作用。④/⑤/⑥/⑦ 必须纯函数

## 4. 三条信息流

### 4.1 用户意图流

```
用户按键 → crossterm Event → TuiMsg::Key(key) → App::update_key()
  → InputIntent / ConversationIntent → Model.apply() → Change
    ├→ Coordinator → Effect（如 Submitted → StartRun）→ result Intent
    └→ ViewModelDirty → ViewAssembler → ViewModel → Render
```

### 4.2 Agent 事件流

```
Runtime ChatStream → tokio::spawn task → sdk::ChatEvent
  → sdk_event_to_ui_event（adapter/event_mapping.rs）
  → UiEvent → mpsc channel (cap 256)
  → ui_rx → tokio::select! → TuiMsg::Ui(ui_event)
  → App::update_agent_event()
  → map_agent_event（adapter/agent_event.rs，ACL，只产 Intent）
  → AgentEventMapping { intents }
  → root_reducer → Model Change → Coordinator Effect → result Intent
  → ViewModelDirty → ViewAssembler → Render
```

Interaction request id 由 Runtime 生成并作为纯值 SDK DTO 进入同一事件链；UserQuestions、ToolApproval、PlanApproval、HardPause 四种 body 都走这条链。processing 不生成 id、不接管 sender，也不写 UI 状态。

### 4.3 视图反馈流

```
ViewModelDirty { output, status, input, dialog }
  → flush_dirty_view_models（每帧 draw 前）
    ├─ dirty.output → refresh_output_document_from_model（memo'd by revision）
    ├─ dirty.status → clear_status（lazy rebuild）
    ├─ dirty.input → reassemble input view model
    └─ dirty.dialog → reassemble dialog view model
  → Render
```

## 5. Model Context

TUI Model 按投影来源划分六个 Context；字段、状态机、Intent 与 Change 的唯一战术真相是 [02-model.md](02-model.md)，本文 **NEVER** 复制完整结构：

| Context | 职责 | 纯度 |
|---|---|---|
| Conversation | Run / RunStep、tool call、timeline、Interaction 与运行期投影 | 纯（无 ratatui / I/O） |
| Input | buffer、cursor、selection、history、completion | 纯 |
| Diagnostic | error、warning、notice、blocking request | 纯 |
| Session | session metadata、resume、save 与 Task 投影 | 纯 |
| Config | provider / model 投影 | 纯 |
| Workspace | TUI-owned workspace snapshot 与异步 metadata 投影 | 纯；branch / kind 只由 Effect 结果回填 |

每个 Context **MUST** 遵循 `Intent → Model.apply → Change`；Coordinator **MUST** 只从 Change 派生 Effect，Effect 结果再作为 TUI-owned Intent 回到 Model。Context 之间 **NEVER** 直接读写彼此状态。六个 Context 的核心字段私有；ViewAssembler / key translator 只能取得不可变 accessor 或只读 projection view，只有 root reducer 可调用 mutation facade。

## 6. Msg / Intent / Change / Effect 枚举

### 6.1 TuiMsg（统一输入信号）

```rust
enum TuiMsg {
    Key(KeyEvent),
    Paste(String),
    Resize(u16, u16),
    Mouse(MouseEvent),
    Ui(UiEvent),                        // SDK 事件经第一层 ACL 转成的 TUI-owned DTO
    Intent(AgentIntent),                // effect runner 产出的 TUI-owned result Intent
    SpinnerTick,                        // 90ms spinner 动画帧
}
```

### 6.2 Intent（用户/系统意图）

每个 Context 有独立的 Intent 枚举：

```rust
enum ConversationIntent {
    StartRun { text },
    ProjectRunStarted { run_id, text },
    ProjectRunResumed { run_id },
    ProjectRunCompleting { run_id },
    ProjectRunFailed { run_id, message },
    ProjectRunCancelling { run_id },
    ProjectRunCancelled { run_id },
    RequestRunCancellation { run_id },
    ShowInteraction { request_id, run_id, body },
    UpdateInteractionDraft { request_id, action },
    ConfirmInteraction { request_id },
    CancelInteraction { request_id },
    InteractionReplySent { request_id },
    InteractionCancelled { request_id },
    InteractionReplyFailed { request_id, message },
    // ...
}
enum InputIntent { InsertChar(char), DeleteChar, MoveCursor, SubmitInput, ... }
enum DiagnosticIntent { DismissNotice, ShowDetails, ... }
enum SessionIntent { ResumeSession { id }, ListSessions, ... }
enum ConfigIntent { ProviderModelChanged { provider, model_id }, ... }
enum WorkspaceIntent { ApplySnapshot(WorkspaceSnapshot), ApplyMetadata(WorkspaceMetadata), ... }
```

### 6.3 Change（Model 变更产出）

```rust
enum ConversationChange { RunStartRequested, RunCancellationRequested, RunStarted, RunCompleting, RunCancelling, RunCancelled, RunCompleted, ToolCallStarted, MessageAppended, ... }
enum InputChange { BufferModified, SelectionChanged, Submitted, ... }
enum ModelChange { OutputDirty, StatusDirty, InputDirty, DialogDirty }
```

### 6.4 Effect（副作用请求）

```rust
enum Effect {
    StartRun { text: String },
    SubmitInput { text: String },
    RequestRunCancellation { run_id: RunId },
    SendInteractionReply { request_id: UiInteractionRequestId, reply: UiInteractionReply },
    CancelInteraction { request_id: UiInteractionRequestId },
    RequestRender,
    SpawnTask { task: AsyncTask },
    // ...
}
```

## 7. ViewAssembler / ViewModel / ViewState

### 7.1 组装管线

```
Model + ViewState → ViewAssembler → ViewModel → Render
```

| 层 | 职责 | 纯度 |
|---|---|---|
| ViewAssembler | 读 Model + ViewState，产出 ViewModel | ✅ 纯（无 ratatui/IO） |
| ViewModel | 纯数据结构，供 Render 消费 | ✅ 纯 |
| ViewState | scroll/collapse/selection/animation/cache | 可变状态 |
| Render | 读 ViewModel + ViewState + Cache → 写 ratatui Buffer | ratatui 依赖 |

### 7.2 四个 ViewAssembler

| Assembler | 产出 | 输入 |
|---|---|---|
| OutputViewAssembler | OutputViewModel（对话块列表） | ConversationModel 只读 projection + OutputViewState |
| StatusViewAssembler | StatusLineViewModel（状态栏） | ConversationModel.run_runtime() + SessionModel 只读 projection |
| InputViewAssembler | InputAreaViewModel（输入框） | InputModel 只读 projection |
| DialogViewAssembler | DialogViewModel（弹窗） | DiagnosticModel.active_prompt() |

### 7.3 三层缓存

| 缓存层 | 位置 | Key | 失效条件 |
|---|---|---|---|
| BlockCache | render/output/block_cache | `{version: u64, text_width: u16}` | block_version 变化或 text_width 变化 |
| GuttedCache | render/output/document_renderer | `{block_version, text_width, depth, marker_frame}` | Running 状态 blink 周期失效 |
| OutputArea force_repaint | render/output_area | `{total_lines, block_count}` | block_count 变化或 total_lines 减少 |

- **ViewModelDirty bitfield** 控制每帧只重算脏部分
- **OutputViewCache memo**：`(conversation.revision(), workspace_root)` 不变时跳过 `assemble_from_conversation` 全量重建
- Running ToolCall 的 `marker_frame = animation_frame / BLINK_DIVISOR`——每个 blink 周期强制 re-cache
- `workspace_root` 纳入 CacheKey——worktree 切换时自动失效

### 7.4 ViewState

```rust
struct AppViewState {
    output: OutputViewState,            // scroll/collapse/selection
    status: StatusViewState,            // status line scroll/selection
    input: InputViewState,              // cursor display/selection
    dialog: DialogViewState,            // dialog cursor
    animation: AnimationState,          // 仅 spinner_frame 等视觉动画帧
    dirty: ViewModelDirty,              // {output, status, input, dialog}
}
```

`SpinnerPhase` **MUST** 由 `RunProjectionStatus`、`RunStepProjectionStatus` 与 Model 中的运行上下文纯函数派生；`ViewState` 只保存视觉动画帧，**NEVER** 保存业务 phase 或 `run_active` 副本。

`ViewState` 的可变性只覆盖 scroll / collapse / selection / animation / cache 等瞬时交互与渲染状态；Run、Interaction、timeline、input buffer 等 UI 业务投影仍只在 Model 中变更。

## 8. SDK DTO 边界

### 8.1 Published Language

Runtime 拥有 `AgentClient` 入站 OHS 与 `ChatEvent` Published Language，SDK 只发布该稳定契约。跨边界类型遵循单一来源：

| 类型 | 唯一定义方 | 发布方式 |
|---|---|---|
| `AgentClient` / `ChatEvent` | Runtime | SDK re-export 或由同一 schema 生成 |
| `ContentBlock` 与共享值类型 | Shared Kernel | SDK re-export |
| TUI `UiEvent` 与投影 DTO | TUI | 只在 TUI 内部可见 |

Runtime wire DTO **MUST** 在 `adapter/event_mapping.rs` 一次性转换成 TUI-owned DTO；`UiEvent`、Intent、Model、ViewModel 与 Render **NEVER** import `sdk::*` DTO。转换必须保持封闭枚举的穷尽匹配，并以序列化 round-trip / shape 测试锁定 Published Language。

### 8.2 Interaction reply 边界

Runtime-owned `ChatEvent::InteractionRequested` 只携可序列化 run/request identity 与纯值 body；`event_mapping` 穷尽转换 `UserQuestions`、`ToolApproval`、`PlanApproval`、`HardPause` 为 TUI-owned `UiInteractionBody`，无损保留 `run_id` 并把 request ID 包装为 `UiInteractionRequestId`。Model 用 run identity 拒绝旧、未知或未路由 Run 的迟到投影；Composition 已登记 parent-mediated adapter 的 Sub Run 仍是合法来源，并保留 parent/sub correlation。Effect runner 把 request ID 与 body-specific reply 无损映回 SDK `InteractionRequestId` / `InteractionReply`，调用 `AgentClient::reply_interaction` / `cancel_interaction`；TUI 任一层 **NEVER** 持有 sender 或 Runtime continuation。完整协议见 [03-event-flow-and-acl.md](03-event-flow-and-acl.md) §4。

## 9. 架构门禁

门禁编号与 [03-event-flow-and-acl.md](03-event-flow-and-acl.md) §8 保持一致。

| # | 门禁 | Target 证明 |
|---|---|---|
| 1 | Adapter / ViewAssembler isolation | `adapter/` 与 `view_assembler/` **NEVER** import `render::*` |
| 2 | Model purity | `model/` **NEVER** import ratatui、tokio、process、channel、AgentClient 或 SDK DTO |
| 3 | Render isolation | `render/` 只读 ViewModel / ViewState，不改变 Model |
| 4 | ViewAssembler boundary | 只读 Model + ViewState，产出 ViewModel；无 I/O、副作用或 ratatui 类型 |
| 5 | ViewModel dependency | 纯数据，不依赖可变 Model 或 ratatui |
| 6 | Agent event adapter | SDK event DTO 只出现在 processing boundary 与 `adapter/event_mapping.rs`；`UiEvent` 之后零 SDK DTO |
| 7 | TEA purity | `update/`、reducer 与 ACL **NEVER** spawn、await、执行命令、发 channel 或直接调用 AgentClient |
| 8 | Interaction resource isolation | 四类 Runtime request body 的 id 贯穿 SDK / TUI ACL / AgentClient command；TUI 全树零 sender、pending waiter 与自生成协议 id |
| 9 | Event exhaustiveness | 构造每个 UiEvent 变体，断言第二层 ACL 产生显式 Context Intent；禁止 wildcard 与默认空 mapping |
| 10 | Model write isolation | 六 Context 核心字段私有；`apply` / `reduce_*` 生产调用点只有 `update/root_reducer.rs`，adapter / Coordinator / ViewAssembler 只取得不可变 projection |

每条门禁 **MUST** 有架构测试，并保留一次故意违规能失败的证明。迁移状态、旧路径与退役清单只在 [Migration Governance](../../03-engineering/migration-governance.md) O6 维护。

## 10. 目标态不变量

1. **唯一事件链**：SDK event → `event_mapping` TUI DTO → `AgentEventMapping` intents → reducer Change → Coordinator Effect → effect runner → result Intent；任何 `UiEvent` **NEVER** 直达 Model。
2. **唯一 Model 写入口**：六 Context 核心字段私有，root reducer 是 mutation facade 的唯一调用方；Model 变更只返回 Change，不调用 runtime、git、channel 或 timer。
3. **唯一副作用入口**：Coordinator 只从 Change 生成 Effect；effect runner 执行 I/O，并把结果包装为 TUI-owned Intent 回到 reducer。
4. **唯一渲染输入**：ViewAssembler 从 Model + ViewState 产出 ViewModel；Render 不持有 Model 副本。
5. **异步结果防陈旧覆盖**：Workspace metadata 等 Effect 同时携带资源 identity 与 revision；结果只在 tuple 匹配时 apply。
6. **派生状态不复制**：spinner phase 与可见性由 Run / RunStep 投影纯函数计算，ViewState 只持视觉交互状态。
7. **互补投影原子更新**：结构化 Conversation 投影（runs / queued / progress）与 `timeline` 由同一 reducer 事务维护；只约束重叠稳定 ID、相对顺序、关联与终态，**NEVER** 声称二者可完整互相重建。
8. **Runtime 状态权威**：AgentClient interaction command result 只结束本地交互，不推进 Run；只有 SDK `RunResumed` 才恢复 Running，取消先投影 `RunCancelling`，仅 `RunCancelled` 进入终态。TUI 同时最多投影一个 active interaction；Runtime 对并发 Tool suspension 按稳定 ToolCall 顺序逐个发布，TUI **NEVER** 建第二个 pending registry。

## 11. 相关文档

- 原始 TUI 设计（历史归档）：[../../../snapshot/design/04-tui-design.md](../../../snapshot/design/04-tui-design.md)
- Runtime-owned AgentClient OHS：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- SDK Published Language：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 统一语言（TUI/TEA/Context）：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：八层 TEA 管线、三条信息流、Context、SDK DTO 边界与架构门禁 | #795 |
| 2026-07-12 | DDD/Hexagonal/Clean 评审：收敛 reducer、ACL、event mapping 与 ViewAssembler 的职责边界 | #798 评审 |
| 2026-07-14 | 事件主链统一为 TUI-owned DTO → Intent → Change → Coordinator Effect → result Intent；实现差距记录收口到 Migration Governance O6 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | Model 核心字段私有；Run 恢复 / 取消只投影 Runtime 权威事件；runs / timeline 明确为互补投影 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
