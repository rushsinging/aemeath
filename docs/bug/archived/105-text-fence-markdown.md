# Bug #105：TUI 中 ```text fenced block 被当作代码块显示而非 Markdown 渲染

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-06 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认修复 |
| 修复 | f9fe551, 6e7a5fb |

## 症状

assistant 或工具结果输出 ` ```text ... ``` ` 时，TUI 会显示围栏行，并把内容按代码块/语法高亮处理。用户期望 `text` fence 作为 "Markdown 文本容器"：隐藏围栏，内容按普通 Markdown 渲染（例如 `**bold**` 加粗、列表项渲染为列表）。

## 根因

`render_fenced_markdown` 对所有非 `diff` fence 统一走 `syntax::language_by_extension` / `theme::CODE` 分支，缺少 `text` 语言的 Markdown 语义特判。

## 修复

1. ` ```text ` fence 内内容走 `markdown(line, base_style, width)`。
2. `text` fence 的开闭围栏行不进入渲染结果。
3. 保持无语言 fence、其他语言 fence、`diff` fence 既有行为不变。
4. 添加回归测试覆盖 Markdown 样式生效、围栏隐藏，以及普通代码 fence 不受影响。

## 验证

- `cargo test -p cli test_fenced_text_block_renders_inner_markdown_without_fence_lines`
- `cargo test -p cli test_fenced_unlabeled_code_block_still_renders_fence_and_code`
- 用户确认修复。
