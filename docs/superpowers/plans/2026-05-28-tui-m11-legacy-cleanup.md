# TUI M11：Legacy 清理与架构封口计划

## 背景

M6-M10 完成后，TUI 的事实状态、update reducer、Effect 边界应已基本迁移到 DDD / Model-View 架构。M11 是最后的封口 milestone：瘦身 `core::App`，清理 legacy state/API，补齐架构守卫，确保后续改动不会重新绕过 Model/View 边界。

## 目标

1. **MUST** `core::App` 只作为 composition/runtime shell，不再直接持有业务事实状态。
2. **MUST** 删除或隔离 legacy `core::state` 中已被 Model/ViewState 替代的类型。
3. **MUST** `output_area`、`input_area`、`status_bar` 都退化为 render/widget adapter。
4. **MUST** 删除 production path 中的 legacy `Cmd` adapter。
5. **MUST** 补齐架构守卫，防止 update/render/model/effect 边界回退。
6. **MUST** 更新 #75 feature 状态和相关 bug 追踪条目。

## 非目标

1. **MUST NOT** 做大规模视觉 redesign。
2. **MUST NOT** 改变用户命令语义。
3. **MUST NOT** 删除仍被 runtime 必需的 session/processing glue，除非已有 EffectExecutor 替代。
4. **MUST NOT** 为了减少文件数牺牲边界清晰度。

## 目标目录职责

迁移完成后，`apps/cli/src/tui` 目录职责应为：

```text
tui/
├── core/             # run_loop、terminal/runtime glue、App shell
├── model/            # Conversation/Input/Runtime/Diagnostic/Session facts
├── update/           # mapper + reducer + coordinator，纯逻辑
├── effect/           # effect value + executor + result mapper
├── view_state/       # scroll/selection/layout/focus/animation 等 UI 状态
├── view_model/       # render input DTO
├── view_assembler/   # Model + ViewState -> ViewModel
├── output_area/      # output render/widget adapter
├── input/            # input render/widget/event adapter
├── display/          # status/dialog/task/theme/syntax render adapter
├── completion/       # pure completion domain service 或 input 子模块
└── session/          # session runtime adapter；不保存 UI 事实状态
```

## core::App 目标形态

建议：

```rust
pub struct App {
    pub model: TuiModel,
    pub view_state: TuiViewState,
    pub view_cache: TuiViewCache,
    pub effect_executor: EffectExecutor,
    pub agent_client: Option<Arc<dyn sdk::AgentClient>>,
}
```

或者：

```rust
pub struct App {
    pub shell: TuiShell,
    pub runtime: TuiRuntimeHandles,
}
```

要求：

- 不直接暴露 `chat: ChatState`。
- 不直接暴露 `input: InputState`。
- 不直接暴露 `session: SessionState`。
- 不直接暴露 `status_bar` 业务 setter。

## 清理清单

### core/state

逐项处理：

| legacy 类型 | 目标 |
|---|---|
| `ChatState` | `ConversationModel` + `RuntimeModel` |
| `InputState` | `InputModel` + `InputViewState` |
| `SessionState` | `SessionModel` |
| `UiLayout` | `LayoutViewState` |
| `AskUserState` | `DiagnosticModel` + `InputMode::PromptAnswer` |
| `TerminalSize` | `LayoutViewState` |

处理方式：

- 删除已无引用类型。
- 对暂时保留的兼容类型标记 `legacy` 并加 TODO 指向 issue #75。
- 不允许新代码引用 legacy state。

### output_area

保留：

- markdown rendering。
- diff rendering。
- wrapping。
- selection rendering。
- widget draw。

删除/迁移：

- conversation facts。
- streaming facts。
- queued messages facts。
- tool status facts。
- ask_user block facts。
- spinner business phase facts。

### input

保留：

- render。
- terminal event low-level adapter。
- paste event decoding。

删除/迁移：

- text buffer fact。
- history fact。
- suggestions fact。
- pending image fact。
- submit route decision。

### display

保留：

- theme。
- syntax highlight。
- status/dialog/task rendering。

删除/迁移：

- status business setters。
- token/tps/session/workspace facts。

### session

保留：

- SDK/session store adapter。
- resume IO。
- processing runtime glue。

删除/迁移：

- UI fact state。
- direct output/status mutation。

## 架构守卫

M11 必须新增或强化以下 guard：

### Model purity

- `apps/cli/src/tui/model` 禁止：
  - `ratatui`
  - `crossterm`
  - `tokio::spawn`
  - `.await`
  - `Command::new`
  - clipboard/file IO

### Update purity

- `apps/cli/src/tui/update` 禁止：
  - `.await`
  - `tokio::spawn`
  - `Command::new`
  - channel send
  - direct widget mutation

### Render boundary

- `output_area/render*` / `display/render*` 禁止：
  - 修改 Model。
  - 根据 tool id 改状态。
  - 产生 Effect。

### Effect boundary

- 只有 `effect/executor`、`core/run_loop`、runtime adapter 可以执行副作用。
- 禁止其他目录直接运行 hook、clipboard、git command、AgentClient::chat。

### Legacy import ban

- 禁止新代码引用 `core::state::*` legacy 类型。
- 禁止 production path 引用 `core::msg::Cmd`。

## 实施步骤

### Step 1：引用盘点

使用 Grep/编译错误盘点：

```text
ChatState
InputState
SessionState
UiLayout
AskUserState
Cmd::
output_area.push_
input_area.
status_bar.set_
```

形成删除顺序。

### Step 2：App 字段替换

将 `App` 字段替换为：

- `model: TuiModel`
- `view_state: TuiViewState`
- render adapters
- effect executor/runtime handles

### Step 3：删除 legacy state

按引用最少到最多删除。

要求：

- 每删一类运行 `cargo check -p cli`。
- 如文件超过 400 行，顺手按职责拆分。

### Step 4：删除 legacy Cmd adapter

确认 M10 后无 production 引用，删除：

- `core::msg::Cmd` 或将其限定为测试。
- `effect/legacy_cmd.rs`。
- legacy cmd exec 分支。

### Step 5：清理 widget setter

删除或私有化：

- `output_area.push_*` 中已无 production 使用的方法。
- `input_area` editing setter。
- `status_bar.set_*`。

### Step 6：更新文档和追踪

更新：

- `docs/feature/active.md` #75 状态为待确认。
- 若 #62/#65/#71/#74 在迁移中被修复，更新对应 bug active 条目为待确认并记录修复提交。
- 如未修复，明确剩余风险。

### Step 7：最终验证

运行：

```text
git diff --check
.agents/hooks/check-architecture-guards.sh
cargo fmt --check
cargo clippy -p cli --all-targets
cargo test -p cli
cargo check -p cli
```

如果 clippy 全量耗时或现有 warning 阻塞，至少记录原因，并保证 `cargo test -p cli` / `cargo check -p cli` 通过。

## 验收标准

1. **MUST** `core::App` 不再直接持有 legacy business state。
2. **MUST** production path 不再使用 legacy `Cmd`。
3. **MUST** `core/update` 不再直接修改 output/input/status widget business state。
4. **MUST** output/input/status widget 只消费 ViewModel/ViewState 或低层 event。
5. **MUST** 架构守卫能阻止 model/update/render/effect 边界绕过。
6. **MUST** #75 feature 文档更新到待确认或说明剩余项。
7. **MUST** 通过最终验证命令。

## 风险与回滚

### 风险

- 删除 legacy state 会触发大量编译错误，需要小步推进。
- 旧测试可能依赖 widget 内部字段，需要改为 ViewModel 断言。
- 如果 M6-M10 有未完成迁移，M11 容易变成大杂烩。

### 回滚策略

- M11 必须在 M6-M10 全部验收后执行。
- 每个 legacy 类型单独提交。
- guard 最后启用，避免迁移中间态阻断开发。
