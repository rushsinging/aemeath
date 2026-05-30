# Bug #65：工具结果 fenced code block 后续内容继续显示为 code 颜色

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | TUI 渲染 / Markdown fence 状态机 |

## 症状

TUI 输出区展示工具结果时，如果结果内容包含 fenced code block，代码块结束后后续普通内容仍显示为 code 颜色。例如：

```text
✓ replaced 1 occurrence(s) in
/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/docs/superpowers/plans/2026-05-24-task-window-refactor.md
```

之后的 assistant 文本与下一个 tool call 行被错误套用 code block 样式，降低可读性。

## 根因

Assistant message 与 tool result 各自实现了 Markdown fence 状态机：

- `blocks/assistant_message.rs` 与 `blocks/tool_call.rs::format_result_lines` 重复实现，状态机在 tool result 渲染片段结束后未复位，`in_code_block` 跨 block 泄漏。
- 工具结果 styled spans / cache 跨 block 复用，code block foreground/background 样式未在 fence 结束行后清空。

## 修复

随 #58 渲染管线重构 G2：

- 把 assistant 与工具结果共用的 fence/markdown/table 状态机提取为共享原语 `primitives/fenced.rs::render_fenced_markdown(text, base_style, indent, width) -> Vec<RenderedLine>`。
- `blocks/assistant_message.rs` 与 `blocks/tool_call.rs::format_result_lines` 改为调用共享原语（DRY，fence 渲染单一实现）。
- 每个 block 独立渲染、状态机随调用销毁，fence 结束后普通行恢复 base 色，结构上隔离泄漏。
- Edit 工具 `---DIFF---` diff 渲染路径（G1）保持优先：`render_tool_call` 先判 Edit diff，否则才走 fence/markdown 渲染。
- #63 中 tool result 升为独立 `OutputBlockKind::ToolResult` 子块（render 逻辑从 tool_call 移入 `blocks/tool_result.rs`），结构隔离进一步加固。

## 回归测试

- `primitives/fenced.rs::test_fenced_does_not_leak_code_color_after_close`
- `blocks/tool_call.rs::test_tool_call_result_fence_does_not_leak_code_color_after_close`
- 无闭合 fence / `max_lines=0` / 空结果三个边界测试

## 相关提交

- `cbf250d` docs: 新增 code block 样式泄漏 bug (refs #65)
- `7376799` feat(tui): 工具结果接入共享 fence/markdown 渲染 + 结果摘要 gap 收口 (refs #58 refs #65)
- `4c936f4` feat(tui): tool result 升为子块（ToolResult 变体），删 blocks 字段，结构隔离 #65 (refs #63)
- `ddeac80` docs: #63 spec 对照 main 二次修订（去 #65/#76 修复声明）

## 验证

2026-05-30 用户确认 bug #65 已修复。
