# Feature #23: TUI 字符串/切片安全索引收口

- **完成日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认完成

## 目标

把按字符索引/切片等易越界操作收口到 `safe_text` 工具模块，根治 Bug #4 / #8 / #28 类 panic。

## 完成内容

- 提供 `safe_char_slice`、`safe_str_slice_by_char`、`clamp_char_range`、`truncate_unicode_width`、`col_to_char_idx`、`safe_char_at`、`clamp_split_index`、`str_display_width` 等 API
- 配合 lint 规则与单元测试覆盖边界
- 禁止业务路径直接 `chars[from..to]` / `s[i..j]`
