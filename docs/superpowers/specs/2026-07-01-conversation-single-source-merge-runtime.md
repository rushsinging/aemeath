# ConversationModel 单一真相源：RuntimeModel 合并 + Intent trait 分发

**日期**：2026-07-01
**关联**：supercedes `2026-05-29-tui-s1-spinner-task-single-source.md` 的双 model 架构

## 问题

TUI 当前有两个并行 model：

- **ConversationModel**：对话内容真相（chats、timeline、tool_calls、ask_user 等）
- **RuntimeModel**：运行态真相（spinner、usage、tps、workspace、task_status、processing_jobs、status_notice、thinking、graph_phase、compact_progress 等）

同一个 UiEvent 被**双写**——document 走 ConversationIntent → ConversationModel，spinner/status 走 RuntimeIntent → RuntimeModel。spinner phase 的设置散布在 `update_ui`（~20 处命令式 `self.spinner_phase()` / `self.spinner_stop()`）、`tool_flow_projector`、`agent_event.rs`、`model.rs`（SetCompactProgress 自动启动 spinner）等位置。

根因：RuntimeModel 的存在迫使 spinner / status line 维护独立于 document 的写入路径，产生双写、散布、和状态同步顺序依赖。

## 设计

### 核心决策：RuntimeModel 彻底删除，所有字段并入 ConversationModel

ConversationModel 成为 TUI 的**唯一 model 真相源**。document（对话块）、spinner phase、status line（usage/tps/workspace/task/processing/notice/thinking/graph_phase）、compact progress 全部从 ConversationModel 读取。

### 目标结构

```rust
pub struct ConversationModel {
    // ── 对话内容（现有，不变）──
    pub chats: Vec<Chat>,
    pub active_chat_id: Option<ChatId>,
    pub timeline: OutputTimelineModel,
    pub queued_submissions: Vec<QueuedSubmission>,
    pub agent_progress: Vec<AgentProgressEntry>,
    next_chat_sequence: usize,
    next_block_sequence: usize,
    revision: u64,
    active_text_block_id: Option<String>,
    active_text_context: Option<(ChatId, ChatTurnId)>,
    active_thinking_block_id: Option<String>,
    active_thinking_context: Option<(ChatId, ChatTurnId)>,

    // ── 运行态（从 RuntimeModel 搬入）──
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub workspace: WorkspaceState,
    pub usage: UsageSummary,
    pub live_tps: Option<f64>,
    pub task_status: TaskStatusSnapshot,
    pub processing_jobs: Vec<ProcessingJob>,
    pub spinner: SpinnerModel,
    pub status_notice: StatusNotice,
    pub thinking: bool,
    pub graph_phase: Option<String>,
    pub transient_notice_expiry: Option<Instant>,
    pub compact_progress: Option<CompactProgressModel>,
}
```

### Intent 模式：trait 分发替代 match 分发

当前 `apply` 在 `match intent` 中为每个 variant 写逻辑。改为 intent 自治：

```rust
pub trait ConversationUpdate {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange>;
}

impl ConversationModel {
    pub fn apply<U: ConversationUpdate>(&mut self, update: U) -> Vec<ConversationChange> {
        let changes = update.update(self);
        if !changes.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        changes
    }
}
```

每个 intent variant 拆成独立 struct，各自 `impl ConversationUpdate`：

```rust
pub struct StartChat { pub submission: String }
pub struct SetSpinnerPhase { pub phase: SpinnerPhase }
pub struct StopSpinner {}
pub struct RecordUsage { pub input_tokens: u64, ... }
pub struct UpdateWorkspace { pub cwd: String, pub worktree: Option<String> }
// ... 所有现有 ConversationIntent + RuntimeIntent variant
```

`ConversationIntent` enum 保留为**传输容器**（`AgentEventMapping.conversation: Vec<ConversationIntent>` 需要类型擦除），只做 match 转发：

```rust
pub enum ConversationIntent {
    StartChat(StartChat),
    SetSpinnerPhase(SetSpinnerPhase),
    StopSpinner(StopSpinner),
    // ...
}

impl ConversationUpdate for ConversationIntent {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        match self {
            Self::StartChat(inner) => inner.update(model),
            Self::SetSpinnerPhase(inner) => inner.update(model),
            // ...
        }
    }
}
```

### RuntimeObservation / RuntimeIntent / RuntimeChange 删除

- **RuntimeObservation**：删除。其变体（AssistantText、ToolCallStart 等）的 UiEvent→ConversationIntent 映射已在 `agent_event.rs` 的 `runtime_observation_from_ui_event` + `ToolFlowProjector` 中完成；RuntimeObservation 只是中间层，合并后 UiEvent 直接映射为 ConversationIntent。
- **RuntimeIntent**：所有 variant 拆成 struct 并入 ConversationIntent 体系。
- **RuntimeChange**：所有 variant 合并入 ConversationChange。
- **RuntimeModel**：删除整个文件。

### AgentEventMapping 简化

```rust
pub struct AgentEventMapping {
    pub conversation: Vec<ConversationIntent>,  // ← 所有 intent（含原 RuntimeIntent）
    pub diagnostic: Vec<DiagnosticIntent>,
    pub session: Vec<SessionIntent>,
    pub effects: Vec<Effect>,
    // runtime 字段删除
}
```

`root_reducer::reduce_agent_event` 中删除 `for intent in mapping.runtime` 循环，全部走 `model.conversation.apply(intent)`。

### Spinner 写入收敛

删除 `update_ui` 中所有命令式 spinner 调用（`self.spinner_phase()` / `self.spinner_stop()`），改为在 `map_agent_event` 中集中映射为 `ConversationIntent::SetSpinnerPhase` / `StopSpinner`。

| UiEvent | spinner intent（在 map_agent_event 中产出） |
|---|---|
| `Text` | `SetSpinnerPhase(Generating)` |
| `Thinking` | `SetSpinnerPhase(Thinking)` |
| `ToolCallStart` | `SetSpinnerPhase(CallingTool(name))` |
| `ToolResult` | 从 ConversationModel 数 running tool_calls：0 → `SetSpinnerPhase(Thinking)`；>0 → `SetSpinnerPhase(CallingTools { remaining })` |
| `AgentProgress` | `SetSpinnerPhase(AgentWorking)` |
| `HookEvent` | `SetSpinnerPhase(Hook { .. })`；PostCompact → `StopSpinner` |
| `CompactProgress` | `SetSpinnerPhase(Compacting)` |
| `Done` / `Cancelled` / `Error` | `StopSpinner` |
| `MessagesSync` | `StopSpinner`（compact 完成） |
| `AskUserBatch` | `StopSpinner` |
| `ReflectionStarted` | `SetSpinnerPhase(Reflecting)` |
| `ReflectionDone` | `StopSpinner` |

**ToolResult remaining 问题**：同一批次 intent 顺序 apply。`ObserveToolResult` 先执行，更新 `tool_calls[].status`；随后 `SetSpinnerPhase` 的 `update()` 从 model 数出 running 数量。因为 spinner intent 在同批次中排在 tool result 之后，所以此时 model 已是最新状态。

### 渲染层适配

| Assembler | 当前数据源 | 新数据源 |
|---|---|---|
| `LiveStatusAssembler` | `runtime.spinner` + `runtime.compact_progress` + `runtime.task_status` | `conversation.spinner` + `conversation.compact_progress` + `conversation.task_status` |
| `StatusViewAssembler` | `RuntimeModel` + `SessionModel` + `DiagnosticModel` | `ConversationModel`（运行态字段）+ `SessionModel` + `DiagnosticModel` |

### TuiModel 简化

```rust
pub struct TuiModel {
    pub conversation: ConversationModel,
    pub diagnostic: DiagnosticModel,
    pub input: InputModel,
    pub session: SessionModel,
    // runtime 字段删除
}
```

### 删除的文件

- `model/runtime/model.rs`（RuntimeModel 定义）
- `model/runtime/intent.rs`（RuntimeIntent enum）
- `model/runtime/change.rs`（RuntimeChange enum）
- `model/runtime/spinner.rs`（SpinnerModel 定义——搬到 conversation 下）
- `model/runtime_observation.rs`（RuntimeObservation 中间层）
- `model/runtime/` 目录下其余文件中的类型（WorkspaceState、UsageSummary、TaskStatusSnapshot、ProcessingJob、StatusNotice、CompactProgressModel 等）按需搬迁到 `model/conversation/` 下或保留在公共位置
- `app/update/spinner.rs`（命令式 spinner 辅助方法）

### 设计文档更新

`docs/design/04-tui-design.md` 中 Model Context 章节：
- 删除 "Runtime Model" 小节
- Conversation Model 描述更新：不再只是"对话会话"，而是"TUI 唯一 model 真相源"
- Intent/Change 章节合并 RuntimeIntent/RuntimeChange 到 ConversationIntent/ConversationChange

## 非目标

- 不改 SessionModel（保持独立）
- 不改 DiagnosticModel（保持独立）
- 不改 InputModel（保持独立）
- 不改 spinner 视觉（glyph/verb/90ms 周期）
- 不改 ViewAssembler / ViewModel / ViewState 的分层边界
- 不改渲染管线（OutputArea / StatusBar 渲染逻辑不变，数据源换）

## 架构约束保持

- Model purity guard：`model/` 禁止 ratatui/IO/tokio/AgentClient/channel — 不变
- ViewAssembler boundary guard：允许读 model，禁止 IO — 不变
- ConversationModel 仍是纯 reducer（`apply` 无副作用）
