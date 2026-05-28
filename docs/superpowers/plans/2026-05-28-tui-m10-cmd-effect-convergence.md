# TUI M10：Cmd → Effect 收敛计划

## 背景

M5 已新增 `Effect` / `EffectResult` / `EffectExecutor`，并提供 legacy `Cmd` adapter。但现有 TUI runtime 仍主要通过 `core::msg::Cmd` 描述副作用，例如：

- `SpawnProcessing`
- `SaveCurrentSession`
- `RunHookNotification`
- `ReadClipboardImage`
- `ProcessImageFile`
- `SetCurrentTurn`
- `FetchReminderRecap`

M10 目标是用 `Effect` 替换 legacy `Cmd`，让副作用描述语言统一，并让 effect result 重新进入 update loop。

## 目标

1. **MUST** `Effect` 覆盖所有现有 `Cmd` 语义。
2. **MUST** `EffectExecutor` 执行真实副作用，并把结果转换为 `TuiMsg::EffectCompleted` 或现有兼容 event。
3. **MUST** update/reducer 不再返回 `Cmd`。
4. **MUST** legacy `Cmd` adapter 只作为临时兼容层，M10 完成后应删除或只保留测试辅助。
5. **MUST** 副作用执行失败进入 DiagnosticModel。
6. **MUST** 对每类 Effect 添加测试或集成验证。

## 非目标

1. **MUST NOT** 改变 AgentClient SDK 的 chat 行为。
2. **MUST NOT** 在 effect executor 中保存业务事实状态；executor 只执行并返回结果。
3. **MUST NOT** 在 model/update 中 `.await`。
4. **MUST NOT** 一次性重写 run_loop；可以逐步接入。

## Cmd 到 Effect 映射

| legacy Cmd | 目标 Effect | 说明 |
|---|---|---|
| `Cmd::None` | `Effect::None` 或空 vec | 可直接省略。 |
| `Cmd::Quit` | `Effect::QuitApplication` | 通知 run_loop 退出。 |
| `Cmd::SpawnProcessing` | `Effect::SpawnAgentChat` | 发起 AgentClient::chat。 |
| `Cmd::SaveCurrentSession` | `Effect::SaveSession` | session save IO。 |
| `Cmd::RunHookNotification` | `Effect::RunHook` | hook notification。 |
| `Cmd::ReadClipboardImage` | `Effect::ReadClipboardImage` | clipboard IO。 |
| `Cmd::ProcessImageFile` | `Effect::ProcessImageFile` | image file IO。 |
| `Cmd::SetCurrentTurn` | `Effect::SetCurrentTurn` 或 SessionIntent | 如无副作用，应改为 model intent。 |
| `Cmd::FetchReminderRecap` | `Effect::FetchReminderRecap` | memory/task recap IO。 |

## Effect 扩展

建议扩展：

```rust
pub enum Effect {
    None,
    QuitApplication,
    RequestRender,
    SpawnAgentChat { chat_id: String, request: ChatRequestSpec },
    CancelAgentChat { chat_id: String },
    SaveSession { session_id: Option<String> },
    RunHook { name: String, payload: HookPayload },
    ReadClipboardImage,
    ProcessImageFile { path: PathBuf },
    FetchReminderRecap,
    FetchTaskStatus,
    RefreshWorkspaceStatus { path: PathBuf },
    StartTimer { id: String, interval: Duration },
    StopTimer { id: String },
}
```

注意：

- `ChatRequestSpec` 应是 TUI 内部 effect 数据，不一定直接暴露 SDK 类型。
- 如果 effect 需要 `Arc<dyn AgentClient>`，应由 executor 持有依赖，不应放入 Effect value。

## EffectResult 扩展

建议：

```rust
pub enum EffectResult {
    Noop,
    QuitRequested,
    AgentChatStarted { chat_id: String },
    AgentChatFailed { chat_id: String, message: String },
    SessionSaved { session_id: Option<String> },
    HookFinished { name: String, success: bool, message: Option<String> },
    ClipboardImageRead { image: sdk::ImageAttachment },
    ImageFileProcessed { image: sdk::ImageAttachment },
    ReminderRecapFetched { line: String },
    TaskStatusFetched { snapshot: TaskStatusSnapshot },
    WorkspaceStatusRefreshed { snapshot: WorkspaceSnapshot },
    TimerStarted { id: String },
    TimerStopped { id: String },
    Failed { message: String },
}
```

## EffectExecutor 依赖注入

`EffectExecutor` 应持有 runtime 依赖：

```rust
pub struct EffectExecutor {
    pub agent_client: Option<Arc<dyn sdk::AgentClient>>,
    pub hook_runner: HookRunner,
    pub session_store: SessionStoreHandle,
    pub ui_tx: mpsc::Sender<UiEvent>,
}
```

要求：

- executor 可以 async。
- executor 内部可以 spawn，但必须集中在 effect 层。
- 所有结果必须回流为 `EffectResult` / `UiEvent`。

## Run loop 接入

目标流程：

```text
TuiUpdateResult.effects
→ run_loop/effect dispatcher
→ EffectExecutor::execute(effect).await/spawn
→ EffectResult
→ TuiMsg::EffectCompleted
→ update
```

对于长任务：

- `SpawnAgentChat` 可以启动 async task。
- chat stream event 仍通过 `UiEvent`/Agent event channel 回流。
- start/fail/done metadata 通过 EffectResult 回流。

## 实施步骤

### Step 1：补齐 Effect 枚举和 Result

覆盖所有 Cmd 语义。

测试：

- 每个 legacy Cmd 都有等价 Effect。
- `Effect` Debug/Clone/Eq 对纯值可用。

### Step 2：实现真实 EffectExecutor 分支

逐步实现：

1. SaveSession。
2. RunHook。
3. Clipboard/Image。
4. FetchReminderRecap / FetchTaskStatus。
5. SpawnAgentChat。
6. WorkspaceStatus。

### Step 3：替换 UpdateResult

把：

```rust
UpdateResult { cmd, pending_slash }
```

改为：

```rust
UpdateResult { effects, pending_slash }
```

或直接复用 `TuiUpdateResult`。

### Step 4：删除 Cmd 返回路径

替换所有 `Cmd::` 构造点。

必须用 Grep 确认：

```text
Cmd::
core::msg::Cmd
legacy_cmd
```

### Step 5：run_loop 执行 Effect

将原来的 cmd exec 分支改为 effect dispatcher。

### Step 6：EffectResult mapper

新增/完善：

```text
apps/cli/src/tui/update/effect_result_mapper.rs
```

把 effect result 转为 Runtime/Session/Input/Diagnostic intent。

### Step 7：删除或隔离 legacy adapter

M10 完成后：

- 删除 `effect/legacy_cmd.rs`；或
- 标记 `#[cfg(test)]`；或
- 只保留迁移兼容测试，不在 production path 使用。

### Step 8：架构守卫

守卫：

- 禁止 `core/update` 返回 `Cmd`。
- 禁止新增 `Cmd::` 使用。
- 禁止 `update` 中 `.await` / `tokio::spawn`。
- 禁止 `model` 中出现 EffectExecutor 依赖。

## 验收标准

1. **MUST** production path 不再依赖 legacy `Cmd`。
2. **MUST** 所有副作用通过 `Effect` 描述。
3. **MUST** EffectExecutor 执行失败能回流 DiagnosticModel。
4. **MUST** chat spawn/session save/hook/clipboard/reminder/task/workspace 都有对应 Effect。
5. **MUST** run_loop 能调度 async effect 并把结果送回 update。
6. **MUST** 通过：

```text
git diff --check
.agents/hooks/check-architecture-guards.sh
cargo test -p cli
cargo check -p cli
```

## 风险与回滚

### 风险

- `SpawnProcessing` 是最复杂副作用，涉及 queue drain、session sync、chat stream。
- Hook notification 与 Done/Stop hook 时序敏感，可能影响 #49。
- Clipboard/image 依赖平台能力，测试需要 mock。

### 回滚策略

- 每类 Cmd 单独迁移。
- `Cmd` adapter 保留到最后一个提交。
- 最后再启用禁止 `Cmd::` 的 guard。
