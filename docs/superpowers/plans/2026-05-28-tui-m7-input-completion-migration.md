# TUI M7：Input / Completion 迁移计划

## 背景

M3 已建立 `InputModel` / `InputDocument` / `InputSubmission`、`InputViewAssembler` 和 `update::input_mapper`，但 legacy `apps/cli/src/tui/input` 仍持有大量输入事实状态和交互逻辑：文本、光标、历史、selection、suggestions、pending images、paste/clipboard 等。

这导致输入区存在两套状态来源：

1. 新的 `model/input`。
2. 旧的 `input/input_area` 和 `core::state::InputState`。

M7 目标是让 `InputModel + InputViewState` 成为唯一事实源，`input_area` 退化为纯渲染/事件适配组件。

相关已知问题：

- #49：last turn 输入残留在 queue 区域。
- #72：agent 双层循环中一轮结束后不自动读取 input queue。
- #75：TUI Model/View 架构迁移。

## 目标

1. **MUST** 将输入文本、cursor、selection、history、completion、attachments 的事实状态迁移到 `model/input` 或 `view_state/input`。
2. **MUST** `InputArea` 不再决定提交路由；提交路由只能通过 `update::input_mapper` / coordinator。
3. **MUST** completion 生成逻辑通过 InputModel intent/change 接入，不再直接修改 `InputArea` suggestion 状态。
4. **MUST** clipboard/paste/image 读取通过 `Effect` 表达，不在 input reducer 中直接执行 IO。
5. **MUST** 保持现有键盘操作行为：输入、删除、移动、历史上下、Tab/Enter 补全、paste、图片附件提示。
6. **MUST** 新增核心逻辑测试覆盖正常、边界、错误路径。

## 非目标

1. **MUST NOT** 在本 milestone 中重做补全排序算法。
2. **MUST NOT** 改变 slash command 的业务语义。
3. **MUST NOT** 改变 AskUserQuestion 的用户交互语义；只调整其输入状态归属。
4. **MUST NOT** 直接删除 legacy `input_area`，先变为 render adapter。

## 现状问题点

### legacy input 目录职责

`apps/cli/src/tui/input` 当前包含：

```text
input/
├── clipboard.rs       # clipboard/image IO
├── input_area/        # 文本编辑、历史、selection、render、resize、suggestions
├── mouse_handler.rs   # 鼠标事件
└── paste_handler.rs   # paste 处理
```

其中 `input_area` 同时承担：

- 输入 buffer。
- cursor。
- selection。
- history。
- suggestions。
- render。
- pending image count。

### completion 当前独立但未接入 InputModel

`completion/` 目前是相对纯的 domain service：

- commands
- files
- models
- sessions
- parser
- types

它应继续保持纯逻辑，但调用入口需要从 legacy component 移到 `InputModel` intent 处理。

## 设计

### InputModel 扩展

建议扩展：

```text
apps/cli/src/tui/model/input/
├── edit.rs
├── completion_request.rs
├── completion_result.rs
├── attachment.rs
└── command_line.rs
```

核心状态：

```rust
pub struct InputModel {
    pub document: InputDocument,
    pub history: InputHistory,
    pub completions: Vec<InputCompletion>,
    pub completion_active: bool,
    pub selected_completion: Option<usize>,
    pub attachments: Vec<InputAttachment>,
    pub mode: InputMode,
}

pub enum InputMode {
    Normal,
    PromptAnswer,
    Completion,
}
```

### InputViewState 职责

`view_state/input.rs` 保留显示交互状态：

```rust
pub struct InputViewState {
    pub focused: bool,
    pub viewport_offset: usize,
    pub preferred_column: Option<usize>,
    pub composing: bool,
}
```

原则：

- cursor 位置如果代表文本编辑事实，应在 `InputDocument`。
- viewport/focus/composing 只影响显示，应在 `InputViewState`。

### InputIntent 扩展

建议：

```rust
InputIntent::InsertChar(char)
InputIntent::InsertText(String)
InputIntent::Backspace
InputIntent::Delete
InputIntent::MoveCursorLeft
InputIntent::MoveCursorRight
InputIntent::MoveCursorHome
InputIntent::MoveCursorEnd
InputIntent::MoveHistoryPrevious
InputIntent::MoveHistoryNext
InputIntent::RequestCompletion { context }
InputIntent::SelectCompletionNext
InputIntent::SelectCompletionPrevious
InputIntent::AcceptCompletion
InputIntent::AttachImage { image }
InputIntent::ClearAttachments
InputIntent::Submit
InputIntent::Clear
```

### Completion 接入

新增纯 service adapter：

```text
apps/cli/src/tui/model/input/completion_service.rs
```

或者保留 `completion/` 目录，只在 `update` 中调用：

```text
InputChange::CompletionRequested { context }
→ coordinator 调用 completion::generate_suggestions
→ InputIntent::SetCompletions
```

如果 completion 需要读取文件系统，必须通过 `Effect::GenerateCompletion` 或 runtime service 表达，不能在 model 中执行 IO。

### Paste / Clipboard / Image

迁移方向：

```text
Key/Paste event
→ InputIntent::PasteRequested
→ Effect::ReadClipboard / Effect::ProcessImageFile
→ EffectResult
→ Msg
→ InputIntent::InsertText / AttachImage
```

要求：

- `input/clipboard.rs` 迁到 effect executor 或 runtime adapter。
- `paste_handler.rs` 只做 terminal paste event 到 intent 的映射。

## 实施步骤

### Step 1：补齐 InputModel 编辑测试

覆盖：

1. 插入普通字符。
2. 在空文本 backspace 不 panic。
3. 多字节字符 cursor 移动不切断 UTF-8。
4. history previous/next 边界。
5. completion accept 替换正确 token。
6. attachment 添加/清空。
7. submit 后生成 `InputSubmission` 并清空输入。

验证：

```text
cargo test -p cli tui::model::input
```

### Step 2：实现 InputIntent 完整编辑 reducer

把 `input_area/editing.rs` 的纯编辑逻辑迁移到 `model/input`。

要求：

- **MUST** 使用 char index / byte index 安全转换工具。
- **MUST** 复用现有 `sdk::CharIdx` 或项目已有索引类型，避免重复逻辑。

### Step 3：接入 completion

迁移 suggestions 状态：

- `completion_active`
- `selected_completion`
- `completions`

`InputViewAssembler` 负责把这些变为 `InputViewModel`。

### Step 4：迁移 key/mouse/paste mapper

新增或扩展：

```text
apps/cli/src/tui/update/input_key_mapper.rs
apps/cli/src/tui/update/input_mouse_mapper.rs
apps/cli/src/tui/update/paste_mapper.rs
```

这些 mapper 只能产出 Intent/Effect，不能直接修改组件。

### Step 5：改造 InputArea

新增：

```rust
impl InputArea {
    pub fn sync_from_view_model(&mut self, vm: &InputViewModel) { ... }
}
```

或更理想：

```rust
pub fn render_input(vm: &InputViewModel, state: &InputViewState, area: Rect, buf: &mut Buffer)
```

旧字段保留到 M11 清理，但新 update 路径不得写入旧字段。

### Step 6：替换 core update 输入路径

优先替换：

- 普通字符输入。
- Enter 提交。
- history 上下。
- suggestion navigation。
- paste。
- image attachment。

### Step 7：新增架构守卫

守卫规则：

- 禁止 `core/update` 直接调用 `input_area.insert*` / `delete*` / `set_pending_images`。
- 禁止 `model/input` 使用 clipboard 或文件系统 IO。
- 禁止 `input_area` 决定 `StartChat` / `QueueSubmission` / `AnswerPrompt`。

## 验收标准

1. **MUST** 输入文本、cursor、history、completion、attachments 的事实状态来自 `InputModel`。
2. **MUST** `InputArea` 不再作为输入事实源。
3. **MUST** Enter 提交仍通过 `update::input_mapper::route_submission`。
4. **MUST** paste/clipboard/image 读取通过 Effect。
5. **MUST** completion 行为与迁移前兼容。
6. **MUST** 通过：

```text
git diff --check
.agents/hooks/check-architecture-guards.sh
cargo test -p cli
cargo check -p cli
```

## 风险与回滚

### 风险

- 多字节字符 cursor 逻辑容易回归。
- completion 与 slash command 参数解析依赖旧 input_area token 位置。
- AskUserQuestion 的 PromptAnswer 模式可能与普通输入模式交叉。

### 回滚策略

- 先只迁移纯编辑逻辑，保留 legacy render。
- completion 单独提交。
- paste/clipboard 单独提交。
- 每组键位迁移后运行对应测试。
