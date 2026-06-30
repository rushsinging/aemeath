# 设计：Slash 命令统一走 runtime 事件流（请求-响应 → 流式事件通道）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/497
> 子 issue（已完成）: #493（compact 进度 Gauge，PR #501）
> 日期: 2026-06-30
> 状态: 设计待 review

## 1. 背景与根问题

TUI 层 slash 命令有两种执行模式：

1. **流式（正确模式）**：命令经 `Effect` → `spawn_guarded` 后台执行，通过 `UiEvent`/`ChatEvent` 回流。TUI 主循环不阻塞，runtime 可推中间事件。代表：`/reflect`、`/update`。
2. **请求-响应（问题模式）**：TUI 直接调 SDK trait 的 `async fn`，`.await` 阻塞主循环，期间 runtime 无法推任何事件。代表：`/compact`、`/model`、`/context` 等。

`/compact` 最典型——LLM 摘要可能耗时数十秒，期间 TUI 完全阻塞，无法显示进度。

### 1.1 现状清单：仍在请求-响应模式的 slash 命令

| 命令 | 调用点 | SDK 方法 | 阻塞原因 |
|---|---|---|---|
| `/compact`（2 处） | `slash.rs:46-84`、`slash.rs:273-296` | `compact_messages(...)` | LLM 摘要耗时长 |
| `/model`（切换） | `slash.rs:321` | `switch_model(...)` | 构造新 provider，网络握手 |
| `/context` | `slash.rs:105-107` | `estimate_context(...)` | token 估算 |
| `/think` | `slash.rs:364` | `set_thinking(...)` | 配置切换 |
| `/resume` | `slash.rs:381` | `load_session(...)` | 从磁盘加载历史 |
| 任意未识别 slash | `slash.rs:191` | `execute_command(...)` | runtime 命令注册表执行 |

另有 4 个"半流式"Effect（`/save`、`/memory`、`/paste`、`RunHook`）虽走了 `Effect`，但 `executor.rs` 内部仍直接 `.await`，本质等价阻塞。

### 1.2 #493（PR #501）的遗留缺口（重要）

#493 PR body 声称"`/compact` 命令改为走 runtime 主循环事件流（`ChatInputEvent::Compact`）"，但代码审查发现 **TUI 侧从未发送 `ChatInputEvent::Compact`**：

- `ChatInputEvent::Compact` 变体已存在（`packages/sdk/src/chat.rs:63`）
- runtime idle gate 已能处理它（`input_gate.rs:228`，busy 时 defer 到 buffer）
- runtime idle 分支已能执行 `CompactRequested`（`loop_runner.rs:233`）
- **但 TUI `slash.rs:46-84` 和 `slash.rs:273-296` 仍直接调 `ac.compact_messages().await`**

即 #493 只完成了 `auto_compact`（主循环内压缩）的进度事件流改造，`/compact` slash 命令本身的"走事件流"改造**未落地**。PR #501 的 progress Gauge 对手动 `/compact` 实际无效（因为 `/compact` 走的是请求-响应，根本不经过主循环，拿不到 `CompactProgress` 事件）。

### 1.3 Skill 执行路径（无需独立改造）

确认 skill 有三条执行路径，**均无独立的阻塞 SDK 调用**：

1. **TUI 本地 alias 查找**（`slash.rs:220-227`、`207-216`）：纯本地同步，`find_skill_by_alias` → 取 `skill.content` → 作为 prompt 返回。不调 SDK、不阻塞。
2. **经 `execute_command` → `RunSkill`**（`slash.rs:358-361`）：唯一阻塞点在 `execute_command().await`，随该子 issue 一并解决。
3. **skill content 提交后**：走主对话循环，已是流式。

结论：skill 无独立子 issue，路径 2 随 `execute_command` 子 issue 受益。

## 2. 指导原则

**单一真相 = runtime 主循环 idle 分支**。所有 slash 命令（含耗时与即时）统一为一条模式：

> TUI 发 `ChatInputEvent::Xxx` → runtime idle 分支异步执行 SDK trait 方法 → 通过 `RuntimeStreamEvent` 推进度/结果 → TUI 渲染。

TUI 主循环 **NEVER** 因单个命令阻塞。`#493` 建立的 `Compact` 事件通道是首个落地原型，本设计将其推广为通用模式。

**不采用折中方案**（短命令走 `spawn_guarded`、长命令走主循环），原因：会留下两套并存的命令执行模式，违背 issue 统一目标，且后续收敛成本更高。

## 3. 目标架构

### 3.1 统一事件通道扩展

`ChatInputEvent` 新增变体（`packages/sdk/src/chat.rs`）：

```rust
pub enum ChatInputEvent {
    // ... 现有变体 ...
    Compact,          // 已存在，但 TUI 从未发送（#493 遗留）
    SwitchModel { model_spec: String },   // /model 切换
    EstimateContext,                      // /context
    SetThinking { enabled: bool },        // /think
    ResumeSession { session_id: String }, // /resume
    ExecuteCommand { name: String, args: String }, // 兜底：未内置命令 + skill 路径 2
}
```

设计要点：
- `model_spec` 为原始参数字符串（如 `gpt-5` 或 `provider/model`），runtime 侧解析，保持 SDK 层薄。
- `ExecuteCommand` 统辖所有未在 TUI 硬编码的命令，含返回 `RunSkill` 的 skill 路径。
- busy 时所有命令遵循与 `Compact` 相同的 defer 策略（放回 `PendingInputBuffer`，等回合结束回 idle）。

### 3.2 runtime idle 分支扩展

`IdleResult`（`loop_runner.rs:1179`）从离散变体改为统一载体，避免每加一个命令就加一个变体：

```rust
enum IdleResult {
    Resumed,
    Shutdown,
    CommandExecuted,   // 替代 CompactRequested，泛化所有命令
}
```

`GateOutcome`（`input_gate.rs:48`）新增统一的 pending command 载体，替代单一 `compact_requested: bool`：

```rust
pub struct GateOutcome {
    // ... 现有字段 ...
    pub pending_command: Option<PendingCommand>,  // 替代 compact_requested
}

pub enum PendingCommand {
    Compact,
    SwitchModel { model_spec: String },
    EstimateContext,
    SetThinking { enabled: bool },
    ResumeSession { session_id: String },
    ExecuteCommand { name: String, args: String },
}
```

`apply_gate`（`input_gate.rs:121`）的 idle 分支匹配各 `ChatInputEvent` 变体，填充 `pending_command`。

### 3.3 结果/进度事件

`RuntimeStreamEvent`（`events.rs:80`）新增变体：

```rust
pub enum RuntimeStreamEvent {
    // ... 现有变体（含 CompactProgress）...
    
    // /model
    ModelSwitchProgress { stage: ModelSwitchStage },  // Connecting / Ready
    ModelSwitched { display_name: String, provider_id: String },
    ModelSwitchFailed { error: String },
    
    // /context
    ContextEstimated { usage: ContextUsage },  // input_tokens, context_size, ratio
    
    // /think
    ThinkingChanged { enabled: bool },
    
    // /resume
    ResumeProgress { stage: ResumeStage },  // Loading / Restoring
    SessionLoaded { messages: Vec<Message>, session_id: String },
    ResumeFailed { error: String },
    
    // execute_command（含 skill RunSkill）
    CommandResult { result: CommandResultView },  // Success/Error/Action/Confirm 的视图
}
```

`CommandResultView`（SDK 层 `CommandResult` 的 `Clone+Send` 投影）承载 `execute_command` 的结果，TUI 侧映射回现有 `handle_command_action` 逻辑。

### 3.4 TUI 侧改造

`slash.rs` 每个命令分支从 `ac.xxx().await` 改为 `self.chat.push_input_event(ChatInputEvent::Xxx {...})`。

示例（`/compact` 两处）：

```rust
// 改造前（slash.rs:46-84）
cmd if cmd == format!("/{}", cmd::COMPACT) => {
    if let Some(ref ac) = self.agent_client {
        match ac.compact_messages(...).await { ... }  // 阻塞
    }
}

// 改造后
cmd if cmd == format!("/{}", cmd::COMPACT) => {
    self.chat.push_input_event(sdk::ChatInputEvent::Compact);
    self.model.runtime.apply(RuntimeIntent::SetSpinnerPhase(SpinnerPhase::Compacting));
}
```

`CommandAction::Compact`（`slash.rs:273`）同理改为 `push_input_event(Compact)`。

TUI 事件回流处理（`processing.rs:39` `sdk_event_to_ui_event` + `agent_event.rs:30` `map_agent_event`）：每个新 `RuntimeStreamEvent` 变体补一处映射 → `RuntimeIntent` 或直接 `UiEvent`。

### 3.5 回显机制（关键：文案位置迁移）

#### 现状：同步回显

当前 slash 命令执行完，TUI **同步直接调用** `append_system_notice` / `append_error_notice` 写入 `ConversationModel`：

```
slash.rs: ac.xxx().await → Ok(result) → append_system_notice("[xxx done]")
```

命令执行与回显在同一次 `.await` 调用里串行完成，回显即时、同步。所有回显文案散落在 `slash.rs`（TUI 层）。

#### 改造后：事件驱动回显

命令走 runtime 事件流异步执行，TUI 不再原地拿到结果，**回显必须改为由事件回流触发**。现成通道已存在，无需新建：

```
runtime 执行完 → sink.send_event(SystemMessage("[xxx done]"))
              → ChatEvent::SystemMessage         (chat_event.rs 映射)
              → UiEvent::SystemMessage           (processing.rs:105)
              → map_agent_event                  (agent_event.rs:69)
              → ConversationIntent::AppendSystemMessage  (写入 ConversationModel)
```

错误同理：`RuntimeStreamEvent::Error` → `UiEvent::Error` → `ConversationIntent::AppendError`（`agent_event.rs:55`，还附带 `DiagnosticIntent::RecordNotice` + `Effect::RunHook("error")`）。

#### 三类回显的处理策略

| 回显类型 | 现状（同步） | 改造后（事件流） | 用的事件 |
|---|---|---|---|
| **成功通知**（`[compacted]`、`[switched to gpt-5]`） | `append_system_notice` | runtime 发 `SystemMessage` → 自动渲染 | `RuntimeStreamEvent::SystemMessage` |
| **错误通知**（`compact failed: ...`） | `append_error_notice` | runtime 发 `Error` → 自动渲染 | `RuntimeStreamEvent::Error` |
| **进度反馈**（compact Gauge、model "connecting"） | 无 | runtime 发专用 progress 事件 → intent | `RuntimeStreamEvent::CompactProgress` 等 |

#### 执行前 vs 执行后的回显

- **执行前**（命令已接收、即将执行）：仍可在 `slash.rs` 同步回显，如设 spinner phase——因为这些不依赖执行结果，TUI 发完事件就知道要做。
- **执行后**（结果/进度）：**必须**由 runtime 事件回流驱动，TUI 不再原地知道结果。

#### 文案位置迁移（重要）

当前所有 `append_system_notice("[xxx done]")` 文案散落在 `slash.rs`（TUI 层）。改造后这些文案要**迁移到 runtime 层**——因为 TUI 不再知道命令执行结果，只有 runtime 知道：

```rust
// 改造前（slash.rs，TUI 层持有结果文案）
match ac.compact_messages(...).await {
    Ok((compacted, was_compacted)) => {
        self.append_system_notice("[compacted]");  // ← 文案在 TUI
    }
    Err(e) => self.append_error_notice(format!("compact failed: {}", e)),
}

// 改造后（slash.rs 只发事件 + 设 spinner，无文案）
self.chat.push_input_event(sdk::ChatInputEvent::Compact);
self.model.runtime.apply(RuntimeIntent::SetSpinnerPhase(SpinnerPhase::Compacting));

// 文案迁移到 runtime（loop_runner.rs idle 分支，compact 完成后）
sink.send_event(RuntimeStreamEvent::SystemMessage(
    format!("[compacted: {} → {} messages]", old_len, new_len)
)).await;
```

净效果：`slash.rs` 大幅瘦身（每个命令分支从 ~30 行 match 缩为 2-3 行），回显文案逻辑集中到 runtime idle 分支。

> **注**：`RunSkill` / `InjectMessage` 这类返回 prompt 的 action 仍是"输入注入"——`slash.rs` 侧 `return Some(content)` 把内容作为下一轮用户输入提交，不走结果事件回显，保持现状。

### 3.6 Spinner / 进度态接通（以 compact 为原型）

#### 两条路径的 spinner 现状对比

compact 有两个入口，spinner 接通方式截然不同：

| 入口 | spinner 启动 | 进度（Gauge） | spinner 停止 | 是否经过 hook 事件链 |
|---|---|---|---|---|
| **auto_compact**（主循环内，✅ 正确） | runtime 发 `HookEvent(PreCompact)` → TUI `hook_spinner_phase` → `SpinnerPhase::Compacting` | runtime 发 `CompactProgress` → `SetCompactProgress` | runtime 发 `HookEvent(PostCompact)` → TUI `StopSpinner` | **是**，全链路接通 |
| **手动 `/compact`**（slash 旧路径，❌ 问题） | `slash.rs:49` TUI 自己手动 `SetSpinnerPhase(Compacting)` | **无**（走请求-响应，拿不到 `CompactProgress`） | `slash.rs:62/77` 手动 `StopSpinner` | **否**，绕过 hook 链 |

即手动 `/compact` 的 spinner 是 TUI「自管自启自停」的孤岛，与 runtime 的 `CompactProgress` 事件流完全脱节——这正是 #493 的 Gauge 对手动 `/compact` 无效的根因。

#### 改造目标：手动 `/compact` 复用 auto_compact 的 spinner 事件链

手动 `/compact` 走事件流（子 issue 0）后，应复用 auto_compact 已接通的事件链，而非 TUI 自管 spinner：

```
slash.rs: push_input_event(Compact)  // 只发事件，不设 spinner
        ↓
runtime idle 分支执行 manual_compact:
  - 发 HookEvent(PreCompact)  → TUI 自动设 Compacting spinner   ← 启动
  - 发 CompactProgress{...}   → TUI 渲染 Gauge                   ← 进度
  - 发 HookEvent(PostCompact) → TUI 自动 StopSpinner + 清 Gauge  ← 停止
  - 发 SystemMessage("[compacted: ...]")  → TUI 回显结果
```

关键变化：**`slash.rs` 不再手动设/停 spinner**，完全由 runtime 的 hook + progress 事件驱动。

#### 实现要点

1. **runtime `manual_compact`**（`compact.rs`）需发 `PreCompact` / `PostCompact` hook 事件（当前 `auto_compact` 发了，但需确认 `manual_compact` 是否也发——见子 issue 0 验证）。
2. **`slash.rs` 删除手动 spinner 代码**：移除 `slash.rs:49-51`（`SetSpinnerPhase(Compacting)`）、`slash.rs:62`（`StopSpinner`）、`slash.rs:77`（`StopSpinner`）。
3. **错误路径 spinner 停止**：compact 失败时，runtime 须发 `PostCompact`（或 `Error` 事件触发 TUI 停 spinner）——确保失败时 spinner 不卡死。当前 `agent_event.rs:55` 的 `Error` 映射只写 error notice + RunHook，**没有 StopSpinner**，需补。
4. **`MessagesSync` 双重保险**：`compact_progress.rs` 的清理机制（`MessagesSync` handler 清 `compact_progress`）对两条路径都生效——runtime compact 后发 `MessagesSync` 替换 TUI 镜像，同时清掉 Gauge 态。

#### 其他命令的 spinner 策略

| 命令 | spinner 需要？ | 接通方式 |
|---|---|---|
| `/model`（switch_model） | 是（provider 握手耗时） | runtime 发专用 `ModelSwitchProgress` → `SetSpinnerPhase(CallingTool("model"))` 或复用进度模型 |
| `/resume`（load_session） | 是（磁盘 IO） | runtime 发 `ResumeProgress` → spinner phase |
| `/context`（estimate_context） | 否（快） | 无 spinner，仅结果 `SystemMessage` 回显 |
| `/think`（set_thinking） | 否（快） | 无 spinner，仅结果 `SystemMessage` 回显 |
| `execute_command` | 按命令而定 | 复用 `SystemMessage`/`Error` 回显，长命令可加 spinner |

### 3.7 半流式 Effect 改造

`/save`、`/memory`、`/paste`、`RunHook`（`executor.rs`）内部 `.await` 改为 `spawn_guarded` 后台执行，结果经 `UiEvent` 回流。这 4 个不涉及 runtime 主循环协调，独立于上述主流程，可并行实施。

## 4. 子 issue 拆分与执行顺序

| 序 | 子 issue | 命令 | 复杂度 | 依赖 | 并行性 |
|---|---|---|---|---|---|
| 0 | 修复 #493 遗留：`/compact` 真正走事件流 | `/compact`（2 处） | 低 | — | 首个，验证模式 |
| 1 | `/model` 走事件流 | `switch_model` | 中 | 0 | 可与 2-4 并行 |
| 2 | `/resume` 走事件流 | `load_session` | 中 | 0 | 可与 1/3/4 并行 |
| 3 | `/context` 走事件流 | `estimate_context` | 低 | 0 | 可与 1/2/4 并行 |
| 4 | `/think` 走事件流 | `set_thinking` | 低 | 0 | 可与 1/2/3 并行 |
| 5 | `execute_command` 走事件流 | 兜底命令 + skill 路径 2 | 高 | 1-4 | 最后做 |
| 6 | 半流式 Effect `spawn_guarded` 化 | `/save` 等 4 个 | 低 | — | 任意时刻 |

### 子 issue 0（修复 #493 遗留）详情

这是最高优先级——#493 声称完成但 `/compact` slash 命令实际未走事件流，手动 `/compact` 看不到 progress Gauge。

改动范围：
1. `apps/cli/src/tui/app/slash.rs:46-84`：删除 `ac.compact_messages().await`，改为 `push_input_event(Compact)` + 设 `Compacting` spinner
2. `apps/cli/src/tui/app/slash.rs:273-296`（`CommandAction::Compact`）：同上
3. TUI 处理 `CompactProgress` → `SetCompactProgress`（#493 已建好映射，但 `/compact` 走旧路径从未触发）
4. TUI 处理 compact 完成后的 `MessagesSync`（runtime compact 后会发，用于替换 TUI messages 镜像）

验证：手动 `/compact` 时 TUI 显示 Gauge 进度条，compact 完成后 messages 正确替换、Gauge 消失。

## 5. 每个子 issue 的通用改造模板

### 5.1 SDK 层（`packages/sdk/src/`）
- `ChatInputEvent` 新增变体（如 `SwitchModel { model_spec }`）
- `ChatEvent` 新增结果/进度变体（如 `ModelSwitched { display_name }`）
- `RuntimeStreamEvent` → `ChatEvent` 映射补一行（`chat_event.rs`）

### 5.2 Runtime 层（`agent/features/runtime/`）
- `input_gate.rs`：idle 分支匹配新 `ChatInputEvent` 变体 → 填充 `GateOutcome.pending_command`
- `loop_runner.rs`：idle 分支 match `PendingCommand::Xxx` → 执行 SDK trait 方法 → `sink.send_event` 推进度/结果
- 长命令：trait 方法增加 `progress: Option<&dyn ProgressFn>` 回调（参照 `compact_messages_with_llm` 的 `CompactProgressFn`）

### 5.3 TUI 层（`apps/cli/src/tui/`）
- `slash.rs`：命令分支改为 `push_input_event(ChatInputEvent::Xxx)`（替代 `.await`）
- `processing.rs`：`sdk_event_to_ui_event` 补映射
- `agent_event.rs`：`map_agent_event` → 新 `RuntimeIntent`
- 渲染：进度类复用 Gauge / 通知类走 `AppendSystemNotice`

### 5.4 验证
- `cargo test -p cli -p runtime -p sdk`
- `cargo clippy -p cli -p runtime -p sdk`
- 手动验证：命令执行期间 TUI 不阻塞，长命令有进度反馈

## 6. 验收标准

- AC1: 每个 slash 命令执行期间 TUI 主循环不阻塞（可渲染中间状态）
- AC2: 长耗时命令（compact、switch_model、load_session）有进度反馈
- AC3: 短耗时命令（set_thinking、estimate_context）执行后 UI 正确更新
- AC4: 半流式 Effect 改为 `spawn_guarded` 后不引入回归
- AC5: 手动 `/compact` 正确显示 progress Gauge（修复 #493 遗留）
- AC6: skill 经 `execute_command` 路径执行时不阻塞（路径 2 随子 issue 5 解决）

## 7. 风险

1. **`execute_command` 迁移面最大**：CommandRegistry 内每个命令的副作用路径都要适配事件回流，建议留到最后并充分测试
2. **`IdleResult`/`GateOutcome` 泛化**：从 `compact_requested: bool` 改为 `pending_command: Option<PendingCommand>` 是结构性变更，需同步更新 6+ 个 idle match 站点（`loop_runner.rs` 中所有 `IdleResult::CompactRequested` 分支）
3. **向后兼容**：SDK trait 方法签名不变（仅 TUI 不再直接调），`no_tui.rs` 入口需同步改走事件通道或保留直调
4. **busy 时命令排队**：参照 `/compact` 的 busy-defer 策略（放回 buffer 等 idle）
5. **#493 遗留需优先修复**：否则子 issue 1-5 复用的"模式"本身是未验证的

## 8. 关联文件索引

### Runtime
- `agent/features/runtime/src/business/chat/looping/events.rs` — `RuntimeStreamEvent` + `CompactStage`
- `agent/features/runtime/src/business/chat/looping/input_gate.rs` — `GateOutcome`、`apply_gate` idle 分支
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs` — `IdleResult`、idle 分支、`CompactRequested` 处理
- `agent/features/runtime/src/business/chat/looping/compact.rs` — auto/manual compact 编排

### SDK
- `packages/sdk/src/chat.rs` — `ChatInputEvent`、`ChatRequest`
- `packages/sdk/src/chat_event.rs` — `ChatEvent`

### TUI
- `apps/cli/src/tui/app/slash.rs` — 所有 slash 命令分支（改造主战场）
- `apps/cli/src/tui/effect/session/processing.rs` — `sdk_event_to_ui_event`
- `apps/cli/src/tui/adapter/agent_event.rs` — `map_agent_event`
- `apps/cli/src/tui/model/runtime/compact_progress.rs` — `CompactProgressModel`（进度模型参考）
- `apps/cli/src/tui/model/runtime/intent.rs` — `RuntimeIntent`
- `apps/cli/src/tui/app/runtime.rs` — `find_skill_by_alias`

### Compact 相关（#493 落地，本设计复用基线）
- `agent/features/runtime/src/business/compact/summary.rs` — `compact_messages_with_llm` + `CompactProgressFn`
- `agent/features/runtime/src/core/client/trait_compact.rs` — `compact_messages` trait
