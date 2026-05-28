# TUI M8：Runtime / Session / Status 迁移计划

## 背景

M4 已建立 `RuntimeModel` / `DiagnosticModel`，并让 `StatusViewAssembler`、`DialogViewAssembler` 能从模型生成视图模型。但 legacy 状态仍分散在：

- `core::App` 字段。
- `core::state::ChatState` / `SessionState`。
- `display::status_bar::StatusBar` 内部字段。
- `session/processing.rs`、`session/resume.rs`、`session/session_lifecycle.rs`。
- `core/mod.rs` 中 workspace/status 相关函数直接执行 `git` 命令。

M8 目标是让 runtime/session/status 的事实状态归位，为后续 M9 reducer 收敛和 M10 effect 执行打基础。

相关 feature/bug：

- #49：last turn input queue。
- #69：worktree 中 LLM 仍尝试搜索主分支路径。
- #72：agent loop queue drain。
- #53：TUI Model/View 架构迁移。

## 目标

1. **MUST** RuntimeModel 成为 provider/model/workspace/usage/tps/task status/processing job 的事实源。
2. **MUST** 新增或明确 `SessionModel`，承载 session id、messages sync 状态、resume metadata、save dirty 状态。
3. **MUST** StatusBar 只消费 `StatusLineViewModel`，不再保存业务事实状态。
4. **MUST** workspace/git branch/worktree 检测通过 Effect 或 runtime adapter 完成，结果回流为 RuntimeIntent。
5. **MUST** session lifecycle 中的 IO 通过 Effect 描述，不在 update/model 中直接执行。
6. **MUST** 保持现有状态栏显示行为兼容。

## 非目标

1. **MUST NOT** 改动底层 session 存储格式。
2. **MUST NOT** 改变 AgentClient SDK 对外契约，除非 M8 中发现缺失必要 snapshot 字段。
3. **MUST NOT** 重写 task window UI。
4. **MUST NOT** 在 model 层执行 `git`、文件 IO、网络 IO。

## 现状问题点

### core/App 仍混合 runtime 状态

`core/mod.rs` 中 `App` 目前持有：

- `chat: ChatState`
- `input: InputState`
- `session: SessionState`
- `layout: UiLayout`
- `skills`
- `agent_client`
- `output_area` / `input_area` / `status_bar`

其中 provider/model、usage、workspace、session、processing 状态跨多个对象维护。

### status_bar 仍是状态容器

`core/update/ui_event.rs` 中仍调用：

- `self.status_bar.set_tps(tps)`
- `self.status_bar.set_tokens(...)`
- workspace/status 相关 setter

这些都应迁入 RuntimeModel + StatusViewAssembler。

### workspace 检测仍直接执行 git

`core/mod.rs` 中：

- `git_branch_for(path)` 调用 `Command::new("git")`。
- `worktree_kind_for(path)` 调用 `git rev-parse`。

这类副作用应移到 EffectExecutor 或 runtime adapter。

## 设计

### RuntimeModel 扩展

建议扩展：

```text
apps/cli/src/tui/model/runtime/
├── provider.rs
├── workspace.rs
├── usage.rs
├── processing_job.rs
├── task_status.rs
├── status_metrics.rs
└── intent.rs
```

关键状态：

```rust
pub struct RuntimeModel {
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub workspace: WorkspaceState,
    pub usage: UsageSummary,
    pub live_tps: Option<f64>,
    pub task_status: TaskStatusSnapshot,
    pub processing_jobs: Vec<ProcessingJob>,
}

pub struct WorkspaceState {
    pub path_base: Option<String>,
    pub working_root: Option<String>,
    pub branch: Option<String>,
    pub kind: WorktreeKind,
}
```

### 新增 SessionModel

建议新增：

```text
apps/cli/src/tui/model/session/
├── mod.rs
├── model.rs
├── intent.rs
├── change.rs
├── metadata.rs
└── resume.rs
```

关键状态：

```rust
pub struct SessionModel {
    pub current_session_id: Option<String>,
    pub dirty: bool,
    pub message_count: usize,
    pub resume_candidates: Vec<SessionResumeCandidate>,
    pub save_status: SessionSaveStatus,
}
```

SessionModel 只保存 TUI 所需投影，不替代底层 session store。

### RuntimeIntent / SessionIntent

建议：

```rust
RuntimeIntent::SetProviderModel { provider, model_id }
RuntimeIntent::WorkspaceSnapshotReceived { path_base, working_root, branch, kind }
RuntimeIntent::RecordUsage { input_tokens, output_tokens, cost_usd }
RuntimeIntent::RecordLiveTps { tps }
RuntimeIntent::SetTaskStatus { total, completed, in_progress }
RuntimeIntent::StartProcessingJob { id, chat_id }
RuntimeIntent::FinishProcessingJob { id, result }
```

```rust
SessionIntent::SetCurrentSession { id }
SessionIntent::MarkDirty
SessionIntent::MessagesSynced { message_count }
SessionIntent::SaveStarted
SessionIntent::SaveFinished
SessionIntent::ResumeCandidatesLoaded { candidates }
```

### StatusViewAssembler

`StatusViewAssembler` 应只读：

- RuntimeModel。
- SessionModel。
- DiagnosticModel。
- 少量 ViewState。

输出：

- left：model/provider/workspace。
- center：processing/diagnostic。
- right：tokens/tps/session/task summary。
- severity：来自 DiagnosticModel。

### Effect 扩展

建议新增：

```rust
Effect::RefreshWorkspaceStatus { path: PathBuf }
Effect::SaveSession { session_id: Option<String> }
Effect::LoadResumeCandidates
Effect::FetchTaskStatus
```

执行结果：

```rust
EffectResult::WorkspaceStatusRefreshed { ... }
EffectResult::SessionSaved { ... }
EffectResult::ResumeCandidatesLoaded { ... }
EffectResult::TaskStatusFetched { ... }
```

## 实施步骤

### Step 1：SessionModel 建模与测试

新增 `model/session`，测试覆盖：

1. 设置当前 session。
2. messages sync 后 dirty 状态变化。
3. save started/finished。
4. resume candidate 空列表边界。
5. save failed 错误状态。

### Step 2：RuntimeModel 扩展

迁移 usage/tps/provider/workspace/task status/processing job 字段。

测试覆盖：

- usage 累加。
- live tps 更新。
- workspace snapshot 替换。
- processing job start/finish。

### Step 3：StatusViewAssembler 扩展

让 status assembler 完整消费 RuntimeModel + SessionModel + DiagnosticModel。

验证 status 文本兼容现有快照测试。

### Step 4：workspace git 检测迁到 Effect

把 `git_branch_for` / `worktree_kind_for` 移出 update/model，变为 effect executor 或 runtime adapter。

要求：

- model/update 只接收检测结果。
- 失败时写入 DiagnosticModel，不 panic。

### Step 5：session lifecycle 接入 Effect

逐步替换：

- save current session。
- load resume candidates。
- processing job start/finish。

### Step 6：StatusBar 改为纯渲染

新增或收敛：

```rust
StatusBar::render_view_model(&StatusLineViewModel)
```

旧 setter 保留到 M11，但新 update 不再调用。

### Step 7：守卫

新增 guard：

- 禁止 `core/update` 调用 `status_bar.set_*`。
- 禁止 `model/runtime` / `model/session` 使用 `Command::new` 或文件 IO。
- 禁止 `display/status_bar*` 依赖 RuntimeModel 以外的业务状态可变引用。

## 验收标准

1. **MUST** provider/model/workspace/usage/tps/task status 事实状态来自 RuntimeModel。
2. **MUST** session id/dirty/save/resume 投影来自 SessionModel。
3. **MUST** StatusBar 只消费 StatusLineViewModel。
4. **MUST** workspace git 检测通过 Effect 回流。
5. **MUST** `core/update` 不再直接调用 status_bar setter。
6. **MUST** 通过：

```text
git diff --check
.agents/hooks/check-architecture-guards.sh
cargo test -p cli
cargo check -p cli
```

## 风险与回滚

### 风险

- 状态栏显示碎片多，容易遗漏 token/tps/session 字段。
- workspace/worktree 状态涉及 worktree 工具语义，需避免 #69 回归。
- session lifecycle 与 processing 交叉，替换顺序不当会影响保存。

### 回滚策略

- RuntimeModel、SessionModel 先作为 mirror 状态接入。
- status_bar setter 保留到 M11。
- workspace Effect 单独提交，失败可回退。
