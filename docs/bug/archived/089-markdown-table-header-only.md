# Bug #89：TUI markdown 表格只渲染表头，分隔行与数据行原样泄漏

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | TUI 渲染 / markdown 表格收集循环 |

## 症状

模型回复正文中的 markdown 表格在 TUI 渲染时只显示表头，分隔行 `|---|` 与数据行被原样输出为 ASCII `| a | b |`，看起来"表格不渲染"。

## 根因

`render_fenced_markdown`（`primitives/fenced.rs`）的表格块收集循环：

```text
while is_table_row(src[end])
```

在遇到分隔行 `|---|` 时停止——`is_table_row` 对分隔行返回 false（其定义含 `&& !is_table_separator`），故 `block_src` 只含表头一行，`table()` 仅渲染表头，分隔行与全部数据行被 `idx=end` 跳过后当普通文本原样输出。旧测试断言过弱（仅查 `│`，表头单独即含 `│`）掩盖了该 bug。

## 修复

收集循环改为：

```text
while is_table_row(src[end]) || is_table_separator(src[end])
```

整块（表头 + 分隔 + 数据行）交给 `table()` 渲染。

## 回归测试

`test_table_block_renders_separator_and_all_data_rows`：断言无原样 `|---`、数据行带 `│` 且无原始 ASCII `|`。

## 相关提交

- `6ec1468` fix(tui): markdown 表格渲染全部行而非仅表头 (refs #89)
- `5698120` Merge bug/table-diff-render: markdown 表格渲染全部行 (refs #89)

## 验证

2026-05-30 用户确认 bug #89 已修复。
