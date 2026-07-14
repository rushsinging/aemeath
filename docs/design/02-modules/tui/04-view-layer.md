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
- **ViewState**：可变的瞬时交互 / 渲染状态（scroll/collapse/selection/animation），**NEVER** 复制 Model 业务投影
- **Render**：读 ViewModel + ViewState + Cache → 写 ratatui Buffer

本文定义封闭的 10 种 Target block；新增种类 **MUST** 同步更新 assembler 穷尽匹配、cache version 与 render 测试。

## 2. 10 种 Block 类型

### 2.1 完整列表

OutputViewModel 的核心是 `roots: Vec<OutputBlockView>`——一个块树。每个块是以下 10 种之一：

| # | Block 类型 | 共享结构 | 来源 Model | 说明 |
|---|---|---|---|---|
| 1 | `UserMessage` | TextBlockView | `UserMessage` / `QueuedUserMessage` timeline item | 用户输入；queued 通过 delivery semantic 显式展示 |
| 2 | `AssistantMessage` | TextBlockView | `AssistantText` timeline item | LLM 回复文本 |
| 3 | `ThinkingMessage` | TextBlockView | `ThinkingText` timeline item | LLM reasoning 文本 |
| 4 | `SystemNotice` | TextBlockView | `SystemMessage` / owning ToolCall 下的 `AgentProgress` | 系统消息与嵌套 sub-agent 进度 |
| 5 | `DiagnosticNotice` | TextBlockView | `Error` timeline item | 错误/警告/提示 |
| 6 | `ToolCall` | ToolCallBlockView | `ToolCall` timeline item + `runs` 重叠投影 | 工具调用（含子块） |
| 7 | `ToolResult` | ToolResultBlockView | `ToolResult` timeline item + `runs` 重叠投影 | 工具结果（嵌入或独立） |
| 8 | `HookNotice` | TextBlockView | `HookNotice` timeline item | Hook 执行通知 |
| 9 | `ModelStreamPlaceholder` | TextBlockView | ConversationModel 只读 placeholder projection | 流式输出占位（"…" 动画） |
| 10 | `Interaction` | InteractionBlockView | `Interaction` timeline item | UserQuestions / ToolApproval / PlanApproval / HardPause typed 交互 |

### 2.2 结构体定义

5 种文本类共享 `TextBlockView`：

```rust
struct TextBlockView {
    block_id: BlockId,
    text: String,
    plain: String,                     // 纯文本（用于选区复制）
    spans: Vec<LineSpan>,              // TUI-owned span；Render 边界再映射为 ratatui 类型
    style_kind: BlockStyleKind,        // 颜色/样式标签
    delivery: Option<MessageDelivery>, // Submitted / Queued；非用户消息为 None
}
```

```rust
struct ToolCallBlockView {
    block_id: BlockId,
    tool_name: String,
    args_summary: String,
    status: ToolSemanticStatus,        // ViewModel-owned Running / Success / Error / Cancelled
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

struct InteractionBlockView {
    block_id: BlockId,
    request_id: UiInteractionRequestId,
    body: InteractionBodyView,
    phase: InteractionPhaseView,
}

enum InteractionBodyView {
    UserQuestions { questions: Vec<UserQuestionView>, current: usize },
    ToolApproval { title: String, detail: String, selected: Option<ApprovalDecisionView> },
    PlanApproval { title: String, detail: String, selected: Option<ApprovalDecisionView> },
    HardPause { reason: String, recent_actions: Vec<String>, continue_selected: bool },
}

enum InteractionPhaseView {
    Collecting,
    Confirming,
    Pending,       // ← ReplyPending + CancelPending 合并
    Replied,
    Cancelled,
    ReplyFailed { message: String },  // ← ReplyFailed 重命名以与 Model 一致
}
```

> **Model → View 映射**：`InteractionPhase::ReplyPending` 与 `CancelPending` 在 ViewModel 合并为 `Pending`（用户无需区分两种等待）。`ReplyFailed` 保持同名。

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
| ToolResult | 所有 display-affecting 字段 hash（result_text + data projection + style） | 文本、结构化 diff 或样式变化 |
| HookNotice | 内容 hash | Hook 通知不变 → 永不失效 |
| ModelStreamPlaceholder | 固定版本 + animation_frame | 每个 blink 周期失效 |
| Interaction | request id + body + draft + phase hash | 问题 / decision / diagnostic / 光标 / phase 变化 |

### 2.4 嵌套规则

```rust
// view_model/nesting.rs
// 允许的父子关系：
// ToolCall → { AssistantMessage, DiagnosticNotice, SystemNotice, ToolResult }
// 其他 block 不允许有子块
const MAX_BLOCK_DEPTH: usize = 3;
```

- 只有 `ToolCall` 可以有子块——嵌入的 `ToolResult` 作为子块
- `push_child_checked` 只在 depth=1 使用；Target 嵌套深度固定为一层
- 独立 `ToolResult`（无对应 ToolCall）作为顶层块渲染

## 3. ViewAssembler

### 3.1 四个 Assembler

| Assembler | 输入 | 产出 | dirty bit |
|---|---|---|---|
| OutputViewAssembler | ConversationModel 只读 projection + OutputViewState | OutputViewModel | `dirty.output` |
| StatusViewAssembler | ConversationModel.run_runtime() + SessionModel 只读 projection | StatusLineViewModel + LiveStatusViewModel | `dirty.status` |
| InputViewAssembler | InputModel 只读 projection | InputAreaViewModel | `dirty.input` |
| DialogViewAssembler | DiagnosticModel.active_prompt() | DialogViewModel | `dirty.dialog` |

### 3.2 OutputViewAssembler 组装流程

```
ConversationModel.timeline().items()
  │
  ├─ 遍历 OutputTimelineItem
  │   ├─ UserMessage → TextBlockView (UserMessage, delivery=Submitted)
  │   ├─ AssistantText → TextBlockView (AssistantMessage)
  │   ├─ ThinkingText → TextBlockView (ThinkingMessage)
  │   ├─ ToolCall → ToolCallBlockView
  │   │   └─ 嵌入的 ToolResult 作为 children
  │   ├─ ToolResult（非嵌入）→ ToolResultBlockView（顶层）
  │   ├─ HookNotice → TextBlockView (HookNotice)
  │   ├─ SystemMessage → TextBlockView (SystemNotice)
  │   ├─ Error → TextBlockView (DiagnosticNotice)
  │   ├─ QueuedUserMessage → UserMessage (delivery=Queued)
  │   ├─ AgentProgress → owning ToolCall 下的 SystemNotice child
  │   └─ Interaction → InteractionBlockView（穷尽四种 body）
  │
  ├─ ConversationModel.model_stream_placeholder() → TextBlockView (placeholder)
  │
  ├─ 组装 OutputBlockView 树（按嵌套规则）
  │
  └─ 产出 OutputViewModel { roots: Vec<OutputBlockView> }
```

`follow_tail` 只来自 `OutputViewState`；ViewModel **NEVER** 携带第二个 hint 或默认常量副本。

`AgentProgress` 找不到 owning ToolCall 时必须降级为顶层 `DiagnosticNotice` 并携带关联 ID，**NEVER** 静默丢弃。

### 3.3 OutputViewCache memo

```rust
fn refresh_output_document_from_model(&mut self, model: &ConversationModel, workspace_root: &Path, view_state: &OutputViewState) {
    let revision = model.revision();
    let collapsed_revision = view_state.collapsed_revision(); // collapse/expand 变化时自增
    let key = (revision, workspace_root.clone(), collapsed_revision);

    // memo：revision + workspace_root + collapsed_revision 不变时跳过全量 assemble
    if self.output_cache_key == Some(key) {
        return;
    }

    let view_model = self.output_assembler.assemble_from_conversation(model, workspace_root, view_state);
    self.output_view_model = view_model;
    self.output_cache_key = Some(key);
}
```

- **revision** 是 ConversationModel 的单调递增版本号——每次 `apply(intent)` 产生 Change 时递增
- **workspace_root** 纳入 key——worktree 切换时强制全量重建
- memo **MUST** 避免每帧全量遍历 timeline；revision 变化时仍以相同输入得到确定性 ViewModel

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
}
```

- 90ms tick 推进 `frame`，到达 11 后归零
- `SpinnerPhase` 与 verb 由 StatusViewAssembler 从 ConversationModel 只读 projection 派生并放入 `LiveStatusViewModel`
- ViewState 只持 animation frame；**NEVER** 持 phase、verb、run_active 或可见性副本

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
        // 如 ConversationChange::RunStarted → output + status dirty
        // 如 InputChange::BufferModified → input dirty
    }
}
```

- 每帧 `flush_dirty_view_models`：只重算 dirty 标记的 Assembler
- timeline 内容 Change 必须触发 `output` dirty；纯 spinner / tps Change 只触发 `status` dirty，dirty bit 由 Change 变体机械推导

## 5. 三层缓存策略

### 5.1 缓存总览

| 层 | 位置 | Key | 失效条件 | 用途 |
|---|---|---|---|---|
| **OutputViewCache memo** | App | `(revision, workspace_root, collapsed_revision)` | revision 变化、worktree 切换或 collapse/expand 切换 | 跳过全量 assemble |
| **BlockCache** | render/output | `(version, text_width)` | block_version 或 text_width 变化 | 跳过 per-block 行渲染 |
| **GuttedCache** | document_renderer | `(block_version, text_width, depth, marker_frame)` | 上述 + depth + 动画帧 | 跳过 per-(block,depth) gutter 布局 |
| **force_repaint** | OutputArea | `(last_total_lines, last_block_count)` | block 数变化或行数减少 | 终端 clear + 全量重绘 |

### 5.2 BlockCache

```rust
struct BlockCache {
    blocks: HashMap<CacheKey, Rc<Vec<RenderedLine>>>,
    lru: LruIndex<CacheKey>,
    max_entries: NonZeroUsize,
}

struct CacheKey {
    version: u64,               // block.cache_version()
    text_width: u16,            // gutter-deducted width
}
```

- **key 不含 depth**——同一 block 在不同 depth 的文本内容相同（gutter 宽度在 GuttedCache 层处理）
- `RenderedLine` 用 `Rc<Vec<...>>`——cheap clone
- **retain(live_set)**：每次 render 后优先删除不在 OutputViewModel 中的 block
- `max_entries` 是强制容量上限；live block 仍超过上限时按 LRU 淘汰屏外旧 entry，下一次需要时可确定性重建
- GuttedCache 使用同一容量策略；缓存淘汰 **NEVER** 改变 ViewModel 或用户可观察内容

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

`force_repaint` 是终端正确性兜底：只允许由上述纯决策函数触发，**NEVER** 成为业务状态或绕过 ViewModel 的刷新路径。

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

v0.1.0 的 Target 固定使用编译时 Catppuccin Macchiato palette，因此 cache key 不含 theme。若未来引入运行时主题，必须先扩展 RenderCtx 与 cache-version 契约，**NEVER** 直接读取全局配置。

## 7. Effect 副作用

Effect 是 Model Change 的副作用反馈分支，与 ViewAssembler 渲染分支并列：

```
③ Coordinator → ④ Model.apply(Intent) → Change
                                         │
                                    ③ Coordinator 续
                                         │
                                    ⑧ Effect（只从 Change 派生）
                                         │
                                         └─ runner → result Intent → root reducer
```

| Effect | 触发来源 | 执行 |
|---|---|---|
| `StartRun { text }` | `ConversationChange::RunStartRequested` | Runtime-owned `AgentClient` start method |
| `RequestRunCancellation { run_id }` | `ConversationChange::RunCancellationRequested` | Runtime cancel port；accepted 与 terminal 分离 |
| `SendInteractionReply { request_id, reply }` | `ConversationChange::InteractionReplyRequested` | Runtime-owned `AgentClient::reply_interaction` |
| `CancelInteraction { request_id }` | `ConversationChange::InteractionCancelRequested` | Runtime-owned `AgentClient::cancel_interaction` |
| `RequestRender` | 任何 Change | 标记 dirty → 下一帧 draw |

> **设计原则**：Effect 只由 Coordinator 从 Change 派生，ViewAssembler 不产生 Effect——Assembler 是纯函数。

## 8. 架构门禁

### 8.1 视图层 Target 门禁

> 以下编号是**视图层专属门禁**（V1–V8），与 [01-architecture-and-dataflow.md §9](01-architecture-and-dataflow.md) 和 [03-event-flow-and-acl.md §8](03-event-flow-and-acl.md) 的全局门禁（G1–G10）是独立体系。全局门禁覆盖 TUI 全树，视图层门禁只约束 view / render / assembler 子树。

| # | 门禁 | Target 证明 |
|---|---|---|
| 1 | Adapter / ViewAssembler isolation | `adapter/`、`view_assembler/` **NEVER** import `render::*` |
| 2 | ViewAssembler boundary | 只读 Model accessor + ViewState，禁止 I/O、副作用、ratatui 与 Model mutation API |
| 3 | ViewModel purity | `view_model/` **NEVER** import ratatui、crossterm、tokio、Model 或 SDK 类型 |
| 4 | Render isolation | `render/` 只读 ViewModel / ViewState / Cache，禁止 Model 引用与状态变更逻辑 |
| 5 | ViewState direction | `view_state/` **NEVER** import `render/`；共享 display enum 由 ViewModel / presentation contract 拥有 |
| 6 | Model write isolation | ViewAssembler / Render **NEVER** 获得 `&mut TuiModel`；Model mutation 调用点只有 root reducer |
| 7 | Cache boundedness | BlockCache / GuttedCache 容量测试证明 entry 数不超过配置上限，淘汰前后渲染等价 |
| 8 | Exhaustive presentation | 每个 OutputTimelineItem 都显式 assemble；queued / 四类 Interaction / progress **NEVER** 静默丢弃 |

每条门禁 **MUST** 有 architecture test 或 invariant test，并保留一次故意违规能失败的证据。实现差距与退役清单只在 [Migration Governance](../../03-engineering/migration-governance.md) O6 / TUI-7 维护。

## 9. Target 不变量与验收

1. ViewAssembler、ViewModel、ViewState、Render 的依赖只向右流动；共享类型 **NEVER** 由 Render 反向发布。
2. Input 只存在一个 `InputAreaViewModel` 展示契约，**NEVER** 再建字段重复的 render model。
3. collapse 由输入翻译更新 `OutputViewState.collapsed_blocks`，Assembler 读取该集合，cache version 包含 collapsed；全链路测试覆盖展开 / 折叠。
4. `QueuedUserMessage` 必须组装成带 `Queued` semantic 的 UserMessage；所有 timeline variant 都有显式 assembler 分支。
5. BlockCache 与 GuttedCache 必须 bounded；所有影响可见输出的 ToolResult data / style 字段必须进入 `cache_version()`。
6. `follow_tail` 只在 OutputViewState 定义一次；**NEVER** 存在常量 hint、无消费字段、no-op event / effect、无调用 production module 或全局 `allow(dead_code)`。
7. ViewState 只持 scroll / selection / collapse / animation / cache 等瞬时状态；Run、Interaction、spinner phase / verb 与 input buffer 只从 Model / ViewModel 读取。

## 10. 相关文档

- TUI 架构总览（八层管线 + 三条信息流）：[01-architecture-and-dataflow.md](01-architecture-and-dataflow.md)
- TUI Model 层：[02-model.md](02-model.md)
- TUI 事件流与 ACL：[03-event-flow-and-acl.md](03-event-flow-and-acl.md)
- 原始 TUI 设计（历史归档）：[../../../snapshot/design/04-tui-design.md](../../../snapshot/design/04-tui-design.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：10 种 block 类型、ViewAssembler 组装、ViewState 状态机、三层缓存、Render 管线与架构门禁 | #798 |
| 2026-07-14 | 收敛 Target-only 视图契约：只读 Model、瞬时 ViewState、bounded cache、穷尽 timeline 组装与可执行门禁 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | OutputViewCache memo key 统一为三元组 `(revision, workspace_root, collapsed_revision)`（§3.3 / §5.1）（#10 阻断修复） | [#972](https://github.com/rushsinging/aemeath/issues/972) |
