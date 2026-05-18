# Feature #31: TUI 架构守卫脚本（TEA 纯度 + 400 行限制）

- **完成日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认完成

## 目标

全仓 Rust 文件拆分并新增架构守卫入口，确保 TUI update 函数纯度与文件行数限制。

## 完成内容

1. `scripts/check-rust-file-lines.sh`：强制所有 `.rs` 文件不超过 400 行
2. `scripts/check-tui-tea-purity.sh`：禁止 TUI `update/` 中直接执行 spawn/hook/clipboard/image 等副作用
3. `scripts/check-architecture-guards.sh`：聚合 400 行、TEA 纯度与 unsafe text ops 检查
