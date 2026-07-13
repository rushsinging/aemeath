# TUI · 视图层设计（ViewAssembler / ViewModel / ViewState / Render）

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#798（S2）
> 本文深入视图层细节——10 种 block 类型、ViewAssembler 组装规则、ViewState 状态机、三层缓存策略、Render widget 设计。总览见 [01-architecture-and-dataflow.md](01-architecture-and-dataflow.md) §7。

## 1. 定位

视图层是 TEA 管线的第⑤-⑦层：

```
④ Model → ⑤ ViewAssembler → ⑥ ViewModel → ⑦ Render → ratatui Buffer
```

- **ViewAssembler**：纯函数，读 Model + ViewState → 产出 ViewModel
- **ViewModel**：纯数据结构，无 ratatui 依赖
- **ViewState**：可变状态（scroll/collapse/selection/animation），不属于 Model
- **Render**：读 ViewModel + ViewState + Cache → 写 ratatui Buffer

> **设计文档说 8 种 block，代码实际 10 种**——本文以代码为准，列出全部 10 种。

## 2. 10 种 Block 类型

### 2.1 完整列表

OutputViewModel 的核心是 `roots: Vec<OutputBlockView>`——一个块树。每个块是以下 10 种之一：

| # | Block 类型 | 共享结构 | 来源 Model | 说明 |
|---|---|---|---|---|
| 1 | `UserMessage` | TextBlockView | ChatTurn.user_text | 用户输入文本 |
| 2 | `AssistantMessage` | TextBlockView | ChatTurn.assistant_text | LLM 回复文本 |
| 3 | `ThinkingMessage` | TextBlockView | ChatTurn.thinking | LLM reasoning 文本 |
| 4 | `SystemNotice` | TextBlockView | ChatTurn.system_message | 系统消息（compact 通知等） |
| 5 | `DiagnosticNotice` | TextBlockView | ChatTurn.diagnostic | 错误/警告/提示 |
| 6 | `ToolCall` | ToolCallBlockView | ChatTurn.tool_call | 工具调用（含子块） |
| 7 | `ToolResult` | ToolResultBlockView | ChatTurn.tool_result | 工具结果（嵌入或独立） |
| 8 | `HookNotice` | TextBlockView | ChatTurn.hook_notice | Hook 执行通知 |
| 9 | `ModelStreamPlaceholder` | TextBlockView | ConversationModel 全局占位 | 流式输出占位（"…" 动画） |
| 10 | `AskUserBatch` | AskUserBlockView | ConversationModel.ask_user | AskUser 问题批量弹窗 |

### 2.2 结构体定义

5 种文本类共享 `TextBlockView`：

```rust
struct TextBlockView {
    block_id: BlockId,
    text: String,
    plain: String,                     // 纯文本（用于选区复制）
    spans: Vec<LineSpan>,              // ratatui Spans（不含 ratatui 类型，用自有映射）
    style_kind: BlockStyleKind,        // 颜色/样式标签
}
```

```rust
struct ToolCallBlockView {
    block_id: BlockId,
    tool_name: String,
    args_summary: String,
    status: ToolCallStatus,            // Running / Success / Error
    collapsed: bool,                   // 折叠状态
    workspace_root: Option<PathBuf>,   // 纳入 Hash——worktree 切换失效缓存
    children: Vec<OutputBlockView>,    // 嵌入的 ToolResult 子块
}

struct ToolResultBlockView {
    block_id: BlockId,
    tool_name: String,
    result_text: String,
    data: serde_json::Value,           // Edit diff 等结构化数据（不纳入 Hash）
    is_error: bool,
    style_kind: BlockStyleKind,
}

struct AskUserBlockView {
    block_id: BlockId,
    questions: Vec<AskUserQuestionView>,
}
```

### 2.3 cache_version()

每个 block 实现了 `cache_version()` 返回版本号，参与 BlockCache key：

| Block 类型 | cache_version 来源 | 失效条件 |
|---|---|---|
| UserMessage | 内容 hash | 用户消息不变 → 永不失效 |
| AssistantMessage | 内容 hash | 流式追加 → 每次追加失效 |
| ThinkingMessage | 内容 hash | 同上 |
| SystemNotice | 内容 hash | 系统消息不变 → 永不失效 |
| DiagnosticNotice | 内容 hash | 错误状态变化 |
| ToolCall | 状态 hash（name + args + status + collapsed + workspace_root） | 状态变化 / worktree 切换 / 折叠切换 |
| ToolResult | result_text hash（data 不纳入） | 结果文本变化 |
| HookNotice | 内容 hash | Hook 通知不变 → 永不失效 |
| ModelStreamPlaceholder | 固定版本 + animation_frame | 每个 blink 周期失效 |
| AskUserBatch | 问题列表 hash | 问题变化 / 光标移动 |

### 2.4 嵌套规则

```rust
// view_model/nesting.rs
// 允许的父子关系：
// ToolCall → { AssistantMessage, DiagnosticNotice, SystemNotice, ToolResult }
// 其他 block 不允许有子块
const MAX_BLOCK_DEPTH: usize = 3;
```

- 只有 `ToolCall` 可以有子块——嵌入的 `ToolResult` 作为子块
- `push_child_checked` 只在 depth=1 使用（目前嵌套只一层）
- 独立 `ToolResult`（无对应 ToolCall）作为顶层块渲染

## 3. ViewAssembler

### 3.1 四个 Assembler

| Assembler | 输入 | 产出 | dirty bit |
|---|---|---|---|
| OutputViewAssembler | ConversationModel + OutputViewState | OutputViewModel | `dirty.output` |
| StatusViewAssembler | ConversationModel.runtime + SessionModel | StatusLineViewModel + LiveStatusViewModel | `dirty.status` |
| InputViewAssembler | InputModel | InputAreaViewModel | `dirty.input` |
| DialogViewAssembler | DiagnosticModel.active_prompt | DialogViewModel | `dirty.dialog` |

### 3.2 OutputViewAssembler 组装流程

```
ConversationModel.timeline.items()
  │
  ├─ 遍历 OutputTimelineItem
  │   ├─ UserText → TextBlockView (UserMessage)
  │   ├─ AssistantText → TextBlockView (AssistantMessage)
  │   ├─ Thinking → TextBlockView (ThinkingMessage)
  │   ├─ ToolCallStarted → ToolCallBlockView (Running)
  │   │   └─ 嵌入的 ToolResult 作为 children
  │   ├─ ToolResult（非嵌入）→ ToolResultBlockView（顶层）
  │   ├─ HookExecuted → TextBlockView (HookNotice)
  │   ├─ SystemMessage → TextBlockView (SystemNotice)
  │   ├─ Diagnostic → TextBlockView (DiagnosticNotice)
  │   ├─ QueuedUserMessage → 跳过（spinner banner 替代）
  │   ├─ AgentProgress → DiagnosticNotice（防御性，A4.2 回归守卫）
  │   └─ ModelStreamPlaceholder → TextBlockView (placeholder)
  │
  ├─ 组装 OutputBlockView 树（按嵌套规则）
  │
  └─ 产出 OutputViewModel { roots: Vec<OutputBlockView>, follow_tail: bool }
```

> **已知 gap**：`follow_tail_hint` 字段始终为 `true` 且无消费方——死字段。

### 3.3 OutputViewCache memo

```rust
fn refresh_output_document_from_model(&mut self, model: &ConversationModel, workspace_root: &Path) {
    let revision = model.revision();
    let key = (revision, workspace_root.clone());

    // memo：revision + workspace_root 不变时跳过全量 assemble
    if self.output_cache_key == Some(key) {
        return;
    }

    let view_model = self.output_assembler.assemble_from_conversation(model, workspace_root);
    self.output_view_model = view_model;
    self.output_cache_key = Some(key);
}
```

- **revision** 是 ConversationModel 的单调递增版本号——每次 `apply(intent)` 产生 Change 时递增
- **workspace_root** 纳入 key——worktree 切换时强制全量重建
- 这是"大会话伪卡死"问题的根因修复——避免每帧全量遍历 chats

### 3.4 StatusViewAssembler

产出两个 ViewModel：

```rust
struct StatusLineViewModel {
    // Runtime 行：model_name / input_tokens / output_tokens / tps / ctx% / api_name
    runtime_row: Vec<StatusBarSegment>,
    // Context 行：cwd / git_branch / permission_mode / session_id
    context_row: Vec<StatusBarSegment>,
}

struct LiveStatusViewModel {
    spinner: Option<SpinnerViewModel>,    // verb + phase
    task_lines: Vec<TaskLineViewModel>,   // 当前任务进度
    compact_progress: Option<CompactProgressView>,
}
```

## 4. ViewState 状态机

### 4.1 OutputViewState

```rust
struct OutputViewState {
    // 滚动
    scroll_offset: usize,               // 当前顶部行
    follow_tail: bool,                  // 是否跟随尾部
    max_scroll: usize,                  // 最大可滚动行

    // 选区
    selection_start: Option<(usize, usize)>,  // (line, col)
    selection_end: Option<(usize, usize)>,
    selection_active: bool,

    // 折叠
    collapsed_blocks: HashSet<BlockId>,

    // 缓存
    last_total_lines: usize,            // force_repaint 判断
    last_block_count: usize,
}
```

#### 滚动状态机

```
用户按键            → 状态变化
─────────────────────────────────────
PageUp              → scroll_offset -= page_height
PageDown            → scroll_offset += page_height; follow_tail = false
Home                → scroll_offset = 0; follow_tail = false
End                 → scroll_offset = max_scroll; follow_tail = true
ArrowUp             → scroll_offset -= 1; follow_tail = false
ArrowDown           → scroll_offset += 1（未到尾）；follow_tail = true（到尾）
新内容追加           → if follow_tail { scroll_offset = max_scroll }
窗口 Resize         → max_scroll 重算
```

#### 选区状态机

```
鼠标按下             → selection_start = hit_position; selection_active = true
鼠标拖动             → selection_end = current_position
鼠标释放             → selection_active = false（选区保留）
复制快捷键           → 提取选区文本 → clipboard
Esc / 新输入         → 清除选区
```

### 4.2 SpinnerAnim

```rust
struct SpinnerAnim {
    frame: usize,                       // 当前帧（0-10，11 帧呼吸周期）
    phase: SpinnerPhase,                // Idle / Thinking / Working / Compact
    phase_frame: usize,                 // phase 级帧计数
    verb: String,                       // "Thinking" / "Reading" / "Editing" 等
}
```

- 90ms tick 推进 `frame`，到达 11 后归零
- `phase` 从 `model.conversation.runtime.spinner` 读取（单一来源目标态）
- `verb` 从 `RuntimeState` 读取
- `phase_frame` 独立计数——phase 切换时归零

> **已知问题**：spinner 状态三处同步（model + view_state.spinner + view_state.animation.spinner_frame）。目标态统一为 model 单一来源（见 [01-architecture-and-dataflow.md](01-architecture-and-dataflow.md) §10.4）。

### 4.3 ViewModelDirty bitfield

```rust
struct ViewModelDirty {
    output: bool,
    status: bool,
    input: bool,
    dialog: bool,
}

impl ViewModelDirty {
    fn dirty_from_model_changes(changes: &[ModelChange]) -> Self {
        // 根据 ModelChange 变体推导哪些 view 需要重算
        // 如 ConversationChange::ChatStarted → output + status dirty
        // 如 InputChange::BufferModified → input dirty
    }
}
```

- 每帧 `flush_dirty_view_models`：只重算 dirty 标记的 Assembler
- streaming chunk 通常只触发 `status` dirty（spinner/tps 变化），`output` 缓存到 revision 变化

## 5. 三层缓存策略

### 5.1 缓存总览

| 层 | 位置 | Key | 失效条件 | 用途 |
|---|---|---|---|---|
| **OutputViewCache memo** | App | `(revision, workspace_root)` | revision 变化或 worktree 切换 | 跳过全量 assemble |
| **BlockCache** | render/output | `(version, text_width)` | block_version 或 text_width 变化 | 跳过 per-block 行渲染 |
| **GuttedCache** | document_renderer | `(block_version, text_width, depth, marker_frame)` | 上述 + depth + 动画帧 | 跳过 per-(block,depth) gutter 布局 |
| **force_repaint** | OutputArea | `(last_total_lines, last_block_count)` | block 数变化或行数减少 | 终端 clear + 全量重绘 |

### 5.2 BlockCache

```rust
struct BlockCache {
    blocks: HashMap<CacheKey, Rc<Vec<RenderedLine>>>,
}

struct CacheKey {
    version: u64,               // block.cache_version()
    text_width: u16,            // gutter-deducted width
}
```

- **key 不含 depth**——同一 block 在不同 depth 的文本内容相同（gutter 宽度在 GuttedCache 层处理）
- `RenderedLine` 用 `Rc<Vec<...>>`——cheap clone
- **retain(live_set)**：每次 render 后，删除不在当前 OutputViewModel 中的 block——唯一的内存回收机制
- **无 LRU / 容量上限**——如果 block 持续增长（大量 AskUser 批次），缓存无界

### 5.3 GuttedCache

```rust
struct GuttedCache {
    entries: HashMap<GuttedKey, Rc<RenderedBlock>>,
}

struct GuttedKey {
    block_version: u64,
    text_width: u16,
    depth: usize,
    marker_frame: Option<u64>,      // Running 状态 blink 周期 / Thinking 动画帧
}
```

- 在 BlockCache 基础上增加 **depth**（gutter 宽度 = `outer - depth*2 - 2`）和 **marker_frame**（Running 状态每 blink 周期失效）
- 静态 block（非 Running/Thinking）`marker_frame = None` → 永久缓存（只要 block_version 不变）
- `retain(live_set)` 与 BlockCache 保持同步

### 5.4 force_repaint 决策

```rust
fn should_force_repaint(&self, total_lines: usize, block_count: usize) -> bool {
    // 首帧：usize::MAX 哨兵 → 总是 true
    if self.last_repaint_total == usize::MAX { return true; }

    // block 数变化 → 清屏（避免残留）
    if block_count != self.last_block_count { return true; }

    // 行数减少 → 清屏（向上滚动 / compact 后内容变少）
    if total_lines < self.last_repaint_total { return true; }

    // 流式追加（行数增长）和滚动 → 不清屏（避免闪烁）
    false
}
```

> **已知 workaround**：`#520` force-repaint 依赖 ratatui `terminal.clear()` 的副作用——ratatui 0.30 改进 diff 渲染后可移除。

## 6. Render 层

### 6.1 渲染管线

```
每帧 draw(terminal):
  │
  ├─ flush_dirty_view_models（重算脏的 ViewModel）
  │
  ├─ refresh_live_status_from_model（同步 spinner verb/phase）
  │
  ├─ refresh_output_scroll_from_view_state（更新 max_scroll / follow_tail）
  │
  ├─ should_force_repaint → optional terminal.clear()
  │
  └─ terminal.draw(|frame| {
       │
       ├─ 计算布局：output_area / input_area / suggestions / status_bar
       │
       ├─ OutputDocumentRenderer::render_model_document(
       │     view_model, content_width, fallback, animation_frame
       │   ) → RenderedDocument
       │   ├─ 遍历 OutputBlockView 树
       │   ├─ BlockCache 查/写（per-block 行渲染）
       │   └─ GuttedCache 查/写（per-(block,depth) gutter 布局）
       │
       ├─ output_area.replace_document(doc)
       ├─ output_area.render(area, buffer, view_state, live_status)
       │   ├─ 构建 screen_line_map（逻辑行 → 物理行映射）
       │   ├─ 绘制 scrollbar
       │   ├─ 绘制选区高亮（selection_overlay）
       │   └─ 绘制 spinner 行
       │
       ├─ status_bar.render(area, buffer, selection, status_vm)
       │   └─ 2 行：Runtime 行 + Context 行
       │
       └─ input_area.draw(area, buffer, input_vm, selection, suggestions)
           ├─ tui_textarea::TextArea 渲染
           └─ 手动绘制选区高亮
     })
```

### 6.2 RenderedDocument

```rust
struct RenderedDocument {
    blocks: Vec<RenderedBlock>,
    total_lines: usize,
}

struct RenderedBlock {
    block_id: BlockId,
    lines: Rc<Vec<RenderedLine>>,
    depth: usize,
}

struct RenderedLine {
    plain: String,                   // 纯文本（选区复制用）
    spans: Vec<LineSpan>,            // 渲染用 span
    gutter_cols: usize,              // gutter 宽度（选区跳过）
}
```

### 6.3 选区复制

```
选区 start/end → screen_line_map 反查 → 逻辑行范围
  → 遍历 RenderedLine.plain（跳过 gutter_cols）
  → join("\n") → clipboard
```

- `plain ⊆ spans.content`——每个可见字符都在 plain 中（不变量，测试覆盖）
- gutter 不纳入 plain（选区跳过 chrome）
- spinner 行不可选（`usize::MAX` 哨兵）

### 6.4 主题

Catppuccin Macchiato 调色板，编译时常量：

| 语义别名 | 颜色 | 用途 |
|---|---|---|
| `TEXT` | `#CAD3F5` | 默认文本 |
| `ACCENT` | `#ED8796` | 强调（工具名等） |
| `ERROR` | `#ED6973` | 错误 |
| `USER_BG` | `#363A4F` | 用户消息背景 |
| `DIFF_ADD_BG` | `#1A2421` | diff 新增行 |
| `SPINNER_BASE` | `#5B6078` | spinner 基色 |

> **TODO(theme)**：引入运行时主题后加 `theme` 字段到 `RenderCtx`，`theme_version` 纳入 `CacheKey`。当前编译时固定。

## 7. Effect 副作用

Effect 在 ViewAssembler 之后产生——当 Intent 需要 Runtime 介入时：

```
③ Coordinator → ④ Model.apply(Intent) → Change
                                         │
                                    ③ Coordinator 续
                                         │
                                    ⑧ Effect（从 Change 或 Intent 派生）
```

| Effect | 触发来源 | 执行 |
|---|---|---|
| `StartChat { text }` | ConversationIntent::StartChat | `chat.push_input_event()` |
| `AbortChat` | ConversationIntent::AbortChat | `chat.abort()` |
| `RequestRender` | 任何 Change | 标记 dirty → 下一帧 draw |
| `SpawnTask { task }` | Slash 命令 / Resume | `tokio::spawn` via `spawn_guarded` |

> **设计原则**：Effect 由 Coordinator 从 Intent/Change 派生，ViewAssembler 不产生 Effect——Assembler 是纯函数。

## 8. 架构门禁

### 8.1 视图层相关门禁

| # | 门禁 | 实现方式 | 现状 |
|---|---|---|---|
| 1 | adapter/view_assembler → render | arch test：禁止 import `render::*` | ✅ 已实现 |
| 4 | ViewAssembler boundary | arch test：允许读 model/view_state，禁止 IO/副作用/ratatui | ❌ 缺失 |
| 5 | ViewModel dependency | arch test：禁止依赖 model 可变类型和 ratatui | ❌ 缺失 |
| 3 | Render isolation | arch test：render/ 禁止状态变更逻辑 | ❌ 缺失 |

### 8.2 现有守卫

```rust
// architecture_tests.rs
fn test_adapter_and_view_assembler_production_do_not_depend_on_render_modules() {
    // adapter/ 和 view_assembler/ 的 .rs 文件（排除 #[cfg(test)]）
    // 禁止包含 "crate::tui::render::"
}
```

### 8.3 待补门禁

**ViewModel purity guard**：`view_model/` 禁止 import `ratatui::*` / `crossterm::*` / `tokio::*` / `crate::tui::model::*`。

**Render isolation guard**：`render/` 中的 widget 禁止可变状态（只读 ViewModel + ViewState + Cache）。

**ViewState → render 依赖修复**：`view_state/status.rs` 当前导入 `render::status::StatusBarRow`——方向反了。目标态：`StatusBarRow` 在 view_state 或 view_model 中定义。

## 9. 现状缺口与迁移动作

| 目标 | 现状 | 迁移动作 |
|---|---|---|
| spinner 状态统一 | ⚠️ 三处同步 | model 单一来源，view_state 只存动画帧 |
| `follow_tail_hint` 死字段 | ⚠️ 始终 true，无消费方 | 删除 |
| `collapsed` 无 setter | ⚠️ 折叠 UI 未实现 | 添加 Intent + ViewState 追踪 |
| BlockCache 无容量上限 | ⚠️ 无界增长 | 添加 LRU 或容量上限 |
| `view_state → render` 依赖 | ⚠️ StatusBarRow 方向反了 | 移到 view_state/view_model |
| `InputRenderModel` 重复 | ⚠️ 与 InputAreaViewModel 字段重复 | 删除，直接消费 InputAreaViewModel |
| force_repaint workaround | ⚠️ #520 依赖 terminal.clear() | ratatui 0.30 diff 渲染改进后移除 |
| 运行时主题 | ❌ 编译时固定 | `theme` 纳入 RenderCtx + CacheKey |
| 架构门禁 4/5 | ❌ 缺失 | 补 arch test |
| Intent 风格统一 | ⚠️ Conversation 用 trait dispatch，其他用 enum | 统一为 enum match |
| `QueuedUserMessage` 被丢弃 | ⚠️ 产出但不渲染 | 删除 timeline item 或恢复渲染 |
| `ToolResult data` 不纳入 Hash | ⚠️ 同文本不同 data 不失效 | 按需——Edit 场景 data 每帧读，可接受 |

## 10. 相关文档

- TUI 架构总览（八层管线 + 三条信息流）：[01-architecture-and-dataflow.md](01-architecture-and-dataflow.md)
- TUI Model 层：[02-model.md](02-model.md)
- TUI 事件流与 ACL：[03-event-flow-and-acl.md](03-event-flow-and-acl.md)
- 原始 TUI 设计（历史归档）：[../../04-tui-design.md](../../04-tui-design.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：10 种 block 类型、ViewAssembler 组装、ViewState 状态机、三层缓存、Render 管线、架构门禁、死代码清单 | #798 |
