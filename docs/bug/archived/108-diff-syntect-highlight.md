# Bug #108: TUI diff 代码块没有统一走 syntect 高亮

**状态**: 已修复 (已确认)
**优先级**: 中
**发现日期**: 2026-06

## 修复

`render_unified_diff` 可从 `+++`/`---`/`diff --git` 文件头推断扩展名；新增/删除/上下文正文均去掉 diff 前缀后走 syntect，`+`/`-`、hunk/meta 保留 diff 语义色；Edit diff 删除/新增/上下文正文也统一走 syntect。
