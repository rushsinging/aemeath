# TUI M9：Update Reducer 收敛计划

## 背景

M1-M5 建立了目标架构边界，M6-M8 将逐步把 output/input/runtime/session 的事实状态迁入 Model。但只要 `core/update` 仍直接修改组件和 legacy state，架构仍会回到“事件处理器到处写状态”的模式。

M9 目标是把 `core/update` 收敛为纯 `update` reducer：外部事件进入后，统一转换为 Intent，更新 Model，产出 Change、Dirty 标记和 Effect。`core` 只保留 loop glue。

## 目标

1. **MUST** 新增统一 `TuiModel` / `TuiMsg` / `TuiUpdateResult` 或等价结构。
2. **MUST** `update` 只修改 Model/ViewState，不直接调用渲染组件方法。
3. **MUST** Agent/SDK event 必须经过 mapper 转换为内部 Intent。
4. **MUST** terminal key/mouse/resize/tick 必须经过 mapper/reducer，不能散落在 `App` impl 中。
5. **MUST** update 产出 `Effect`，不执行 IO、不 spawn、不发 channel。
6. **MUST** 引入 `ViewModelDirty`，避免任意状态变化都全量重组。
7. **MUST** 逐步废弃 `core::msg::Cmd` 的 update 返回路径，为 M10 做准备。

## 非目标

1. **MUST NOT** 在本 milestone 中删除所有 legacy `core/update/*` 文件；先迁移主路径。
2. **MUST NOT** 改变用户可见交互语义。
3. **MUST NOT** 把 runtime async 执行塞进 reducer。
4. **MUST NOT** 在 render 阶段补业务状态。

## 现状问题点

`core/update/ui_event.rs` 目前直接：

- 修改 `self.chat`。
- 修改 `self.output_area`。
- 修改 `self.input_area`。
- 修改 `self.status_bar`。
- 返回 `Cmd`。

`core/update/key.rs` / `enter.rs` / `ask_user_key.rs` 等也存在类似情况：

- 键盘事件直接操作 input_area。
- AskUserQuestion 状态与 output block 耦合。
- Enter 分支同时处理提交、slash command、prompt answer、queue。

这些应被拆成：

```text
External Event
→ TuiMsg
→ mapper
→ Intent[]
→ Model.apply
→ Change[]
→ Dirty + Effect[]
```

## 设计

### TuiModel

建议新增：

```text
apps/cli/src/tui/update/model.rs
```

或：

```text
apps/cli/src/tui/model/root.rs
```

结构：

```rust
pub struct TuiModel {
    pub conversation: ConversationModel,
    pub input: InputModel,
    pub runtime: RuntimeModel,
    pub diagnostic: DiagnosticModel,
    pub session: SessionModel,
}
```

### TuiViewState

建议：

```rust
pub struct TuiViewState {
    pub output: OutputViewState,
    pub input: InputViewState,
    pub layout: LayoutViewState,
    pub animation: AnimationViewState,
}
```

### TuiMsg

建议：

```rust
pub enum TuiMsg {
    TerminalKey(KeyEvent),
    TerminalMouse(MouseEvent),
    TerminalResize { width: u16, height: u16 },
    AgentEvent(UiEvent),
    EffectCompleted(EffectResult),
    TimerTick { id: String },
    RenderTick,
}
```

短期可以复用现有 `UiEvent`，但必须通过 `AgentEventMapper`。

### TuiUpdateResult

```rust
pub struct TuiUpdateResult {
    pub changes: Vec<TuiChange>,
    pub dirty: ViewModelDirty,
    pub effects: Vec<Effect>,
}
```

`ViewModelDirty`：

```rust
pub struct ViewModelDirty {
    pub output: bool,
    pub input: bool,
    pub status: bool,
    pub dialog: bool,
    pub layout: bool,
}
```

### Mapper 分层

建议新增：

```text
apps/cli/src/tui/update/
├── agent_event_mapper.rs
├── key_event_mapper.rs
├── mouse_event_mapper.rs
├── resize_mapper.rs
├── effect_result_mapper.rs
├── root_reducer.rs
└── dirty.rs
```

职责：

- mapper：外部协议 → 内部 intent/effect。
- reducer：调用各 ModelContext apply。
- coordinator：处理跨 context 规则。

### 跨 Context 规则

集中在 coordinator：

- Input submitted：
  - prompt active → DiagnosticIntent::AnswerPrompt + Effect::SendPromptAnswer
  - conversation idle → ConversationIntent::StartChat + Effect::SpawnAgentChat
  - conversation running → ConversationIntent::QueueSubmission
- Agent tool result orphan：
  - ConversationModel 记录 orphan change
  - DiagnosticModel 记录 warning
- Error：
  - ConversationModel append error block
  - DiagnosticModel record error
  - RuntimeModel finish processing job
  - Effect::RunHook

## 实施步骤

### Step 1：新增 TuiModel / TuiViewState / Dirty

建立 root model，不替换旧 App。

测试：

- default 初始化。
- dirty merge。
- model context apply 顺序稳定。

### Step 2：AgentEventMapper

把 `UiEvent` 映射到：

- ConversationIntent
- RuntimeIntent
- DiagnosticIntent
- SessionIntent
- Effect

优先覆盖 M6/M8 已迁移事件。

测试覆盖：

- Text。
- ToolResult。
- Usage。
- Error。
- MessagesSync。
- AskUserQuestion。

### Step 3：KeyEventMapper

把 key event 转为 InputIntent/ViewState intent/Effect。

优先覆盖：

- char input。
- Enter。
- Esc。
- Up/Down。
- Ctrl-C。

### Step 4：RootReducer / Coordinator

实现：

```rust
pub fn update(model: &mut TuiModel, view_state: &mut TuiViewState, msg: TuiMsg) -> TuiUpdateResult
```

要求：

- 无 IO。
- 无 channel send。
- 无 component mutation。

### Step 5：App 接入新 update

在 `core/update` 中逐步把旧分支替换为调用新 `update`。

短期流程：

```text
legacy App receives event
→ build TuiMsg
→ update(&mut self.model, &mut self.view_state, msg)
→ assemble dirty view models
→ sync/render legacy widgets
→ execute effects in core runtime
```

### Step 6：移除直接组件修改

逐步替换并加 guard：

- `self.output_area.push_*`
- `self.input_area.* editing`
- `self.status_bar.set_*`
- `self.chat.*` direct mutation
- `self.session.*` direct mutation

### Step 7：测试主事件流

新增 integration-ish reducer 测试：

1. 用户输入 Enter → idle → SpawnAgentChat effect。
2. 用户输入 Enter → running → queued submission。
3. Agent Text → output dirty。
4. Agent ToolCall/ToolResult → tool status completed。
5. Agent Error → diagnostic error + hook effect。
6. EffectResult SessionSaved → session clean。

## 验收标准

1. **MUST** 存在统一 root update 函数，且纯逻辑可测试。
2. **MUST** Agent event 通过 mapper，不直接修改组件。
3. **MUST** key/mouse/resize 至少主路径通过 mapper。
4. **MUST** update 产出 `Effect` 而不是执行副作用。
5. **MUST** dirty 标记驱动 ViewAssembler。
6. **MUST** 新增 guard 禁止 `apps/cli/src/tui/update` 和迁移后的 `core/update` 直接调用组件 mutation。
7. **MUST** 通过：

```text
git diff --check
.agents/hooks/check-architecture-guards.sh
cargo test -p cli
cargo check -p cli
```

## 风险与回滚

### 风险

- `core/update` 分支多，直接替换容易漏行为。
- AskUserQuestion 与 slash command 是复杂交叉点。
- Dirty/view assembler 接入不完整会导致 UI 不刷新。

### 回滚策略

- Agent event 与 key event 分开提交。
- 每次只替换一组 UiEvent/KeyEvent。
- root reducer 先 mirror 旧行为，再开启 guard。
