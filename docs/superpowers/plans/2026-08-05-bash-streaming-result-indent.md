# Bash 流式结果统一缩进 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Bash 流式预览与最终 ToolResult 通过同一子块和 gutter 路径渲染，消除额外两列缩进。

**Architecture:** View Assembler 将运行中预览装配为临时 `ToolResult` 子节点；`ToolCall` renderer 只渲染调用信息。临时结果和最终结果都由 `tool_result` renderer 产出内容，并由 document renderer 的 gutter 根据 depth 统一注入 marker 与缩进。

**Tech Stack:** Rust、ratatui、现有 TUI retained ViewModel/Assembler/Renderer、Cargo tests。

---

## 文件结构

- 修改 `apps/cli/src/tui/view_model/output.rs`：从 ToolCall view 移除渲染职责重复的 activity 字段。
- 修改 `apps/cli/src/tui/view_assembler/output_tool_view.rs`：投影运行中预览文本。
- 修改 `apps/cli/src/tui/view_assembler/output.rs`：创建临时或最终 ToolResult 子节点。
- 修改 `apps/cli/src/tui/view_assembler/output_tests/tool_result_tests.rs`：覆盖临时子块和最终替换行为。
- 修改 `apps/cli/src/tui/render/output/blocks/tool_call.rs`：删除手工 marker/缩进渲染及旧测试。
- 修改 `apps/cli/src/tui/render/output/document_renderer.rs` 与对应测试：更新行数估算并验证两条路径 gutter 对齐。
- 修改所有 `ToolCallBlockView` fixture：适配字段调整。

### Task 1: 建立 Assembler 失败证据

- [ ] 修改 `tool_result_tests.rs`，断言运行中 preview 出现在 `tool_node.children[0]` 的 `ToolResultBlockView.result_text`，而不是父 ToolCall 内部。
- [ ] 增加完成态断言：仅有一个最终 ToolResult 子块且内容来自权威 payload。
- [ ] 运行 `cargo test -p cli tui::view_assembler::output_tests::tool_result_tests -- --nocapture`，确认运行态子块断言失败。
- [ ] 提交测试失败证据对应改动。

### Task 2: 将流式预览装配为 ToolResult 子块

- [ ] 在 `ToolCallBlockView` 中将 `activity_lines` 替换为只表达数据的 `streaming_preview: Option<String>`。
- [ ] 在 `find_tool_view` 中仅对未完成工具投影 `call.activities.join("\n")`，完成态投影 `None`。
- [ ] 在 `assemble_item` 中优先使用最终 `result_summary`；无最终结果时使用 `streaming_preview`，创建稳定 id 为 `<tool-id>-streaming-result` 的 ToolResult 子块。
- [ ] 更新全部 fixture 编译错误。
- [ ] 运行 Task 1 定向测试，确认通过。
- [ ] 提交 Assembler 改动。

### Task 3: 删除 ToolCall 内部缩进管理

- [ ] 删除 `render_tool_call` 对 activity 的 `⎿ `、两个空格和 wrap prefix 拼接。
- [ ] 删除或改写旧 activity renderer 测试，断言 ToolCall block 只包含 header/detail。
- [ ] 从 `estimate_block_lines` 删除 ToolCall activity 行估算。
- [ ] 运行 `cargo test -p cli tui::render::output::blocks::tool_call -- --nocapture`，确认通过。
- [ ] 提交 renderer 改动。

### Task 4: 验证 gutter 对齐

- [ ] 在 `document_renderer/tests.rs` 构造相同内容的 streaming ToolResult 与 final ToolResult depth-1 子块。
- [ ] 分别渲染并断言首行 gutter span 文本、`gutter_cols`、续行内容起始列相同，首行仅有一个 `⎿`。
- [ ] 运行该定向测试并确认通过。
- [ ] 提交场景测试。

### Task 5: 完整验证与门禁收尾

- [ ] 运行 `cargo fmt --check`；若失败运行 `cargo fmt` 后重跑。
- [ ] 运行 `cargo test -p cli`。
- [ ] 运行 `cargo build -p cli`。
- [ ] 运行 `cargo clippy -p cli --all-targets -- -D warnings`。
- [ ] 检查 `git diff --check` 和 `git status --short`。
- [ ] 更新 Issue checklist 和验证证据；人工 TUI smoke 若当前环境不可交互则明确标记待用户确认。
