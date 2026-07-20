# TUI / CLI

**Scope**：`apps/cli/src/**`——CLI 二进制入口、TUI（ratatui）、旧版 REPL（rustyline）、工具调用的 TUI 展示。
**主触发**：改 `apps/cli/src/**`。
**次触发**：改输入 / 渲染 / 快捷键 / 选区复制，或新增工具显示注册。
**配套**：所有 Rust 通用规范见 `rust-coding.md`；Agent 循环与 tool 执行编排在 `runtime.md`。

## TEA 副作用纪律

- **NEVER** 在 `update()` 中直接调用 `tokio::spawn`、hook notification、clipboard/image 等副作用——所有副作用通过 `Cmd` 描述并由 runtime 执行。
- TUI 状态遵循单一真相：用户输入的真相只在 model 的输入文档，**NEVER** 直接改渲染层缓冲。

## 工具调用的 TUI 显示（ToolDisplayEntry）

- 工具在 TUI 输出区如何显示，通过 `inventory` 收集的 `ToolDisplayEntry` 注册：
  - 收集点：`apps/cli/src/tui/render/output/tool_display.rs`（`inventory::collect!(ToolDisplayEntry)`）。
  - 内置工具显示：`apps/cli/src/tui/render/output/tool_display/tool_impls.rs`。
  - 任务类工具显示：`apps/cli/src/tui/render/output/tool_display/task_impls.rs`。
- 新增工具的展示样式：在对应 `*_impls.rs` 用 `inventory::submit!(ToolDisplayEntry { ... })` 声明，无需改渲染主循环。
- 注意：这套 `inventory` 仅控制**显示**。slash 命令由 Tools-owned `CommandDescriptor` / `CommandCatalogPort` / `CommandRouterPort` 注册和解析，经 SDK/Composition 注入；CLI/TUI/no-TUI **NEVER** 维护静态命令清单或独立业务 parser。

## ToolRenderPolicy 系统

`ToolRenderPolicy` 控制工具在 TUI 输出区的渲染行为，由 `ToolDisplay::render_policy()` 返回。

### 策略结构

```rust
pub struct ToolRenderPolicy {
    pub header: HeaderPolicy,   // header 行渲染策略
    pub details: DetailsPolicy, // details 区域渲染策略
    pub result: ResultPolicy,   // result 子块渲染策略
}
```

### HeaderPolicy

| Variant | 说明 |
|---|---|
| `Standard` | 标准 header，带 ● marker |
| `Compact` | 紧凑 header，单行无 marker（如 TaskUpdate） |
| `CustomIcon(&'static str)` | 自定义图标（如 📋 EnterPlanMode） |

### DetailsPolicy

| Variant | 说明 |
|---|---|
| `Expanded` | 展开显示 details |
| `Hidden` | 隐藏 details |

### ResultPolicy

| Variant | 说明 |
|---|---|
| `Hidden` | 不显示 result 子块（如 Read/Write/AskUserQuestion） |
| `Visible { max_lines, render_kind, tail_mode }` | 显示 result 子块 |

- `max_lines: Option<usize>` — 最大行数，`None` 表示全部显示
- `render_kind: ResultRender` — 渲染类型（`Plain` 或 `Diff`）
- `tail_mode: bool` — tail 模式，只显示最后 N 行（如 Bash）

### ResultRender

| Variant | 说明 |
|---|---|
| `Plain` | 纯文本原样预览，保留原文（含行号/缩进），不做 markdown 重渲染。适用于 Read/Bash/Grep 等 |
| `Diff` | unified diff 渲染，解析 Edit 结果的 `---DIFF---` 为加减色 diff |

### 各工具 render_policy 配置表

| Tool | Header | Details | Result |
|---|---|---|---|
| Bash | Standard | **Expanded** | `Visible { 5, Plain, tail=true }` |
| Read | Standard | Hidden | Hidden |
| Write | Standard | Hidden | Hidden |
| Edit | Standard | Hidden | `Visible { None, Diff, false }` |
| Glob | Standard | Hidden | `Visible { 5, Plain, false }` |
| Grep | Standard | Hidden | `Visible { 5, Plain, false }` |
| Agent | Standard | **Expanded** | `Visible { 5, Plain, false }` |
| EnterWorktree | Standard | Hidden | `Visible { 16, Plain, false }` |
| ExitWorktree | Standard | Hidden | `Visible { 16, Plain, false }` |
| WebFetch | Standard | Hidden | `Visible { 5, Plain, false }` |
| AskUserQuestion | Standard | Hidden | Hidden |
| TaskCreate | Compact | Hidden | Hidden |

### 关键设计决策

- **渲染类型由工具显式声明**（`render_policy`），渲染层据此分发，不按 `---DIFF---` 字符或硬编码工具名猜测。
- **未注册工具回退策略**：`ResultPolicy::Visible { max_lines: Some(5), render_kind: Plain, tail_mode: false }`。
- **Agent 是唯一使用 `DetailsPolicy::Expanded` 的内置工具**，展示 prompt 预览（截断 200 字符）。
- **Edit 是唯一使用 `ResultRender::Diff` 的工具**，显示全部 diff 行（`max_lines: None`）。
- **Bash 使用 `tail_mode: true`**，只显示最后 5 行输出，避免长命令输出淹没 TUI。
- **TaskCreate 使用 `Compact` 单行模式**，description 合并进 header（`TaskCreate {subject}: {description}`）。
- **ToolResult 子块首行使用 `⎿` 圆角连接**作为 gutter marker（由 `gutter.rs` 按 `OutputBlockKind::ToolResult` 注入），连接到父 ToolCall header。
- **Tool display name 着色为 `ACCENT_BRIGHT`（Mauve）**，与参数文本（`TEXT`）和元信息（`TEXT_MUTED`）形成视觉层次。`format_header_line` 默认实现按 `display_name` 前缀拆分；不支持前缀匹配的（如 emoji 前缀的 `CustomIcon`）自动 fallback 为整体 raw。

## 主题色板（Catppuccin Macchiato）

TUI 采用 **Catppuccin Macchiato** 暗色主题。原始色值定义在 `apps/cli/src/tui/render/theme/palette.rs`，各组件 **MUST** 引用语义色常量，**NEVER** 硬编码 `Color::Rgb(...)`（palette.rs 内部定义与少量自定义色除外）。

### 语义色映射表

| 语义色常量 | Macchiato 原色 | 色值 | 用途 |
|---|---|---|---|
| `TEXT` | text | rgb(202,211,245) | 主文本、tool header 参数文本 |
| `TEXT_MUTED` | subtext0 | rgb(165,173,203) | 次级文本、tool header 元信息（如 `L1:L2000`） |
| `TEXT_DIM` | overlay0 | rgb(110,115,141) | 弱化文本、tool result 预览 |
| `BORDER` | surface1 | rgb(73,77,100) | 面板边框 |
| `ACCENT` | blue | rgb(138,173,244) | 聚焦边框、status bar、cursor、品牌强调 |
| `ACCENT_BRIGHT` | mauve | rgb(198,160,246) | syntax keyword、**tool display name** |
| `SURFACE` | base | rgb(36,39,58) | 深色背景 |
| `STATUS_BG` | base | rgb(36,39,58) | 状态栏背景 |
| `SURFACE_ELEVATED` | surface0 | rgb(54,58,79) | 浮层背景 |
| `SELECTION_BG` | surface1 | rgb(73,77,100) | 选中文本背景 |
| `USER` | 自定义 | rgb(220,232,255) | 用户消息前景 |
| `USER_BG` | 自定义 | rgb(63,95,143) | 用户消息背景 |
| `TOOL_RUNNING` | peach | rgb(245,169,127) | 工具运行中状态、行内代码 |
| `SUCCESS` | green | rgb(166,218,149) | 成功状态 marker（✓） |
| `WARNING` | yellow | rgb(238,212,159) | 警告 |
| `ERROR` | red | rgb(237,135,150) | 错误状态 marker（✗） |
| `THINKING` | overlay1 | rgb(128,135,162) | thinking 文本 |
| `LINK` | blue | rgb(138,173,244) | Markdown 链接 |
| `SPINNER_BASE` | teal | rgb(139,213,202) | Spinner 基础色 |
| `SPINNER_HIGHLIGHT` | green | rgb(166,218,149) | Spinner 高亮 |

### 用色约定

- **ACCENT (Blue)**：UI 框架级交互元素——聚焦边框、status bar、cursor、链接。**NEVER** 用于内容区文本着色。
- **ACCENT_BRIGHT (Mauve)**：内容区强调——syntax keyword、tool display name。语义为"操作类型/关键字"。
- **TEXT / TEXT_MUTED / TEXT_DIM**：三级文本层次，tool header 内 `name(Mauve) > args(TEXT) > meta(TEXT_MUTED)`。
- **状态色**：`SUCCESS`(Green) / `TOOL_RUNNING`(Peach) / `ERROR`(Red) 专用于状态 marker，**NEVER** 用于普通文本。

## Agent tool call 显示

### AgentProgressEvent 数据流

Sub-agent 的实时活动通过 `AgentProgressEvent` 从 runtime 流向 TUI，**MUST** 保持结构化传递，**NEVER** 中途压扁成字符串：

```
SubAgentRun.role_name_for_log / model_name_for_log
  → AgentProgressKind::Started { role, model }   (share 层)
  → setup.rs emit Started 事件                    (runtime 层)
  → AgentProgressKindView::Started                (SDK 层透传)
  → UpdateAgentMeta intent                        (TUI adapter)
  → ToolCall.agent_meta                           (TUI model 层)
  → AgentMetaView                                 (view_model 层投影)
  → merge_agent_meta 合并到 JSON 副本             (render 层)
  → AgentDisplay::format_header_line_with_result 显示
```

关键约定：

- **MUST** `AgentProgressEvent` 新增字段 **MUST** 加 `#[serde(default)]`，保证历史 session 反序列化兼容。
- **MUST** view_model 层 **MUST NOT** 直接引用 model 层类型（架构守卫拦截），**MUST** 定义独立的 view 类型（如 `AgentMetaView`）并在 view_assembler 处投影。
- **MUST** Agent tool 的 role/model 显示（`[role: xxx] [model: xxx]`）**MUST** 在 `AgentDisplay::format_header_line_with_result` 中处理——这是实际被调用的入口。**NEVER** 只改 trait 默认方法 `format_header` 而不动覆写方法，覆写会切断调用链。
- **MUST** main agent 的 ToolCall（Read/Write/Bash 等）`agent_meta` 始终为 `None` → 天然不显示 role/model。**NEVER** 在主 turn context 注入 role/model。

### Sub-agent 实时活动展示（未来）

`AgentProgressEvent` 是 sub-agent → TUI 的结构化实时事件通道，为后续"实时显示 subagent 在做什么"打下基础：

- 当前：`Started` 携带 role/model，`Message` / `ToolCalls` 被 `format_agent_progress` 压扁成字符串。
- 未来：`Message` / `ToolCalls` **SHOULD** 保持结构化传递，TUI 展开嵌套显示（类似 Claude Code 的 SubAgent 折叠块）。
