# Bash 流式结果统一缩进设计

## 背景

Bash 的实时 stdout 通过 `ToolProgressEvent` 写入 `ToolCall.streaming_preview`，随后投影为 `ToolCallBlockView.activity_lines`。当前 `tool_call` renderer 在父块内部手工拼接 `⎿ ` 和续行空格；正式结果则装配为 depth-1 `ToolResult` 子块，由 gutter 注入 marker 与缩进。因此流式结果比正式结果多两列缩进，并绕过窄屏和宽度计算策略。

## 目标

- 流式预览与最终结果使用相同的结构化 `ToolResult` 子块渲染路径。
- marker、层级缩进、续行对齐、窄屏降级和可用宽度全部由 gutter 统一管理。
- 工具完成后只显示权威最终结果，不重复显示流式预览。
- 不修改 tools、runtime 或 SDK 的流式事件链路。

## 设计

`ToolCallBlockView` 不再携带供父块 renderer 绘制的 `activity_lines`。View Assembler 在工具运行且有流式预览时，为 `ToolCall` 创建临时 depth-1 `ToolResult` 子块；其文本来自当前 streaming preview。工具完成且存在权威 result 时，Assembler 只创建最终 `ToolResult` 子块。

`tool_call` renderer 只负责 header 和参数 detail，不再生成 `⎿` 或任何结果缩进。临时与最终 `ToolResult` 均通过 `tool_result` renderer 生成无 gutter 内容，再由 `document_renderer` 根据 depth 调用 gutter 统一注入。

临时子块使用与最终子块不同且稳定的 block id，避免缓存身份混淆；其版本随预览内容变化，使流式更新正确失效。

## 数据流

`ToolProgressEvent` → `ToolCall.streaming_preview` → View Assembler → 临时 `ToolResult` 子节点 → `tool_result` renderer → `document_renderer` → gutter。

工具完成后：权威 `ToolResultPayload` → View Assembler → 最终 `ToolResult` 子节点 → 同一 renderer 与 gutter 路径。

## 测试与验证

1. L1：先修改 renderer 测试表达 `ToolCall` 不再自行输出 activity marker/缩进。
2. L2：Assembler 测试断言运行中的流式预览成为 `ToolResult` 子块，完成后仅保留最终子块。
3. L4：文档渲染测试比较流式与最终结果首行及续行的 gutter 起始列，必须一致。
4. 运行 CLI 定向测试、`cargo build -p cli` 与 `cargo clippy -p cli --all-targets`。
5. 人工 smoke：在 TUI 中执行持续输出的 Bash 命令，确认视觉对齐。

## 非目标

- 不改变 tail 5 行策略。
- 不改变 Bash stdout 的事件协议或持久化。
- 不重构其他工具的进度语义。
