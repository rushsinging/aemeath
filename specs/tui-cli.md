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
