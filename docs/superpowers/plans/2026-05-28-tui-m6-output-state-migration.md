# TUI M6：Output 事实状态迁移计划

## 背景

M1-M5 已建立 `model` / `view_model` / `view_state` / `view_assembler` / `update` / `effect` 的目标边界，但 `apps/cli/src/tui/output_area` 仍是输出事实状态、渲染状态和组件行为的混合体。`core/update/ui_event.rs` 仍直接调用 `output_area.push_*`、`start_spinner`、`finish_streaming`、`push_tool_result_with_diff` 等方法。

这会导致 tool call、streaming text、queued message、spinner、AskUserQuestion block 等状态在多个位置被修改，正是“信息更新有时候乱”的主要风险来源。

相关已知问题：

- #49：last turn 期间 input queue 残留。
- #62：Grep 工具运行态标题文字不可见。
- #65：工具结果 fenced code block 后续内容继续显示为 code 颜色。
- #71：TUI 渲染缓存越界 panic。
- #74：`/reflect` 后续文本颜色全部变暗。
- #75：TUI Model/View 架构迁移。

## 目标

1. **MUST** 将用户可见对话事实状态迁移到 `model/conversation`，`output_area` 不再作为事实源。
2. **MUST** 将输出区交互状态迁移到 `view_state/output`，例如 scroll、selection、render cache key、visible height。
3. **MUST** 让 `OutputViewAssembler` 成为从 ConversationModel 到 OutputViewModel 的唯一映射入口。
4. **MUST** 保留现有 `output_area` 渲染能力，但它只消费 `OutputViewModel + OutputViewState`。
5. **MUST** 对 tool lifecycle 建立稳定 ID 关联，禁止 render 阶段用文本或行号反推 tool 状态。
6. **MUST** 修复或显式覆盖 #65/#74 同族样式泄漏风险：block 边界必须复位 markdown/style 状态。
7. **MUST** 为新增核心逻辑添加单元测试，覆盖正常、边界、错误路径。

## 非目标

1. **MUST NOT** 在本 milestone 中重写 markdown renderer。
2. **MUST NOT** 一次性删除 `output_area`，先做兼容迁移。
3. **MUST NOT** 改动 agent/runtime 协议，除非发现 TUI 内部无法表达必要事件。
4. **MUST NOT** 引入新的异步副作用；本 milestone 只处理状态归属和渲染输入。

## 现状问题点

### `output_area` 当前混合职责

`apps/cli/src/tui/output_area/mod.rs` 当前包含：

- `lines: VecDeque<OutputLine>`：显示行，同时承担历史事实。
- `streaming_buffer` / `streaming_start` / `synthetic_think_open`：assistant stream 状态。
- `queued_line_count` / `queued_messages`：排队输入显示与事实混合。
- `is_selecting` / `selection_start` / `selection_end` / `screen_line_map`：ViewState。
- `spinner` / `task_status_lines`：runtime/diagnostic/status 与展示混合。
- `ask_user_block_start`：Diagnostic prompt 与输出行耦合。
- `rendered_cache` / `rendered_line_content`：渲染缓存。

### `core/update/ui_event.rs` 当前直接修改组件

典型路径：

```text
UiEvent::Text
→ self.output_area.set_spinner_phase(...)
→ self.output_area.append_assistant_text(...)
```

```text
UiEvent::ToolResult
→ self.output_area.push_tool_result_with_diff(...)
→ self.chat.active_tool_call_ids.remove(...)
→ self.output_area.start_spinner()/set_spinner_phase(...)
```

这些修改需要迁移为：

```text
UiEvent
→ AgentEventMapper
→ ConversationIntent / RuntimeIntent / DiagnosticIntent
→ Model Change
→ OutputViewAssembler
→ output_area.render(view_model, view_state)
```

## 设计

### 新增/扩展 ConversationModel 类型

建议新增文件：

```text
apps/cli/src/tui/model/conversation/
├── stream.rs
├── output_event.rs
├── queued_submission.rs
├── agent_progress.rs
└── block.rs
```

核心类型建议：

```rust
pub struct AssistantStream {
    pub turn_id: ChatTurnId,
    pub kind: AssistantStreamKind,
    pub buffer: String,
    pub synthetic_think_open: bool,
}

pub enum AssistantStreamKind {
    Text,
    Thinking,
}

pub struct QueuedSubmission {
    pub id: String,
    pub text: String,
}

pub struct AgentProgressEntry {
    pub tool_id: String,
    pub message: String,
}

pub enum ConversationBlock {
    UserMessage { id: String, text: String },
    AssistantText { id: String, text: String },
    Thinking { id: String, text: String },
    ToolCall { id: ToolCallId },
    ToolResult { id: ToolCallId, output: String, is_error: bool, image_count: usize },
    System { id: String, text: String },
    Error { id: String, text: String },
    QueuedUserMessage { id: String, text: String },
}
```

说明：

- `ConversationBlock` 是用户可见对话语义，不是预换行后的 `OutputLine`。
- markdown、diff、颜色只属于 ViewModel/Render，不属于 ConversationModel。
- `ToolCall.status` 仍是 tool 图标和颜色唯一来源。

### 扩展 Intent / Change

建议扩展：

```rust
ConversationIntent::AppendAssistantText { text }
ConversationIntent::AppendThinkingText { text }
ConversationIntent::CompleteTextBlock
ConversationIntent::ObserveToolCallStart { name, index }
ConversationIntent::ObserveToolArgumentsDelta { index, name, partial_args }
ConversationIntent::ObserveToolCall { id, name, summary }
ConversationIntent::ObserveToolResult { id, tool_name, output, is_error, image_count }
ConversationIntent::AppendSystemMessage { text }
ConversationIntent::AppendError { text }
ConversationIntent::QueueSubmission { submission }
ConversationIntent::ClearQueuedSubmissions
ConversationIntent::RecordAgentProgress { tool_id, message }
```

对应 `ConversationChange` 需要能表达：

- stream started/appended/completed
- tool pending/running/completed/failed/orphan
- queued submission added/cleared
- output dirty
- style boundary reset required

### OutputViewState 拆分

`view_state/output.rs` 应包含：

```rust
pub struct OutputViewState {
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub is_selecting: bool,
    pub selection_start: Option<SelectedTextRange>,
    pub selection_end: Option<SelectedTextRange>,
    pub screen_line_map: Vec<ScreenLineMapEntry>,
    pub last_visible_height: usize,
    pub render_revision: u64,
}
```

渲染缓存必须以 `render_revision` / block id / width 为 key，不能以陈旧 line index 作为唯一依据。

### OutputViewAssembler 扩展

`view_assembler/output.rs` 应负责：

- `ConversationBlock` → `OutputBlockViewModel`
- `ToolCall.status` → semantic style/icon
- `QueuedSubmission` → queued block
- `AgentProgressEntry` → progress block
- `Diagnostic prompt` 不在 output 中隐式维护，由 `DialogViewAssembler` 处理

要求：

1. **MUST** 每个 block 有稳定 `block_id`。
2. **MUST** block 边界明确重置 markdown/style 状态。
3. **MUST** tool result 和 assistant text 是不同 block，不能共享 renderer 状态。
4. **MUST** orphan tool result 进入 DiagnosticModel，同时可在 output 中显示 warning block。

### output_area 兼容层

短期保留 `OutputArea`，但新增纯渲染入口：

```rust
impl OutputArea {
    pub fn render_view_model(&mut self, vm: &OutputViewModel, state: &mut OutputViewState, area: Rect, buf: &mut Buffer) { ... }
}
```

旧 `push_*` 方法在迁移期间可以保留，但新路径不得继续新增调用。

## 实施步骤

### Step 1：补齐回归测试

新增测试优先覆盖：

1. tool start → args delta → tool call id → result 按 id 完成，标题状态稳定。
2. tool result 先于 tool call id 到达时，产生 orphan diagnostic。
3. assistant text block 后接 tool result fenced code block，再接 assistant text，不发生 style 泄漏。
4. `/reflect` system block 后接 assistant text，不发生 system style 泄漏。
5. render cache 在 `MAX_LINES` 附近不会访问越界 index。
6. queued user message block 在 drain 后被清除。

涉及路径：

- `apps/cli/src/tui/model/conversation/model.rs`
- `apps/cli/src/tui/view_assembler/output.rs`
- `apps/cli/src/tui/output_area/rendered_lines.rs`
- `apps/cli/src/tui/output_area/render_blocks.rs`

### Step 2：扩展 ConversationModel

实现 stream、block、queued submission、agent progress 状态。

验证：

```text
cargo test -p cli tui::model::conversation
```

### Step 3：扩展 OutputViewModel / OutputViewAssembler

让 output assembler 能从 ConversationModel 生成完整 OutputViewModel。

验证：

```text
cargo test -p cli tui::view_assembler::output
```

### Step 4：增加 output_area 新渲染入口

保留旧 API，但新增只消费 ViewModel 的入口。

验证：

```text
cargo test -p cli tui::output_area
```

### Step 5：替换 `core/update/ui_event.rs` 中 output 事件路径

优先替换以下事件：

- `Text`
- `Thinking`
- `TextBlockComplete`
- `ToolCallStart`
- `ToolArgumentsDelta`
- `ToolCall`
- `ToolResult`
- `AgentProgress`
- `SystemMessage`
- `Error`
- `Cancelled`
- `ReflectionDone`

每替换一组必须运行局部测试。

### Step 6：新增架构守卫

新增 guard：

- 禁止 `core/update` 直接调用 `output_area.push_*`。
- 禁止 `core/update` 直接调用 `output_area.start_spinner` / `stop_spinner` / `set_spinner_phase`。
- 禁止 `output_area/render*` 修改 `ConversationModel`。

## 验收标准

1. **MUST** `output_area` 不再作为 conversation/tool/streaming/queue 事实源。
2. **MUST** `core/update/ui_event.rs` 的主要 Agent event 不再直接调用 `output_area.push_*`。
3. **MUST** tool call 图标/颜色只由 `ToolCall.status` 通过 ViewAssembler 映射。
4. **MUST** #65/#74 同类 style 泄漏有回归测试。
5. **MUST** #71 渲染缓存越界有回归测试或明确修复。
6. **MUST** 架构守卫接入 `.agents/hooks/check-architecture-guards.sh`。
7. **MUST** 通过：

```text
git diff --check
.agents/hooks/check-architecture-guards.sh
cargo test -p cli
cargo check -p cli
```

## 风险与回滚

### 风险

- output_area 现有测试依赖 `OutputLine`，迁移可能引发大量测试调整。
- markdown/diff/tool_display 与新 block 模型衔接可能暴露隐藏样式 bug。
- 如果一次替换所有 UiEvent，容易引入行为回归。

### 回滚策略

- 每个事件族单独提交。
- 保留旧 `push_*` API 到 M11 再清理。
- 新路径通过 feature-seam 式函数逐步接入，失败时可单独回退某个事件族。
