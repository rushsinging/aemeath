# Bug #109: TUI syntect 高亮主题使用 base16-ocean.dark，与 Catppuccin Macchiato UI 主题不一致

**状态**: 已修复 (已确认)
**优先级**: 中
**发现日期**: 2026-06

## 修复

补齐官方 Catppuccin Macchiato palette 命名常量，并用这些常量手写 syntect Theme 构造器；`syntax.rs` 不再加载 `base16-ocean.dark`，Rust keyword 等 token 使用 Macchiato 色系。
