# Feature 50: TUI 按 TEA + DDD 架构重构

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 用 TEA（The Elm Architecture）分层重构 TUI，解决 96 文件 / 12457 行的臃肿问题。

**Architecture:**  
- **TEA 层**：`State` 展示状态、`Msg` 事件、`Update` 纯状态转换 `(State, Msg) -> (State, Cmd)`、`Cmd` 副作用描述、`Render` 纯渲染 `State -> Frame`  
- **DDD 对接**：TUI 是 CLI Adapter，不定义 Domain Model。`State` 是对 `runtime::api` 领域模型的视图投影 + 纯 UI 状态。`Cmd` 委托给 `runtime::api` 执行。  
- **不变约束**：保持现有业务逻辑和测试行为不变，不引入新依赖，不改 CLI 外部接口。

**Tech Stack:** Rust、ratatui、crossterm、tokio。所有改动在 `apps/cli/src/tui/` 内。

---

## DDD 分层对接

```
crates/runtime/               Domain + Application
  ├── api.rs                  ← DDD 公开契约（DTO、Error、Port）
  ├── chat/app.rs             ← Application Service
  └── chat/domain/            ← Domain Model（Aggregate Root）
          │
          │ runtime::api::*
          ▼
apps/cli/src/tui/             Adapter（展示层）
  ├── state/                  ← 展示状态（runtime::api DTO 的视图投影 + 纯 UI 状态）
  │   ├── chat.rs             ← 投影 messages、tool_calls、tokens 等
  │   ├── input.rs            ← 纯 UI 输入队列、粘贴追踪
  │   ├── session.rs          ← 投影 session_id、cwd、configs、memory_config 等
  │   └── layout.rs           ← 纯 UI 布局 rects、dialog、终端尺寸
  │
  ├── msg.rs                  ← Msg + Cmd 枚举
  ├── update/                 ← 状态转换（纯函数，不调 runtime::api）
  ├── render/                 ← 渲染（State + Components → ratatui Frame）
  ├── widgets/                ← 无状态 UI 组件（markdown、diff、tool_display）
  ├── cmd_exec.rs             ← Cmd 执行器（持有 runtime::api 引用，执行 Cmd 副作用）
  └── run_loop.rs             ← 编排循环：recv msg → update → cmd_exec → render
```

**关键区别**：
- `runtime::chat::domain` 的 Model 是 **领域模型**（Aggregate、Entity、Value Object）
- `tui::state` 的 State 是 **展示状态**（对 DTO 的视图投影，不含业务逻辑）
- `tui::render` 不做 IO 和业务逻辑，只把 State 画到 Frame 上

---

## 当前问题诊断

### 1. `App` struct 64 字段不可管理（`app/mod.rs:33-89`）

```
App 字段分类：
├── 聊天投影: messages, pending_images, tool_call_active, active_tool_call_ids,
│             turn_count, pending_reflection, is_processing,
│             total_{input,output}_tokens, total_api_calls, last_input_tokens
├── 输入 UI:   input_queue, just_pasted, last_click
├── 会话投影:  session_id, cwd, session_created_at, skills, cached_sessions,
│              workspace_context, context_size, models_config,
│              current_model_display, system_prompt_text
├── 布局 UI:   output_area_rect, input_area_rect, status_bar_rect,
│              last_terminal_size, active_dialog, dialog_model_keys,
│              ask_user_state, ask_user_reply_tx, should_exit, last_ctrlc
├── 组件引用:  output_area, input_area, status_bar
└── 基础设施:  client, hook_runner, task_store, session_reminders,
               memory_config, json_logger
```

单一 struct 混合了 **4 类关注点**，每次新增交互都要加字段。

### 2. `output_area/` 25 文件职责混杂

| 职责 | 文件 | 行数 |
|---|---|---|
| 核心渲染 | `render.rs` `render_blocks.rs` `render_spans.rs` `render_status.rs` | ~600 |
| 内容管理 | `content.rs` `types.rs` `mod.rs` | ~350 |
| 选择 | `selection.rs` `selection_render.rs` `selection_tests.rs` | ~250 |
| 滚动/缓存 | `scroll.rs` `rendered_cache.rs` `rendered_lines.rs` `resize.rs` | ~900 |
| 流式 | `streaming.rs` `spinner.rs` `display.rs` | ~300 |
| Markdown | `markdown.rs` `markdown/` | ~500 |
| 工具展示 | `tool_display.rs` `tool_display/` `tool_display_agent_tests.rs` | ~400 |
| Diff | `diff.rs` | ~100 |

渲染逻辑、状态缓存、流处理混杂。`rendered_lines.rs`（398 行）和 `rendered_cache.rs`（316 行）分别用不同策略做同一件事——缓存。

### 3. `update/key.rs` 320 行超长（超标 25%）

8 个 update 子文件对 `App` 全部字段有写权限，无封装。key.rs 是键盘事件路由 + 编辑逻辑 + 提交逻辑的杂合体。

### 4. 无统一 Render 入口

渲染分散在 `output_area/render.rs`、`input_area/render.rs`、`status_bar/`、`dialog/`、`task_list.rs`，各自直接操作 `Frame`，无法在入口层统一编排布局。

---

## 目标架构

```
tui/
├── state/                    # Adapter 展示状态（纯数据 struct）
│   ├── mod.rs                # TuiState 根（组合 Chat/Input/Session/Layout）
│   ├── chat.rs               # ChatState: messages, tool_calls, turns, reflection, tokens
│   ├── input.rs              # InputState: queue, paste state, click tracking
│   ├── session.rs            # SessionState: id, cwd, skills, configs, workspace, memory_config
│   └── layout.rs             # UiLayout: rects, dialog, ask_user, terminal size, should_exit
│
├── msg.rs                    # Msg 事件 + Cmd 副作用枚举
│
├── update/                   # 纯状态转换（不调 runtime::api，不操作 IO）
│   ├── mod.rs                # update() 入口，Msg → (State, Cmd)
│   ├── key.rs                # 键盘分支（< 250 行）
│   ├── paste.rs              # 粘贴
│   ├── mouse.rs              # 鼠标
│   ├── resize.rs             # resize
│   ├── ui_event.rs           # 异步 UI 事件到达
│   ├── enter.rs              # 提交处理
│   ├── done.rs               # 结束处理
│   └── ask_user.rs           # AskUserQuestion 交互（Feature 49 逻辑）
│
├── render/                   # 纯渲染（State + 组件 → ratatui Frame，不做 IO）
│   ├── mod.rs                # render() 入口（布局编排）
│   ├── chat/                 # 聊天区渲染
│   │   ├── mod.rs            # ChatRender
│   │   ├── content.rs        # 行内容管理
│   │   ├── blocks.rs         # 块渲染
│   │   ├── scroll.rs         # 滚动 + 缓存（合并 rendered_{lines,cache}）
│   │   ├── selection.rs      # 文本选择
│   │   └── streaming.rs      # 流式更新 + spinner
│   ├── input.rs              # 输入区（InputArea 渲染逻辑收束）
│   ├── status_bar.rs         # 状态栏
│   ├── dialog.rs             # 对话框
│   └── task_list.rs          # 任务列表
│
├── widgets/                  # 无状态 UI 组件（不依赖 App / State，只依赖 Frame + 数据）
│   ├── markdown.rs
│   ├── diff.rs
│   ├── tool_display.rs
│   └── spinner.rs
│
├── cmd_exec.rs               # Cmd 执行器（持有 runtime::api 引用：client, hook_runner, task_store, session_reminders, json_logger）
│
├── run_loop.rs               # 编排循环: recv msg → update → cmd_exec → render
│
└── app.rs                    # App 组合：TuiState + Components + CmdExecutor
```

**文件数**：96 → 约 55（减少 42%）

---

## 实施阶段

### Phase 1: State 拆分（不移动文件，只改 App struct）

**目标**：把 `App` 的 64 个字段收束为 4 个 `State` struct + 组件引用 + 基础设施引用。

**步骤：**

1.1 创建 `state/chat.rs`，定义 `ChatState`：
```rust
pub struct ChatState {
    pub messages: Vec<Message>,
    pub pending_images: Vec<ProcessedImage>,
    pub tool_call_active: bool,
    pub active_tool_call_ids: HashSet<String>,
    pub turn_count: usize,
    pub pending_reflection: Option<ReflectionOutput>,
    pub is_processing: bool,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_api_calls: u64,
    pub last_input_tokens: u64,
}
```

1.2 创建 `state/input.rs`，定义 `InputState`：
```rust
pub struct InputState {
    pub input_queue: VecDeque<String>,
    pub just_pasted: bool,
    pub last_click: Option<(Instant, u16, u16)>,
}
```

1.3 创建 `state/session.rs`，定义 `SessionState`：
```rust
pub struct SessionState {
    pub session_id: String,
    pub cwd: PathBuf,
    pub session_created_at: Option<String>,
    pub skills: HashMap<String, Skill>,
    pub cached_sessions: Vec<(String, String)>,
    pub workspace_context: Option<WorkspaceContext>,
    pub context_size: usize,
    pub models_config: ModelsConfig,
    pub current_model_display: String,
    pub system_prompt_text: String,
    pub memory_config: MemoryConfig,
}
```

1.4 创建 `state/layout.rs`，定义 `UiLayout`：
```rust
pub struct UiLayout {
    pub output_area_rect: Rect,
    pub input_area_rect: Rect,
    pub status_bar_rect: Rect,
    pub last_terminal_size: Option<TerminalSize>,
    pub active_dialog: Option<Dialog>,
    pub dialog_model_keys: Vec<String>,
    pub ask_user_state: Option<AskUserState>,
    pub ask_user_reply_tx: Option<oneshot::Sender<String>>,
    pub should_exit: bool,
    pub last_ctrlc: Option<Instant>,
}
```

1.5 `App` 简化为：
```rust
pub struct App {
    pub chat: ChatState,
    pub input_state: InputState,
    pub session: SessionState,
    pub layout: UiLayout,
    pub output_area: OutputArea,
    pub input_area: InputArea,
    pub status_bar: StatusBar,
    pub client: Option<Arc<LlmClient>>,
    pub hook_runner: HookRunner,
    pub task_store: Option<Arc<TaskStore>>,
    pub session_reminders: Arc<Mutex<SessionReminders>>,
    pub memory_config: MemoryConfig,
    pub json_logger: Option<Arc<Mutex<JsonLogger>>>,
}
```
字段：64 → 14

1.6 机械替换所有 `self.xxx` 引用：
- 聊天投影 `self.messages` → `self.chat.messages`
- UI 布局 `self.should_exit` → `self.layout.should_exit`
- 会话 `self.cwd` → `self.session.cwd`
- 输入 `self.input_queue` → `self.input_state.input_queue`
- 注意 `self.input_area`（组件引用）保持不变，避免和 `self.input_state` 混淆

**验收**：`cargo check` + `cargo test -p cli` 通过，`app/mod.rs` < 80 行。

---

### Phase 2: Render 重组（建立纯渲染层）

**目标**：把 output_area 渲染分散的逻辑收束到 `render/chat/`，提取 `widgets/`。

**步骤：**

2.1 建立 `render/chat/` 子目录：
- `output_area/render.rs` + `render_blocks.rs` + `render_spans.rs` → `render/chat/blocks.rs`
- `output_area/content.rs` + `types.rs` → `render/chat/content.rs`
- `output_area/scroll.rs` + `rendered_cache.rs` + `rendered_lines.rs` → `render/chat/scroll.rs`
- `output_area/selection.rs` + `selection_render.rs` → `render/chat/selection.rs`
- `output_area/streaming.rs` + `spinner.rs` + `display.rs` → `render/chat/streaming.rs`

2.2 建立 `widgets/` 目录：
- `output_area/markdown.rs` + `markdown/` → `widgets/markdown.rs`
- `output_area/diff.rs` → `widgets/diff.rs`
- `output_area/tool_display.rs` + `tool_display/` → `widgets/tool_display.rs`

2.3 创建 `render/mod.rs`，定义 `fn render(app, frame)` 统一入口，编排区域渲染顺序。

2.4 测试文件随源文件迁移，路径不变。

**验收**：`cargo check` + `cargo test -p cli` 通过，`output_area/` 从 25 文件减至 ≤ 12 文件。

---

### Phase 3: Update 收束（消除超长文件）

**目标**：`update/key.rs` 从 320 行 → < 250 行。

**步骤：**

3.1 拆分 `update/key.rs`：
- 普通输入/编辑/Enter 逻辑 → 并入现有 `enter.rs`
- 特殊按键导航（Ctrl+C, Esc, Tab, 箭头）→ `key/navigation.rs`
- Slash 命令处理 → `key/slash.rs`

3.2 新增 `update/paste.rs` 和 `update/mouse.rs`，从 `update/mod.rs` 的 Paste/Mouse 分支抽出。

3.3 确保每个 update 子文件只访问对应的 State 子对象（如键盘分支只读 `layout` + `input_state`）。

**验收**：无单个 update 子文件超过 250 行。

---

### Phase 4: Cmd 执行器

**目标**：`run_loop.rs` + `processing.rs` 的副作用逻辑集中到 `cmd_exec.rs`。

**步骤：**

4.1 创建 `cmd_exec.rs`，定义 Cmd 执行函数：
- `execute_spawn_processing(ctx)` → 调用 `processing::process_input`
- `execute_save_session(msgs)` → 触发异步保存
- `execute_read_clipboard_image()` → 剪贴板读图
- `execute_process_image_file(path)` → 图片处理
- `execute_hook_notification(msg, kind)` → Hook 通知

4.2 `run_loop.rs` 简化为编排循环（< 200 行）：
```rust
loop {
    tokio::select! {
        msg = recv_event() => { /* terminal event */ },
        ui_event = ui_rx.recv() => { /* async event */ },
        _ = spinner.tick() => { /* timer */ },
    }
    let result = self.update(msg, &ui_tx, &active_cancel, &spawn_refs);
    cmd_exec::execute(result.cmd, ...).await;
    self.render(frame);
}
```

**验收**：`run_loop.rs` < 200 行，`processing.rs` < 270 行。

---

### Phase 5: Feature 49 确认归档

**目标**：确认 AskUserQuestion 在重构后一致，归档 Feature 49。

**步骤：**

5.1 确认 `AskUserState` 和 `BUILTIN_OPTION_*` 移至 `state/layout.rs` 后功能不变

5.2 确认 `update/ask_user.rs` 行为不变

5.3 将 Feature 49 从 `docs/feature/active.md` 移到 `docs/feature/archived/049-ask-user-builtin-options.md`

**验收**：AskUserQuestion 在 TUI 中交互正常（手动验证），active.md 无 Feature 49。

---

### Phase 6: 清理合规

**目标**：行数、import、可见性收束。

**步骤：**

6.1 删除死代码：`queued.rs`、未引用 re-export

6.2 确认全部文件 < 400 行

6.3 `pub` → `pub(crate)` 收束

6.4 `cargo fmt` + `cargo clippy` + `cargo test -p cli`

---

## 不变约束

| 约束 | 说明 |
|---|---|
| 不新增 crate | 所有改动在 `apps/cli/src/tui/` 内 |
| 不新增外部依赖 | Cargo.toml 不变 |
| CLI 入口不变 | `main.rs` → TUI 启动路径不变 |
| 测试语义不变 | 测试迁移路径跟随源文件，不修改断言 |
| Feature 49 不丢失 | `AskUserState` 随 `UiLayout` 保留 |
| 不与 runtime 命名冲突 | `State` 非 `Model`，`Render` 非 `View` |

---

## DDD 命名对照

| 项目 DDD 概念 | TUI 对应 | 关系 |
|---|---|---|
| `runtime::chat::domain::*`（Domain Model） | `tui::state::ChatState` | State 是 Domain DTO 的视图投影 |
| `runtime::api`（Application 公开契约） | `tui::cmd_exec.rs` | 通过 api 调用执行 Cmd |
| `crates/core::message::Message` | `tui::state::ChatState.messages` | 直接持有 Domain 类型引用 |
| `crates/core::config::*` | `tui::state::SessionState` | 配置值的展示副本 |
| 不存在 | `tui::render/` | Adapter 层渲染，DDD 无对应 |

---

## 执行顺序

```
Phase 1 (State 拆分) → Phase 2 (Render 重组) → Phase 3 (Update 收束)
                                                       ↓
Phase 4 (Cmd 执行器) ← Phase 5 (Feature 49 归档) ← Phase 6 (清理合规)
```

**并行机会**：Phase 4 和 Phase 5 互不依赖。

---

## 当前进展

- [ ] Phase 1: State 拆分
- [ ] Phase 2: Render 重组
- [ ] Phase 3: Update 收束
- [ ] Phase 4: Cmd 执行器
- [ ] Phase 5: Feature 49 确认归档
- [ ] Phase 6: 清理合规
