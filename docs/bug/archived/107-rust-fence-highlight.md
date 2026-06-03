# Bug #107: TUI Rust fenced code 使用 `rust` 语言名时没有 syntect 高亮

**状态**: 已修复 (已确认)
**优先级**: 中
**发现日期**: 2026-06

## 修复

新增 `language_by_fence_info`，将 Markdown fence 语言名 `rust` 映射到 syntect 可识别的 `rs`，同时保留 `rs` 扩展名路径；`fenced.rs` 改用 fence info 解析入口。
