# TUI · 架构与数据流

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#795（S2）
> 本文定义 TUI 的八层 TEA 管线、三条信息流、Model Context 分层、枚举定义、ViewAssembler/ViewModel/ViewState、SDK DTO 边界与架构门禁。TUI 是入站适配器，不承载业务。

## 1. 定位

TUI 是**入站适配器**（Hexagonal Primary Adapter）：

- 通过 `AgentClient` trait（SDK 出站端口）与 Runtime 通信
- **不承载业务逻辑**——所有业务决策在 Runtime，TUI 只负责状态投影和用户输入翻译
- **纯展示层**——Model 不执行 IO、不调 AgentClient、不发 channel（reducer 纯化目标，见 §10）
- 基于 The Elm Architecture（TEA）变体：event → update → model → view → effect

> **与 `04-tui-design.md` 的关系**：本文是 `04-tui-design.md` 的迁移深化版，修正了代码实现与设计文档的分歧，补充了架构门禁和缺口分析。原文件保留为历史归档。

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
│  出站端口：AgentClient trait（SDK 定义）             │
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
   │                ├─ map_agent_event（SDK → Intent + Effect）
   │                ├─ root_reducer（Intent → Model change）
   │                └─ update_ui（同步 view_state）
       │
       ▼
④ Model             TuiModel { conversation, input, diagnostic, session }
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
⑧ Effect            Effect enum（SendMessage / StartChat / AbortChat / ...）
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
- ③ 是**唯一可产生副作用的层**（通过 Effect）——④/⑤/⑥/⑦ 必须纯函数（目标态）

## 4. 三条信息流

### 4.1 用户意图流

```
用户按键 → crossterm Event → TuiMsg::Key(key) → App::update_key()
  → InputIntent / ConversationIntent → Model.apply() → Change
  → ViewModelDirty → ViewAssembler → ViewModel → Render
  → Effect（如 SubmitInput → StartChat）
```

### 4.2 Agent 事件流

```
Runtime ChatStream → tokio::spawn task → sdk::ChatEvent
  → sdk_event_to_ui_event（effect/session/processing/event_mapping.rs）
  → UiEvent → mpsc channel (cap 256)
  → ui_rx → tokio::select! → TuiMsg::Ui(ui_event)
  → App::update_agent_event()
  → map_agent_event_with_tool_header（adapter/agent_event.rs，ACL）
  → AgentEventMapping { conversation_intents, diagnostic_intents, session_intents, effects }
  → root_reducer → Model change → ViewModelDirty → ViewAssembler → Render
```

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

### 5.1 3+1 结构

设计文档原定 4 Context（Conversation / Input / Runtime / Diagnostic），代码实际为 **3+1**——RuntimeState 内聚在 Conversation 中：

```rust
struct TuiModel {
    conversation: ConversationModel,    // 对话 + 运行时状态
    input: InputModel,                  // 输入 buffer/cursor/selection/history
    diagnostic: DiagnosticModel,        // 错误/警告/提示/阻塞请求
    session: SessionModel,              // session metadata + resume 候选列表
}
```

### 5.2 ConversationModel 内部分层

RuntimeState 与 chat 生命周期紧密耦合（chat 启动 → spinner 开始，chat 完成 → spinner 停止），不拆成独立 Context。通过**内聚子模块 + 字段私有化**控制 ConversationModel 臃肿：

```rust
struct ConversationModel {
    // 对话结构
    chats: Vec<Chat>,
    active_chat_id: Option<ChatId>,
    timeline: OutputTimelineModel,      // 渲染用扁平时间线

    // 运行时子模块（内聚，字段私有化）
    runtime: RuntimeState,
    ask_user: AskUserState,
}

struct RuntimeState {
    // 全部字段私有，只经业务方法操作
    spinner: SpinnerModel,
    usage: UsageTracker,
    workspace: WorkspaceState,
    task_status: TaskStatusTracker,
    compact_progress: Option<CompactProgress>,
    live_tps: Option<f64>,
    processing_jobs: ProcessingJobTracker,
    thinking: Option<ThinkingState>,
    graph_phase: Option<GraphPhase>,
    status_notice: Option<StatusNotice>,
}
```

> **设计决策**：RuntimeState 不拆出独立 Context。理由：
> 1. spinner/usage/workspace 与 chat 生命周期耦合——chat 启动时初始化，完成时清理
> 2. 拆出去会增加跨 Context 的 Intent/Change 通信开销
> 3. 臃肿通过子模块封装控制，不通过拆 Context 控制
> 4. `model/runtime/` 目录保留给 SessionModel（session metadata）

### 5.3 各 Context 职责

| Context | 职责 | 纯度 |
|---|---|---|
| Conversation | chat/turn 生命周期、tool call 追踪、timeline、RuntimeState、AskUser | ✅ 纯（无 ratatui/IO） |
| Input | buffer/cursor/selection/history/completion/queue | ✅ 纯 |
| Diagnostic | 错误/警告/提示/阻塞请求 | ✅ 纯 |
| Session | session metadata、resume 候选列表、cwd | ✅ 纯 |

### 5.4 Intent / Change 模式

每个 Context 遵循统一的 Intent → apply → Change 模式：

```rust
// Conversation: struct-per-variant + trait dispatch
trait ConversationUpdate {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange>;
}

// Input / Diagnostic / Session: enum + match
impl InputModel {
    fn apply(&mut self, intent: InputIntent) -> Vec<InputChange>;
}
```

> **已知不一致**：Conversation 用 struct-per-variant + trait dispatch，其他三个用 enum match。三种风格同一架构意图——后续统一（见 §10）。

## 6. Msg / Intent / Change / Effect 枚举

### 6.1 TuiMsg（统一输入信号）

```rust
enum TuiMsg {
    Key(KeyEvent),
    Paste(String),
    Resize(u16, u16),
    Mouse(MouseEvent),
    Ui(UiEvent),                        // SDK 事件经 ACL 后的 UiEvent
    SpinnerTick,                        // 90ms spinner 动画帧
}
```

> **死代码**（见 §10）: `TimerTick` / `RenderTick` / `EffectCompleted` / `TerminalKey` / `TerminalMouse` / `TerminalResize` / `AgentEvent` 已定义但从未产生。

### 6.2 Intent（用户/系统意图）

每个 Context 有独立的 Intent 枚举：

```rust
enum ConversationIntent { StartChat { text }, SubmitInput, AbortChat, PauseChat, ResumeChat, ... }
enum InputIntent { InsertChar(char), DeleteChar, MoveCursor, SubmitInput, ... }
enum DiagnosticIntent { DismissNotice, ShowDetails, ... }
enum SessionIntent { ResumeSession { id }, ListSessions, ... }
```

### 6.3 Change（Model 变更产出）

```rust
enum ConversationChange { ChatStarted, ChatCompleted, ToolCallStarted, MessageAppended, ... }
enum InputChange { BufferModified, SelectionChanged, Submitted, ... }
enum ModelChange { OutputDirty, StatusDirty, InputDirty, DialogDirty }
```

### 6.4 Effect（副作用请求）

```rust
enum Effect {
    StartChat { text: String },
    SubmitInput { text: String },
    AbortChat,
    PauseChat,
    ResumeChat,
    RequestRender,
    SpawnTask { task: AsyncTask },
    // ...
}
```

> **死代码**（见 §10）: `StartTimer` / `StopTimer` / `RunHook` / `SetCurrentTurn` 已定义但 no-op。

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
| OutputViewAssembler | OutputViewModel（对话块列表） | ConversationModel + OutputViewState |
| StatusViewAssembler | StatusLineViewModel（状态栏） | ConversationModel.runtime + SessionModel |
| InputViewAssembler | InputAreaViewModel（输入框） | InputModel |
| DialogViewAssembler | DialogViewModel（弹窗） | DiagnosticModel.active_prompt |

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
    spinner: SpinnerAnim,               // frame/verb/phase
    animation: AnimationState,          // spinner_frame 等
    dirty: ViewModelDirty,              // {output, status, input, dialog}
}
```

> **已知问题**：spinner 状态三处同步——`model.conversation.runtime.spinner` + `view_state.spinner` + `view_state.animation.spinner_frame`。目标态统一为单一来源（见 §10）。

## 8. SDK DTO 边界

### 8.1 当前问题

SDK ↔ Runtime 类型同步存在三种方式，风险不一：

| 类别 | 当前同步方式 | 风险 |
|---|---|---|
| 30+ tool result 类型 | `pub use` re-export（单一来源） | ✅ 无 |
| `ChatEvent` ↔ `RuntimeStreamEvent` | 444 行手工 match 转换（`convert.rs`） | ⚠️ 高——已有 5 处结构漂移 |
| `ContentBlock` | JSON round-trip（`serde_json::from_value(to_value(...))`） | ⚠️ 脆弱——加变体静默降级 |

**已发现的漂移**：

| 字段 | RuntimeStreamEvent | ChatEvent (SDK) | 漂移 |
|---|---|---|---|
| DoneWithDuration | `duration: Duration` | `duration_ms: u64` | 重命名 + 改类型 |
| UserMessagesAdopted | `Vec<(InputId, Message)>` | `Vec<ChatMessage>` | tuple → flat，input_id 丢失 |
| GraphPhaseChanged | `ReasoningNode, ReasoningLevel` | `String, String, String` | 类型擦除 |
| WorkingDirectoryChanged | `PersistedWorkspaceContext` | `WorkspaceContextView` | 不同 struct |
| CompactProgress | `CompactStage, usize` | `String, u32` | 类型擦除 |

### 8.2 目标态：SDK DTO 从 runtime auto-gen

**原则：Runtime 是类型定义的唯一来源，SDK 不应两边维护。**

| 类别 | 目标 | 迁移动作 |
|---|---|---|
| tool result 类型 | ✅ 保持 `pub use` re-export | 无 |
| `ChatEvent` | Runtime 定义 → SDK re-export 或 codegen | 删除 `convert.rs` 444 行手工 match；runtime 直接暴露 `ChatEvent`，SDK re-export |
| `ContentBlock` | share 定义 → SDK re-export | 删除 JSON round-trip；SDK 直接 `pub use share::message::ContentBlock` |
| 架构守卫 | CI test 验证两侧 JSON shape 一致 | 添加 round-trip 测试 |

### 8.3 迁移动作

1. `ChatEvent` 收口：Runtime 的 `RuntimeStreamEvent` 直接作为 SDK 的 `ChatEvent`（或通过 `From` impl），删除 `convert.rs` 手工 match
2. `ContentBlock` 收口：SDK `pub use share::message::ContentBlock`，删除 JSON round-trip
3. 添加 CI 守卫：序列化所有变体，断言两侧 JSON shape 一致
4. SDK 的 `sdk → share` 依赖白名单已存在——`ContentBlock` re-export 不违反分层

## 9. 架构门禁

### 9.1 门禁规则

| # | 门禁 | 实现方式 | 现状 |
|---|---|---|---|
| 1 | adapter/view_assembler → render | arch test：禁止 import `render::*` | ✅ 已实现 |
| 2 | Model purity | arch test：`model/` 禁止 import ratatui/tokio/std::process/AgentClient | ❌ 缺失 |
| 3 | Render isolation | arch test：`render/` 禁止状态变更逻辑（只读 Model/ViewState） | ❌ 缺失 |
| 4 | ViewAssembler boundary | arch test：允许读 model/view_state，禁止 IO/副作用/ratatui | ❌ 缺失 |
| 5 | ViewModel dependency | arch test：禁止依赖 model 可变类型和 ratatui | ❌ 缺失 |
| 6 | Agent event adapter | arch test：SDK event 类型只在 `adapter/` 出现 | ❌ 缺失 |
| 7 | TEA purity | arch test：`update/` 禁止 `tokio::spawn`/`Command::new`/`.await` | ❌ 缺失 |

### 9.2 现有守卫实现

```rust
// architecture_tests.rs — 唯一已实现的门禁
fn test_adapter_and_view_assembler_production_do_not_depend_on_render_modules() {
    // 遍历 adapter/ 和 view_assembler/ 下的 .rs 文件
    // 排除 #[cfg(test)] 模块
    // 断言源码不含 "crate::tui::render::"
}
```

### 9.3 目标态

补齐 6 条缺失门禁，每条用相同的 `production_source` 模式（strip `#[cfg(test)]` + grep import）。

## 10. 现状缺口与迁移动作

### 10.1 reducer 纯化

**当前**：`root_reducer::apply_conversation_changes` 直接调 `runtime.start_chat()` / `complete_chat()` / `start_tool_call()` 等——reducer 产生副作用。

**目标态**：

```
当前：root_reducer → apply_conversation_change → runtime.start_chat()（直接副作用）
目标：root_reducer → apply_conversation_change → 产出 ConversationChange
    → Coordinator 消费 Change → 生成 Effect
    → EffectExecutor 执行 Effect（副作用隔离）
```

**迁移动作**：
1. RuntimeState 字段私有化（当前全 `pub`，TODO 已标注）
2. `apply_conversation_changes` 改为只产出 Change，不调 runtime 方法
3. 新增 `ChangeConsumer`——消费 Change 生成 Effect
4. spinner/usage/workspace 状态变更是 Change 的副作用，不直接调

**实现**：本期其他 issue 承接，不在 #795 范围。

### 10.2 死代码清单

#### Msg 变体（定义但从未产生）

| 变体 | 处理 |
|---|---|
| `TuiMsg::TimerTick` | 删除——设计预留但未使用 |
| `TuiMsg::RenderTick` | 删除——同上 |
| `TuiMsg::EffectCompleted` | 删除——EffectExecutor 同步执行，无回传 |
| `TuiMsg::TerminalKey` | 删除——与 `TuiMsg::Key` 重复 |
| `TuiMsg::TerminalMouse` | 删除——与 `TuiMsg::Mouse` 重复 |
| `TuiMsg::TerminalResize` | 删除——与 `TuiMsg::Resize` 重复 |
| `TuiMsg::AgentEvent` | 删除——与 `TuiMsg::Ui(UiEvent)` 语义重复 |

#### Effect 变体（定义但 no-op）

| 变体 | 处理 |
|---|---|
| `Effect::StartTimer` | 删除——Timer 系统未实现 |
| `Effect::StopTimer` | 删除——同上 |
| `Effect::RunHook` | 删除——Hook 经 Runtime 执行，不经 TUI Effect |
| `Effect::SetCurrentTurn` | 删除——turn 追踪在 Model 内部 |

#### 模块级 `#![allow(dead_code)]`

| 位置 | 处理 |
|---|---|
| `model.rs` | 移除——暴露真实死代码后逐个清理 |
| `update.rs` | 移除——同上 |
| `effect.rs` | 移除——同上 |
| `view_state.rs` | 移除——同上 |

#### 无调用方模块

| 模块 | 处理 |
|---|---|
| `update/coordinator.rs::effects_for_input_change` | 接线或删除——真实路由在 `app/update/ui_event.rs` |

#### 其他死代码

| 项目 | 处理 |
|---|---|
| `UiEvent::ReflectionDone` / `ReflectionApplyDone` | 删除——`#[allow(dead_code)]`，映射时静默丢弃 |
| `agent_event.rs::_diagnostic` | 删除——无调用方 |
| `EffectResult` | 删除——无生产方 |
| `merge_dirty()` | 内联——单行 wrapper |

### 10.3 架构守卫补齐

补齐 6 条缺失门禁（见 §9）。每条用 `production_source` 模式实现。

### 10.4 spinner 状态统一

**当前**：三处同步——`model.conversation.runtime.spinner` + `view_state.spinner` + `view_state.animation.spinner_frame`。

**目标态**：`model.conversation.runtime.spinner` 为单一来源，`view_state` 只存动画帧（frame），verb/phase 从 model 读取。

### 10.5 sync I/O 移出 ui_rx

**当前**：`event_mapping.rs` 的 `WorkingDirectoryChanged` 同步调 `git branch` + `worktree kind`（2 个子进程），阻塞 ui_rx consumer。

**目标态**：移到 Effect 中异步执行，或加缓存（`WorkingDirectoryChanged` 低频事件，可缓存 branch + worktree kind）。

### 10.6 view_state → render 依赖违规

**当前**：`view_state/status.rs` 导入 `render::status::StatusBarRow`——view_state 依赖 render，方向反了。

**目标态**：`StatusBarRow` 在 view_state 或 view_model 中定义，render re-export。

### 10.7 InputRenderModel 重复

**当前**：`render/input/input_render_model.rs` 的 `InputRenderModel` 与 `InputAreaViewModel` 字段几乎完全重复。

**目标态**：删除 `InputRenderModel`，`InputArea` widget 直接消费 `InputAreaViewModel`。

### 10.8 ConversationModel 双重表示

**当前**：`chats: Vec<Chat>` + `OutputTimelineModel { items }` 并行维护，无测试验证一致性。

**目标态**：添加 invariant 测试——每次 `start_chat` / `append_*` / `complete_chat` 后断言两者同步。

### 10.9 Intent 风格统一

**当前**：Conversation 用 struct-per-variant + trait dispatch；Input / Diagnostic / Session 用 enum match。

**目标态**：统一为 enum match（更简单，减少文件数）或统一为 struct-per-variant（更可扩展）。后续 issue 决策。

## 11. 相关文档

- 原始 TUI 设计（历史归档）：[../../04-tui-design.md](../../04-tui-design.md)
- Runtime 端口（AgentClient = TUI 出站端口）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- SDK Published Language：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 统一语言（TUI/TEA/Context）：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：八层 TEA 管线、三条信息流、3+1 Context、SDK DTO 边界、架构门禁、死代码清单、reducer 纯化目标态 | #795 |
