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
- 注意：这套 `inventory` 仅控制**显示**。slash 命令的注册是另一套机制（`CommandDescriptor`），见 `runtime.md`。

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
| Bash | Standard | Hidden | `Visible { 5, Plain, tail=true }` |
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
- **ToolResult 子块首行使用 `└─` 拐角**作为 gutter marker（由 `gutter.rs` 按 `OutputBlockKind::ToolResult` 注入），连接到父 ToolCall header。
